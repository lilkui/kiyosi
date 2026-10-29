use crate::error::domain;
use crate::instruments::asian::{ArithmeticAveragePriceOption, GeometricAveragePriceOption};
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
macro_rules! asian_engine {
    ($engine:ident, $option:ident) => {
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $engine;
        #[pymethods]
        impl $engine {
            #[new]
            fn new() -> Self {
                Self
            }
            fn price(
                &self,
                py: Python<'_>,
                instrument: &$option,
                context: &PricingContext,
            ) -> PyResult<f64> {
                let option = instrument.0.clone();
                let context = context.0.clone();
                domain(py, py.detach(|| core::$engine.price(&option, &context)))
            }
            #[pyo3(signature = (instrument, context, greeks=None, **settings))]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                instrument: &$option,
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
                        core::$engine.price_with_greeks(&option, &context, request, shifts)
                    }),
                )?))
            }
        }
    };
}
asian_engine!(
    AnalyticGeometricAveragePriceEngine,
    GeometricAveragePriceOption
);
asian_engine!(
    TurnbullWakemanArithmeticAveragePriceEngine,
    ArithmeticAveragePriceOption
);
