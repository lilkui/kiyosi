use crate::conversions::{date, dates, real};
use crate::error::domain;
use crate::instruments::barrier;
use pyo3::prelude::*;

/// Cash one-touch or no-touch contract with two inclusive boundaries.
///
/// Notes
/// -----
/// Construct with cash_double_touch or cash_double_no_touch. Prior at-hit
/// payments have no remaining value.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleTouchOption(pub kiyosi::DoubleTouchOption);

monitoring_properties!(
    DoubleTouchOption,
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
    /// Return the nonnegative finite cash payout.
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
        #[pyo3(signature = (*, effective_date, expiry_date, lower_barrier, upper_barrier, payout, $($settlement=kiyosi::SettlementTiming::default(),)? observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(
            effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            lower_barrier: &Bound<'_, PyAny>, upper_barrier: &Bound<'_, PyAny>, payout: &Bound<'_, PyAny>,
            $(#[pyo3(from_py_with = super::binary_barrier::settlement)] $settlement: kiyosi::SettlementTiming,)?
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        ) -> PyResult<DoubleTouchOption> {
            let mut terms = kiyosi::DoubleTouchOptionTerms::new(date(effective_date)?, date(expiry_date)?, real(lower_barrier)?, real(upper_barrier)?, kiyosi::DoubleBarrierType::$kind);
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
    [],
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
