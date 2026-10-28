use crate::conversions::{date, direction, python_date, real};
use crate::market::PricingContext;
use crate::{analytics, core, error::domain};
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct EuropeanOption(pub(crate) core::EuropeanOption);

#[pymethods]
impl EuropeanOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date))]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let direction = direction(option_type)?;
        Ok(Self(domain(
            py,
            core::EuropeanOption::new(
                direction,
                real(strike)?,
                date(effective_date)?,
                date(expiry_date)?,
            ),
        )?))
    }

    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            core::OptionType::Call => "call",
            core::OptionType::Put => "put",
        }
    }
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticVanillaEngine;

#[pymethods]
impl AnalyticVanillaEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &EuropeanOption,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        Ok(analytics::PricingResult(domain(
            py,
            core::AnalyticVanillaEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request,
                shifts,
            ),
        )?))
    }

    fn price(
        &self,
        py: Python<'_>,
        instrument: &EuropeanOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        // Keep the GIL for the short scalar formula, matching the legacy analytic contract.
        domain(
            py,
            core::AnalyticVanillaEngine.price(&instrument.0, &context.0),
        )
    }
}
