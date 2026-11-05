use crate::calendar::TradingCalendar;
use crate::conversions::{real, timestamp, timestamps};
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
/// observation_mode : {'continuous', 'scheduled'}, optional
///     Monitoring frequency; defaults to continuous.
/// observation_times : iterable of datetime.datetime, optional
///     Strictly increasing scheduled times within life; empty for continuous monitoring.
/// touch_state : {'untouched', 'touched'} or None, optional
///     Strictly prior touch history; defaults to unknown.
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays.
///
/// Raises
/// ------
/// InvalidInputError
///     If numeric terms, life, monitoring or calendar terms are invalid, or
///     at-hit settlement is requested for double no-touch.
///
/// Notes
/// -----
/// Double one-touch pays when either barrier is touched; double no-touch
/// pays only if neither barrier is touched during monitoring.
/// The cash_double_touch and cash_double_no_touch functions are conveniences. Prior at-hit
/// payments have no remaining value.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleTouchOption(pub kiyosi::DoubleTouchOption);

monitoring_properties!(
    DoubleTouchOption,
    #[new]
    #[pyo3(signature = (*, effective_time, expiry_time, lower_barrier, upper_barrier, barrier_type, payout, settlement_timing=kiyosi::SettlementTiming::default(), observation_mode=kiyosi::ObservationMode::default(), observation_times=Vec::new(), touch_state=None, calendar=TradingCalendar::default()))]
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
        #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
        #[pyo3(from_py_with = timestamps)] observation_times: Vec<kiyosi::Timestamp>,
        #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        calendar: TradingCalendar,
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
        terms.observation_mode = observation_mode;
        terms.observation_times = observation_times;
        terms.touch_state = touch_state;
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::DoubleTouchOption::cash(
            terms, payout,
        ))?))
    },
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// lower_barrier : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// upper_barrier : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_type : {'knock_in', 'knock_out'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// settlement_timing : {'at_hit', 'at_expiry'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_mode : {'continuous', 'scheduled'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_times : Iterable[datetime.datetime], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned observation calendar; defaults to weekdays on construction and is
    ///     retained on replacement.
    /// payout : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    ///
    /// Returns
    /// -------
    /// object
    ///     Independent contract of the same class, preserving omitted history and terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation; the original is unchanged.
    /// TypeError
    ///     If a representation or field name is invalid.
    #[pyo3(signature = (*, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, lower_barrier=crate::conversions::OMITTED, upper_barrier=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, settlement_timing=crate::conversions::OMITTED, observation_mode=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payout=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
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
        #[pyo3(from_py_with = crate::conversions::supplied)] observation_mode: Option<
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
            calendar: calendar
                .map(|v| v.extract::<TradingCalendar>().map(|v| v.0))
                .transpose()?,
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
            observation_mode: observation_mode
                .map(crate::instruments::barrier::mode)
                .transpose()?,
            observation_times: observation_times
                .map(crate::conversions::timestamps)
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
        match self.0.barrier_type() {
            kiyosi::DoubleBarrierType::KnockIn => "knock_in",
            kiyosi::DoubleBarrierType::KnockOut => "knock_out",
        }
    },
    /// Return whether touching activates payment.
    #[getter]
    fn is_one_touch(&self) -> bool {
        self.0.is_one_touch()
    },
    /// Return at-hit or expiry payment; no-touch contracts settle only at expiry.
    #[getter]
    fn settlement_timing(&self) -> &'static str {
        match self.0.settlement_timing() {
            kiyosi::SettlementTiming::AtHit => "at_hit",
            kiyosi::SettlementTiming::AtExpiry => "at_expiry",
        }
    }
);

macro_rules! double_touch_factory {
    (@settlement) => { kiyosi::SettlementTiming::default() };
    (@settlement $settlement:ident) => { $settlement };
    ($name:ident, $kind:ident, [$($settlement:ident)?], $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, effective_time, expiry_time, lower_barrier, upper_barrier, payout, $($settlement=kiyosi::SettlementTiming::default(),)? observation_mode=kiyosi::ObservationMode::default(), observation_times=Vec::new(), touch_state=None, calendar=TradingCalendar::default()))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(
            effective_time: &Bound<'_, PyAny>, expiry_time: &Bound<'_, PyAny>,
            lower_barrier: &Bound<'_, PyAny>, upper_barrier: &Bound<'_, PyAny>, payout: &Bound<'_, PyAny>,
            $(#[pyo3(from_py_with = super::binary_barrier::settlement)] $settlement: kiyosi::SettlementTiming,)?
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = timestamps)] observation_times: Vec<kiyosi::Timestamp>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
            calendar: TradingCalendar,
        ) -> PyResult<DoubleTouchOption> {
            DoubleTouchOption::new(effective_time, expiry_time, lower_barrier, upper_barrier,
                kiyosi::DoubleBarrierType::$kind, payout,
                double_touch_factory!(@settlement $($settlement)?),
                observation_mode, observation_times, touch_state, calendar)
        }
    };
}
double_touch_factory!(
    cash_double_touch,
    KnockIn,
    [settlement_timing],
    r"Create a cash payment activated by either inclusive boundary.

Parameters
----------
effective_time : datetime.datetime
    Inclusive contract start.
expiry_time : datetime.datetime
    Inclusive expiry time.
lower_barrier : float
    Positive finite lower trigger; equality counts as a touch.
upper_barrier : float
    Finite upper trigger, strictly above the lower trigger.
payout : float
    Positive finite cash payment.
settlement_timing : {'at_hit', 'at_expiry'}, optional
    At-hit or expiry payment; no-touch contracts settle only at expiry.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied times.
observation_times : iterable of datetime.datetime, optional
    Strictly increasing observation times within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.
calendar : TradingCalendar, optional
    Owned observation calendar; defaults to weekdays, independently of PricingContext.

Returns
-------
DoubleTouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life times or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Trading-calendar compatibility is checked
on construction; strictly prior history is checked during pricing. Prior at-hit
payments have no remaining value. No-touch payments occur only at
expiry."
);
double_touch_factory!(
    cash_double_no_touch,
    KnockOut,
    [],
    r"Create an expiry cash payment requiring neither boundary to be touched.

Parameters
----------
effective_time : datetime.datetime
    Inclusive contract start.
expiry_time : datetime.datetime
    Inclusive expiry time.
lower_barrier : float
    Positive finite lower trigger; equality counts as a touch.
upper_barrier : float
    Finite upper trigger, strictly above the lower trigger.
payout : float
    Positive finite cash payment.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied times.
observation_times : iterable of datetime.datetime, optional
    Strictly increasing observation times within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.
calendar : TradingCalendar, optional
    Owned observation calendar; defaults to weekdays, independently of PricingContext.

Returns
-------
DoubleTouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life times or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Trading-calendar compatibility is checked
on construction; strictly prior history is checked during pricing. Prior at-hit
payments have no remaining value. No-touch payments occur only at
expiry."
);
