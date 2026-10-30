use crate::conversions::{check_changes, date, dates, direction, python_date, real};
use crate::error::domain;
use crate::schedule::ObservationSchedule;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

pub(crate) fn kind(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::BarrierType> {
    match value.extract::<&str>()? {
        "up_and_in" => Ok(kiyosi::BarrierType::UpAndIn),
        "up_and_out" => Ok(kiyosi::BarrierType::UpAndOut),
        "down_and_in" => Ok(kiyosi::BarrierType::DownAndIn),
        "down_and_out" => Ok(kiyosi::BarrierType::DownAndOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
    }
}
pub(crate) fn mode(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::ObservationMode> {
    match value.extract::<&str>()? {
        "continuous" => Ok(kiyosi::ObservationMode::Continuous),
        "scheduled" => Ok(kiyosi::ObservationMode::Scheduled),
        _ => Err(PyValueError::new_err("unknown observation_mode")),
    }
}
pub(crate) fn timing(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::RebateTiming> {
    match value.extract::<&str>()? {
        "at_hit" => Ok(kiyosi::RebateTiming::AtHit),
        "at_expiry" => Ok(kiyosi::RebateTiming::AtExpiry),
        _ => Err(PyValueError::new_err("unknown rebate_timing")),
    }
}
pub(crate) fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<kiyosi::BarrierTouchState>> {
    if value.is_none() {
        return Ok(None);
    }
    match value.extract::<&str>()? {
        "untouched" => Ok(Some(kiyosi::BarrierTouchState::Untouched)),
        "touched" => Ok(Some(kiyosi::BarrierTouchState::Touched)),
        _ => Err(PyValueError::new_err("unknown touch_state")),
    }
}

/// Single-barrier vanilla option with inclusive barrier touches.
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
/// barrier_level : float
///     Positive finite trigger level; equality counts as a touch.
/// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
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
///     If levels, rebate, life dates, monitoring schedule or rebate timing
///     are invalid. Trading-day and valuation-history checks occur at pricing.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BarrierOption(pub kiyosi::BarrierOption);

#[pymethods]
impl BarrierOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date, barrier_level, barrier_type, rebate=kiyosi::BarrierOptionTerms::DEFAULT_REBATE, rebate_timing=kiyosi::RebateTiming::default(), observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        barrier_level: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] rebate: f64,
        #[pyo3(from_py_with = timing)] rebate_timing: kiyosi::RebateTiming,
        #[pyo3(from_py_with = mode)] observation_mode: kiyosi::ObservationMode,
        #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
        #[pyo3(from_py_with = history)] touch_state: Option<kiyosi::BarrierTouchState>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::BarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            real(barrier_level)?,
            kind(barrier_type)?,
        );
        terms.rebate = rebate;
        terms.rebate_timing = rebate_timing;
        terms.observation_mode = observation_mode;
        terms.observation_dates = observation_dates;
        terms.touch_state = touch_state;
        Ok(Self(domain(py, kiyosi::BarrierOption::new(terms))?))
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
        if self.0.option_type() == kiyosi::OptionType::Call {
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
    /// Return positive finite trigger level; equality counts as a touch.
    #[getter]
    fn barrier_level(&self) -> f64 {
        self.0.barrier_level()
    }
    /// Return trigger direction and knock-in or knock-out behavior.
    #[getter]
    fn barrier_type(&self) -> &'static str {
        match self.0.barrier_type() {
            kiyosi::BarrierType::UpAndIn => "up_and_in",
            kiyosi::BarrierType::UpAndOut => "up_and_out",
            kiyosi::BarrierType::DownAndIn => "down_and_in",
            kiyosi::BarrierType::DownAndOut => "down_and_out",
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
            kiyosi::RebateTiming::AtHit => "at_hit",
            kiyosi::RebateTiming::AtExpiry => "at_expiry",
        }
    }
    /// Return continuous monitoring or observations on the supplied dates.
    #[getter]
    fn observation_mode(&self) -> &'static str {
        match self.0.observation_mode() {
            kiyosi::ObservationMode::Continuous => "continuous",
            kiyosi::ObservationMode::Scheduled => "scheduled",
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
    /// Return mean fixing spacing in Actual/365 Fixed years.
    #[getter]
    fn mean_observation_year_fraction(&self) -> f64 {
        self.0.mean_observation_year_fraction()
    }
    /// Return strictly prior touch history, or None if unknown.
    #[getter]
    fn touch_state(&self) -> Option<&'static str> {
        self.0.touch_state().map(|s| match s {
            kiyosi::BarrierTouchState::Untouched => "untouched",
            kiyosi::BarrierTouchState::Touched => "touched",
        })
    }
}
