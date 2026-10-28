use super::*;

pub(super) fn kind(value: &Bound<'_, PyAny>) -> PyResult<core::BarrierType> {
    match value.extract::<&str>()? {
        "up_and_in" => Ok(core::BarrierType::UpAndIn),
        "up_and_out" => Ok(core::BarrierType::UpAndOut),
        "down_and_in" => Ok(core::BarrierType::DownAndIn),
        "down_and_out" => Ok(core::BarrierType::DownAndOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
    }
}
pub(super) fn mode(value: &Bound<'_, PyAny>) -> PyResult<core::ObservationMode> {
    match value.extract::<&str>()? {
        "continuous" => Ok(core::ObservationMode::Continuous),
        "scheduled" => Ok(core::ObservationMode::Scheduled),
        _ => Err(PyValueError::new_err("unknown observation_mode")),
    }
}
pub(super) fn timing(value: &Bound<'_, PyAny>) -> PyResult<core::RebateTiming> {
    match value.extract::<&str>()? {
        "at_hit" => Ok(core::RebateTiming::AtHit),
        "at_expiry" => Ok(core::RebateTiming::AtExpiry),
        _ => Err(PyValueError::new_err("unknown rebate_timing")),
    }
}
pub(super) fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<core::BarrierTouchState>> {
    if value.is_none() {
        return Ok(None);
    }
    match value.extract::<&str>()? {
        "untouched" => Ok(Some(core::BarrierTouchState::Untouched)),
        "touched" => Ok(Some(core::BarrierTouchState::Touched)),
        _ => Err(PyValueError::new_err("unknown touch_state")),
    }
}

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BarrierOption(pub core::BarrierOption);

#[pymethods]
impl BarrierOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date, barrier_level, barrier_type, **settings))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        barrier_level: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        check_changes(
            settings,
            &[
                "rebate",
                "rebate_timing",
                "observation_mode",
                "observation_dates",
                "touch_state",
            ],
        )?;
        let mut terms = core::BarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            real(barrier_level)?,
            kind(barrier_type)?,
        );
        if let Some(settings) = settings {
            if let Some(v) = settings.get_item("rebate")? {
                terms.rebate = real(&v)?;
            }
            if let Some(v) = settings.get_item("rebate_timing")? {
                terms.rebate_timing = timing(&v)?;
            }
            if let Some(v) = settings.get_item("observation_mode")? {
                terms.observation_mode = mode(&v)?;
            }
            if let Some(v) = settings.get_item("observation_dates")? {
                terms.observation_dates = dates(&v)?;
            }
            if let Some(v) = settings.get_item("touch_state")? {
                terms.touch_state = history(&v)?;
            }
        }
        Ok(Self(domain(py, core::BarrierOption::new(terms))?))
    }
    #[pyo3(signature = (**fields))]
    fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(fields, &["touch_state"])?;
        let state = match fields
            .map(|f| f.get_item("touch_state"))
            .transpose()?
            .flatten()
        {
            Some(value) => history(&value)?,
            None => self.0.touch_state(),
        };
        Ok(Self(domain(py, self.0.replace(state))?))
    }
    #[getter]
    fn option_type(&self) -> &'static str {
        if self.0.option_type() == core::OptionType::Call {
            "call"
        } else {
            "put"
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
    #[getter]
    fn barrier_level(&self) -> f64 {
        self.0.barrier_level()
    }
    #[getter]
    fn barrier_type(&self) -> &'static str {
        match self.0.barrier_type() {
            core::BarrierType::UpAndIn => "up_and_in",
            core::BarrierType::UpAndOut => "up_and_out",
            core::BarrierType::DownAndIn => "down_and_in",
            core::BarrierType::DownAndOut => "down_and_out",
        }
    }
    #[getter]
    fn rebate(&self) -> f64 {
        self.0.rebate()
    }
    #[getter]
    fn rebate_timing(&self) -> &'static str {
        match self.0.rebate_timing() {
            core::RebateTiming::AtHit => "at_hit",
            core::RebateTiming::AtExpiry => "at_expiry",
        }
    }
    #[getter]
    fn observation_mode(&self) -> &'static str {
        match self.0.observation_mode() {
            core::ObservationMode::Continuous => "continuous",
            core::ObservationMode::Scheduled => "scheduled",
        }
    }
    #[getter]
    fn observation_schedule(&self) -> ObservationSchedule {
        ObservationSchedule(self.0.observation_schedule().clone())
    }
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observation_dates()
            .iter()
            .map(|&d| python_date(py, d))
            .collect()
    }
    #[getter]
    fn mean_observation_year_fraction(&self) -> f64 {
        self.0.mean_observation_year_fraction()
    }
    #[getter]
    fn touch_state(&self) -> Option<&'static str> {
        self.0.touch_state().map(|s| match s {
            core::BarrierTouchState::Untouched => "untouched",
            core::BarrierTouchState::Touched => "touched",
        })
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticBarrierEngine;

#[pymethods]
impl AnalyticBarrierEngine {
    #[new]
    fn new() -> Self {
        Self
    }
    fn price(
        &self,
        py: Python<'_>,
        instrument: &BarrierOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(
            py,
            py.detach(|| core::AnalyticBarrierEngine.price(&option, &context)),
        )
    }
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &BarrierOption,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(analytics::PricingResult(domain(
            py,
            py.detach(|| {
                core::AnalyticBarrierEngine.price_with_greeks(&option, &context, request, shifts)
            }),
        )?))
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct CoxRossRubinsteinBarrierEngine(pub(crate) core::CoxRossRubinsteinBarrierEngine);

#[pymethods]
impl CoxRossRubinsteinBarrierEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(fields, &["step_count"])?;
        let mut settings = core::BinomialSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("step_count"))
            .transpose()?
            .flatten()
        {
            settings.step_count = integer(&value)?;
        }
        Ok(Self(domain(
            py,
            core::CoxRossRubinsteinBarrierEngine::new(settings),
        )?))
    }
    #[getter]
    fn step_count(&self) -> i32 {
        self.0.settings().step_count
    }
    fn __repr__(&self) -> String {
        format!(
            "CoxRossRubinsteinBarrierEngine(step_count={})",
            self.step_count()
        )
    }
    fn price(
        &self,
        py: Python<'_>,
        instrument: &BarrierOption,
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
        instrument: &BarrierOption,
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
