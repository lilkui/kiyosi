use crate::error::domain;
use crate::instruments::double_barrier::DoubleBarrierOption;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticDoubleBarrierEngine;

#[pymethods]
impl AnalyticDoubleBarrierEngine {
    #[new]
    fn new() -> Self {
        Self
    }
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            DoubleBarrierOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(
            py,
            py.detach(|| core::AnalyticDoubleBarrierEngine.price(&option, &context)),
        )
    }
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            DoubleBarrierOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| {
                core::AnalyticDoubleBarrierEngine
                    .price_with_greeks(&option, &context, request, shifts)
            }),
        )?))
    }
}
