use crate::conversions::{check_changes, date, python_date, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct Accumulator(pub(crate) core::Accumulator);

#[pymethods]
impl Accumulator {
    #[new]
    #[pyo3(signature = (*, strike, knock_out_level, daily_quantity, acceleration_factor, effective_date, expiry_date, **history))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        strike: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        daily_quantity: &Bound<'_, PyAny>,
        acceleration_factor: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        history: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(history, &["accumulated_quantity"])?;
        let mut terms = core::AccumulatorTerms::new(
            real(strike)?,
            real(knock_out_level)?,
            real(daily_quantity)?,
            real(acceleration_factor)?,
            date(effective_date)?,
            date(expiry_date)?,
        );
        if let Some(value) = history
            .map(|f| f.get_item("accumulated_quantity"))
            .transpose()?
            .flatten()
        {
            terms.accumulated_quantity = real(&value)?;
        }
        Ok(Self(domain(py, core::Accumulator::new(terms))?))
    }
    #[pyo3(signature = (*, accumulated_quantity))]
    fn replace(&self, py: Python<'_>, accumulated_quantity: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            self.0.replace(real(accumulated_quantity)?),
        )?))
    }
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    #[getter]
    fn daily_quantity(&self) -> f64 {
        self.0.daily_quantity()
    }
    #[getter]
    fn acceleration_factor(&self) -> f64 {
        self.0.acceleration_factor()
    }
    #[getter]
    fn accumulated_quantity(&self) -> f64 {
        self.0.accumulated_quantity()
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
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
