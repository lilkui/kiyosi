use crate::conversions::{date, dates, python_date, real, supplied};
use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDate;

macro_rules! autocallable_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pymethods]
        impl $name {
            $($extra)*
            /// Returns a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None
            ///     Strictly prior events. None is unknown; the string none means no
            ///     event.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omitted fields retain their values.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation. The original is unchanged.
            #[pyo3(signature = (*, barrier_state))]
            fn replace(&self, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
                Ok(Self(domain(self.0.replace(state(barrier_state)?))?))
            }
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
            /// Returns a copy of the contractual observation dates.
            #[getter]
            fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
                self.0
                    .observations()
                    .iter()
                    .map(|r| python_date(py, r.date))
                    .collect()
            }
            /// Returns copied knock-out thresholds in observation order.
            #[getter]
            fn knock_out_levels(&self) -> Vec<f64> {
                self.0
                    .observations()
                    .iter()
                    .map(|r| r.knock_out_level)
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
            /// Returns inclusive first contract date.
            #[getter]
            fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.effective_date())
            }
            /// Returns final contract date; expiry is at midnight UTC.
            #[getter]
            fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.expiry_date())
            }
        }
    };
}

fn state(value: &Bound<'_, PyAny>) -> PyResult<Option<kiyosi::AutocallableBarrierState>> {
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(match value.extract::<&str>()? {
        "none" => kiyosi::AutocallableBarrierState::None,
        "knocked_in" => kiyosi::AutocallableBarrierState::KnockedIn,
        "knocked_out" => kiyosi::AutocallableBarrierState::KnockedOut,
        _ => return Err(PyValueError::new_err("unknown autocallable barrier_state")),
    }))
}
fn state_name(value: Option<kiyosi::AutocallableBarrierState>) -> Option<&'static str> {
    value.map(|s| match s {
        kiyosi::AutocallableBarrierState::None => "none",
        kiyosi::AutocallableBarrierState::KnockedIn => "knocked_in",
        kiyosi::AutocallableBarrierState::KnockedOut => "knocked_out",
    })
}
fn mode(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::KnockInObservationMode> {
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
fn records(values: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::AutocallableObservation>> {
    values
        .try_iter()?
        .map(|item| Ok(item?.extract::<PyRef<'_, AutocallableObservation>>()?.0))
        .collect()
}
fn reals(values: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    values.try_iter()?.map(|item| real(&item?)).collect()
}

/// Input observation validated when its note is constructed.
///
/// Parameters
/// ----------
/// date : datetime.date
///     Observation date at midnight UTC.
/// knock_out_level : float
///     Positive inclusive knock-out threshold.
/// coupon_rate : float, optional
///     Finite annual coupon rate in decimal units.
/// coupon_barrier_level : float, optional
///     Inclusive Phoenix coupon threshold; must be zero for snowballs.
///
/// Notes
/// -----
/// This record checks Python representations only. Note construction
/// validates contractual levels and coupons.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AutocallableObservation(pub kiyosi::AutocallableObservation);
#[pymethods]
impl AutocallableObservation {
    #[new]
    #[pyo3(signature = (date, knock_out_level, *, coupon_rate=kiyosi::AutocallableObservation::DEFAULT_COUPON_RATE, coupon_barrier_level=kiyosi::AutocallableObservation::DEFAULT_COUPON_BARRIER_LEVEL))]
    fn new(
        date: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] coupon_rate: f64,
        #[pyo3(from_py_with = real)] coupon_barrier_level: f64,
    ) -> PyResult<Self> {
        let mut record = kiyosi::AutocallableObservation::new(
            crate::conversions::date(date)?,
            real(knock_out_level)?,
        );
        record.coupon_rate = coupon_rate;
        record.coupon_barrier_level = coupon_barrier_level;
        Ok(Self(record))
    }
    /// Return observation date at midnight UTC.
    #[getter]
    fn date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.date)
    }
    /// Return the supplied knock-out threshold; the note constructor validates it.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level
    }
    /// Return the supplied annual coupon in decimal units; note construction validates it.
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.0.coupon_rate
    }
    /// Return the supplied Phoenix coupon threshold; note construction validates it.
    #[getter]
    fn coupon_barrier_level(&self) -> f64 {
        self.0.coupon_barrier_level
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "AutocallableObservation({}, {:?}, coupon_rate={:?}, coupon_barrier_level={:?})",
            self.date(py)?.repr()?,
            self.0.knock_out_level,
            self.0.coupon_rate,
            self.0.coupon_barrier_level
        ))
    }
}

/// Associate parallel sequences into input observation records.
///
/// Parameters
/// ----------
/// observation_dates : iterable of date
///     Strictly increasing observation dates within the contract life.
/// knock_out_levels : iterable of float
///     Positive inclusive knock-out levels, one per observation date.
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
///     If sequence lengths differ.
///
/// Notes
/// -----
/// The note constructor validates contractual values and ordering.
#[pyfunction]
#[pyo3(signature = (observation_dates, knock_out_levels, *, knock_out_coupon_rates=None, coupon_barrier_levels=None))]
pub(crate) fn autocallable_observations(
    observation_dates: &Bound<'_, PyAny>,
    knock_out_levels: &Bound<'_, PyAny>,
    knock_out_coupon_rates: Option<&Bound<'_, PyAny>>,
    coupon_barrier_levels: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<AutocallableObservation>> {
    let dates = dates(observation_dates)?;
    let levels = reals(knock_out_levels)?;
    let coupons = knock_out_coupon_rates.map(reals).transpose()?;
    let barriers = coupon_barrier_levels.map(reals).transpose()?;
    Ok(domain(kiyosi::autocallable_observations(
        &dates,
        &levels,
        coupons.as_deref(),
        barriers.as_deref(),
    ))?
    .into_iter()
    .map(AutocallableObservation)
    .collect())
}

macro_rules! snowball_preset {
    ($name:ident, $coupon:ident => $coupon_doc:literal, $level:ident => $level_doc:literal,
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
        /// observation_dates : iterable of date
        ///     Strictly increasing observation dates within the contract life.
        /// effective_date : datetime.date
        ///     Inclusive first contract date.
        /// expiry_date : datetime.date
        ///     Final contract date; expiry is at midnight UTC.
        /// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
        ///     Strictly prior events. None is unknown; the string none means no event.
        /// principal_ratio : float, optional
        ///     Nonnegative normalized principal, independent of coupons and losses.
        ///
        /// Returns
        /// -------
        /// SnowballOption
        ///     Validated owned contract.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If contractual amounts, life dates or observation terms are invalid.
        ///
        /// Notes
        /// -----
        #[doc = concat!("Knock-in is observed ", $monitoring, ".")]
        /// Unit principal, zero lower strike and initial spot as upper strike are used
        /// unless the preset overrides them. History and trading-calendar consistency
        /// are checked during pricing.
        #[doc = $notes]
        #[pyfunction]
        #[pyo3(signature = (*, $coupon, initial_spot, knock_in_level, $level, $($extra,)* observation_dates, effective_date, expiry_date, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
            $coupon: &Bound<'_, PyAny>,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            $level: &Bound<'_, PyAny>,
            $($extra: &Bound<'_, PyAny>,)*
            observation_dates: &Bound<'_, PyAny>,
            effective_date: &Bound<'_, PyAny>,
            expiry_date: &Bound<'_, PyAny>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = real)] principal_ratio: f64,
        ) -> PyResult<SnowballOption> {
            let mut terms = kiyosi::SnowballPresetTerms::new(
                real($coupon)?, real(initial_spot)?, real(knock_in_level)?, real($level)?,
                date(effective_date)?, date(expiry_date)?, dates(observation_dates)?,
            );
            $(let $extra = $convert($extra)?;)*
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_ratio = principal_ratio;
            Ok(SnowballOption(domain(terms.expand($preset))?))
        }
    };
    (@type real) => { "float" };
    (@type date) => { "datetime.date" };
}
snowball_preset!(
    standard_snowball,
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
    first_coupon_rate => "Finite signed annual coupon through first_stage_end_date.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [
        second_coupon_rate => "Finite signed annual coupon after the cutoff and at maturity." => real,
        first_stage_end_date => "Inclusive first-stage end within life and strictly before expiry." => date
    ],
    kiyosi::SnowballPreset::EarlyProfit { first_stage_end_date, second_coupon_rate },
    "Creates a two-stage early-profit snowball.",
    "on every trading day",
    r"Knock-out uses the observation's stage rate over the whole holding period.
Maturity uses the final stage rate. Cutoffs need not be observation or trading
dates, stages may have no observations, and rates need not decrease."
);
snowball_preset!(
    butterfly_snowball,
    first_coupon_rate => "Finite signed annual coupon through first_stage_end_date.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [
        second_coupon_rate => "Finite signed annual coupon in the second stage." => real,
        third_coupon_rate => "Finite signed annual coupon in the third stage and at maturity." => real,
        first_stage_end_date => "Inclusive first-stage end within life and before second_stage_end_date." => date,
        second_stage_end_date => "Inclusive second-stage end strictly before expiry." => date
    ],
    kiyosi::SnowballPreset::Butterfly {
        first_stage_end_date, second_coupon_rate, second_stage_end_date, third_coupon_rate
    },
    "Creates a three-stage butterfly snowball.",
    "on every trading day",
    r"Knock-out uses the observation's stage rate over the whole holding period.
Maturity uses the final stage rate. Cutoffs need not be observation or trading
dates, stages may have no observations, and rates need not decrease."
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
/// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}
///     Observe knock-in on every trading day or only at expiry.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string none means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// Raises
/// ------
/// InvalidInputError
///     If amounts, strikes, life dates or observation terms are invalid.
///     Pricing checks the calendar and prior barrier history.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct SnowballOption(pub kiyosi::SnowballOption);
autocallable_properties! {
    SnowballOption,
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        knock_in_observation_mode: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::SnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(initial_spot)?,
            real(knock_in_level)?,
            real(upper_strike)?,
            real(lower_strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        terms.knock_in_observation_mode = mode(knock_in_observation_mode)?;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        Ok(Self(domain(kiyosi::SnowballOption::new(terms))?))
    },
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
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
    /// Return the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    },
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "SnowballOption(maturity_coupon_rate={:?}, initial_spot={:?}, knock_in_level={:?}, upper_strike={:?}, lower_strike={:?}, knock_in_observation_mode={:?}, barrier_state={}, principal_ratio={:?}, effective_date={}, expiry_date={}, observations=[{}])",
            self.maturity_coupon_rate(),
            self.initial_spot(),
            self.knock_in_level(),
            self.upper_strike(),
            self.lower_strike(),
            self.knock_in_observation_mode(),
            state,
            self.principal_ratio(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
}

/// Snowball with knock-out coupons and no knock-in or downside loss.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string none means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// Raises
/// ------
/// InvalidInputError
///     If coupons, principal, dates or observations are invalid, or
///     barrier_state is knocked_in; this contract has no knock-in barrier.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinarySnowballOption(pub kiyosi::BinarySnowballOption);
autocallable_properties! {
    BinarySnowballOption,
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO))]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::BinarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        Ok(Self(domain(kiyosi::BinarySnowballOption::new(terms))?))
    },
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    },
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    },
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "BinarySnowballOption(maturity_coupon_rate={:?}, principal_ratio={:?}, barrier_state={}, effective_date={}, expiry_date={}, observations=[{}])",
            self.maturity_coupon_rate(),
            self.principal_ratio(),
            state,
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
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
/// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}
///     Observe knock-in on every trading day or only at expiry.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string none means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// Raises
/// ------
/// InvalidInputError
///     If coupons, principal, barriers, dates or observations are invalid.
///     minimum_coupon_rate must not exceed maturity_coupon_rate.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TernarySnowballOption(pub kiyosi::TernarySnowballOption);
autocallable_properties! {
    TernarySnowballOption,
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        minimum_coupon_rate: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        knock_in_observation_mode: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::TernarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(minimum_coupon_rate)?,
            real(knock_in_level)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        terms.knock_in_observation_mode = mode(knock_in_observation_mode)?;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        Ok(Self(domain(kiyosi::TernarySnowballOption::new(terms))?))
    },
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
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
    /// Return the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    },
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        Ok(format!(
            "TernarySnowballOption(maturity_coupon_rate={:?}, minimum_coupon_rate={:?}, knock_in_level={:?}, knock_in_observation_mode={:?}, principal_ratio={:?}, barrier_state={}, effective_date={}, expiry_date={}, observations=[{}])",
            self.maturity_coupon_rate(),
            self.minimum_coupon_rate(),
            self.knock_in_level(),
            self.knock_in_observation_mode(),
            self.principal_ratio(),
            state,
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
}

/// Autocallable with conditional period coupons and capped downside loss.
///
/// Parameters
/// ----------
/// coupon_rate : float, optional
///     Finite signed annual coupon rate. Supply exactly one of coupon_rate
///     and coupon_per_period; omission or Ellipsis leaves a choice unspecified.
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
/// knock_in_observation_mode : {'every_trading_day', 'at_expiry'}
///     Observe knock-in on every trading day or only at expiry.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered contractual observations within life.
/// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
///     Strictly prior events. None is unknown; the string none means no
///     event.
/// principal_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
///
/// Raises
/// ------
/// TypeError
///     If both coupon choices or neither are supplied, or a supplied value is not real.
/// InvalidInputError
///     If amounts, strikes, life dates or observations are invalid.
///     Observation coupon_rate must be zero; coupon_barrier_level is nonnegative.
///
/// Notes
/// -----
/// Annual coupons accrue over contractual observation periods on Actual/365 Fixed.
/// Fixed per-period coupons pay once per qualifying observation, even for an
/// observation on the effective date. Explicit None is not a coupon value.
/// Coupon and knock-out barriers are inclusive; knock-in is strict. Past
/// coupons are settled. There is no coupon memory or maturity coupon.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PhoenixOption(pub kiyosi::PhoenixOption);
autocallable_properties! {
    PhoenixOption,
    #[new]
    #[pyo3(signature = (*, coupon_rate=crate::conversions::OMITTED, coupon_per_period=crate::conversions::OMITTED, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        #[pyo3(from_py_with = supplied)] coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] coupon_per_period: Option<&Bound<'_, PyAny>>,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        knock_in_observation_mode: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
    ) -> PyResult<Self> {
        let coupon = match (coupon_rate, coupon_per_period) {
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
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        terms.coupon = coupon;
        terms.knock_in_observation_mode = mode(knock_in_observation_mode)?;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        Ok(Self(domain(kiyosi::PhoenixOption::new(terms))?))
    },
    /// Return copied Phoenix coupon thresholds in observation order.
    #[getter]
    fn coupon_barrier_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_barrier_level)
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
    /// Return the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    },
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let records = self
            .observations()
            .iter()
            .map(|r| r.__repr__(py))
            .collect::<PyResult<Vec<_>>>()?
            .join(", ");
        let state = self
            .barrier_state()
            .map_or_else(|| "None".into(), |s| format!("{s:?}"));
        let coupon = match self.0.terms().coupon {
            kiyosi::PhoenixCoupon::Annual(value) => format!("coupon_rate={value:?}"),
            kiyosi::PhoenixCoupon::PerPeriod(value) => format!("coupon_per_period={value:?}"),
        };
        Ok(format!(
            "PhoenixOption({}, initial_spot={:?}, knock_in_level={:?}, upper_strike={:?}, lower_strike={:?}, knock_in_observation_mode={:?}, barrier_state={}, principal_ratio={:?}, effective_date={}, expiry_date={}, observations=[{}])",
            coupon,
            self.initial_spot(),
            self.knock_in_level(),
            self.upper_strike(),
            self.lower_strike(),
            self.knock_in_observation_mode(),
            state,
            self.principal_ratio(),
            self.effective_date(py)?.repr()?,
            self.expiry_date(py)?.repr()?,
            records
        ))
    }
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
        /// observation_dates : iterable of datetime.date
        ///     Strictly increasing observation and payment dates within life.
        /// effective_date : datetime.date
        ///     Inclusive first contract date.
        /// expiry_date : datetime.date
        ///     Final contract date at midnight UTC.
        /// barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
        ///     Strictly prior events; None means unknown history.
        /// principal_ratio : float, optional
        ///     Nonnegative normalized principal, independent of coupon and loss components.
        ///
        /// Returns
        /// -------
        /// PhoenixOption
        ///     Validated owned contract, with expiry-only knock-in and zero lower strike.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If numeric terms, dates or observations are invalid.
        ///
        /// Notes
        /// -----
        /// Coupons pay once per qualifying observation, including a qualifying KO,
        /// with no day accrual or memory. KO and coupon barriers are inclusive;
        /// knock-in is strict. Observation dates are payment dates; no observation
        /// is implicitly added at expiry. Calendar and history consistency are
        /// checked during pricing. Past coupons are settled.
        #[pyfunction]
        #[pyo3(signature = (*, coupon_per_period, initial_spot, knock_in_level, knock_out_level, $($extra,)* observation_dates, effective_date, expiry_date, barrier_state=None, principal_ratio=kiyosi::DEFAULT_PRINCIPAL_RATIO))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
            coupon_per_period: &Bound<'_, PyAny>,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            knock_out_level: &Bound<'_, PyAny>,
            $($extra: &Bound<'_, PyAny>,)*
            observation_dates: &Bound<'_, PyAny>,
            effective_date: &Bound<'_, PyAny>,
            expiry_date: &Bound<'_, PyAny>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = real)] principal_ratio: f64,
        ) -> PyResult<PhoenixOption> {
            let mut terms = kiyosi::PhoenixPresetTerms::new(
                real(coupon_per_period)?, real(initial_spot)?, real(knock_in_level)?,
                real(knock_out_level)?, date(effective_date)?, date(expiry_date)?,
                dates(observation_dates)?,
            );
            $(let $extra = real($extra)?;)*
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_ratio = principal_ratio;
            Ok(PhoenixOption(domain(terms.expand($preset))?))
        }
    };
}
phoenix_preset!(
    fixed_coupon_note,
    [],
    kiyosi::PhoenixPreset::FixedCouponNote,
    "Creates an FCN with an unconditional fixed coupon at each surviving observation."
);
phoenix_preset!(
    digital_coupon_note,
    [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on KO dates."],
    kiyosi::PhoenixPreset::DigitalCouponNote { coupon_barrier_level },
    "Creates a DCN with fixed per-period coupons conditional on an explicit barrier."
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
