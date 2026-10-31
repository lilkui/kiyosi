use crate::conversions::{date, dates, direction, python_date, real, supplied};
use crate::error::domain;
use crate::instruments::barrier;
use crate::schedule::ObservationSchedule;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDate;

/// Cash or asset digital with barrier monitoring and strict exercise.
///
/// Notes
/// -----
/// Construct with cash_binary_barrier_option or asset_binary_barrier_option.
/// Barrier equality touches; terminal strike equality pays zero.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinaryBarrierOption(pub kiyosi::BinaryBarrierOption);

/// Strike-free cash or asset one-touch or no-touch contract.
///
/// Notes
/// -----
/// Use a directional factory such as cash_one_touch_up or asset_no_touch_down.
/// Prior at-hit payments are settled.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TouchOption(pub kiyosi::TouchOption);

macro_rules! monitoring_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pymethods]
        impl $name {
            $($extra)*
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
            #[pyo3(signature = (*, touch_state=None))]
            fn replace(&self,
                #[pyo3(from_py_with = supplied)] touch_state: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                let state = match touch_state {
                    Some(value) => barrier::history(value)?,
                    None => self.0.touch_state(),
                };
                Ok(Self(self.0.replace(state)))
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
            fn barrier_level(&self) -> f64 { self.0.barrier_level() }
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
            /// Return cash or asset settlement as a choice string.
            #[getter]
            fn payoff_type(&self) -> &'static str { if self.0.payout().is_some() { "cash" } else { "asset" } }
            /// Return the cash payout, or None for asset settlement.
            #[getter]
            fn payout(&self) -> Option<f64> { self.0.payout() }
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
                self.0.observation_dates().iter().map(|&d| python_date(py, d)).collect()
            }
            /// Return mean fixing spacing in Actual/365 Fixed years.
            #[getter]
            fn mean_observation_year_fraction(&self) -> f64 { self.0.mean_observation_year_fraction() }
            /// Return strictly prior touch history, or None if unknown.
            #[getter]
            fn touch_state(&self) -> Option<&'static str> {
                self.0.touch_state().map(|s| match s {
                    kiyosi::BarrierTouchState::Untouched => "untouched",
                    kiyosi::BarrierTouchState::Touched => "touched",
                })
            }
        }
    };
}
monitoring_properties!(
    BinaryBarrierOption,
    /// Return call or put payoff direction.
    #[getter]
    fn option_type(&self) -> &'static str {
        if self.0.option_type() == kiyosi::OptionType::Call {
            "call"
        } else {
            "put"
        }
    },
    /// Return the exercise or purchase price in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
);
monitoring_properties!(
    TouchOption,
    /// Return whether touching activates payment.
    #[getter]
    fn is_one_touch(&self) -> bool {
        self.0.is_one_touch()
    },
    /// Return whether the barrier triggers upward.
    #[getter]
    fn is_up(&self) -> bool {
        self.0.is_up()
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

macro_rules! binary_factory {
    ($name:ident, $constructor:ident $(, $payout:ident)?, $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date, barrier_level, barrier_type, $($payout,)? observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
            effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, barrier_type: &Bound<'_, PyAny>,
            $($payout: &Bound<'_, PyAny>,)?
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        ) -> PyResult<BinaryBarrierOption> {
            let mut terms = kiyosi::BinaryBarrierTerms::new(direction(option_type)?, real(strike)?,
                date(effective_date)?, date(expiry_date)?, real(barrier_level)?, barrier::kind(barrier_type)?);
            $(let $payout = real($payout)?;)?
            terms.observation_mode = observation_mode;
            terms.observation_dates = observation_dates;
            terms.touch_state = touch_state;
            Ok(BinaryBarrierOption(domain(kiyosi::BinaryBarrierOption::$constructor(terms $(, $payout)?))?))
        }
    };
}
binary_factory!(
    cash_binary_barrier_option,
    cash,
    payout,
    r"Create a cash digital with barrier monitoring and strict terminal
exercise.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive finite exercise or purchase price.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Trigger direction and knock-in or knock-out behavior.
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
BinaryBarrierOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. Terminal strike
equality pays zero."
);
binary_factory!(
    asset_binary_barrier_option,
    asset,
    r"Create an asset digital with barrier monitoring and strict terminal
exercise.

Parameters
----------
option_type : {'call', 'put'}
    Call or put payoff direction.
strike : float
    Positive finite exercise or purchase price.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Trigger direction and knock-in or knock-out behavior.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied dates.
observation_dates : iterable of date, optional
    Strictly increasing observation dates within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.

Returns
-------
BinaryBarrierOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. Terminal strike
equality pays zero."
);

pub(crate) fn settlement(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::SettlementTiming> {
    match value.extract::<&str>()? {
        "at_hit" => Ok(kiyosi::SettlementTiming::AtHit),
        "at_expiry" => Ok(kiyosi::SettlementTiming::AtExpiry),
        _ => Err(PyValueError::new_err("unknown settlement_timing")),
    }
}

macro_rules! touch_factory {
    ($(#[$attribute:meta])* $name:ident, $constructor:ident, $kind:ident, [$($settlement:ident)?] $(, $payout:ident)?, $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, effective_date, expiry_date, barrier_level, $($payout,)? $($settlement=kiyosi::SettlementTiming::default(),)? observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
        $(#[$attribute])*
        pub(crate) fn $name(effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, $($payout: &Bound<'_, PyAny>,)?
            $(#[pyo3(from_py_with = settlement)] $settlement: kiyosi::SettlementTiming,)?
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        ) -> PyResult<TouchOption> {
            let mut terms = kiyosi::TouchOptionTerms::new(date(effective_date)?, date(expiry_date)?, real(barrier_level)?, kiyosi::BarrierType::$kind);
            $(let $payout = real($payout)?;)?
            $(terms.settlement_timing = $settlement;)?
            terms.observation_mode = observation_mode;
            terms.observation_dates = observation_dates;
            terms.touch_state = touch_state;
            Ok(TouchOption(domain(kiyosi::TouchOption::$constructor(terms $(, $payout)?))?))
        }
    };
}
touch_factory!(
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    cash_one_touch_up,
    cash,
    UpAndIn,
    [settlement_timing],
    payout,
    r"Create a cash payment activated by upward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
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
TouchOption
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
touch_factory!(
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    cash_one_touch_down,
    cash,
    DownAndIn,
    [settlement_timing],
    payout,
    r"Create a cash payment activated by downward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
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
TouchOption
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
touch_factory!(
    cash_no_touch_up,
    cash,
    UpAndOut,
    [],
    payout,
    r"Create a cash payment requiring no upward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
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
TouchOption
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
touch_factory!(
    cash_no_touch_down,
    cash,
    DownAndOut,
    [],
    payout,
    r"Create a cash payment requiring no downward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
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
TouchOption
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
touch_factory!(
    asset_one_touch_up,
    asset,
    UpAndIn,
    [settlement_timing],
    r"Create an asset payment activated by upward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
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
TouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. A current at-hit
payment transfers spot; future continuous hits transfer the barrier
amount. No-touch payments occur only at expiry."
);
touch_factory!(
    asset_one_touch_down,
    asset,
    DownAndIn,
    [settlement_timing],
    r"Create an asset payment activated by downward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
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
TouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. A current at-hit
payment transfers spot; future continuous hits transfer the barrier
amount. No-touch payments occur only at expiry."
);
touch_factory!(
    asset_no_touch_up,
    asset,
    UpAndOut,
    [],
    r"Create an asset payment requiring no upward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied dates.
observation_dates : iterable of date, optional
    Strictly increasing observation dates within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.

Returns
-------
TouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. A current at-hit
payment transfers spot; future continuous hits transfer the barrier
amount. No-touch payments occur only at expiry."
);
touch_factory!(
    asset_no_touch_down,
    asset,
    DownAndOut,
    [],
    r"Create an asset payment requiring no downward barrier touch.

Parameters
----------
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
observation_mode : {'continuous', 'scheduled'}, optional
    Continuous monitoring or observations on the supplied dates.
observation_dates : iterable of date, optional
    Strictly increasing observation dates within the contract life.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.

Returns
-------
TouchOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Barrier equality counts as a touch. Strictly prior history and trading
calendar compatibility are checked during pricing. A current at-hit
payment transfers spot; future continuous hits transfer the barrier
amount. No-touch payments occur only at expiry."
);

#[derive(FromPyObject)]
pub(crate) enum BinaryInstrument<'py> {
    Binary(PyRef<'py, BinaryBarrierOption>),
    Touch(PyRef<'py, TouchOption>),
}
