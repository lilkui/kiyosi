use super::*;

macro_rules! sampling_settings {
    ($engine:ident, $($methods:item),* $(,)?) => {
        #[pymethods]
        impl $engine {
            #[new]
            #[pyo3(signature = (**fields))]
            fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                check_changes(
                    fields,
                    &[
                        "path_count",
                        "step_count",
                        "seed",
                        "thread_count",
                        "sampling",
                        "antithetic",
                    ],
                )?;
                let mut settings = core::MonteCarloSettings::default();
                if let Some(fields) = fields {
                    if let Some(value) = fields.get_item("path_count")? {
                        settings.path_count = integer(&value)?;
                    }
                    if let Some(value) = fields.get_item("step_count")? {
                        settings.step_count = integer(&value)?;
                    }
                    if let Some(value) = fields.get_item("thread_count")? {
                        settings.thread_count = integer(&value)?;
                    }
                    if let Some(value) = fields.get_item("seed")?.filter(|value| !value.is_none()) {
                        check_integer(&value)?;
                        settings.seed = Some(value.extract::<u64>()?);
                    }
                    if let Some(value) = fields.get_item("sampling")? {
                        settings.sampling = match value.extract::<&str>()? {
                            "pseudo_random" => core::MonteCarloSampling::PseudoRandom,
                            "sobol" => core::MonteCarloSampling::Sobol,
                            _ => return Err(PyValueError::new_err("unknown Monte Carlo sampling")),
                        };
                    }
                    if let Some(value) = fields.get_item("antithetic")? {
                        settings.antithetic = value.cast::<PyBool>()?.extract()?;
                    }
                }
                Ok(Self(domain(py, core::$engine::new(settings))?))
            }

            #[getter]
            fn path_count(&self) -> i32 {
                self.0.settings().path_count
            }
            #[getter]
            fn step_count(&self) -> i32 {
                self.0.settings().step_count
            }
            #[getter]
            fn seed(&self) -> Option<u64> {
                self.0.settings().seed
            }
            #[getter]
            fn thread_count(&self) -> i32 {
                self.0.settings().thread_count
            }
            #[getter]
            fn sampling(&self) -> &'static str {
                match self.0.settings().sampling {
                    core::MonteCarloSampling::PseudoRandom => "pseudo_random",
                    core::MonteCarloSampling::Sobol => "sobol",
                }
            }
            #[getter]
            fn antithetic(&self) -> bool {
                self.0.settings().antithetic
            }
            $($methods)*
        }
    };
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloVanillaEngine(pub(crate) core::MonteCarloVanillaEngine);

sampling_settings!(
    MonteCarloVanillaEngine,
    fn price(
        &self,
        py: Python<'_>,
        instrument: binomial::VanillaInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        match instrument {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
        }
    },
    fn price_with_statistics(
        &self,
        py: Python<'_>,
        instrument: &EuropeanOption,
        context: &PricingContext,
    ) -> PyResult<MonteCarloEstimate> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(MonteCarloEstimate(domain(
            py,
            py.detach(|| engine.price_with_statistics(&option, &context)),
        )?))
    },
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: binomial::VanillaInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let engine = self.0;
        let context = context.0.clone();
        let result = match instrument {
            binomial::VanillaInstrument::European(option) => {
                let shifts = analytics::shifts(settings)?;
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let default = core::NumericalShiftSettings {
                    spot_shift: core::MonteCarloVanillaEngine::default_american_spot_shift(
                        &context,
                    ),
                    ..Default::default()
                };
                let fields = settings
                    .map(Bound::copy)
                    .transpose()?
                    .unwrap_or_else(|| PyDict::new(py));
                if fields
                    .get_item("spot_shift")?
                    .is_some_and(|value| value.is_none())
                {
                    fields.del_item("spot_shift")?;
                }
                let shifts = analytics::shifts_with_defaults(Some(&fields), default)?;
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
        }?;
        Ok(analytics::PricingResult(result))
    }
);

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloEstimate(core::MonteCarloEstimate);

#[pymethods]
impl MonteCarloEstimate {
    #[getter]
    fn price(&self) -> f64 {
        self.0.price()
    }
    #[getter]
    fn standard_error(&self) -> Option<f64> {
        self.0.standard_error()
    }
    #[getter]
    fn confidence_interval(&self) -> Option<(f64, f64)> {
        self.0.confidence_interval()
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloDigitalEngine(pub(crate) core::MonteCarloDigitalEngine);

sampling_settings!(
    MonteCarloDigitalEngine,
    fn price(
        &self,
        py: Python<'_>,
        instrument: digital::DigitalInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.contract().clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: digital::DigitalInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let engine = self.0;
        let option = instrument.contract().clone();
        let context = context.0.clone();
        Ok(analytics::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAsianEngine(pub(crate) core::MonteCarloAsianEngine);

sampling_settings!(
    MonteCarloAsianEngine,
    fn price(
        &self,
        py: Python<'_>,
        instrument: asian::AsianInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        match instrument {
            asian::AsianInstrument::Geometric(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
        }
    },
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: asian::AsianInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let engine = self.0;
        let context = context.0.clone();
        let result = match instrument {
            asian::AsianInstrument::Geometric(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
        }?;
        Ok(analytics::PricingResult(result))
    }
);

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloBarrierEngine(pub(crate) core::MonteCarloBarrierEngine);

sampling_settings!(
    MonteCarloBarrierEngine,
    fn price(
        &self,
        py: Python<'_>,
        instrument: &barrier::BarrierOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &barrier::BarrierOption,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(analytics::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloDoubleBarrierEngine(pub(crate) core::MonteCarloDoubleBarrierEngine);

sampling_settings!(
    MonteCarloDoubleBarrierEngine,
    fn price(
        &self,
        py: Python<'_>,
        instrument: &double_barrier::DoubleBarrierOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &double_barrier::DoubleBarrierOption,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(analytics::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAutocallableEngine(pub(crate) core::MonteCarloAutocallableEngine);

sampling_settings!(
    MonteCarloAutocallableEngine,
    fn price(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        autocallable::with_note!(
            instrument,
            note,
            domain(py, py.detach(|| engine.price(&note, &context)))
        )
    },
    fn price_with_statistics(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
    ) -> PyResult<MonteCarloEstimate> {
        let engine = self.0;
        let context = context.0.clone();
        autocallable::with_note!(
            instrument,
            note,
            Ok(MonteCarloEstimate(domain(
                py,
                py.detach(|| engine.price_with_statistics(&note, &context))
            )?))
        )
    },
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let engine = self.0;
        let context = context.0.clone();
        autocallable::with_note!(
            instrument,
            note,
            Ok(analytics::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&note, &context, request, shifts))
            )?))
        )
    }
);
