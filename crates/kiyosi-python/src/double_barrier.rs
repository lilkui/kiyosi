use super::barrier::{history, mode, timing};
use super::*;
use crate::schedule::ObservationSchedule;

fn kind(value: &Bound<'_, PyAny>) -> PyResult<core::DoubleBarrierType> {
    match value.extract::<&str>()? {
        "knock_in" => Ok(core::DoubleBarrierType::KnockIn),
        "knock_out" => Ok(core::DoubleBarrierType::KnockOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
    }
}

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleBarrierOption(pub core::DoubleBarrierOption);

#[pymethods]
impl DoubleBarrierOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date, lower_barrier, upper_barrier, barrier_type, **settings))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        lower_barrier: &Bound<'_, PyAny>,
        upper_barrier: &Bound<'_, PyAny>,
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
        let mut terms = core::DoubleBarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            real(lower_barrier)?,
            real(upper_barrier)?,
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
        Ok(Self(domain(py, core::DoubleBarrierOption::new(terms))?))
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
    fn lower_barrier(&self) -> f64 {
        self.0.lower_barrier()
    }
    #[getter]
    fn upper_barrier(&self) -> f64 {
        self.0.upper_barrier()
    }
    #[getter]
    fn barrier_type(&self) -> &'static str {
        match self.0.barrier_type() {
            core::DoubleBarrierType::KnockIn => "knock_in",
            core::DoubleBarrierType::KnockOut => "knock_out",
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
    fn touch_state(&self) -> Option<&'static str> {
        self.0.touch_state().map(|s| match s {
            core::BarrierTouchState::Untouched => "untouched",
            core::BarrierTouchState::Touched => "touched",
        })
    }
}

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
        instrument: &DoubleBarrierOption,
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
        instrument: &DoubleBarrierOption,
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
                core::AnalyticDoubleBarrierEngine
                    .price_with_greeks(&option, &context, request, shifts)
            }),
        )?))
    }
}
