use crate::calendar::TradingCalendar;
use crate::conversions::{python_timestamp, real, supplied, timestamp, timestamps};
use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

macro_rules! autocallable_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pymethods]
        impl $name {
            $($extra)*
            /// Return the timestamp through which supplied history is known, or None.
            #[getter]
            fn history_time<'py>(&self, py: pyo3::Python<'py>) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::PyAny>>> {
                kiyosi::OptionContract::history_time(&self.0).map(|time| crate::conversions::python_timestamp(py, time)).transpose()
            }

            /// Returns a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            ///     Strictly prior events. None is unknown; the string no_event means no
            ///     event.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omission or Ellipsis retains history.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation. The original is unchanged.
            #[pyo3(signature = (*, barrier_state=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
            fn replace_history(
                &self,
                #[pyo3(from_py_with = supplied)] barrier_state: Option<&Bound<'_, PyAny>>,

                #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
            ) -> PyResult<Self> {
                let history_time = history_time
                    .map(crate::conversions::optional_timestamp)
                    .transpose()?
                    .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

                let history = match barrier_state {
                    Some(value) => state(value)?,
                    None => self.0.barrier_state(),
                };
                Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(self.0.replace_history(history))?, history_time))?))
            }
            /// Returns an owned copy of the contract's monitoring calendar.
            #[getter]
            fn calendar(&self) -> TradingCalendar { TradingCalendar(self.0.calendar().clone()) }
            /// Returns owned copies of the contractual observations.
            #[getter]
            fn observations(&self) -> Vec<AutocallableObservation> {
                self.0
                    .observations()
                    .iter()
                    .copied()
                    .map(AutocallableObservation)
                    .collect()
            }
            /// Returns an owned whole-life knock-in schedule, or None without knock-in.
            ///
            /// Daily monitoring includes the effective instant at the contractual clock
            /// on open dates. Expiry-only monitoring returns just expiry. Observations
            /// for knock-out and coupons remain available through observations.
            #[getter]
            fn knock_in_observation_schedule(&self) -> PyResult<Option<crate::schedule::ObservationSchedule>> {
                Ok(domain(self.0.knock_in_observation_schedule())?.map(crate::schedule::ObservationSchedule))
            }
            /// Returns a copy of the contractual observation times.
            #[getter]
            fn observation_times<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
                self.0
                    .observations()
                    .iter()
                    .map(|r| python_timestamp(py, r.time()))
                    .collect()
            }
            /// Returns copied knock-out thresholds in observation order.
            #[getter]
            fn knock_out_levels(&self) -> Vec<f64> {
                self.0
                    .observations()
                    .iter()
                    .map(|r| r.knock_out_level())
                    .collect()
            }
            /// Returns nonnegative normalized principal, independent of coupons and losses.
            #[getter]
            fn principal_ratio(&self) -> f64 {
                self.0.terms().principal_ratio
            }
            /// Returns strictly prior autocallable history, or None if unknown.
            #[getter]
            fn barrier_state(&self) -> Option<&'static str> {
                state_name(self.0.barrier_state())
            }
            /// Returns inclusive first contract timestamp.
            #[getter]
            fn effective_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                python_timestamp(py, self.0.effective_time())
            }
            /// Returns inclusive expiry time.
            #[getter]
            fn expiry_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                python_timestamp(py, self.0.expiry_time())
            }
        }
    };
}

macro_rules! snowball_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        autocallable_properties! {
            $name,
            $($extra,)*
            /// Returns an independent note with its observation coupon schedule shifted.
            ///
            /// Parameters
            /// ----------
            /// first_observation_coupon : float
            ///     New first observation coupon; all observation coupons shift equally.
            /// quote_convention : CouponQuoteConvention
            ///     Whether to shift the maturity coupon or preserve it.
            ///
            /// Returns
            /// -------
            /// object
            ///     New note of the same type, preserving history and other terms.
            ///
            /// Raises
            /// ------
            /// InvalidInputError
            ///     If shifted coupons are non-finite or violate contractual constraints.
            #[pyo3(signature = (first_observation_coupon, *, quote_convention))]
            fn with_shifted_coupons(
                &self,
                first_observation_coupon: &Bound<'_, PyAny>,
                quote_convention: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(self.0.with_shifted_coupons(real(first_observation_coupon)?, coupon_convention(quote_convention)?))?))
            }
        }
    };
}

pub(crate) fn coupon_convention(
    value: &Bound<'_, PyAny>,
) -> PyResult<kiyosi::CouponQuoteConvention> {
    match value.extract::<&str>()? {
        "shift_maturity_coupon" => Ok(kiyosi::CouponQuoteConvention::ShiftMaturityCoupon),
        "preserve_maturity_coupon" => Ok(kiyosi::CouponQuoteConvention::PreserveMaturityCoupon),
        _ => Err(PyValueError::new_err("unknown coupon quote convention")),
    }
}

pub(crate) fn state(
    value: &Bound<'_, PyAny>,
) -> PyResult<Option<kiyosi::AutocallableBarrierState>> {
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(match value.extract::<&str>()? {
        "no_event" => kiyosi::AutocallableBarrierState::NoEvent,
        "knocked_in" => kiyosi::AutocallableBarrierState::KnockedIn,
        "knocked_out" => kiyosi::AutocallableBarrierState::KnockedOut,
        _ => return Err(PyValueError::new_err("unknown autocallable barrier_state")),
    }))
}
fn state_name(value: Option<kiyosi::AutocallableBarrierState>) -> Option<&'static str> {
    value.map(|s| match s {
        kiyosi::AutocallableBarrierState::NoEvent => "no_event",
        kiyosi::AutocallableBarrierState::KnockedIn => "knocked_in",
        kiyosi::AutocallableBarrierState::KnockedOut => "knocked_out",
    })
}
pub(crate) fn mode(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::KnockInObservationMode> {
    match value.extract::<&str>()? {
        "every_trading_day" => Ok(kiyosi::KnockInObservationMode::EveryTradingDay),
        "at_expiry" => Ok(kiyosi::KnockInObservationMode::AtExpiry),
        _ => Err(PyValueError::new_err("unknown knock_in_observation_mode")),
    }
}
fn mode_name(value: kiyosi::KnockInObservationMode) -> &'static str {
    match value {
        kiyosi::KnockInObservationMode::EveryTradingDay => "every_trading_day",
        kiyosi::KnockInObservationMode::AtExpiry => "at_expiry",
    }
}
pub(crate) fn records(values: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::AutocallableObservation>> {
    values
        .try_iter()?
        .map(|item| Ok(item?.extract::<PyRef<'_, AutocallableObservation>>()?.0))
        .collect()
}
fn reals(values: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    values.try_iter()?.map(|item| real(&item?)).collect()
}

/// Observation with validated numeric terms for an autocallable note.
///
/// Parameters
/// ----------
/// time : datetime.datetime
///     Exact observation time.
/// knock_out_level : float
///     Positive inclusive knock-out threshold.
/// Use snowball(..., coupon_rate=...) or phoenix(..., coupon_barrier_level=...)
/// for product-specific coupon terms. The basic constructor uses zero coupon fields.
///
/// Notes
/// -----
/// Numeric terms are validated in Rust on construction. Note construction also
/// validates observation ordering, life bounds and product-specific constraints.
/// Invalid numeric terms raise InvalidInputError.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AutocallableObservation(pub kiyosi::AutocallableObservation);
#[pymethods]
impl AutocallableObservation {
    #[new]
    #[pyo3(signature = (time, knock_out_level))]
    fn new(time: &Bound<'_, PyAny>, knock_out_level: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::AutocallableObservation::new(
            timestamp(time)?,
            real(knock_out_level)?,
        ))?))
    }
    /// Create a snowball observation with an annual knock-out coupon.
    #[staticmethod]
    #[pyo3(signature = (time, knock_out_level, *, coupon_rate))]
    fn snowball(
        time: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        coupon_rate: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::AutocallableObservation::snowball(
            timestamp(time)?,
            real(knock_out_level)?,
            real(coupon_rate)?,
        ))?))
    }
    /// Create a Phoenix observation with a coupon threshold; the note supplies the amount.
    #[staticmethod]
    #[pyo3(signature = (time, knock_out_level, *, coupon_barrier_level))]
    fn phoenix(
        time: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        coupon_barrier_level: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::AutocallableObservation::phoenix(
            timestamp(time)?,
            real(knock_out_level)?,
            real(coupon_barrier_level)?,
        ))?))
    }
    /// Return a validated observation with the supplied fields replaced.
    ///
    /// Omitted fields or Ellipsis retain their values. Invalid numeric terms raise
    /// InvalidInputError; representation failures use native Python exceptions.
    /// Note construction checks schedule and product-specific constraints.
    #[pyo3(signature = (*, time=crate::conversions::OMITTED, knock_out_level=crate::conversions::OMITTED, coupon_rate=crate::conversions::OMITTED, coupon_barrier_level=crate::conversions::OMITTED))]
    fn replace(
        &self,
        #[pyo3(from_py_with = supplied)] time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] knock_out_level: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] coupon_barrier_level: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self(domain(self.0.replace(
            kiyosi::AutocallableObservationChanges {
                time: time.map(timestamp).transpose()?,
                knock_out_level: knock_out_level.map(real).transpose()?,
                coupon_rate: coupon_rate.map(real).transpose()?,
                coupon_barrier_level: coupon_barrier_level.map(real).transpose()?,
            },
        ))?))
    }
    /// Return exact observation time.
    #[getter]
    fn time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.time())
    }
    /// Return the positive finite knock-out threshold.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    /// Return the finite signed annual coupon in decimal units.
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.0.coupon_rate()
    }
    /// Return the finite nonnegative Phoenix coupon threshold.
    #[getter]
    fn coupon_barrier_level(&self) -> f64 {
        self.0.coupon_barrier_level()
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let (kind, field, value) = if self.0.coupon_barrier_level() == 0.0 {
            ("snowball", "coupon_rate", self.0.coupon_rate())
        } else {
            (
                "phoenix",
                "coupon_barrier_level",
                self.0.coupon_barrier_level(),
            )
        };
        Ok(format!(
            "AutocallableObservation.{kind}({}, {:?}, {field}={value:?})",
            self.time(py)?.repr()?,
            self.0.knock_out_level()
        ))
    }
}

/// Associate parallel sequences into input observation records.
///
/// Parameters
/// ----------
/// observation_times : iterable of datetime.datetime
///     Observation times; the note constructor checks ordering and life bounds.
/// knock_out_levels : iterable of float
///     Positive inclusive knock-out levels, one per observation timestamp.
/// knock_out_coupon_rates : iterable of float or None, optional
///     Annual snowball coupons; None uses zero for every observation.
/// coupon_barrier_levels : iterable of float or None, optional
///     Phoenix coupon thresholds; None uses zero for every observation.
///
/// Returns
/// -------
/// list of AutocallableObservation
///     Owned result; no input is mutated.
///
/// Raises
/// ------
/// InvalidInputError
///     If sequence lengths differ or numeric levels or coupons are invalid.
///
/// Notes
/// -----
/// The note constructor validates ordering, life bounds and product-specific constraints.
#[pyfunction]
#[pyo3(signature = (observation_times, knock_out_levels, *, knock_out_coupon_rates=None, coupon_barrier_levels=None))]
pub(crate) fn autocallable_observations(
    observation_times: &Bound<'_, PyAny>,
    knock_out_levels: &Bound<'_, PyAny>,
    knock_out_coupon_rates: Option<&Bound<'_, PyAny>>,
    coupon_barrier_levels: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<AutocallableObservation>> {
    let times = timestamps(observation_times)?;
    let levels = reals(knock_out_levels)?;
    let coupons = knock_out_coupon_rates.map(reals).transpose()?;
    let barriers = coupon_barrier_levels.map(reals).transpose()?;
    Ok(domain(kiyosi::autocallable_observations(
        &times,
        &levels,
        coupons.as_deref(),
        barriers.as_deref(),
    ))?
    .into_iter()
    .map(AutocallableObservation)
    .collect())
}

macro_rules! snowball_preset {
    ($name:ident, $clock:ident $(= $clock_default:tt)?, $coupon:ident => $coupon_doc:literal, $level:ident => $level_doc:literal,
     [$($extra:ident => $extra_doc:literal => $convert:ident),*], $preset:expr,
     $summary:literal, $monitoring:literal, $notes:literal) => {
        #[doc = $summary]
        ///
        /// Parameters
        /// ----------
        #[doc = concat!(stringify!($coupon), " : float")]
        #[doc = concat!("    ", $coupon_doc)]
        /// initial_spot : float
        ///     Positive reference spot used to normalize downside losses.
        /// knock_in_level : float
        ///     Positive strict downside threshold.
        #[doc = concat!(stringify!($level), " : float")]
        #[doc = concat!("    ", $level_doc)]
        $(
            #[doc = concat!(stringify!($extra), " : ", snowball_preset!(@type $convert))]
            #[doc = concat!("    ", $extra_doc)]
        )*
        /// observation_clock_time : datetime.time or None
        ///     Required for daily monitoring; absent for expiry-only monitoring.
        /// observation_times : iterable of datetime.datetime
        ///     Strictly increasing observation times within the contract life.
        /// effective_time : datetime.datetime
        ///     Inclusive contract start.
        /// expiry_time : datetime.datetime
        ///     Inclusive expiry time.
        /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        ///     Strictly prior events. None is unknown; the string no_event means no event.
        /// principal_ratio : float, optional
        ///     Nonnegative normalized principal, independent of coupons and losses.
        /// calendar : TradingCalendar, optional
        ///     Owned contractual calendar; defaults to weekdays.
        ///
        /// Returns
        /// -------
        /// SnowballOption
        ///     Validated owned contract.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If contractual amounts, life times or observation terms are invalid.
        ///
        /// Notes
        /// -----
        #[doc = concat!("Knock-in is observed ", $monitoring, ".")]
        /// Principal defaults to one. The lower strike is zero and the upper strike
        /// is initial spot unless the preset overrides them. Calendar compatibility
        /// is checked on construction; history is checked during pricing.
        #[doc = $notes]
        #[pyfunction]
        #[pyo3(signature = (*, $coupon, initial_spot, knock_in_level, $level, $($extra,)* observation_times, effective_time, expiry_time, $clock $(= $clock_default)?, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO, calendar=TradingCalendar::default(), history_time=None))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
            $coupon: &Bound<'_, PyAny>,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            $level: &Bound<'_, PyAny>,
            $($extra: &Bound<'_, PyAny>,)*
            observation_times: &Bound<'_, PyAny>,
            effective_time: &Bound<'_, PyAny>,
            expiry_time: &Bound<'_, PyAny>,
            $clock: Option<&Bound<'_, PyAny>>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = real)] principal_ratio: f64,
            calendar: TradingCalendar,

            #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
        ) -> PyResult<SnowballOption> {
            let mut terms = kiyosi::SnowballPresetTerms::new(
                real($coupon)?, real(initial_spot)?, real(knock_in_level)?, real($level)?,
                timestamp(effective_time)?, timestamp(expiry_time)?, timestamps(observation_times)?,
                $clock.map(crate::conversions::clock_time).transpose()?,
            );
            $(let $extra = $convert($extra)?;)*
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_ratio = principal_ratio;
            terms.calendar = calendar.0;
            Ok(SnowballOption(domain(kiyosi::OptionContract::with_history_time(domain(terms.expand($preset))?, history_time))?))
        }
    };
    (@type real) => { "float" };
    (@type timestamp) => { "datetime.datetime" };
}
snowball_preset!(
    standard_snowball,
    observation_clock_time,
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [],
    kiyosi::SnowballPreset::Standard,
    "Creates constant knock-out coupons and levels with daily knock-in monitoring.",
    "on every trading day",
    ""
);
snowball_preset!(
    european_snowball,
    observation_clock_time=None,
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [],
    kiyosi::SnowballPreset::European,
    "Creates a snowball with knock-in observed only at expiry.",
    "only at expiry",
    ""
);
snowball_preset!(
    step_down_snowball,
    observation_clock_time,
    coupon_rate => "Finite annual coupon rate in decimal units.",
    initial_knock_out_level => "Positive knock-out threshold at the first observation.",
    [knock_out_level_decrement => "Absolute price decrement after each observation; may be negative." => real],
    kiyosi::SnowballPreset::StepDown {
        knock_out_level_decrement
    },
    "Creates a snowball with decreasing knock-out levels.",
    "on every trading day",
    r"Decrements apply after each observation; the first uses the initial
value. Signed coupons and decrements are accepted if all expanded terms
remain valid."
);
snowball_preset!(
    both_down_snowball,
    observation_clock_time,
    initial_coupon_rate => "Annual coupon rate at the first observation.",
    initial_knock_out_level => "Positive knock-out threshold at the first observation.",
    [
        coupon_rate_decrement => "Absolute decimal-rate decrement after each observation." => real,
        knock_out_level_decrement => "Absolute price decrement after each observation; may be negative." => real
    ],
    kiyosi::SnowballPreset::BothDown {
        coupon_rate_decrement,
        knock_out_level_decrement
    },
    "Creates a snowball with decreasing coupons and knock-out levels.",
    "on every trading day",
    r"Decrements apply after each observation; the first uses the initial
value. Signed coupons and decrements are accepted if all expanded terms
remain valid. Maturity uses the final observation coupon."
);
snowball_preset!(
    dual_coupon_snowball,
    observation_clock_time,
    knock_out_coupon_rate => "Annual coupon rate for knock-out settlement.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [maturity_coupon_rate => "Finite annual maturity coupon rate in decimal units." => real],
    kiyosi::SnowballPreset::DualCoupon {
        maturity_coupon_rate
    },
    "Creates a snowball with separate knock-out and maturity coupons.",
    "on every trading day",
    ""
);
snowball_preset!(
    parachute_snowball,
    observation_clock_time,
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [final_knock_out_level => "Positive threshold replacing only the final knock-out level." => real],
    kiyosi::SnowballPreset::Parachute {
        final_knock_out_level
    },
    "Creates a snowball with a distinct final knock-out level.",
    "on every trading day",
    ""
);
snowball_preset!(
    otm_snowball,
    observation_clock_time,
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [upper_strike => "Positive upper terminal settlement strike." => real],
    kiyosi::SnowballPreset::Otm { upper_strike },
    "Creates a snowball with a replaced upper terminal strike.",
    "on every trading day",
    ""
);
snowball_preset!(
    loss_capped_snowball,
    observation_clock_time,
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [lower_strike => "Nonnegative lower terminal strike, at most the upper strike." => real],
    kiyosi::SnowballPreset::LossCapped { lower_strike },
    "Creates a snowball whose lower terminal strike caps downside loss.",
    "on every trading day",
    ""
);

snowball_preset!(
    early_profit_snowball,
    observation_clock_time,
    first_coupon_rate => "Finite signed annual coupon rate through first_stage_end_time.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [
        second_coupon_rate => "Finite signed annual coupon rate after the cutoff and at maturity." => real,
        first_stage_end_time => "Inclusive first-stage end within life and strictly before expiry." => timestamp
    ],
    kiyosi::SnowballPreset::EarlyProfit { first_stage_end_time, second_coupon_rate },
    "Creates a two-stage early-profit snowball.",
    "on every trading day",
    r"Knock-out uses the observation's stage rate over the whole holding period.
Maturity uses the final stage rate. Cutoffs need not be observation or trading
times, stages may have no observations, and rates need not decrease."
);
snowball_preset!(
    butterfly_snowball,
    observation_clock_time,
    first_coupon_rate => "Finite signed annual coupon rate through first_stage_end_time.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [
        second_coupon_rate => "Finite signed annual coupon in the second stage." => real,
        third_coupon_rate => "Finite signed annual coupon in the third stage and at maturity." => real,
        first_stage_end_time => "Inclusive first-stage end within life and before second_stage_end_time." => timestamp,
        second_stage_end_time => "Inclusive second-stage end strictly before expiry." => timestamp
    ],
    kiyosi::SnowballPreset::Butterfly {
        first_stage_end_time, second_coupon_rate, second_stage_end_time, third_coupon_rate
    },
    "Creates a three-stage butterfly snowball.",
    "on every trading day",
    r"Knock-out uses the observation's stage rate over the whole holding period.
Maturity uses the final stage rate. Cutoffs need not be observation or trading
times, stages may have no observations, and rates need not decrease."
);

/// Snowball with knock-out coupons and capped knock-in downside loss.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// initial_spot : float
///     Positive reference spot used to normalize downside losses.
/// knock_in_level : float
///     Positive strict downside threshold.
/// upper_strike : float
///     Positive upper terminal settlement strike.
/// lower_strike : float
///     Nonnegative lower terminal strike, at most the upper strike.
/// observation_clock_time : datetime.time or None
///     Explicit daily clock; supply None for expiry-only monitoring.
/// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
///     Observe knock-in on every trading day or only at expiry.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string no_event means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If amounts, strikes, life times or observation terms are invalid.
///     Calendar compatibility is checked on construction; prior barrier history
///     is checked during pricing.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct SnowballOption(pub kiyosi::SnowballOption);
snowball_properties! {
    SnowballOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// maturity_coupon_rate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// initial_spot : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_in_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// upper_strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// lower_strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_clock_time : datetime.time or None, optional
    ///     Required for daily monitoring; absent for expiry-only monitoring.
    /// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// principal_ratio : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observations : Iterable[AutocallableObservation], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
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
    #[pyo3(signature = (*, maturity_coupon_rate=crate::conversions::OMITTED, initial_spot=crate::conversions::OMITTED, knock_in_level=crate::conversions::OMITTED, upper_strike=crate::conversions::OMITTED, lower_strike=crate::conversions::OMITTED, knock_in_observation_mode=crate::conversions::OMITTED, observation_clock_time=crate::conversions::OMITTED, barrier_state=crate::conversions::OMITTED, principal_ratio=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, observations=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "explicit Python replacement fields")]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] maturity_coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] initial_spot: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_in_level: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] upper_strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] lower_strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_in_observation_mode: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] observation_clock_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] principal_ratio: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] observations: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::SnowballOptionChanges {
            history_time: Some(history_time),
            calendar: calendar.map(|v| v.extract::<TradingCalendar>().map(|v| v.0)).transpose()?,
            maturity_coupon_rate: maturity_coupon_rate.map(crate::conversions::real).transpose()?,
            initial_spot: initial_spot.map(crate::conversions::real).transpose()?,
            knock_in_level: knock_in_level.map(crate::conversions::real).transpose()?,
            upper_strike: upper_strike.map(crate::conversions::real).transpose()?,
            lower_strike: lower_strike.map(crate::conversions::real).transpose()?,
            knock_in_observation_mode: knock_in_observation_mode.map(crate::instruments::autocallable::mode).transpose()?,
            observation_clock_time: observation_clock_time.map(crate::conversions::optional_clock_time).transpose()?,
            barrier_state: barrier_state.map(crate::instruments::autocallable::state).transpose()?,
            principal_ratio: principal_ratio.map(crate::conversions::real).transpose()?,
            effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            observations: observations.map(crate::instruments::autocallable::records).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
,
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode=kiyosi::KnockInObservationMode::default(), observation_clock_time, effective_time, expiry_time, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO, calendar=TradingCalendar::default(), history_time=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = mode)] knock_in_observation_mode: kiyosi::KnockInObservationMode,
        observation_clock_time: Option<&Bound<'_, PyAny>>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
        calendar: TradingCalendar,

        #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::SnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(initial_spot)?,
            real(knock_in_level)?,
            real(upper_strike)?,
            real(lower_strike)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            records(observations)?,
            observation_clock_time.map(crate::conversions::clock_time).transpose()?,
        );
        terms.knock_in_observation_mode = knock_in_observation_mode;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(kiyosi::SnowballOption::new(terms))?, history_time))?))
    },
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(kiyosi::AutocallableObservation::coupon_rate)
            .collect()
    },
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    },
    /// Return positive reference spot used to normalize downside losses.
    #[getter]
    fn initial_spot(&self) -> f64 {
        self.0.terms().initial_spot
    },
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    },
    /// Return positive upper terminal settlement strike.
    #[getter]
    fn upper_strike(&self) -> f64 {
        self.0.terms().upper_strike
    },
    /// Return nonnegative lower terminal strike, at most the upper strike.
    #[getter]
    fn lower_strike(&self) -> f64 {
        self.0.terms().lower_strike
    },
    /// Returns the daily observation clock time, or None for expiry-only monitoring.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0.observation_clock_time().map(|value| crate::conversions::python_clock_time(py, value)).transpose()
    },
    /// Returns the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    },
}

/// Snowball with knock-out coupons and no knock-in or downside loss.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string no_event means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If coupons, principal, times or observations are invalid, or
///     barrier_state is knocked_in; this contract has no knock-in barrier.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinarySnowballOption(pub kiyosi::BinarySnowballOption);
snowball_properties! {
    BinarySnowballOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// maturity_coupon_rate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// principal_ratio : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observations : Iterable[AutocallableObservation], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
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
    #[pyo3(signature = (*, maturity_coupon_rate=crate::conversions::OMITTED, barrier_state=crate::conversions::OMITTED, principal_ratio=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, observations=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "explicit Python replacement fields")]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] maturity_coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] principal_ratio: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] observations: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::BinarySnowballOptionChanges {
            history_time: Some(history_time),
            calendar: calendar.map(|v| v.extract::<TradingCalendar>().map(|v| v.0)).transpose()?,
            maturity_coupon_rate: maturity_coupon_rate.map(crate::conversions::real).transpose()?,
            barrier_state: barrier_state.map(crate::instruments::autocallable::state).transpose()?,
            principal_ratio: principal_ratio.map(crate::conversions::real).transpose()?,
            effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            observations: observations.map(crate::instruments::autocallable::records).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
,
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, effective_time, expiry_time, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO, calendar=TradingCalendar::default(), history_time=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
        calendar: TradingCalendar,

        #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::BinarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            records(observations)?,
        );
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(kiyosi::BinarySnowballOption::new(terms))?, history_time))?))
    },
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(kiyosi::AutocallableObservation::coupon_rate)
            .collect()
    },
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    },
}

/// Snowball with distinct intact and knocked-in maturity coupons.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// minimum_coupon_rate : float
///     Knocked-in maturity coupon, at most the intact maturity coupon.
/// knock_in_level : float
///     Positive strict downside threshold.
/// observation_clock_time : datetime.time or None
///     Explicit daily clock; supply None for expiry-only monitoring.
/// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
///     Observe knock-in on every trading day or only at expiry.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string no_event means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If coupons, principal, barriers, times or observations are invalid.
///     minimum_coupon_rate must not exceed maturity_coupon_rate.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TernarySnowballOption(pub kiyosi::TernarySnowballOption);
snowball_properties! {
    TernarySnowballOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// maturity_coupon_rate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// minimum_coupon_rate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_in_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_clock_time : datetime.time or None, optional
    ///     Required for daily monitoring; absent for expiry-only monitoring.
    /// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// principal_ratio : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observations : Iterable[AutocallableObservation], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
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
    #[pyo3(signature = (*, maturity_coupon_rate=crate::conversions::OMITTED, minimum_coupon_rate=crate::conversions::OMITTED, knock_in_level=crate::conversions::OMITTED, knock_in_observation_mode=crate::conversions::OMITTED, observation_clock_time=crate::conversions::OMITTED, barrier_state=crate::conversions::OMITTED, principal_ratio=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, observations=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "explicit Python replacement fields")]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] maturity_coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] minimum_coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_in_level: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_in_observation_mode: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] observation_clock_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] principal_ratio: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] observations: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::TernarySnowballOptionChanges {
            history_time: Some(history_time),
            calendar: calendar.map(|v| v.extract::<TradingCalendar>().map(|v| v.0)).transpose()?,
            maturity_coupon_rate: maturity_coupon_rate.map(crate::conversions::real).transpose()?,
            minimum_coupon_rate: minimum_coupon_rate.map(crate::conversions::real).transpose()?,
            knock_in_level: knock_in_level.map(crate::conversions::real).transpose()?,
            knock_in_observation_mode: knock_in_observation_mode.map(crate::instruments::autocallable::mode).transpose()?,
            observation_clock_time: observation_clock_time.map(crate::conversions::optional_clock_time).transpose()?,
            barrier_state: barrier_state.map(crate::instruments::autocallable::state).transpose()?,
            principal_ratio: principal_ratio.map(crate::conversions::real).transpose()?,
            effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            observations: observations.map(crate::instruments::autocallable::records).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
,
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation_mode=kiyosi::KnockInObservationMode::default(), observation_clock_time, effective_time, expiry_time, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO, calendar=TradingCalendar::default(), history_time=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        minimum_coupon_rate: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = mode)] knock_in_observation_mode: kiyosi::KnockInObservationMode,
        observation_clock_time: Option<&Bound<'_, PyAny>>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
        calendar: TradingCalendar,

        #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::TernarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(minimum_coupon_rate)?,
            real(knock_in_level)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            records(observations)?,
            observation_clock_time.map(crate::conversions::clock_time).transpose()?,
        );
        terms.knock_in_observation_mode = knock_in_observation_mode;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(kiyosi::TernarySnowballOption::new(terms))?, history_time))?))
    },
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(kiyosi::AutocallableObservation::coupon_rate)
            .collect()
    },
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    },
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    },
    /// Return the annual knocked-in maturity coupon.
    #[getter]
    fn minimum_coupon_rate(&self) -> f64 {
        self.0.terms().minimum_coupon_rate
    },
    /// Returns the daily observation clock time, or None for expiry-only monitoring.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0.observation_clock_time().map(|value| crate::conversions::python_clock_time(py, value)).transpose()
    },
    /// Returns the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    },
}

/// Autocallable with conditional period coupons and capped downside loss.
///
/// Parameters
/// ----------
/// coupon_rate : float, optional
///     Finite signed annual coupon rate. Supply exactly one of coupon_rate
///     and coupon_per_period; omission, None or Ellipsis leaves a choice unspecified.
/// coupon_per_period : float, optional
///     Finite signed fixed coupon proportion per qualifying observation,
///     without day accrual. The unused coupon property returns None.
/// initial_spot : float
///     Positive reference spot used to normalize downside losses.
/// knock_in_level : float
///     Positive strict downside threshold.
/// upper_strike : float
///     Positive upper terminal settlement strike.
/// lower_strike : float
///     Nonnegative lower terminal strike, at most the upper strike.
/// observation_clock_time : datetime.time or None
///     Explicit daily clock; supply None for expiry-only monitoring.
/// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
///     Observe knock-in on every trading day or only at expiry.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string no_event means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of PricingContext.
///
/// Raises
/// ------
/// TypeError
///     If both coupon choices or neither are supplied, or a supplied value is not real.
/// InvalidInputError
///     If amounts, strikes, life times or observations are invalid.
///     Observation coupon_rate must be zero; coupon_barrier_level is nonnegative.
///
/// Notes
/// -----
/// Annual coupons accrue over contractual observation periods on Actual/365 Fixed.
/// Fixed per-period coupons pay once per qualifying observation, even for an
/// observation on the effective timestamp. Explicit None is not a coupon value.
/// Coupon and knock-out barriers are inclusive; knock-in is strict. Past
/// coupons are settled. There is no coupon memory or maturity coupon.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PhoenixOption(pub kiyosi::PhoenixOption);
autocallable_properties! {
    PhoenixOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// coupon_rate : float, optional
    ///     Replacement value; None, omission or Ellipsis leaves this coupon choice unspecified.
    /// coupon_per_period : float, optional
    ///     Replacement value; None, omission or Ellipsis leaves this coupon choice unspecified.
    /// initial_spot : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_in_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// upper_strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// lower_strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_clock_time : datetime.time or None, optional
    ///     Required for daily monitoring; absent for expiry-only monitoring.
    /// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// principal_ratio : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observations : Iterable[AutocallableObservation], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
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
    ///     If a representation or field name is invalid, or both coupon choices have values.
    #[pyo3(signature = (*, coupon_rate=crate::conversions::OMITTED, coupon_per_period=crate::conversions::OMITTED, initial_spot=crate::conversions::OMITTED, knock_in_level=crate::conversions::OMITTED, upper_strike=crate::conversions::OMITTED, lower_strike=crate::conversions::OMITTED, knock_in_observation_mode=crate::conversions::OMITTED, observation_clock_time=crate::conversions::OMITTED, barrier_state=crate::conversions::OMITTED, principal_ratio=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, observations=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "explicit Python replacement fields")]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] coupon_per_period: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] initial_spot: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_in_level: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] upper_strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] lower_strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_in_observation_mode: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] observation_clock_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] principal_ratio: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] observations: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::PhoenixOptionChanges {
            history_time: Some(history_time),
            calendar: calendar.map(|v| v.extract::<TradingCalendar>().map(|v| v.0)).transpose()?,
            coupon: match (coupon_rate.filter(|v| !v.is_none()), coupon_per_period.filter(|v| !v.is_none())) {
                (None, None) => None,
                (Some(value), None) => Some(kiyosi::PhoenixCoupon::Annual(crate::conversions::real(value)?)),
                (None, Some(value)) => Some(kiyosi::PhoenixCoupon::PerPeriod(crate::conversions::real(value)?)),
                _ => return Err(pyo3::exceptions::PyTypeError::new_err("supply at most one coupon choice")),
            },
            initial_spot: initial_spot.map(crate::conversions::real).transpose()?,
            knock_in_level: knock_in_level.map(crate::conversions::real).transpose()?,
            upper_strike: upper_strike.map(crate::conversions::real).transpose()?,
            lower_strike: lower_strike.map(crate::conversions::real).transpose()?,
            knock_in_observation_mode: knock_in_observation_mode.map(crate::instruments::autocallable::mode).transpose()?,
            observation_clock_time: observation_clock_time.map(crate::conversions::optional_clock_time).transpose()?,
            barrier_state: barrier_state.map(crate::instruments::autocallable::state).transpose()?,
            principal_ratio: principal_ratio.map(crate::conversions::real).transpose()?,
            effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            observations: observations.map(crate::instruments::autocallable::records).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
,
    #[new]
    #[pyo3(signature = (*, coupon_rate=crate::conversions::OMITTED, coupon_per_period=crate::conversions::OMITTED, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode=kiyosi::KnockInObservationMode::default(), observation_clock_time, effective_time, expiry_time, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO, calendar=TradingCalendar::default(), history_time=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        #[pyo3(from_py_with = supplied)] coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] coupon_per_period: Option<&Bound<'_, PyAny>>,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = mode)] knock_in_observation_mode: kiyosi::KnockInObservationMode,
        observation_clock_time: Option<&Bound<'_, PyAny>>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
        calendar: TradingCalendar,

        #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
    ) -> PyResult<Self> {
        let coupon = match (coupon_rate.filter(|v| !v.is_none()), coupon_per_period.filter(|v| !v.is_none())) {
            (Some(value), None) => kiyosi::PhoenixCoupon::Annual(real(value)?),
            (None, Some(value)) => kiyosi::PhoenixCoupon::PerPeriod(real(value)?),
            _ => return Err(PyTypeError::new_err(
                "supply exactly one of coupon_rate and coupon_per_period",
            )),
        };
        let mut terms = kiyosi::PhoenixTerms::new(
            0.0,
            real(initial_spot)?,
            real(knock_in_level)?,
            real(upper_strike)?,
            real(lower_strike)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            records(observations)?,
            observation_clock_time.map(crate::conversions::clock_time).transpose()?,
        );
        terms.coupon = coupon;
        terms.knock_in_observation_mode = knock_in_observation_mode;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(kiyosi::PhoenixOption::new(terms))?, history_time))?))
    },
    /// Returns a note with the quoted coupon in its existing annual or per-period units.
    ///
    /// Applies implied_phoenix_coupon results while preserving the coupon convention,
    /// history and other terms. Non-finite coupons raise InvalidInputError;
    /// invalid numeric representations raise native conversion errors.
    fn with_quoted_coupon(&self, coupon: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(self.0.with_quoted_coupon(real(coupon)?))?))
    },
    /// Return copied Phoenix coupon thresholds in observation order.
    #[getter]
    fn coupon_barrier_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(kiyosi::AutocallableObservation::coupon_barrier_level)
            .collect()
    },
    /// Returns the annual coupon rate, or None for a fixed per-period coupon.
    #[getter]
    fn coupon_rate(&self) -> Option<f64> {
        match self.0.terms().coupon {
            kiyosi::PhoenixCoupon::Annual(value) => Some(value),
            kiyosi::PhoenixCoupon::PerPeriod(_) => None,
        }
    },
    /// Returns the fixed per-period coupon, or None for an annual coupon.
    #[getter]
    fn coupon_per_period(&self) -> Option<f64> {
        match self.0.terms().coupon {
            kiyosi::PhoenixCoupon::PerPeriod(value) => Some(value),
            kiyosi::PhoenixCoupon::Annual(_) => None,
        }
    },
    /// Return positive reference spot used to normalize downside losses.
    #[getter]
    fn initial_spot(&self) -> f64 {
        self.0.terms().initial_spot
    },
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    },
    /// Return positive upper terminal settlement strike.
    #[getter]
    fn upper_strike(&self) -> f64 {
        self.0.terms().upper_strike
    },
    /// Return nonnegative lower terminal strike, at most the upper strike.
    #[getter]
    fn lower_strike(&self) -> f64 {
        self.0.terms().lower_strike
    },
    /// Returns the daily observation clock time, or None for expiry-only monitoring.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0.observation_clock_time().map(|value| crate::conversions::python_clock_time(py, value)).transpose()
    },
    /// Returns the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    },
}

macro_rules! phoenix_preset {
    ($name:ident, [$($extra:ident => $extra_doc:literal),*], $preset:expr, $summary:literal) => {
        #[doc = $summary]
        ///
        /// Parameters
        /// ----------
        /// coupon_per_period : float
        ///     Finite signed coupon proportion per qualifying observation; not annualized.
        /// initial_spot : float
        ///     Positive reference spot used to normalize downside losses.
        /// knock_in_level : float
        ///     Positive strict expiry knock-in threshold and upper settlement strike.
        /// knock_out_level : float
        ///     Positive inclusive knock-out threshold.
        $(
            #[doc = concat!(stringify!($extra), " : float")]
            #[doc = concat!("    ", $extra_doc)]
        )*
        /// observation_times : iterable of datetime.datetime
        ///     Strictly increasing observation and payment times within life.
        /// effective_time : datetime.datetime
        ///     Inclusive contract start.
        /// expiry_time : datetime.datetime
        ///     Inclusive expiry time.
        /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        ///     Strictly prior events; None means unknown history.
        /// principal_ratio : float, optional
        ///     Nonnegative normalized principal, independent of coupon and loss components.
        /// calendar : TradingCalendar, optional
        ///     Owned contractual calendar; defaults to weekdays.
        ///
        /// Returns
        /// -------
        /// PhoenixOption
        ///     Validated owned contract, with expiry-only knock-in and zero lower strike.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If numeric terms, times or observations are invalid.
        ///
        /// Notes
        /// -----
        /// Coupons pay once per qualifying observation, including a qualifying knock-out,
        /// with no day accrual or memory. Knock-out and coupon barriers are inclusive;
        /// knock-in is strict. Observation times are payment times; no observation
        /// is implicitly added at expiry. Calendar compatibility is checked on
        /// construction; history consistency is checked during pricing. Past coupons are settled.
        #[pyfunction]
        #[pyo3(signature = (*, coupon_per_period, initial_spot, knock_in_level, knock_out_level, $($extra,)* observation_times, effective_time, expiry_time, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO, calendar=TradingCalendar::default(), history_time=None))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
            coupon_per_period: &Bound<'_, PyAny>,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            knock_out_level: &Bound<'_, PyAny>,
            $($extra: &Bound<'_, PyAny>,)*
            observation_times: &Bound<'_, PyAny>,
            effective_time: &Bound<'_, PyAny>,
            expiry_time: &Bound<'_, PyAny>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = real)] principal_ratio: f64,
            calendar: TradingCalendar,

            #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
        ) -> PyResult<PhoenixOption> {
            let mut terms = kiyosi::PhoenixPresetTerms::new(
                real(coupon_per_period)?, real(initial_spot)?, real(knock_in_level)?,
                real(knock_out_level)?, timestamp(effective_time)?, timestamp(expiry_time)?,
                timestamps(observation_times)?,
            );
            $(let $extra = real($extra)?;)*
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_ratio = principal_ratio;
            terms.calendar = calendar.0;
            Ok(PhoenixOption(domain(kiyosi::OptionContract::with_history_time(domain(terms.expand($preset))?, history_time))?))
        }
    };
}
phoenix_preset!(
    fixed_coupon_note,
    [],
    kiyosi::PhoenixPreset::FixedCouponNote,
    "Creates a fixed coupon note (FCN) with unconditional coupons at surviving observations."
);
phoenix_preset!(
    digital_coupon_note,
    [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times."],
    kiyosi::PhoenixPreset::DigitalCouponNote { coupon_barrier_level },
    "Creates a digital coupon note (DCN) with fixed per-period coupons conditional on a barrier."
);

#[derive(FromPyObject)]
pub(crate) enum AutocallableInstrument<'py> {
    Phoenix(PyRef<'py, PhoenixOption>),
    Binary(PyRef<'py, BinarySnowballOption>),
    Ternary(PyRef<'py, TernarySnowballOption>),
    Snowball(PyRef<'py, SnowballOption>),
}

macro_rules! with_note {
    ($instrument:expr, $note:ident, $body:expr) => {{
        use $crate::instruments::autocallable::AutocallableInstrument;
        match $instrument {
            AutocallableInstrument::Phoenix(note) => {
                let $note = &note.0;
                $body
            }
            AutocallableInstrument::Binary(note) => {
                let $note = &note.0;
                $body
            }
            AutocallableInstrument::Ternary(note) => {
                let $note = &note.0;
                $body
            }
            AutocallableInstrument::Snowball(note) => {
                let $note = &note.0;
                $body
            }
        }
    }};
}
pub(crate) use with_note;
