use crate::conversions::{python_date, python_timestamp, python_timestamps, real, timestamp};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyTuple};

macro_rules! rate_properties {
    ($name:ident, @replace $changes:ident [$($parameter:ident),+]
     {$($field:ident: $value:expr),+ $(,)?}, $($extra:item),* $(,)?) => {
        value_methods!($name, $changes, [$($parameter),+], {$($field: $value),+}, {
            $($extra)*

            /// Return the scalar rate for dated or undated constant curves.
            ///
            /// Use ``risk_free_curve`` to preserve coverage when passing rates to constructors
            /// or replacements.
            ///
            /// Raises
            /// ------
            /// UnsupportedOperationError
            ///     If the curve is nonconstant.
            ///
            #[getter]
            fn flat_risk_free_rate(&self) -> pyo3::PyResult<f64> {
                $crate::error::domain(self.0.flat_risk_free_rate())
            }
            /// Return the scalar yield for dated or undated constant curves.
            ///
            /// Use ``dividend_curve`` to preserve coverage when passing yields to constructors
            /// or replacements.
            ///
            /// Raises
            /// ------
            /// UnsupportedOperationError
            ///     If the curve is nonconstant.
            ///
            #[getter]
            fn flat_dividend_yield(&self) -> pyo3::PyResult<f64> {
                $crate::error::domain(self.0.flat_dividend_yield())
            }
            /// Return an owned copy of the risk-free curve.
            #[getter]
            fn risk_free_curve(&self) -> $crate::market::RateTermStructure {
                $crate::market::RateTermStructure(self.0.risk_free_curve().clone())
            }
            /// Return an owned copy of the dividend curve.
            #[getter]
            fn dividend_curve(&self) -> $crate::market::RateTermStructure {
                $crate::market::RateTermStructure(self.0.dividend_curve().clone())
            }
        });
    };
}
pub(crate) use rate_properties;

/// Rate term structure with log-linear discount factors.
///
/// Parameters
/// ----------
/// reference_time : datetime.datetime
///     Inclusive curve reference; factors equal one at this instant.
/// times : iterable of datetime.datetime
///     Strictly increasing nodes after the reference.
/// factors : iterable of float
///     Positive finite factors at the nodes; log factors interpolate linearly.
///
/// Raises
/// ------
/// InvalidInputError
///     If nodes or factors are empty or mismatched, times do not increase
///     after the reference, factors are nonpositive or non-finite, or implied
///     forward rates are non-finite.
///
/// Notes
/// -----
/// Uses the Actual/365 Fixed day-count convention. Log-linear interpolation of
/// discount factors gives piecewise constant instantaneous forward rates.
/// Dated curves reject times before their reference; node-based rate curves
/// also reject times after the last node. Negative rates are allowed.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RateTermStructure(pub(crate) kiyosi::RateTermStructure);

#[pymethods]
impl RateTermStructure {
    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        crate::representation::curve(slf.py(), &slf.borrow().0)
    }

    #[new]
    #[pyo3(signature = (*, reference_time, times, factors))]
    fn new(
        reference_time: &Bound<'_, PyAny>,
        times: &Bound<'_, PyAny>,
        factors: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let reference = timestamp(reference_time)?;
        let nodes = crate::conversions::timestamps(times)?;
        let factors = crate::conversions::reals(factors)?;
        Ok(Self(domain(kiyosi::RateTermStructure::new(
            reference, nodes, factors,
        ))?))
    }
    /// Create a continuously compounded flat rate curve.
    ///
    /// Parameters
    /// ----------
    /// rate : float
    ///     Finite annual rate in decimal units; negative rates are allowed.
    /// reference_time : datetime.datetime, optional
    ///     Inclusive lower coverage bound, or None for an undated curve.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Owned flat curve with no upper coverage bound.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the rate is non-finite.
    ///
    #[staticmethod]
    #[pyo3(signature = (rate, *, reference_time=None))]
    fn flat(rate: &Bound<'_, PyAny>, reference_time: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::RateTermStructure::flat(
            real(rate)?,
            reference_time.map(timestamp).transpose()?,
        ))?))
    }
    /// Create an exactly constant rate with inclusive coverage bounds.
    ///
    /// Parameters
    /// ----------
    /// rate : float
    ///     Finite continuously compounded annual rate; negative rates are allowed.
    /// reference_time, max_time : datetime.datetime
    ///     Inclusive coverage bounds. Equal bounds select a single instant.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Owned curve retaining the exact scalar rate and both bounds, including
    ///     through shifts and pickle. Nodes and factors are empty, as for flat().
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the rate is non-finite or max_time precedes reference_time.
    ///
    #[staticmethod]
    #[pyo3(signature = (rate, *, reference_time, max_time))]
    fn bounded_flat(
        rate: &Bound<'_, PyAny>,
        reference_time: &Bound<'_, PyAny>,
        max_time: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::RateTermStructure::bounded_flat(
            real(rate)?,
            timestamp(reference_time)?,
            timestamp(max_time)?,
        ))?))
    }
    /// Return the scalar rate when all implied forwards are exactly constant.
    ///
    /// Nonconstant curves return None. Node-based coverage is preserved. This
    /// query does not change the curve or its coverage bounds.
    ///
    #[getter]
    fn exact_flat_rate(&self) -> Option<f64> {
        self.0.exact_flat_rate()
    }
    /// Return the inclusive lower coverage bound, or None if undated.
    #[getter]
    fn reference_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .reference_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    /// Return the inclusive upper bound, or None for an unbounded flat curve.
    #[getter]
    fn max_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .max_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    /// Return a copy of the supplied curve node times.
    #[getter]
    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        python_timestamps(py, self.0.nodes())
    }
    /// Return owned node factors including shifts, allowing underflow to zero.
    ///
    /// Raises
    /// ------
    /// ComputationFailureError
    ///     If a log factor is non-finite or exponentiation overflows.
    ///
    /// Notes
    /// -----
    /// Zero factors cannot reconstruct a curve through its constructor.
    /// Reconstruct node curves from base_factors, then apply
    /// shifted(parallel_shift). Pickle preserves this representation, including
    /// extreme shifted curves.
    ///
    #[getter]
    fn factors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, domain(self.0.factors())?)
    }
    /// Return original node factors without parallel shifts.
    ///
    /// Flat curves have no node factors. For node curves, pass these factors with
    /// reference_time and nodes to RateTermStructure, then call
    /// shifted(parallel_shift).
    ///
    #[getter]
    fn base_factors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.base_factors())
    }
    /// Return the accumulated node-curve parallel shift in decimal units.
    ///
    /// Flat curves include shifts in their rate and return zero here.
    ///
    #[getter]
    fn parallel_shift(&self) -> f64 {
        self.0.parallel_shift()
    }
    /// Return the factor over an ordered, covered interval.
    ///
    /// Parameters
    /// ----------
    /// start_time, end_time : datetime.datetime
    ///     Ordered interval endpoints.
    ///
    /// Returns
    /// -------
    /// float
    ///     Interval factor; underflow to zero is allowed.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If endpoints are reversed or outside curve coverage.
    /// ComputationFailureError
    ///     If the log factor is non-finite or exponentiation overflows.
    ///
    fn factor(&self, start_time: &Bound<'_, PyAny>, end_time: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.factor(timestamp(start_time)?, timestamp(end_time)?))
    }
    /// Return the log factor over an ordered, covered interval.
    ///
    /// Parameters
    /// ----------
    /// start_time, end_time : datetime.datetime
    ///     Ordered interval endpoints.
    ///
    /// Returns
    /// -------
    /// float
    ///     Log of the interval factor, using Actual/365 Fixed time.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If endpoints are reversed or outside curve coverage.
    /// ComputationFailureError
    ///     If the result is non-finite.
    ///
    fn log_factor(
        &self,
        start_time: &Bound<'_, PyAny>,
        end_time: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(
            self.0
                .log_factor(timestamp(start_time)?, timestamp(end_time)?),
        )
    }
    /// Return the right-hand instantaneous forward rate.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.datetime
    ///     Query instant. The final node uses its left interval.
    ///
    /// Returns
    /// -------
    /// float
    ///     Annual continuously compounded forward rate in decimal units.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the instant is outside curve coverage.
    ///
    fn forward_rate(&self, time: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.forward_rate(timestamp(time)?))
    }
    /// Create a parallel rate shift without changing this curve.
    ///
    /// Parameters
    /// ----------
    /// shift : float
    ///     Finite additive rate shift in decimal units.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Independent curve with unchanged coverage.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the shift or resulting rates are non-finite.
    ///
    fn shifted(&self, shift: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(self.0.shifted(real(shift)?))?))
    }
}

pub(crate) fn rate(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Rate> {
    if let Ok(curve) = value.extract::<PyRef<'_, RateTermStructure>>() {
        Ok(curve.0.clone().into())
    } else {
        Ok(real(value)?.into())
    }
}

/// Black-Scholes-Merton (BSM) model parameters.
///
/// Parameters
/// ----------
/// risk_free_rate : float or RateTermStructure
///     Continuously compounded risk-free rate or owned rate curve.
/// dividend_yield : float or RateTermStructure
///     Continuously compounded dividend yield or owned rate curve.
/// volatility : float
///     Nonnegative finite annualized log-return volatility in decimal units.
///     Zero selects the exact deterministic path implied by the rate curves.
///
/// Raises
/// ------
/// InvalidInputError
///     If scalar rates are non-finite or volatility is negative or non-finite.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BlackScholesMertonParameters(pub(crate) kiyosi::BlackScholesMertonParameters);

rate_properties!(
    BlackScholesMertonParameters,
    @replace BlackScholesMertonParameterChanges [risk_free_rate, dividend_yield, volatility] {
        risk_free_rate: risk_free_rate.map(rate).transpose()?,
        dividend_yield: dividend_yield.map(rate).transpose()?,
        volatility: volatility.map(real).transpose()?,
    },
    #[new]
    #[pyo3(signature = (*, risk_free_rate, dividend_yield, volatility))]
    fn new(
        risk_free_rate: &Bound<'_, PyAny>,
        dividend_yield: &Bound<'_, PyAny>,
        volatility: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::BlackScholesMertonParameters::new(
            rate(risk_free_rate)?,
            rate(dividend_yield)?,
            real(volatility)?,
        ))?))
    },
    /// Return nonnegative finite annualized volatility in decimal units.
    #[getter]
    fn volatility(&self) -> f64 {
        self.0.volatility()
    },
);

/// Market snapshot for one valuation instant.
///
/// Parameters
/// ----------
/// model_parameters : BlackScholesMertonParameters, LocalVolatilityParameters or HestonParameters
///     Validated rates and the selected volatility or variance model.
/// spot_price : float
///     Positive finite underlying spot price.
/// valuation_time : datetime.datetime
///     Valuation time.
///
/// Raises
/// ------
/// InvalidInputError
///     If spot_price is nonpositive or non-finite or valuation_time is outside
///     either rate curve or the local-volatility surface. Contract-life and
///     expiry coverage checks occur at pricing.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PricingContext(pub(crate) kiyosi::PricingContext);

value_methods!(PricingContext, PricingContextChanges,
    [model_parameters, spot_price, valuation_time], {
        model_parameters: model_parameters.map(model_parameters_value).transpose()?,
        spot_price: spot_price.map(real).transpose()?,
        valuation_time: valuation_time.map(timestamp).transpose()?,
    }, {
    #[new]
    #[pyo3(signature = (*, model_parameters, spot_price, valuation_time))]
    fn new(
        model_parameters: &Bound<'_, PyAny>,
        spot_price: &Bound<'_, PyAny>,
        valuation_time: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::PricingContext::new(
            model_parameters_value(model_parameters)?,
            real(spot_price)?,
            timestamp(valuation_time)?,
        ))?))
    }

    /// Return an owned copy of the model parameters.
    #[getter]
    fn model_parameters(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.0.model_parameters() {
            kiyosi::ModelParameters::Heston(m) => {
                Ok(Py::new(py, crate::heston::HestonParameters(m.clone()))?.into_any())
            }
            kiyosi::ModelParameters::BlackScholesMerton(m) => {
                Ok(Py::new(py, BlackScholesMertonParameters(m.clone()))?.into_any())
            }
            kiyosi::ModelParameters::LocalVolatility(m) => Ok(Py::new(
                py,
                crate::local_volatility::LocalVolatilityParameters(m.clone()),
            )?
            .into_any()),
        }
    }
    /// Return positive finite underlying spot price.
    #[getter]
    fn spot_price(&self) -> f64 {
        self.0.spot_price()
    }
    /// Return the date containing the valuation instant.
    #[getter]
    fn valuation_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.valuation_date())
    }
    /// Return valuation time.
    #[getter]
    fn valuation_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.valuation_time())
    }
});

fn model_parameters_value(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::ModelParameters> {
    if let Ok(m) = value.extract::<PyRef<'_, crate::heston::HestonParameters>>() {
        return Ok(m.0.clone().into());
    }
    if let Ok(m) = value.extract::<PyRef<'_, BlackScholesMertonParameters>>() {
        return Ok(m.0.clone().into());
    }
    if let Ok(m) = value.extract::<PyRef<'_, crate::local_volatility::LocalVolatilityParameters>>()
    {
        return Ok(m.0.clone().into());
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected BlackScholesMertonParameters, LocalVolatilityParameters or HestonParameters",
    ))
}
