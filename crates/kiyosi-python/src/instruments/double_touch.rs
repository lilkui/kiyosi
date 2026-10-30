use crate::conversions::{check_changes, date, dates, python_date, real};
use crate::error::domain;
use crate::instruments::barrier;
use crate::schedule::ObservationSchedule;
use kiyosi_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

/// Cash one-touch or no-touch contract with two inclusive boundaries.
///
/// Notes
/// -----
/// Construct with cash_double_touch or cash_double_no_touch. Prior at-hit
/// payments have no remaining value.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleTouchOption(pub core::DoubleTouchOption);

#[pymethods]
impl DoubleTouchOption {
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
    /// Return the nonnegative finite cash payout.
    #[getter]
    fn payout(&self) -> f64 {
        self.0.payout()
    }
    /// Return whether touching activates payment.
    #[getter]
    fn is_one_touch(&self) -> bool {
        self.0.is_one_touch()
    }
    /// Return at-hit or expiry payment; no-touch contracts settle only at expiry.
    #[getter]
    fn settlement_timing(&self) -> &'static str {
        match self.0.settlement_timing() {
            core::SettlementTiming::AtHit => "at_hit",
            core::SettlementTiming::AtExpiry => "at_expiry",
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

macro_rules! double_touch_factory {
    ($name:ident, $kind:ident, $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, effective_date, expiry_date, lower_barrier, upper_barrier, payout, **settings))]
        pub(crate) fn $name(
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
double_touch_factory!(
    cash_double_touch,
    KnockIn,
    r"Create a cash payment activated by either inclusive boundary.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
lower_barrier : float
    Positive finite lower trigger; equality counts as a touch.
upper_barrier : float
    Finite upper trigger, strictly above the lower trigger.
payout : float
    Positive finite cash payment.
settlement_timing : {'at_hit', 'at_expiry'}, optional
    At-hit or expiry payment; no-touch contracts settle only at expiry.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied dates.
observation_dates : iterable of date, optional
    Strictly increasing observation dates within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.

Returns
-------
DoubleTouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. Prior at-hit
payments have no remaining value. No-touch payments occur only at
expiry."
);
double_touch_factory!(
    cash_double_no_touch,
    KnockOut,
    r"Create an expiry cash payment requiring neither boundary to be touched.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
lower_barrier : float
    Positive finite lower trigger; equality counts as a touch.
upper_barrier : float
    Finite upper trigger, strictly above the lower trigger.
payout : float
    Positive finite cash payment.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied dates.
observation_dates : iterable of date, optional
    Strictly increasing observation dates within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.

Returns
-------
DoubleTouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. Prior at-hit
payments have no remaining value. No-touch payments occur only at
expiry."
);
