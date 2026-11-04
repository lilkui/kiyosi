use crate::calendar::TradingCalendar;
use crate::conversions::{direction, real, timestamp, timestamps};
use crate::error::domain;
use crate::instruments::barrier;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Cash-or-nothing or asset-or-nothing option with barrier monitoring.
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
/// Construct with cash_touch_option or asset_touch_option and an explicit barrier_type.
/// Prior at-hit payments are settled.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TouchOption(pub kiyosi::TouchOption);

macro_rules! binary_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        monitoring_properties!(
            $name,
            $($extra,)*
            /// Return positive finite trigger level; equality counts as a touch.
            #[getter]
            fn barrier_level(&self) -> f64 { self.0.barrier_level() },
            /// Return trigger direction and knock-in or knock-out behavior.
            #[getter]
            fn barrier_type(&self) -> &'static str {
                match self.0.barrier_type() {
                    kiyosi::BarrierType::UpAndIn => "up_and_in",
                    kiyosi::BarrierType::UpAndOut => "up_and_out",
                    kiyosi::BarrierType::DownAndIn => "down_and_in",
                    kiyosi::BarrierType::DownAndOut => "down_and_out",
                }
            },
            /// Return cash or asset settlement as a choice string.
            #[getter]
            fn payoff_type(&self) -> &'static str { if self.0.payout().is_some() { "cash" } else { "asset" } },
            /// Return the cash payout, or None for asset settlement.
            #[getter]
            fn payout(&self) -> Option<f64> { self.0.payout() },
            /// Return mean fixing spacing in Actual/365 Fixed years.
            #[getter]
            fn mean_observation_year_fraction(&self) -> f64 { self.0.mean_observation_year_fraction() }
        );
    };
}
binary_properties!(
    BinaryBarrierOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_mode : {'continuous', 'scheduled'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_times : Iterable[datetime.datetime], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned observation calendar; defaults to weekdays on construction and is
    ///     retained on replacement. PricingContext.calendar does not change it.
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
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, barrier_level=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, observation_mode=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payout=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
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
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_level: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_type: Option<
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
        let changes = kiyosi::BinaryBarrierOptionChanges {
            calendar: calendar
                .map(|v| v.extract::<TradingCalendar>().map(|v| v.0))
                .transpose()?,
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            effective_time: effective_time
                .map(crate::conversions::timestamp)
                .transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            barrier_level: barrier_level.map(crate::conversions::real).transpose()?,
            barrier_type: barrier_type
                .map(crate::instruments::barrier::kind)
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
    }
);
binary_properties!(
    TouchOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
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
    ///     retained on replacement. PricingContext.calendar does not change it.
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
    #[pyo3(signature = (*, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, barrier_level=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, settlement_timing=crate::conversions::OMITTED, observation_mode=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payout=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
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
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_level: Option<
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
        let changes = kiyosi::TouchOptionChanges {
            calendar: calendar
                .map(|v| v.extract::<TradingCalendar>().map(|v| v.0))
                .transpose()?,
            effective_time: effective_time
                .map(crate::conversions::timestamp)
                .transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            barrier_level: barrier_level.map(crate::conversions::real).transpose()?,
            barrier_type: barrier_type
                .map(crate::instruments::barrier::kind)
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
        #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time, barrier_level, barrier_type, $($payout,)? observation_mode=kiyosi::ObservationMode::default(), observation_times=Vec::new(), touch_state=None, calendar=TradingCalendar::default()))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
            effective_time: &Bound<'_, PyAny>, expiry_time: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, barrier_type: &Bound<'_, PyAny>,
            $($payout: &Bound<'_, PyAny>,)?
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = timestamps)] observation_times: Vec<kiyosi::Timestamp>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
            calendar: TradingCalendar,
        ) -> PyResult<BinaryBarrierOption> {
            let mut terms = kiyosi::BinaryBarrierTerms::new(direction(option_type)?, real(strike)?,
                timestamp(effective_time)?, timestamp(expiry_time)?, real(barrier_level)?, barrier::kind(barrier_type)?);
            $(let $payout = real($payout)?;)?
            terms.observation_mode = observation_mode;
            terms.observation_times = observation_times;
            terms.touch_state = touch_state;
            Ok(BinaryBarrierOption(domain(kiyosi::BinaryBarrierOption::$constructor(terms $(, $payout)?))?.with_calendar(calendar.0)))
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
effective_time : datetime.datetime
    Inclusive contract start.
expiry_time : datetime.datetime
    Inclusive expiry time.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Trigger direction and knock-in or knock-out behavior.
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
BinaryBarrierOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life times or observation terms are invalid.

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
effective_time : datetime.datetime
    Inclusive contract start.
expiry_time : datetime.datetime
    Inclusive expiry time.
barrier_level : float
    Positive finite trigger level; equality counts as a touch.
barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
    Trigger direction and knock-in or knock-out behavior.
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
BinaryBarrierOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life times or observation terms are invalid.

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
    ($name:ident, $constructor:ident $(, $payout:ident)?) => {
        /// Creates a cash or asset one-touch or no-touch contract.
        ///
        /// Parameters
        /// ----------
        /// effective_time : datetime.datetime
        ///     Inclusive contract start.
        /// expiry_time : datetime.datetime
        ///     Inclusive expiry time.
        /// barrier_level : float
        ///     Positive finite trigger level; equality counts as a touch.
        /// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        ///     In selects one-touch; out selects no-touch. Up/down selects direction.
        $(
            #[doc = concat!(stringify!($payout), " : float")]
            ///     Positive finite cash payment.
        )?
        /// settlement_timing : {'at_hit', 'at_expiry'}, optional
        ///     Defaults to expiry. At-hit settlement requires a one-touch contract.
        /// observation_mode : {'continuous', 'scheduled'}, optional
        ///     Continuous monitoring or observations on the supplied times.
        /// observation_times : iterable of datetime.datetime, optional
        ///     Strictly increasing observation times within the contract life.
        /// touch_state : {'untouched', 'touched'} or None, optional
        ///     Strictly prior history. None is unknown; untouched means known no hit.
        /// calendar : TradingCalendar, optional
        ///     Owned observation calendar; defaults to weekdays on construction and is
        ///     retained on replacement. PricingContext.calendar does not change it.
        ///
        /// Returns
        /// -------
        /// TouchOption
        ///     Validated owned contract. Asset settlement has no cash payout.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If amounts, life times or observation terms are invalid, or at-hit
        ///     settlement is requested for a no-touch contract.
        ///
        /// Notes
        /// -----
        /// History and trading-calendar consistency are checked during pricing.
        /// Prior at-hit payments have no remaining value. A current asset hit
        /// transfers spot; a future continuous asset hit transfers the barrier amount.
        /// See the kiyosi.instruments module examples for construction.
        #[pyfunction]
        #[pyo3(signature = (*, effective_time, expiry_time, barrier_level, barrier_type, $($payout,)? settlement_timing=kiyosi::SettlementTiming::default(), observation_mode=kiyosi::ObservationMode::default(), observation_times=Vec::new(), touch_state=None, calendar=TradingCalendar::default()))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(effective_time: &Bound<'_, PyAny>, expiry_time: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, barrier_type: &Bound<'_, PyAny>,
            $($payout: &Bound<'_, PyAny>,)?
            #[pyo3(from_py_with = settlement)] settlement_timing: kiyosi::SettlementTiming,
            #[pyo3(from_py_with = barrier::mode)] observation_mode: kiyosi::ObservationMode,
            #[pyo3(from_py_with = timestamps)] observation_times: Vec<kiyosi::Timestamp>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
            calendar: TradingCalendar,
        ) -> PyResult<TouchOption> {
            let mut terms = kiyosi::TouchOptionTerms::new(timestamp(effective_time)?, timestamp(expiry_time)?, real(barrier_level)?, barrier::kind(barrier_type)?);
            $(let $payout = real($payout)?;)?
            terms.settlement_timing = settlement_timing;
            terms.observation_mode = observation_mode;
            terms.observation_times = observation_times;
            terms.touch_state = touch_state;
            Ok(TouchOption(domain(kiyosi::TouchOption::$constructor(terms $(, $payout)?))?.with_calendar(calendar.0)))
        }
    };
}
touch_factory!(cash_touch_option, cash, payout);
touch_factory!(asset_touch_option, asset);

#[derive(FromPyObject)]
pub(crate) enum BinaryInstrument<'py> {
    Binary(PyRef<'py, BinaryBarrierOption>),
    Touch(PyRef<'py, TouchOption>),
}

macro_rules! with_binary {
    ($instrument:expr, $option:ident, $body:expr) => {{
        match $instrument {
            $crate::instruments::binary_barrier::BinaryInstrument::Binary(option) => {
                let $option = &option.0;
                $body
            }
            $crate::instruments::binary_barrier::BinaryInstrument::Touch(option) => {
                let $option = &option.0;
                $body
            }
        }
    }};
}
pub(crate) use with_binary;
