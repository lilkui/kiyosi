use super::*;

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

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureAccumulatorEngine(pub(crate) core::QuadratureAccumulatorEngine);

#[pymethods]
impl QuadratureAccumulatorEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["asset_step_count", "integration_standard_deviations"],
        )?;
        let mut settings = core::QuadratureSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("asset_step_count"))
            .transpose()?
            .flatten()
        {
            settings.asset_step_count = integer(&value)?;
        }
        if let Some(value) = fields
            .map(|f| f.get_item("integration_standard_deviations"))
            .transpose()?
            .flatten()
        {
            settings.integration_standard_deviations = real(&value)?;
        }
        Ok(Self(domain(
            py,
            core::QuadratureAccumulatorEngine::new(settings),
        )?))
    }
    #[getter]
    fn asset_step_count(&self) -> i32 {
        self.0.settings().asset_step_count
    }
    #[getter]
    fn integration_standard_deviations(&self) -> f64 {
        self.0.settings().integration_standard_deviations
    }
    fn __repr__(&self) -> String {
        format!(
            "QuadratureAccumulatorEngine(asset_step_count={}, integration_standard_deviations={})",
            self.asset_step_count(),
            self.integration_standard_deviations()
        )
    }
    fn price(
        &self,
        py: Python<'_>,
        instrument: &Accumulator,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    }
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &Accumulator,
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
