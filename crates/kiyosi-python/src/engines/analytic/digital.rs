use crate::error::domain;
use crate::instruments::digital::DigitalInstrument;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticDigitalEngine;

#[pymethods]
impl AnalyticDigitalEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    fn price(
        &self,
        py: Python<'_>,
        instrument: DigitalInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        domain(
            py,
            core::AnalyticDigitalEngine.price(instrument.contract(), &context.0),
        )
    }

    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: DigitalInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        Ok(risk::PricingResult(domain(
            py,
            core::AnalyticDigitalEngine.price_with_greeks(
                instrument.contract(),
                &context.0,
                request,
                shifts,
            ),
        )?))
    }
}
