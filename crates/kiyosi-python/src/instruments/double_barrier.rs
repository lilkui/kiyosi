use crate::conversions::{direction, real, timestamp};
use crate::error::domain;
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
/// observation_times : iterable of datetime.datetime or None, optional
///     Nonempty, strictly increasing times within life; None selects continuous monitoring.
/// touch_state : {'untouched', 'touched'} or None, optional
///     Strictly prior history. None means unknown; untouched means known no
///     hit.
/// calendar : TradingCalendar, optional
///     Explicit override. Omission or Ellipsis inherits a supplied
///     ObservationSchedule calendar; otherwise construction uses weekdays
///     and replacement retains the current calendar.
/// history_time : datetime.datetime or None, optional
///     Timestamp of supplied history, covering events strictly before it.
///     Required with history; refresh payload and timestamp together.
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
    DoubleBarrierOptionChanges,
    /// Return a validated replacement.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// lower_barrier, upper_barrier : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_type : {'knock_in', 'knock_out'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// rebate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// rebate_timing : {'at_hit', 'at_expiry'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_times : Iterable[datetime.datetime] or None, optional
    ///     Omission retains monitoring; None selects continuous monitoring.
    ///     A nonempty iterable selects scheduled monitoring.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Explicit override. Omission or Ellipsis inherits a supplied
    ///     ObservationSchedule calendar; otherwise construction uses weekdays
    ///     and replacement retains the current calendar.
    /// history_time : datetime.datetime or None, optional
    ///     Timestamp of supplied history, covering events strictly before it.
    ///     Required with history; refresh payload and timestamp together.
    ///
    /// Returns
    /// -------
    /// object
    ///     Independent contract of the same class, preserving omitted history and
    ///     terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid.
    ///
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, lower_barrier=crate::conversions::OMITTED, upper_barrier=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, rebate=crate::conversions::OMITTED, rebate_timing=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] lower_barrier: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] upper_barrier: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_type: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] rebate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] rebate_timing: Option<
            &Bound<'_, PyAny>,
        >,

        #[pyo3(from_py_with = crate::conversions::supplied)] observation_times: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<
            &pyo3::Bound<'_, pyo3::PyAny>,
        >,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?;

        let changes = kiyosi::DoubleBarrierOptionChanges {
            history_time,
            calendar: crate::calendar::observation_calendar(calendar, observation_times)?,
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            effective_time: effective_time
                .map(crate::conversions::timestamp)
                .transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            lower_barrier: lower_barrier.map(crate::conversions::real).transpose()?,
            upper_barrier: upper_barrier.map(crate::conversions::real).transpose()?,
            barrier_type: barrier_type
                .map(crate::instruments::double_barrier::kind)
                .transpose()?,
            rebate: rebate.map(crate::conversions::real).transpose()?,
            rebate_timing: rebate_timing
                .map(crate::instruments::barrier::timing)
                .transpose()?,
            observation_times: observation_times
                .map(crate::conversions::optional_timestamps)
                .transpose()?,
            touch_state: touch_state
                .map(crate::instruments::barrier::history)
                .transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    },
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time, lower_barrier, upper_barrier, barrier_type, rebate=kiyosi::BarrierOptionTerms::DEFAULT_REBATE, rebate_timing=kiyosi::RebateTiming::default(), observation_times=None, touch_state=None, calendar=crate::conversions::OMITTED, history_time=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        lower_barrier: &Bound<'_, PyAny>,
        upper_barrier: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] rebate: f64,
        #[pyo3(from_py_with = timing)] rebate_timing: kiyosi::RebateTiming,

        observation_times: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = history)] touch_state: Option<kiyosi::BarrierTouchState>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<
            kiyosi::Timestamp,
        >,
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
        terms.observation_times = observation_times
            .map(crate::conversions::timestamps)
            .transpose()?;
        terms.touch_state = touch_state;
        terms.calendar =
            crate::calendar::observation_calendar(calendar, observation_times)?.unwrap_or_default();
        terms.history_time = history_time;
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(
            domain(kiyosi::DoubleBarrierOption::new(terms))?,
            history_time,
        ))?))
    },
    /// Return call or put payoff direction.
    #[getter]
    fn option_type(&self) -> &'static str {
        if self.0.option_type() == kiyosi::OptionType::Call {
            "call"
        } else {
            "put"
        }
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
        match self.0.barrier_type() {
            kiyosi::DoubleBarrierType::KnockIn => "knock_in",
            kiyosi::DoubleBarrierType::KnockOut => "knock_out",
        }
    },
    /// Return nonnegative finite cash rebate; defaults to zero.
    #[getter]
    fn rebate(&self) -> f64 {
        self.0.rebate()
    },
    /// Return payment timing; at-hit rebates are valid only for knock-outs.
    #[getter]
    fn rebate_timing(&self) -> &'static str {
        match self.0.rebate_timing() {
            kiyosi::RebateTiming::AtHit => "at_hit",
            kiyosi::RebateTiming::AtExpiry => "at_expiry",
        }
    }
);
