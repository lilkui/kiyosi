use std::ops::RangeInclusive;

use crate::conversions::{python_timestamp, real, reals};
use crate::engines::NativeGreeksRequest;
use crate::error::domain;
use pyo3::{exceptions::PyValueError, prelude::*, types::PyTuple};

pub(crate) fn spot_range(value: &Bound<'_, PyAny>) -> PyResult<RangeInclusive<f64>> {
    let pair = value.cast::<PyTuple>()?;
    if pair.len() != 2 {
        return Err(PyValueError::new_err("spot_range requires two bounds"));
    }
    Ok(real(&pair.get_item(0)?)?..=real(&pair.get_item(1)?)?)
}

pub(crate) fn solve<I: kiyosi::OptionContract + Sync>(
    py: Python<'_>,
    engine: &(impl kiyosi::FiniteDifferenceEngine<I> + Sync),
    instrument: &I,
    context: &crate::market::PricingContext,
    range: RangeInclusive<f64>,
) -> PyResult<FiniteDifferenceSolution> {
    let context = &context.0;
    Ok(FiniteDifferenceSolution(domain(crate::cancellation::run(
        py,
        || engine.solve(instrument, context, range),
    )?)?))
}

/// Owned fixed-time solution for repeated spot pricing without another rollback.
///
/// Notes
/// -----
/// Created by a finite-difference engine's ``solve`` method. The contract,
/// model, valuation time and supplied history are fixed. Current observations
/// use each query's spot. Queries may cross current knock-in, knock-out, coupon
/// and settlement conditions. Source objects can be released or replaced.
/// Shared instances permit concurrent reads; long queries release the GIL and
/// support cancellation without partial results. The object is immutable and
/// cannot be constructed directly. Mesh and truncation accuracy require
/// convergence checks; there is no automatic error estimate or refinement.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct FiniteDifferenceSolution(pub(crate) kiyosi::FiniteDifferenceSolution);

impl FiniteDifferenceSolution {
    fn evaluate<T: Send>(
        &self,
        py: Python<'_>,
        queries: usize,
        operation: impl FnOnce() -> kiyosi::Result<T> + Send,
    ) -> PyResult<T> {
        if queries > 32 || queries.saturating_mul(self.0.exercise_intervals()) > 1024 {
            domain(crate::cancellation::run(py, operation)?)
        } else {
            domain(operation())
        }
    }
}

#[pymethods]
impl FiniteDifferenceSolution {
    /// Return the owned closed spot query interval.
    #[getter]
    fn spot_range(&self) -> (f64, f64) {
        let range = self.0.spot_range();
        (*range.start(), *range.end())
    }

    /// Return the fixed valuation time.
    #[getter]
    fn valuation_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.valuation_time())
    }

    /// Return potential Greek capabilities; query statuses describe availability.
    #[getter]
    fn supported<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let supported = self.0.supported();
        PyTuple::new(
            py,
            kiyosi::Greek::ALL
                .into_iter()
                .filter(|&g| supported.has(g))
                .map(kiyosi::Greek::name)
                .collect::<Vec<_>>(),
        )
    }

    /// Evaluate the saved solution at one spot.
    ///
    /// Parameters
    /// ----------
    /// spot_price : float
    ///     Finite positive spot inside the closed query interval.
    ///
    /// Returns
    /// -------
    /// float
    ///     Price with current observations applied to this spot.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the spot is invalid or outside the interval.
    /// ComputationFailureError
    ///     If numerical values or financial price checks fail.
    /// TypeError
    ///     If the numeric representation is invalid, including a bool.
    fn price(&self, py: Python<'_>, #[pyo3(from_py_with = real)] spot_price: f64) -> PyResult<f64> {
        self.evaluate(py, 1, || self.0.price(spot_price))
    }

    /// Evaluate saved prices in input order, preserving duplicates.
    ///
    /// Parameters
    /// ----------
    /// spot_prices : iterable of float
    ///     Spots inside the closed interval. Empty input is valid.
    ///
    /// Returns
    /// -------
    /// tuple of float
    ///     Owned prices, or an empty tuple for empty input.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If any spot or price is invalid; no partial batch is returned.
    /// TypeError
    ///     If the iterable or an element has an invalid representation.
    ///
    /// Notes
    /// -----
    /// Iterator exceptions propagate; consuming an iterator is not rolled back.
    /// Long core work releases the GIL and polls Python signals.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 copies the iterable into an owned Vec before detached work."
    )]
    fn price_many<'py>(
        &self,
        py: Python<'py>,
        #[pyo3(from_py_with = reals)] spot_prices: Vec<f64>,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let values = self.evaluate(py, spot_prices.len(), || self.0.price_many(&spot_prices))?;
        PyTuple::new(py, values)
    }

    /// Evaluate saved price and requested spot Greeks.
    ///
    /// Parameters
    /// ----------
    /// spot_price : float
    ///     Finite positive spot inside the closed interval.
    /// greeks : str or iterable of str
    ///     A Greek name, "all", or names; empty input requests price only.
    /// spot_shift : float, None or Ellipsis, optional
    ///     Positive absolute spot shift, or the Rust-defined automatic default.
    ///     Omission, None and Ellipsis select automatic shifts.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested sensitivities with their availability statuses.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If spot, shift, or financial/numerical validation fails.
    /// TypeError, ValueError
    ///     If representations or Greek names are invalid.
    ///
    /// Notes
    /// -----
    /// BSM supports delta/gamma/speed, local volatility delta/gamma, and Heston
    /// no Greeks. Other requests are unsupported. Financial discontinuities
    /// are undefined; required samples outside the interval have status
    /// "stencil_unavailable". No query extends the domain or repeats rollback.
    #[pyo3(signature = (spot_price, greeks, *, spot_shift=crate::conversions::OMITTED))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        spot_price: &Bound<'_, PyAny>,
        greeks: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = crate::conversions::supplied)] spot_shift: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<crate::risk::PricingResult> {
        let request = NativeGreeksRequest::from_python(greeks, spot_shift, None, None, None)?;
        let spot = real(spot_price)?;
        Ok(crate::risk::PricingResult(self.evaluate(py, 1, || {
            self.0
                .price_with_greeks(spot, request.request, request.settings.spot_shift)
        })?))
    }

    /// Evaluate saved prices and requested Greeks in input order.
    ///
    /// Parameters
    /// ----------
    /// spot_prices : iterable of float
    ///     Spots inside the closed interval; duplicates and empty input are valid.
    /// greeks : str or iterable of str
    ///     Shared request, using price_with_greeks conventions.
    /// spot_shift : float, None or Ellipsis, optional
    ///     Positive spot shift or the Rust-defined automatic default.
    ///
    /// Returns
    /// -------
    /// tuple of PricingResult
    ///     Per-spot prices, sensitivities and statuses.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If any input or query fails, without a partial batch.
    /// TypeError, ValueError
    ///     If representations or Greek names are invalid.
    ///
    /// Notes
    /// -----
    /// Controls are validated even for an empty batch. Iterator exceptions
    /// propagate. Long core work releases the GIL and polls Python signals.
    #[pyo3(signature = (spot_prices, greeks, *, spot_shift=crate::conversions::OMITTED))]
    fn price_many_with_greeks<'py>(
        &self,
        py: Python<'py>,
        spot_prices: &Bound<'py, PyAny>,
        greeks: &Bound<'py, PyAny>,
        #[pyo3(from_py_with = crate::conversions::supplied)] spot_shift: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let request = NativeGreeksRequest::from_python(greeks, spot_shift, None, None, None)?;
        let spots = reals(spot_prices)?;
        let values = self.evaluate(py, spots.len(), || {
            self.0
                .price_many_with_greeks(&spots, request.request, request.settings.spot_shift)
        })?;
        let objects = values
            .into_iter()
            .map(|value| Py::new(py, crate::risk::PricingResult(value)))
            .collect::<PyResult<Vec<_>>>()?;
        PyTuple::new(py, objects)
    }
}
