use crate::calendar::run_date_scan;
use crate::conversions::{direction, real, timestamp};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) fn kind(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::BarrierType> {
    match value.extract::<&str>()? {
        "up_and_in" => Ok(kiyosi::BarrierType::UpAndIn),
        "up_and_out" => Ok(kiyosi::BarrierType::UpAndOut),
        "down_and_in" => Ok(kiyosi::BarrierType::DownAndIn),
        "down_and_out" => Ok(kiyosi::BarrierType::DownAndOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
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
///     Positive finite strike price.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// barrier_level : float
///     Positive finite trigger level; equality counts as a touch.
/// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
///     Trigger direction and knock-in or knock-out behavior.
/// rebate : float, optional
///     Nonnegative finite cash rebate; defaults to zero.
/// rebate_timing : {'at_hit', 'at_expiry'}, optional
///     Payment timing; at-hit rebates are valid only for knock-outs.
/// observation_schedule : ObservationSchedule or iterable of datetime.datetime or None, optional
///     Nonempty, strictly increasing times within life; None selects continuous monitoring.
/// touch_state : {'untouched', 'touched'} or None, optional
///     Strictly prior history. None means unknown; untouched means known no
///     hit.
/// calendar : TradingCalendar, optional
///     Explicit override. Omission or Ellipsis inherits a supplied
///     ObservationSchedule calendar. Otherwise, construction defaults to
///     weekdays and replacement defaults to the instance's calendar.
///
/// Raises
/// ------
/// InvalidInputError
///     If levels, rebate, life times, monitoring schedule, calendar or rebate
///     timing are invalid. Trading-day compatibility is checked on
///     construction; valuation-history checks occur at pricing.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BarrierOption(pub kiyosi::BarrierOption);

monitoring_properties!(
    BarrierOption,
    @replace BarrierOptionChanges, replace_barrier [option_type, strike, effective_time, expiry_time, barrier_level, barrier_type, rebate, rebate_timing, observation_schedule, touch_state, calendar] |self| |times| {
        calendar: crate::calendar::explicit_calendar(calendar)?,
        option_type: option_type.map(crate::conversions::direction).transpose()?,
        strike: strike.map(crate::conversions::real).transpose()?,
        effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
        expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
        barrier_level: barrier_level.map(crate::conversions::real).transpose()?,
        barrier_type: barrier_type.map(crate::instruments::barrier::kind).transpose()?,
        rebate: rebate.map(crate::conversions::real).transpose()?,
        rebate_timing: rebate_timing.map(crate::instruments::barrier::timing).transpose()?,
        observation_schedule: {
            let (schedule, raw_times) = crate::conversions::monitoring_schedule(observation_schedule)?;
            times = raw_times;
            schedule
        },
        touch_state: touch_state.map(crate::instruments::barrier::history).transpose()?,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omitted fields and fields set to Ellipsis default to the instance's values.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Call or put payoff direction.
    /// strike : float, optional
    ///     Positive finite strike price.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// barrier_level : float, optional
    ///     Positive finite trigger level; equality counts as a touch.
    /// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
    ///     Barrier direction and knock-in or knock-out behavior.
    /// rebate : float, optional
    ///     Nonnegative finite cash rebate.
    /// rebate_timing : {'at_hit', 'at_expiry'}, optional
    ///     Rebate payment timing; at-hit rebates require knock-out.
    /// observation_schedule : ObservationSchedule or iterable of datetime.datetime or None, optional
    ///     The instance's monitoring is the default. None selects continuous monitoring.
    ///     A nonempty iterable selects scheduled monitoring.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Strictly prior touch history; None clears it to unknown.
    /// calendar : TradingCalendar, optional
    ///     Explicit override. Omission or Ellipsis inherits a supplied
    ///     ObservationSchedule calendar. Otherwise, construction defaults to
    ///     weekdays and replacement defaults to the instance's calendar.
    ///
    /// Returns
    /// -------
    /// BarrierOption
    ///     Validated copy of the same class.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid.
    ///
    ],
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time, barrier_level, barrier_type, rebate=kiyosi::BarrierOptionTerms::DEFAULT_REBATE, rebate_timing=kiyosi::RebateTiming::default(), observation_schedule=None, touch_state=None, calendar=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        barrier_level: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] rebate: f64,
        #[pyo3(from_py_with = timing)] rebate_timing: kiyosi::RebateTiming,

        observation_schedule: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = history)] touch_state: Option<kiyosi::BarrierTouchState>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::BarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            real(barrier_level)?,
            kind(barrier_type)?,
        );
        terms.rebate = rebate;
        terms.rebate_timing = rebate_timing;
        let (schedule, times) = crate::conversions::monitoring_schedule(observation_schedule)?;
        terms.observation_schedule = schedule.flatten();
        terms.touch_state = touch_state;
        terms.calendar = crate::calendar::explicit_calendar(calendar)?;
        let work = super::monitoring_work(
            terms.observation_schedule.as_ref(),
            times.as_deref(),
            terms.calendar.as_ref(),
        );
        run_date_scan(py, work, || kiyosi::binding_support::barrier_monitoring::new_barrier(terms, times)).map(Self)
    },
    /// Return call or put payoff direction.
    #[getter]
    fn option_type(&self) -> &'static str {
        crate::conversions::direction_name(self.0.option_type())
    },
    /// Return the strike price in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    },
    /// Return positive finite trigger level; equality counts as a touch.
    #[getter]
    fn barrier_level(&self) -> f64 {
        self.0.barrier_level()
    },
    /// Return trigger direction and knock-in or knock-out behavior.
    #[getter]
    fn barrier_type(&self) -> &'static str {
        crate::conversions::barrier_type_name(self.0.barrier_type())
    },
    /// Return nonnegative finite cash rebate; defaults to zero.
    #[getter]
    fn rebate(&self) -> f64 {
        self.0.rebate()
    },
    /// Return payment timing; at-hit rebates are valid only for knock-outs.
    #[getter]
    fn rebate_timing(&self) -> &'static str {
        crate::conversions::rebate_timing_name(self.0.rebate_timing())
    },
);
