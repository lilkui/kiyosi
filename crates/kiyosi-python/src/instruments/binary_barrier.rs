use crate::conversions::{direction, real, timestamp};
use crate::error::domain;
use crate::instruments::barrier;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn payoff(kind: &str, amount: Option<f64>) -> PyResult<kiyosi::BinaryPayoff> {
    match (kind, amount) {
        ("cash", Some(amount)) => Ok(kiyosi::BinaryPayoff::Cash(amount)),
        ("asset", None) => Ok(kiyosi::BinaryPayoff::Asset),
        ("cash", None) => Err(pyo3::exceptions::PyTypeError::new_err(
            "cash payoff requires payout",
        )),
        ("asset", Some(_)) => Err(pyo3::exceptions::PyTypeError::new_err(
            "asset payoff does not accept a cash payout",
        )),
        _ => Err(PyValueError::new_err("payoff_type must be cash or asset")),
    }
}

fn replace_payoff(
    current: Option<f64>,
    kind: Option<&Bound<'_, PyAny>>,
    amount: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<kiyosi::BinaryPayoff>> {
    if kind.is_none() && amount.is_none() {
        return Ok(None);
    }
    let kind = kind
        .map(PyAnyMethods::extract::<&str>)
        .transpose()?
        .unwrap_or(if current.is_some() { "cash" } else { "asset" });
    let amount = match amount {
        Some(value) if value.is_none() => None,
        Some(value) => Some(real(value)?),
        None if kind == "asset" => None,
        None => current,
    };
    payoff(kind, amount).map(Some)
}

// Keep base and monitoring conversion separate to preserve payoff-error precedence.
fn touch_terms(
    effective_time: &Bound<'_, PyAny>,
    expiry_time: &Bound<'_, PyAny>,
    barrier_level: &Bound<'_, PyAny>,
    barrier_type: &Bound<'_, PyAny>,
) -> PyResult<kiyosi::TouchOptionTerms> {
    Ok(kiyosi::TouchOptionTerms::new(
        timestamp(effective_time)?,
        timestamp(expiry_time)?,
        real(barrier_level)?,
        barrier::kind(barrier_type)?,
    ))
}

fn touch_monitoring(
    mut terms: kiyosi::TouchOptionTerms,
    settlement_timing: kiyosi::SettlementTiming,
    observation_times: Option<&Bound<'_, PyAny>>,
    touch_state: Option<kiyosi::BarrierTouchState>,
    calendar: Option<&Bound<'_, PyAny>>,
) -> PyResult<kiyosi::TouchOptionTerms> {
    terms.settlement_timing = settlement_timing;
    terms.observation_times = observation_times
        .map(crate::conversions::timestamps)
        .transpose()?;
    terms.touch_state = touch_state;
    terms.calendar =
        crate::calendar::observation_calendar(calendar, observation_times)?.unwrap_or_default();
    Ok(terms)
}

/// Cash-or-nothing or asset-or-nothing option with barrier monitoring.
///
/// Parameters
/// ----------
/// option_type : {'call', 'put'}
///     Terminal payoff direction; strike equality pays zero.
/// strike : float
///     Positive finite terminal exercise price.
/// effective_time, expiry_time : datetime.datetime
///     Inclusive contract life.
/// barrier_level : float
///     Positive finite trigger level.
/// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
///     Trigger direction and knock-in or knock-out behavior.
/// payoff_type : {'cash', 'asset'}
///     Explicit settlement choice.
/// payout : float or None, optional
///     Positive cash amount required for cash settlement; absent for asset
///     settlement.
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
///     If numeric terms, life, schedule or calendar compatibility are invalid.
///
/// Notes
/// -----
/// The cash_binary_barrier_option and asset_binary_barrier_option functions
/// are conveniences. Barrier equality touches; terminal strike equality pays
/// zero.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinaryBarrierOption(pub kiyosi::BinaryBarrierOption);

/// Strike-free cash or asset one-touch or no-touch contract.
///
/// Parameters
/// ----------
/// effective_time, expiry_time : datetime.datetime
///     Inclusive contract life.
/// barrier_level : float
///     Positive finite trigger level; equality counts as a touch.
/// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
///     In selects one-touch; out selects no-touch. Up/down selects direction.
/// payoff_type : {'cash', 'asset'}
///     Explicit settlement choice.
/// payout : float or None, optional
///     Positive cash amount required for cash settlement; absent for asset
///     settlement.
/// settlement_timing : {'at_hit', 'at_expiry'}, optional
///     Defaults to expiry; at-hit settlement requires a one-touch.
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
///     at-hit settlement is requested for a no-touch.
///
/// Notes
/// -----
/// The cash_touch_option and asset_touch_option factories create one-touch
/// contracts; cash_no_touch_option and asset_no_touch_option create no-touch contracts.
/// Prior at-hit payments are settled.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TouchOption(pub kiyosi::TouchOption);

macro_rules! binary_properties {
    ($name:ident, $changes:ident, $($extra:item),* $(,)?) => {
        monitoring_properties!(
            $name,
            $changes,
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
    BinaryBarrierOptionChanges,
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time, barrier_level, barrier_type, payoff_type, payout=None, observation_times=None, touch_state=None, calendar=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    pub(crate) fn new(
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        barrier_level: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        payoff_type: &str,
        payout: Option<&Bound<'_, PyAny>>,

        observation_times: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::BinaryBarrierTerms::new(
            direction(option_type)?,
            real(strike)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            real(barrier_level)?,
            barrier::kind(barrier_type)?,
        );
        let payoff = payoff(payoff_type, payout.map(real).transpose()?)?;
        terms.observation_times = observation_times
            .map(crate::conversions::timestamps)
            .transpose()?;
        terms.touch_state = touch_state;
        terms.calendar =
            crate::calendar::observation_calendar(calendar, observation_times)?.unwrap_or_default();
        Ok(Self(domain(match payoff {
            kiyosi::BinaryPayoff::Cash(payout) => kiyosi::BinaryBarrierOption::cash(terms, payout),
            kiyosi::BinaryPayoff::Asset => kiyosi::BinaryBarrierOption::asset(terms),
        })?))
    },
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
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
    /// observation_times : iterable of datetime.datetime or None, optional
    ///     Omission retains monitoring; None selects continuous monitoring.
    ///     A nonempty iterable selects scheduled monitoring.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Strictly prior touch history; None clears it to unknown.
    /// payoff_type : {'cash', 'asset'}, optional
    ///     Explicit settlement choice; omission preserves the current kind.
    /// payout : float or None, optional
    ///     Cash amount; None is invalid for cash and never changes the payoff
    ///     kind.
    /// calendar : TradingCalendar, optional
    ///     Explicit override. Omission or Ellipsis inherits a supplied
    ///     ObservationSchedule calendar; otherwise construction uses weekdays
    ///     and replacement retains the current calendar.
    ///
    /// Returns
    /// -------
    /// BinaryBarrierOption
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
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, barrier_level=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payoff_type=crate::conversions::OMITTED, payout=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
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

        #[pyo3(from_py_with = crate::conversions::supplied)] observation_times: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] payoff_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] payout: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::BinaryBarrierOptionChanges {
            calendar: crate::calendar::observation_calendar(calendar, observation_times)?,
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
            observation_times: observation_times
                .map(crate::conversions::optional_timestamps)
                .transpose()?,
            touch_state: touch_state
                .map(crate::instruments::barrier::history)
                .transpose()?,
            payoff: replace_payoff(self.0.payout(), payoff_type, payout)?,
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
    TouchOptionChanges,
    #[new]
    #[pyo3(signature = (*, effective_time, expiry_time, barrier_level, barrier_type, payoff_type, payout=None, settlement_timing=kiyosi::SettlementTiming::default(), observation_times=None, touch_state=None, calendar=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    pub(crate) fn new(
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        barrier_level: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        payoff_type: &str,
        payout: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = settlement)] settlement_timing: kiyosi::SettlementTiming,

        observation_times: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let terms = touch_terms(effective_time, expiry_time, barrier_level, barrier_type)?;
        let payoff = payoff(payoff_type, payout.map(real).transpose()?)?;
        let terms = touch_monitoring(
            terms,
            settlement_timing,
            observation_times,
            touch_state,
            calendar,
        )?;
        Ok(Self(domain(match payoff {
            kiyosi::BinaryPayoff::Cash(payout) => kiyosi::TouchOption::cash(terms, payout),
            kiyosi::BinaryPayoff::Asset => kiyosi::TouchOption::asset(terms),
        })?))
    },
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
    ///
    /// Parameters
    /// ----------
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// barrier_level : float, optional
    ///     Positive finite trigger level; equality counts as a touch.
    /// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
    ///     Barrier direction and knock-in or knock-out behavior.
    /// settlement_timing : {'at_hit', 'at_expiry'}, optional
    ///     Payment timing; at-hit settlement requires a one-touch contract.
    /// observation_times : iterable of datetime.datetime or None, optional
    ///     Omission retains monitoring; None selects continuous monitoring.
    ///     A nonempty iterable selects scheduled monitoring.
    /// touch_state : {'untouched', 'touched'} or None, optional
    ///     Strictly prior touch history; None clears it to unknown.
    /// payoff_type : {'cash', 'asset'}, optional
    ///     Explicit settlement choice; omission preserves the current kind.
    /// payout : float or None, optional
    ///     Cash amount; None is invalid for cash and never changes the payoff
    ///     kind.
    /// calendar : TradingCalendar, optional
    ///     Explicit override. Omission or Ellipsis inherits a supplied
    ///     ObservationSchedule calendar; otherwise construction uses weekdays
    ///     and replacement retains the current calendar.
    ///
    /// Returns
    /// -------
    /// TouchOption
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
    #[pyo3(signature = (*, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, barrier_level=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, settlement_timing=crate::conversions::OMITTED, observation_times=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED, payoff_type=crate::conversions::OMITTED, payout=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
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

        #[pyo3(from_py_with = crate::conversions::supplied)] observation_times: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] payoff_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] payout: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::TouchOptionChanges {
            calendar: crate::calendar::observation_calendar(calendar, observation_times)?,
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
            observation_times: observation_times
                .map(crate::conversions::optional_timestamps)
                .transpose()?,
            touch_state: touch_state
                .map(crate::instruments::barrier::history)
                .transpose()?,
            payoff: replace_payoff(self.0.payout(), payoff_type, payout)?,
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
    (@payout) => { None };
    (@payout $payout:ident) => { Some($payout) };
    ($name:ident, $constructor:ident $(, $payout:ident)?, $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time, barrier_level, barrier_type, $($payout,)? observation_times=None, touch_state=None, calendar=crate::conversions::OMITTED))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
            effective_time: &Bound<'_, PyAny>, expiry_time: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, barrier_type: &Bound<'_, PyAny>,
            $($payout: &Bound<'_, PyAny>,)?

            observation_times: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
            #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

        ) -> PyResult<BinaryBarrierOption> {
            BinaryBarrierOption::new(option_type, strike, effective_time, expiry_time,
                barrier_level, barrier_type, stringify!($constructor), binary_factory!(@payout $($payout)?),
                observation_times, touch_state, calendar)
        }
    };
}
binary_factory!(
    cash_binary_barrier_option,
    cash,
    payout,
    r"Create a barrier cash digital with strict terminal exercise.

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
observation_times : iterable of datetime.datetime or None, optional
    Nonempty, strictly increasing times within life; None selects continuous monitoring.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.
calendar : TradingCalendar, optional
    Explicit override. Omission or Ellipsis inherits a supplied
    ObservationSchedule calendar; otherwise construction uses weekdays
    and replacement retains the current calendar.

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
Barrier equality counts as a touch. Trading-calendar compatibility is
checked on construction; strictly prior history is checked during pricing.
Terminal strike equality pays zero.
"
);
binary_factory!(
    asset_binary_barrier_option,
    asset,
    r"Create a barrier asset digital with strict terminal exercise.

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
observation_times : iterable of datetime.datetime or None, optional
    Nonempty, strictly increasing times within life; None selects continuous monitoring.
touch_state : {'untouched', 'touched'} or None, optional
    Strictly prior history. None means unknown; untouched means known no
    hit.
calendar : TradingCalendar, optional
    Explicit override. Omission or Ellipsis inherits a supplied
    ObservationSchedule calendar; otherwise construction uses weekdays
    and replacement retains the current calendar.

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
Barrier equality counts as a touch. Trading-calendar compatibility is
checked on construction; strictly prior history is checked during pricing.
Terminal strike equality pays zero.
"
);

pub(crate) fn settlement(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::SettlementTiming> {
    match value.extract::<&str>()? {
        "at_hit" => Ok(kiyosi::SettlementTiming::AtHit),
        "at_expiry" => Ok(kiyosi::SettlementTiming::AtExpiry),
        _ => Err(PyValueError::new_err("unknown settlement_timing")),
    }
}

macro_rules! touch_factory {
    ($name:ident, $constructor:ident, $kind:literal, $barriers:literal $(, $payout:ident)?) => {
        #[doc = concat!("Create a cash or asset ", $kind, " contract.")]
        ///
        /// Parameters
        /// ----------
        /// effective_time : datetime.datetime
        ///     Inclusive contract start.
        /// expiry_time : datetime.datetime
        ///     Inclusive expiry time.
        /// barrier_level : float
        ///     Positive finite trigger level; equality counts as a touch.
        #[doc = concat!("barrier_type : ", $barriers)]
        #[doc = concat!("    Select the direction of the ", $kind, " barrier.")]
        ///
        $(
            #[doc = concat!(stringify!($payout), " : float")]
            ///     Positive finite cash payment.
        )?
        /// settlement_timing : {'at_hit', 'at_expiry'}, optional
        ///     Defaults to expiry. At-hit settlement requires a one-touch contract.
        /// observation_times : iterable of datetime.datetime or None, optional
        ///     Nonempty, strictly increasing times within life; None selects continuous monitoring.
        /// touch_state : {'untouched', 'touched'} or None, optional
        ///     Strictly prior history. None is unknown; untouched means known no hit.
        /// calendar : TradingCalendar, optional
        ///     Explicit override. Omission or Ellipsis inherits a supplied
        ///     ObservationSchedule calendar; otherwise construction uses weekdays
        ///     and replacement retains the current calendar.
        ///
        /// Returns
        /// -------
        /// TouchOption
        ///     Validated owned contract. Asset settlement has no cash payout.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If the barrier's payment condition disagrees with the factory, amounts,
        ///     life times or observations are invalid, or no-touch settlement is at-hit.
        ///
        /// Notes
        /// -----
        /// Trading-calendar consistency is checked on construction; history is checked
        /// during pricing. Prior at-hit payments have no remaining value. A current
        /// asset hit transfers spot; a future continuous asset hit transfers the
        /// barrier amount. See the kiyosi.instruments module examples for
        /// construction.
        ///
        #[pyfunction]
        #[pyo3(signature = (*, effective_time, expiry_time, barrier_level, barrier_type, $($payout,)? settlement_timing=kiyosi::SettlementTiming::default(), observation_times=None, touch_state=None, calendar=crate::conversions::OMITTED))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(effective_time: &Bound<'_, PyAny>, expiry_time: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, barrier_type: &Bound<'_, PyAny>,
            $($payout: &Bound<'_, PyAny>,)?
            #[pyo3(from_py_with = settlement)] settlement_timing: kiyosi::SettlementTiming,

            observation_times: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = barrier::history)] touch_state: Option<kiyosi::BarrierTouchState>,
            #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

        ) -> PyResult<TouchOption> {
            let terms = touch_terms(effective_time, expiry_time, barrier_level, barrier_type)?;
            let terms = touch_monitoring(terms, settlement_timing, observation_times, touch_state, calendar)?;
            Ok(TouchOption(domain(kiyosi::TouchOption::$constructor(terms $(, real($payout)?)?))?))
        }
    };
}
touch_factory!(
    cash_touch_option,
    cash_one_touch,
    "one-touch",
    "{'up_and_in', 'down_and_in'}",
    payout
);
touch_factory!(
    asset_touch_option,
    asset_one_touch,
    "one-touch",
    "{'up_and_in', 'down_and_in'}"
);
touch_factory!(
    cash_no_touch_option,
    cash_no_touch,
    "no-touch",
    "{'up_and_out', 'down_and_out'}",
    payout
);
touch_factory!(
    asset_no_touch_option,
    asset_no_touch,
    "no-touch",
    "{'up_and_out', 'down_and_out'}"
);

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
