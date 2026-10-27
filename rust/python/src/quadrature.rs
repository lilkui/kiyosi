use super::*;

macro_rules! quadrature_engine {
    ($engine:ident, $instrument:ty, |$option:ident| $contract:expr) => {
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
                instrument: $instrument,
                context: &PricingContext,
            ) -> PyResult<f64> {
                let $option = instrument;
                let option = $contract.clone();
                let context = context.0.clone();
                domain(py, py.detach(|| core::$engine.price(&option, &context)))
            }

            #[pyo3(signature = (instrument, context, greeks=None, **settings))]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                settings: Option<&Bound<'_, PyDict>>,
            ) -> PyResult<analytics::PricingResult> {
                let request = analytics::request(greeks, settings)?;
                let shifts = analytics::shifts(settings)?;
                let $option = instrument;
                let option = $contract.clone();
                let context = context.0.clone();
                Ok(analytics::PricingResult(domain(
                    py,
                    py.detach(|| {
                        core::$engine.price_with_greeks(&option, &context, request, shifts)
                    }),
                )?))
            }
        }
    };
}

quadrature_engine!(
    QuadratureVanillaEngine,
    PyRef<'_, EuropeanOption>,
    |option| option.0
);
quadrature_engine!(
    QuadratureDigitalEngine,
    digital::DigitalInstrument<'_>,
    |option| option.contract()
);
