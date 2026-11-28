use crate::calendar::run_date_scan;
use crate::conversions::{direction, real, timestamp};
use crate::instruments::barrier::{history, timing};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) fn kind(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::DoubleBarrierType> {
    match value.extract::<&str>()? {
        "knock_in" => Ok(kiyosi::DoubleBarrierType::KnockIn),
        "knock_out" => Ok(kiyosi::DoubleBarrierType::KnockOut),
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
///     Positive finite strike price.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// lower_barrier : float
///     Positive finite lower trigger; equality counts as a touch.
/// upper_barrier : float
///     Finite upper trigger, strictly above the lower trigger.
/// barrier_type : {'knock_in', 'knock_out'}
///     Knock-in or knock-out activation by either barrier.
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
///     If boundaries, strike, rebate, life times, monitoring schedule or
///     rebate timing are invalid. Calendar compatibility is checked on
///     construction; history is checked during pricing.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleBarrierOption(pub kiyosi::DoubleBarrierOption);

monitoring_properties!(
    DoubleBarrierOption,
    @replace DoubleBarrierOptionChanges, replace_double_barrier [option_type, strike, effective_time, expiry_time, lower_barrier, upper_barrier, barrier_type, rebate, rebate_timing, observation_schedule, touch_state, calendar] |self| |times| {
        calendar: crate::calendar::explicit_calendar(calendar)?,
        option_type: option_type.map(crate::conversions::direction).transpose()?,
        strike: strike.map(crate::conversions::real).transpose()?,
        effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
        expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
        lower_barrier: lower_barrier.map(crate::conversions::real).transpose()?,
        upper_barrier: upper_barrier.map(crate::conversions::real).transpose()?,
        barrier_type: barrier_type.map(crate::instruments::double_barrier::kind).transpose()?,
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
    /// lower_barrier, upper_barrier : float, optional
    ///     Positive finite lower and upper triggers, with the lower strictly smaller.
    /// barrier_type : {'knock_in', 'knock_out'}, optional
    ///     Knock-in or knock-out activation at either boundary.
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
    /// DoubleBarrierOption
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
    #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time, lower_barrier, upper_barrier, barrier_type, rebate=kiyosi::BarrierOptionTerms::DEFAULT_REBATE, rebate_timing=kiyosi::RebateTiming::default(), observation_schedule=None, touch_state=None, calendar=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        lower_barrier: &Bound<'_, PyAny>,
        upper_barrier: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] rebate: f64,
        #[pyo3(from_py_with = timing)] rebate_timing: kiyosi::RebateTiming,

        observation_schedule: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = history)] touch_state: Option<kiyosi::BarrierTouchState>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::DoubleBarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            real(lower_barrier)?,
            real(upper_barrier)?,
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
        run_date_scan(py, work, || kiyosi::binding_support::barrier_monitoring::new_double_barrier(terms, times)).map(Self)
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
    /// Return positive finite lower trigger; equality counts as a touch.
    #[getter]
    fn lower_barrier(&self) -> f64 {
        self.0.lower_barrier()
    },
    /// Return finite upper trigger, strictly above the lower trigger.
    #[getter]
    fn upper_barrier(&self) -> f64 {
        self.0.upper_barrier()
    },
    /// Return knock-in or knock-out activation by either barrier.
    #[getter]
    fn barrier_type(&self) -> &'static str {
        crate::conversions::double_barrier_type_name(self.0.barrier_type())
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
    }
);
