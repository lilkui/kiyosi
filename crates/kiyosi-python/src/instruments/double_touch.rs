use crate::conversions::{date, dates, real};
use crate::error::domain;
use crate::instruments::barrier;
use pyo3::prelude::*;

/// Cash double one-touch or double no-touch option with inclusive barriers.
///
/// Notes
/// -----
/// Double one-touch pays when either barrier is touched; double no-touch
/// pays only if neither barrier is touched during monitoring.
/// Construct with cash_double_touch or cash_double_no_touch. Prior at-hit
/// payments have no remaining value.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleTouchOption(pub kiyosi::DoubleTouchOption);

monitoring_properties!(
    DoubleTouchOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// effective_midnight_utc : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_midnight_utc : datetime.date, optional
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
    /// observation_dates : Iterable[date], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
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
    ///     If a representation, field name or mutually exclusive coupon choice is invalid.
    #[pyo3(signature = (*, effective_midnight_utc=crate::conversions::OMITTED, expiry_midnight_utc=crate::conversions::OMITTED, lower_barrier=crate::conversions::OMITTED, upper_barrier=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, settlement_timing=crate::conversions::OMITTED, observation_mode=crate::conversions::OMITTED, observation_dates=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payout=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_midnight_utc: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_midnight_utc: Option<
            &Bound<'_, PyAny>,
        >,
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
        #[pyo3(from_py_with = crate::conversions::supplied)] observation_dates: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] payout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::DoubleTouchOptionChanges {
            effective_midnight_utc: effective_midnight_utc
                .map(crate::conversions::date)
                .transpose()?,
            expiry_midnight_utc: expiry_midnight_utc
                .map(crate::conversions::date)
                .transpose()?,
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
            observation_dates: observation_dates
                .map(crate::conversions::dates)
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
    ($name:ident, $kind:ident, [$($settlement:ident)?], $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, effective_midnight_utc, expiry_midnight_utc, lower_barrier, upper_barrier, payout, $($settlement=kiyosi::SettlementTiming::default(),)? observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(
            effective_midnight_utc: &Bound<'_, PyAny>, expiry_midnight_utc: &Bound<'_, PyAny>,
            lower_barrier: &Bound<'_, PyAny>, upper_barrier: &Bound<'_, PyAny>, payout: &Bound<'_, PyAny>,
            $(#[pyo3(from_py_with = super::binary_barrier::settlement)] $settlement: kiyosi::SettlementTiming,)?
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        ) -> PyResult<DoubleTouchOption> {
            let mut terms = kiyosi::DoubleTouchOptionTerms::new(date(effective_midnight_utc)?, date(expiry_midnight_utc)?, real(lower_barrier)?, real(upper_barrier)?, kiyosi::DoubleBarrierType::$kind);
            let payout = real(payout)?;
            $(terms.settlement_timing = $settlement;)?
            terms.observation_mode = observation_mode;
            terms.observation_dates = observation_dates;
            terms.touch_state = touch_state;
            Ok(DoubleTouchOption(domain(kiyosi::DoubleTouchOption::cash(terms, payout))?))
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
effective_midnight_utc : datetime.date
    Contract starts at 00:00 UTC on this date.
expiry_midnight_utc : datetime.date
    Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
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
    [],
    r"Create an expiry cash payment requiring neither boundary to be touched.

Parameters
----------
effective_midnight_utc : datetime.date
    Contract starts at 00:00 UTC on this date.
expiry_midnight_utc : datetime.date
    Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
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
