use crate::conversions::{python_timestamps, real, reals, timestamp, timestamps};
use crate::error::domain;
use crate::market::{rate, rate_properties};
use pyo3::prelude::*;
use pyo3::types::PyTuple;

/// Local-volatility table indexed by time (rows) and absolute spot (columns).
///
/// Parameters
/// ----------
/// times : iterable of datetime.datetime
///     At least two strictly increasing instants; the first is the reference.
/// spots : iterable of float
///     At least two strictly increasing positive finite prices.
/// values : iterable of iterable of float
///     Rectangular positive finite annualized volatilities (20% is 0.2).
///
/// Raises
/// ------
/// InvalidInputError
///     If the table shape, nodes or values are invalid.
///
/// Notes
/// -----
/// Local volatility interpolates bilinearly in time and absolute spot price,
/// using Actual/365 Fixed year fractions. Time extrapolation is rejected;
/// extrapolation in the spot dimension holds the nearest endpoint volatility
/// constant. Inputs are copied. Properties return owned copies; spot changes
/// and time rolls never translate the surface. Native conversion errors reject
/// booleans, dates, timezone-aware datetimes and invalid iterable elements.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct LocalVolatilitySurface(pub(crate) kiyosi::LocalVolatilitySurface);

#[pymethods]
impl LocalVolatilitySurface {
    #[classattr]
    const _STATE_FIELDS: &'static [&'static str] = &["times", "spots", "values"];

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        crate::representation::surface(slf.py(), &slf.borrow().0)
    }

    #[new]
    #[pyo3(signature = (*, times, spots, values))]
    fn new(
        times: &Bound<'_, PyAny>,
        spots: &Bound<'_, PyAny>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let times = timestamps(times)?;
        let spots = reals(spots)?;
        let values = values
            .try_iter()?
            .map(|row| reals(&row?))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Self(domain(kiyosi::LocalVolatilitySurface::new(
            times, spots, values,
        ))?))
    }
    /// Return copied absolute time nodes as datetimes.
    #[getter]
    fn times<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        python_timestamps(py, self.0.times())
    }
    /// Return copied absolute price nodes.
    #[getter]
    fn spots<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.spots())
    }
    /// Return copied volatility rows, indexed by time then price.
    #[getter]
    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(
            py,
            self.0
                .values()
                .map(|row| PyTuple::new(py, row))
                .collect::<PyResult<Vec<_>>>()?,
        )
    }
    /// Return the largest supplied annualized volatility.
    #[getter]
    fn maximum_volatility(&self) -> f64 {
        self.0.maximum_volatility()
    }
    /// Interpolate sigma at an absolute time and positive spot.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.datetime
    ///     Covered query instant.
    /// spot : float
    ///     Positive finite underlying price; extrapolation holds endpoint
    ///     volatility constant.
    ///
    /// Returns
    /// -------
    /// float
    ///     Annualized local volatility in decimal units.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If time is outside coverage or spot is nonpositive or non-finite.
    ///
    fn volatility(&self, time: &Bound<'_, PyAny>, spot: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.volatility(timestamp(time)?, real(spot)?))
    }
}

/// Local-volatility model parameters.
///
/// Parameters
/// ----------
/// risk_free_rate, dividend_yield : float or RateTermStructure
///     Finite continuously compounded rates or validated rate curves.
/// surface : LocalVolatilitySurface
///     Validated surface in absolute time and price coordinates.
///
/// Raises
/// ------
/// InvalidInputError
///     If a scalar rate is non-finite.
///
/// Notes
/// -----
/// Surface and curve coverage are checked by the context and during pricing
/// over the required remaining horizons. The one-dimensional finite-difference
/// engines support price, delta and gamma; digital pricing requires constant
/// rates.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct LocalVolatilityParameters(pub(crate) kiyosi::LocalVolatilityParameters);

rate_properties!(
    LocalVolatilityParameters,
    @replace LocalVolatilityParameterChanges [risk_free_rate, dividend_yield, surface] {
        risk_free_rate: risk_free_rate.map(rate).transpose()?,
        dividend_yield: dividend_yield.map(rate).transpose()?,
        surface: surface.map(|value| value.extract::<PyRef<'_, LocalVolatilitySurface>>()
            .map(|value| value.0.clone())).transpose()?,
    },
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "LocalVolatilityParameters(risk_free_rate={}, dividend_yield={}, surface={})",
            crate::representation::curve(py, self.0.risk_free_curve())?,
            crate::representation::curve(py, self.0.dividend_curve())?,
            crate::representation::surface(py, self.0.surface())?,
        ))
    },
    #[new]
    #[pyo3(signature = (*, risk_free_rate, dividend_yield, surface))]
    fn new(
        risk_free_rate: &Bound<'_, PyAny>,
        dividend_yield: &Bound<'_, PyAny>,
        surface: &LocalVolatilitySurface,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::LocalVolatilityParameters::new(
            rate(risk_free_rate)?,
            rate(dividend_yield)?,
            surface.0.clone(),
        ))?))
    },
    /// Return an owned copy of the fixed surface.
    #[getter]
    fn surface(&self) -> LocalVolatilitySurface {
        LocalVolatilitySurface(self.0.surface().clone())
    },
);
