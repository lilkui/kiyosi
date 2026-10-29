use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticEuropeanEngine;

#[pymethods]
impl AnalyticEuropeanEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        Ok(risk::PricingResult(domain(
            py,
            core::AnalyticEuropeanEngine.price_with_greeks(
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        // Keep the GIL for this short scalar formula.
        domain(
            py,
            core::AnalyticEuropeanEngine.price(&instrument.0, &context.0),
        )
    }
}
