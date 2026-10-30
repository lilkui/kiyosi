use crate::conversions::{check_changes, date, dates, direction, python_date, real};
use crate::error::domain;
use crate::instruments::barrier::{history, mode, timing};
use crate::schedule::ObservationSchedule;
use kiyosi_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

fn kind(value: &Bound<'_, PyAny>) -> PyResult<core::DoubleBarrierType> {
    match value.extract::<&str>()? {
        "knock_in" => Ok(core::DoubleBarrierType::KnockIn),
        "knock_out" => Ok(core::DoubleBarrierType::KnockOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
    }
}

/// Vanilla option activated or terminated by either inclusive boundary.
///
/// Parameters
/// ----------
/// option_type : {'call', 'put'}
///     Call or put payoff direction.
/// strike : float
///     Positive finite exercise or purchase price.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// lower_barrier : float
///     Positive finite lower trigger; equality counts as a touch.
/// upper_barrier : float
///     Finite upper trigger, strictly above the lower trigger.
/// barrier_type : {'knock_in', 'knock_out'}
///     Trigger direction and knock-in or knock-out behavior.
/// rebate : float, optional
///     Nonnegative finite cash rebate; defaults to zero.
/// rebate_timing : {'at_hit', 'at_expiry'}, optional
///     Payment timing; at-hit rebates are valid only for knock-outs.
/// observation_mode : {'continuous', 'scheduled'}, optional
///     Continuous monitoring or observations on the supplied dates.
/// observation_dates : iterable of date, optional
///     Strictly increasing observation dates within the contract life.
/// touch_state : {'untouched', 'touched'} or None, optional
///     Strictly prior history. None means unknown; untouched means known no
///     hit.
///
/// Raises
/// ------
/// InvalidInputError
///     If boundaries, strike, rebate, life dates, monitoring schedule or
///     rebate timing are invalid. Calendar and history checks occur at pricing.
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
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Strictly prior history. None means unknown; untouched means known no
    ///     hit.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields retain their values.
    ///
    /// Raises
    /// ------
    /// TypeError
    ///     If a field is unknown or touch_state is neither a string nor None.
    /// ValueError
    ///     If touch_state is an unknown choice.
    #[pyo3(signature = (**fields))]
    fn replace(&self, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(fields, &["touch_state"])?;
        let state = match fields
            .map(|f| f.get_item("touch_state"))
            .transpose()?
            .flatten()
        {
            Some(value) => history(&value)?,
            None => self.0.touch_state(),
        };
        Ok(Self(self.0.replace(state)))
    }
    /// Return call or put payoff direction.
    #[getter]
    fn option_type(&self) -> &'static str {
        if self.0.option_type() == core::OptionType::Call {
            "call"
        } else {
            "put"
        }
    }
    /// Return the exercise or purchase price in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return inclusive first contract date.
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    /// Return final contract date; expiry is at midnight UTC.
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
    /// Return positive finite lower trigger; equality counts as a touch.
    #[getter]
    fn lower_barrier(&self) -> f64 {
        self.0.lower_barrier()
    }
    /// Return finite upper trigger, strictly above the lower trigger.
    #[getter]
    fn upper_barrier(&self) -> f64 {
        self.0.upper_barrier()
    }
    /// Return trigger direction and knock-in or knock-out behavior.
    #[getter]
    fn barrier_type(&self) -> &'static str {
        match self.0.barrier_type() {
            core::DoubleBarrierType::KnockIn => "knock_in",
            core::DoubleBarrierType::KnockOut => "knock_out",
        }
    }
    /// Return nonnegative finite cash rebate; defaults to zero.
    #[getter]
    fn rebate(&self) -> f64 {
        self.0.rebate()
    }
    /// Return payment timing; at-hit rebates are valid only for knock-outs.
    #[getter]
    fn rebate_timing(&self) -> &'static str {
        match self.0.rebate_timing() {
            core::RebateTiming::AtHit => "at_hit",
            core::RebateTiming::AtExpiry => "at_expiry",
        }
    }
    /// Return continuous monitoring or observations on the supplied dates.
    #[getter]
    fn observation_mode(&self) -> &'static str {
        match self.0.observation_mode() {
            core::ObservationMode::Continuous => "continuous",
            core::ObservationMode::Scheduled => "scheduled",
        }
    }
    /// Return an owned copy of the monitoring schedule.
    #[getter]
    fn observation_schedule(&self) -> ObservationSchedule {
        ObservationSchedule(self.0.observation_schedule().clone())
    }
    /// Return a copy of the contractual observation dates.
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observation_dates()
            .iter()
            .map(|&d| python_date(py, d))
            .collect()
    }
    /// Return strictly prior touch history, or None if unknown.
    #[getter]
    fn touch_state(&self) -> Option<&'static str> {
        self.0.touch_state().map(|s| match s {
            core::BarrierTouchState::Untouched => "untouched",
            core::BarrierTouchState::Touched => "touched",
        })
    }
}
