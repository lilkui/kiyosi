use crate::error::domain;
use crate::instruments::binary_barrier::BinaryInstrument;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticBinaryBarrierEngine;
#[pymethods]
impl AnalyticBinaryBarrierEngine {
    #[new]
    fn new() -> Self {
        Self
    }
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: BinaryInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let context = context.0.clone();
        match instrument {
            BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| core::AnalyticBinaryBarrierEngine.price(&option, &context)),
                )
            }
            BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| core::AnalyticBinaryBarrierEngine.price(&option, &context)),
                )
            }
        }
    }
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: BinaryInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let context = context.0.clone();
        let result = match instrument {
            BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                py.detach(|| {
                    core::AnalyticBinaryBarrierEngine
                        .price_with_greeks(&option, &context, request, shifts)
                })
            }
            BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                py.detach(|| {
                    core::AnalyticBinaryBarrierEngine
                        .price_with_greeks(&option, &context, request, shifts)
                })
            }
        };
        Ok(risk::PricingResult(domain(py, result)?))
    }
}
