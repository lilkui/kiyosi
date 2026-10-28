use super::*;

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloVanillaEngine(pub(crate) core::MonteCarloVanillaEngine);

#[pymethods]
impl MonteCarloVanillaEngine {
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
        Ok(Self(domain(
            py,
            core::MonteCarloVanillaEngine::new(settings),
        )?))
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

    fn price(
        &self,
        py: Python<'_>,
        instrument: &EuropeanOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    }

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
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(analytics::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
}

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
