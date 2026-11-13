use crate::conversions::{real, timestamp};
use crate::error::domain;
use crate::instruments::barrier;
use pyo3::prelude::*;

/// Cash double one-touch or double no-touch option with inclusive barriers.
///
/// Parameters
/// ----------
/// effective_time, expiry_time : datetime.datetime
///     Inclusive contract life.
/// lower_barrier, upper_barrier : float
///     Positive finite boundaries, with lower strictly below upper.
/// barrier_type : {'knock_in', 'knock_out'}
///     Knock-in selects double one-touch; knock-out selects double no-touch.
/// payout : float
///     Positive finite cash payment.
/// settlement_timing : {'at_hit', 'at_expiry'}, optional
///     Defaults to expiry; at-hit settlement requires double one-touch.
/// observation_times : iterable of datetime.datetime or None, optional
///     Nonempty, strictly increasing times within life; None selects continuous monitoring.
/// touch_state : {'untouched', 'touched'} or None, optional
///     Strictly prior touch history; defaults to unknown.
/// calendar : TradingCalendar, optional
///     Explicit override. Omission or Ellipsis inherits a supplied
///     ObservationSchedule calendar; otherwise construction uses weekdays
///     and replacement retains the current calendar.
///
/// Raises
/// ------
/// InvalidInputError
///     If numeric terms, life, monitoring or calendar terms are invalid, or
///     at-hit settlement is requested for double no-touch.
///
/// Notes
/// -----
/// Double one-touch pays when either barrier is touched; double no-touch pays
/// only if neither barrier is touched during monitoring. Prior at-hit payments
/// have no remaining value.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleTouchOption(pub kiyosi::DoubleTouchOption);

monitoring_properties!(
    DoubleTouchOption,
    #[new]
    #[pyo3(signature = (*, effective_time, expiry_time, lower_barrier, upper_barrier, barrier_type, payout, settlement_timing=kiyosi::SettlementTiming::default(), observation_times=None, touch_state=None, calendar=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    pub(crate) fn new(
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        lower_barrier: &Bound<'_, PyAny>,
        upper_barrier: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = super::double_barrier::kind)] barrier_type: kiyosi::DoubleBarrierType,
        payout: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = super::binary_barrier::settlement)]
        settlement_timing: kiyosi::SettlementTiming,

        observation_times: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::DoubleTouchOptionTerms::new(
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            real(lower_barrier)?,
            real(upper_barrier)?,
            barrier_type,
        );
        let payout = real(payout)?;
        terms.settlement_timing = settlement_timing;
        terms.observation_times = observation_times
            .map(crate::conversions::timestamps)
            .transpose()?;
        terms.touch_state = touch_state;
        terms.calendar =
            crate::calendar::observation_calendar(calendar, observation_times)?.unwrap_or_default();
        Ok(Self(domain(kiyosi::DoubleTouchOption::cash(
            terms, payout,
        ))?))
    },
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
    ///
    /// Parameters
    /// ----------
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// lower_barrier, upper_barrier : float, optional
    ///     Positive finite lower and upper triggers, with the lower strictly smaller.
    /// barrier_type : {'knock_in', 'knock_out'}, optional
    ///     Knock-in or knock-out activation at either boundary.
    /// settlement_timing : {'at_hit', 'at_expiry'}, optional
    ///     Payment timing; at-hit settlement requires a one-touch contract.
    /// observation_times : iterable of datetime.datetime or None, optional
    ///     Omission retains monitoring; None selects continuous monitoring.
    ///     A nonempty iterable selects scheduled monitoring.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Strictly prior touch history; None clears it to unknown.
    /// payout : float, optional
    ///     Positive finite cash payment.
    /// calendar : TradingCalendar, optional
    ///     Explicit override. Omission or Ellipsis inherits a supplied
    ///     ObservationSchedule calendar; otherwise construction uses weekdays
    ///     and replacement retains the current calendar.
    ///
    /// Returns
    /// -------
    /// DoubleTouchOption
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
    #[pyo3(signature = (*, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, lower_barrier=crate::conversions::OMITTED, upper_barrier=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, settlement_timing=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payout=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
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
        #[pyo3(from_py_with = crate::conversions::supplied)] settlement_timing: Option<
            &Bound<'_, PyAny>,
        >,

        #[pyo3(from_py_with = crate::conversions::supplied)] observation_times: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] payout: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::DoubleTouchOptionChanges {
            calendar: crate::calendar::observation_calendar(calendar, observation_times)?,
            effective_time: effective_time
                .map(crate::conversions::timestamp)
                .transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            lower_barrier: lower_barrier.map(crate::conversions::real).transpose()?,
            upper_barrier: upper_barrier.map(crate::conversions::real).transpose()?,
            barrier_type: barrier_type
                .map(crate::instruments::double_barrier::kind)
                .transpose()?,
            settlement_timing: settlement_timing
                .map(crate::instruments::binary_barrier::settlement)
                .transpose()?,
            observation_times: observation_times
                .map(crate::conversions::optional_timestamps)
                .transpose()?,
            touch_state: touch_state
                .map(crate::instruments::barrier::history)
                .transpose()?,
            payout: payout.map(crate::conversions::real).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
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
    /// Return the positive finite cash payout.
    #[getter]
    fn payout(&self) -> f64 {
        self.0.payout()
    },
    /// Return knock_in for one-touch or knock_out for no-touch payment.
    #[getter]
    fn barrier_type(&self) -> &'static str {
        crate::conversions::double_barrier_type_name(self.0.barrier_type())
    },
    /// Return whether touching activates payment.
    #[getter]
    fn is_one_touch(&self) -> bool {
        self.0.is_one_touch()
    },
    /// Return at-hit or expiry payment; no-touch contracts settle only at expiry.
    #[getter]
    fn settlement_timing(&self) -> &'static str {
        crate::conversions::settlement_timing_name(self.0.settlement_timing())
    }
);
