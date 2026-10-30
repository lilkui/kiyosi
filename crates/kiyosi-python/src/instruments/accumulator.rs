use crate::conversions::{date, python_date, real};
use crate::error::domain;
use kiyosi as core;
use pyo3::prelude::*;
use pyo3::types::PyDate;

/// Trading-date purchases settled at knock-out or expiry.
///
/// Parameters
/// ----------
/// strike : float
///     Positive finite exercise or purchase price.
/// knock_out_level : float
///     Positive inclusive knock-out threshold.
/// daily_quantity : float
///     Nonnegative finite quantity purchased on each trading date.
/// acceleration_factor : float
///     Nonnegative quantity multiplier applied strictly below strike.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// accumulated_quantity : float, optional
///     Caller-asserted outstanding quantity strictly before valuation.
///
/// Raises
/// ------
/// InvalidInputError
///     If strike or knock-out level is invalid, quantities are negative or
///     non-finite, or expiry precedes effective_date.
///
/// Notes
/// -----
/// A current midnight knock-out precedes purchases and purchases no units.
/// Acceleration applies strictly below strike. History is caller asserted;
/// do not value a contract whose settlement has already occurred.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct Accumulator(pub(crate) core::Accumulator);

#[pymethods]
impl Accumulator {
    #[new]
    #[pyo3(signature = (*, strike, knock_out_level, daily_quantity, acceleration_factor, effective_date, expiry_date, accumulated_quantity=core::AccumulatorTerms::DEFAULT_ACCUMULATED_QUANTITY))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        strike: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        daily_quantity: &Bound<'_, PyAny>,
        acceleration_factor: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] accumulated_quantity: f64,
    ) -> PyResult<Self> {
        let mut terms = core::AccumulatorTerms::new(
            real(strike)?,
            real(knock_out_level)?,
            real(daily_quantity)?,
            real(acceleration_factor)?,
            date(effective_date)?,
            date(expiry_date)?,
        );
        terms.accumulated_quantity = accumulated_quantity;
        Ok(Self(domain(py, core::Accumulator::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// accumulated_quantity : float
    ///     Caller-asserted outstanding quantity strictly before valuation.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (*, accumulated_quantity))]
    fn replace(&self, py: Python<'_>, accumulated_quantity: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            self.0.replace(real(accumulated_quantity)?),
        )?))
    }
    /// Return the exercise or purchase price in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return positive inclusive knock-out threshold.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    /// Return nonnegative finite quantity purchased on each trading date.
    #[getter]
    fn daily_quantity(&self) -> f64 {
        self.0.daily_quantity()
    }
    /// Return nonnegative quantity multiplier applied strictly below strike.
    #[getter]
    fn acceleration_factor(&self) -> f64 {
        self.0.acceleration_factor()
    }
    /// Return caller-asserted outstanding quantity strictly before valuation.
    #[getter]
    fn accumulated_quantity(&self) -> f64 {
        self.0.accumulated_quantity()
    }
    /// Return inclusive first contract date.
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    /// Return final contract date; expiry is at midnight UTC.
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Accumulator(strike={:?}, knock_out_level={:?}, daily_quantity={:?}, acceleration_factor={:?}, accumulated_quantity={:?}, effective_date={}, expiry_date={})",
            self.strike(),
            self.knock_out_level(),
            self.daily_quantity(),
            self.acceleration_factor(),
            self.accumulated_quantity(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?
        ))
    }
}
