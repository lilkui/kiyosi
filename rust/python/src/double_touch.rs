use super::*;

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleTouchOption(pub core::DoubleTouchOption);

#[pymethods]
impl DoubleTouchOption {
    #[pyo3(signature = (**fields))]
    fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(fields, &["touch_state"])?;
        let state = match fields
            .map(|f| f.get_item("touch_state"))
            .transpose()?
            .flatten()
        {
            Some(value) => barrier::history(&value)?,
            None => self.0.touch_state(),
        };
        Ok(Self(domain(py, self.0.replace(state))?))
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
    fn lower_barrier(&self) -> f64 {
        self.0.lower_barrier()
    }
    #[getter]
    fn upper_barrier(&self) -> f64 {
        self.0.upper_barrier()
    }
    #[getter]
    fn payout(&self) -> f64 {
        self.0.payout()
    }
    #[getter]
    fn is_one_touch(&self) -> bool {
        self.0.is_one_touch()
    }
    #[getter]
    fn settlement_timing(&self) -> &'static str {
        match self.0.settlement_timing() {
            core::SettlementTiming::AtHit => "at_hit",
            core::SettlementTiming::AtExpiry => "at_expiry",
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
    fn touch_state(&self) -> Option<&'static str> {
        self.0.touch_state().map(|s| match s {
            core::BarrierTouchState::Untouched => "untouched",
            core::BarrierTouchState::Touched => "touched",
        })
    }
}

macro_rules! double_touch_factory {
    ($name:ident, $kind:ident) => {
        #[pyfunction]
        #[pyo3(signature = (*, effective_date, expiry_date, lower_barrier, upper_barrier, payout, **settings))]
        fn $name(
            py: Python<'_>, effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            lower_barrier: &Bound<'_, PyAny>, upper_barrier: &Bound<'_, PyAny>, payout: &Bound<'_, PyAny>,
            settings: Option<&Bound<'_, PyDict>>,
        ) -> PyResult<DoubleTouchOption> {
            let kind = core::DoubleBarrierType::$kind;
            check_changes(settings, if kind == core::DoubleBarrierType::KnockIn {
                &["settlement_timing", "observation_mode", "observation_dates", "touch_state"]
            } else { &["observation_mode", "observation_dates", "touch_state"] })?;
            let mut terms = core::DoubleTouchOptionTerms::new(date(effective_date)?, date(expiry_date)?, real(lower_barrier)?, real(upper_barrier)?, kind);
            let payout = real(payout)?;
            if let Some(settings) = settings {
                if let Some(v) = settings.get_item("settlement_timing")? {
                    terms.settlement_timing = match v.extract::<&str>()? {
                        "at_hit" => core::SettlementTiming::AtHit,
                        "at_expiry" => core::SettlementTiming::AtExpiry,
                        _ => return Err(PyValueError::new_err("unknown settlement_timing")),
                    };
                }
                if let Some(v) = settings.get_item("observation_mode")? { terms.observation_mode = barrier::mode(&v)?; }
                if let Some(v) = settings.get_item("observation_dates")? { terms.observation_dates = dates(&v)?; }
                if let Some(v) = settings.get_item("touch_state")? { terms.touch_state = barrier::history(&v)?; }
            }
            Ok(DoubleTouchOption(domain(py, core::DoubleTouchOption::cash(terms, payout))?))
        }
    };
}
double_touch_factory!(cash_double_touch, KnockIn);
double_touch_factory!(cash_double_no_touch, KnockOut);

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<DoubleTouchOption>()?;
    module.add_function(wrap_pyfunction!(cash_double_touch, module)?)?;
    module.add_function(wrap_pyfunction!(cash_double_no_touch, module)?)?;
    Ok(())
}
