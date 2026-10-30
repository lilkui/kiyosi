use crate::conversions::{date, dates, python_date, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDate;

fn state(value: &Bound<'_, PyAny>) -> PyResult<Option<core::AutocallableBarrierState>> {
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(match value.extract::<&str>()? {
        "none" => core::AutocallableBarrierState::None,
        "knocked_in" => core::AutocallableBarrierState::KnockedIn,
        "knocked_out" => core::AutocallableBarrierState::KnockedOut,
        _ => return Err(PyValueError::new_err("unknown autocallable barrier_state")),
    }))
}
fn state_name(value: Option<core::AutocallableBarrierState>) -> Option<&'static str> {
    value.map(|s| match s {
        core::AutocallableBarrierState::None => "none",
        core::AutocallableBarrierState::KnockedIn => "knocked_in",
        core::AutocallableBarrierState::KnockedOut => "knocked_out",
    })
}
fn mode(value: &Bound<'_, PyAny>) -> PyResult<core::KnockInObservationMode> {
    match value.extract::<&str>()? {
        "every_trading_day" => Ok(core::KnockInObservationMode::EveryTradingDay),
        "at_expiry" => Ok(core::KnockInObservationMode::AtExpiry),
        _ => Err(PyValueError::new_err("unknown knock_in_observation_mode")),
    }
}
fn mode_name(value: core::KnockInObservationMode) -> &'static str {
    match value {
        core::KnockInObservationMode::EveryTradingDay => "every_trading_day",
        core::KnockInObservationMode::AtExpiry => "at_expiry",
    }
}
fn records(values: &Bound<'_, PyAny>) -> PyResult<Vec<core::AutocallableObservation>> {
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
pub(crate) struct AutocallableObservation(pub core::AutocallableObservation);
#[pymethods]
impl AutocallableObservation {
    #[new]
    #[pyo3(signature = (date, knock_out_level, *, coupon_rate=core::AutocallableObservation::DEFAULT_COUPON_RATE, coupon_barrier_level=core::AutocallableObservation::DEFAULT_COUPON_BARRIER_LEVEL))]
    fn new(
        date: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] coupon_rate: f64,
        #[pyo3(from_py_with = real)] coupon_barrier_level: f64,
    ) -> PyResult<Self> {
        let mut record = core::AutocallableObservation::new(
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
    py: Python<'_>,
    observation_dates: &Bound<'_, PyAny>,
    knock_out_levels: &Bound<'_, PyAny>,
    knock_out_coupon_rates: Option<&Bound<'_, PyAny>>,
    coupon_barrier_levels: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<AutocallableObservation>> {
    let dates = dates(observation_dates)?;
    let levels = reals(knock_out_levels)?;
    let coupons = knock_out_coupon_rates.map(reals).transpose()?;
    let barriers = coupon_barrier_levels.map(reals).transpose()?;
    Ok(domain(
        py,
        core::autocallable_observations(&dates, &levels, coupons.as_deref(), barriers.as_deref()),
    )?
    .into_iter()
    .map(AutocallableObservation)
    .collect())
}

macro_rules! snowball_preset {
    ($name:ident, $coupon:ident, $level:ident, [$($extra:ident),*], $preset:expr, $doc:literal) => {
        #[doc = $doc]
        #[pyfunction]
        #[pyo3(signature = (*, $coupon, initial_spot, knock_in_level, $level, $($extra,)* observation_dates, effective_date, expiry_date, barrier_state=None, principal_ratio=core::DEFAULT_PRINCIPAL_RATIO))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
            py: Python<'_>,
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
            let mut terms = core::SnowballPresetTerms::new(
                real($coupon)?, real(initial_spot)?, real(knock_in_level)?, real($level)?,
                date(effective_date)?, date(expiry_date)?, dates(observation_dates)?,
            );
            $(let $extra = real($extra)?;)*
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_ratio = principal_ratio;
            Ok(SnowballOption(domain(py, terms.expand($preset))?))
        }
    };
}
snowball_preset!(
    standard_snowball,
    coupon_rate,
    knock_out_level,
    [],
    core::SnowballPreset::Standard,
    r"Create constant knock-out coupons and levels with daily knock-in
monitoring.

Parameters
----------
coupon_rate : float
    Finite annual coupon rate in decimal units.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
knock_out_level : float
    Positive inclusive knock-out threshold.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing."
);
snowball_preset!(
    european_snowball,
    coupon_rate,
    knock_out_level,
    [],
    core::SnowballPreset::European,
    r"Create a snowball with knock-in observed only at expiry.

Parameters
----------
coupon_rate : float
    Finite annual coupon rate in decimal units.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
knock_out_level : float
    Positive inclusive knock-out threshold.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing. Knock-in is
observed only at expiry."
);
snowball_preset!(
    step_down_snowball,
    coupon_rate,
    initial_knock_out_level,
    [knock_out_level_decrement],
    core::SnowballPreset::StepDown {
        knock_out_level_decrement
    },
    r"Create a snowball with decreasing knock-out levels.

Parameters
----------
coupon_rate : float
    Finite annual coupon rate in decimal units.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
initial_knock_out_level : float
    Positive knock-out threshold at the first observation.
knock_out_level_decrement : float
    Absolute price decrement after each observation; may be negative.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing.
Decrements apply after each observation; the first uses the initial
value. Signed coupons and decrements are accepted if all expanded terms
remain valid."
);
snowball_preset!(
    both_down_snowball,
    initial_coupon_rate,
    initial_knock_out_level,
    [coupon_rate_decrement, knock_out_level_decrement],
    core::SnowballPreset::BothDown {
        coupon_rate_decrement,
        knock_out_level_decrement
    },
    r"Create a snowball with decreasing coupons and knock-out levels.

Parameters
----------
initial_coupon_rate : float
    Annual coupon rate at the first observation.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
initial_knock_out_level : float
    Positive knock-out threshold at the first observation.
coupon_rate_decrement : float
    Absolute decimal-rate decrement after each observation.
knock_out_level_decrement : float
    Absolute price decrement after each observation; may be negative.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing.
Decrements apply after each observation; the first uses the initial
value. Signed coupons and decrements are accepted if all expanded terms
remain valid. Maturity uses the final observation coupon."
);
snowball_preset!(
    dual_coupon_snowball,
    knock_out_coupon_rate,
    knock_out_level,
    [maturity_coupon_rate],
    core::SnowballPreset::DualCoupon {
        maturity_coupon_rate
    },
    r"Create a snowball with separate knock-out and maturity coupons.

Parameters
----------
knock_out_coupon_rate : float
    Annual coupon rate for knock-out settlement.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
knock_out_level : float
    Positive inclusive knock-out threshold.
maturity_coupon_rate : float
    Finite annual maturity coupon rate in decimal units.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing."
);
snowball_preset!(
    parachute_snowball,
    coupon_rate,
    knock_out_level,
    [final_knock_out_level],
    core::SnowballPreset::Parachute {
        final_knock_out_level
    },
    r"Create a snowball with a distinct final knock-out level.

Parameters
----------
coupon_rate : float
    Finite annual coupon rate in decimal units.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
knock_out_level : float
    Positive inclusive knock-out threshold.
final_knock_out_level : float
    Positive threshold replacing only the final knock-out level.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing."
);
snowball_preset!(
    otm_snowball,
    coupon_rate,
    knock_out_level,
    [upper_strike],
    core::SnowballPreset::Otm { upper_strike },
    r"Create a snowball with a replaced upper terminal strike.

Parameters
----------
coupon_rate : float
    Finite annual coupon rate in decimal units.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
knock_out_level : float
    Positive inclusive knock-out threshold.
upper_strike : float
    Positive upper terminal settlement strike.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing."
);
snowball_preset!(
    loss_capped_snowball,
    coupon_rate,
    knock_out_level,
    [lower_strike],
    core::SnowballPreset::LossCapped { lower_strike },
    r"Create a snowball whose lower terminal strike caps downside loss.

Parameters
----------
coupon_rate : float
    Finite annual coupon rate in decimal units.
initial_spot : float
    Positive reference spot used to normalize downside losses.
knock_in_level : float
    Positive strict downside threshold.
knock_out_level : float
    Positive inclusive knock-out threshold.
lower_strike : float
    Nonnegative lower terminal strike, at most the upper strike.
observation_dates : iterable of date
    Strictly increasing observation dates within the contract life.
effective_date : datetime.date
    Inclusive first contract date.
expiry_date : datetime.date
    Final contract date; expiry is at midnight UTC.
barrier_state : {'none', 'knocked_in', 'knocked_out'} or None, optional
    Strictly prior events. None is unknown; the string none means no
    event.
principal_ratio : float, optional
    Nonnegative normalized principal, independent of coupons and losses.

Returns
-------
SnowballOption
    Validated owned contract.

Raises
------
InvalidInputError
    If contractual amounts, life dates or observation terms are invalid.

Notes
-----
Daily knock-in monitoring, unit principal, zero lower strike and initial
spot as upper strike are used unless the preset overrides them. History
and trading-calendar consistency are checked during pricing."
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
pub(crate) struct SnowballOption(pub core::SnowballOption);
#[pymethods]
impl SnowballOption {
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=core::DEFAULT_PRINCIPAL_RATIO))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
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
        let mut terms = core::SnowballTerms::new(
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
        Ok(Self(domain(py, core::SnowballOption::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
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
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    /// Return owned copies of the contractual observations.
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    /// Return a copy of the contractual observation dates.
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    /// Return copied knock-out thresholds in observation order.
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    }
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    }
    /// Return positive reference spot used to normalize downside losses.
    #[getter]
    fn initial_spot(&self) -> f64 {
        self.0.terms().initial_spot
    }
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    }
    /// Return positive upper terminal settlement strike.
    #[getter]
    fn upper_strike(&self) -> f64 {
        self.0.terms().upper_strike
    }
    /// Return nonnegative lower terminal strike, at most the upper strike.
    #[getter]
    fn lower_strike(&self) -> f64 {
        self.0.terms().lower_strike
    }
    /// Return nonnegative normalized principal, independent of coupons and losses.
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    /// Return the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    }
    /// Return strictly prior autocallable history, or None if unknown.
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
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
pub(crate) struct BinarySnowballOption(pub core::BinarySnowballOption);
#[pymethods]
impl BinarySnowballOption {
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=core::DEFAULT_PRINCIPAL_RATIO))]
    fn new(
        py: Python<'_>,
        maturity_coupon_rate: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_ratio: f64,
    ) -> PyResult<Self> {
        let mut terms = core::BinarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            date(effective_date)?,
            date(expiry_date)?,
            records(observations)?,
        );
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_ratio = principal_ratio;
        Ok(Self(domain(py, core::BinarySnowballOption::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
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
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    /// Return owned copies of the contractual observations.
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    /// Return a copy of the contractual observation dates.
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    /// Return copied knock-out thresholds in observation order.
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    }
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    }
    /// Return nonnegative normalized principal, independent of coupons and losses.
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    /// Return strictly prior autocallable history, or None if unknown.
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
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
pub(crate) struct TernarySnowballOption(pub core::TernarySnowballOption);
#[pymethods]
impl TernarySnowballOption {
    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=core::DEFAULT_PRINCIPAL_RATIO))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
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
        let mut terms = core::TernarySnowballTerms::new(
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
        Ok(Self(domain(py, core::TernarySnowballOption::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
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
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    /// Return owned copies of the contractual observations.
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    /// Return a copy of the contractual observation dates.
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    /// Return copied knock-out thresholds in observation order.
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    /// Return copied annual knock-out coupons in observation order.
    #[getter]
    fn knock_out_coupon_rates(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_rate)
            .collect()
    }
    /// Return finite annual maturity coupon rate in decimal units.
    #[getter]
    fn maturity_coupon_rate(&self) -> f64 {
        self.0.terms().maturity_coupon_rate
    }
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    }
    /// Return the annual knocked-in maturity coupon.
    #[getter]
    fn minimum_coupon_rate(&self) -> f64 {
        self.0.terms().minimum_coupon_rate
    }
    /// Return nonnegative normalized principal, independent of coupons and losses.
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    /// Return the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    }
    /// Return strictly prior autocallable history, or None if unknown.
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
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
/// coupon_rate : float
///     Finite annual coupon rate in decimal units.
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
///     If amounts, strikes, life dates or observations are invalid.
///     Observation coupon_rate must be zero; coupon_barrier_level is nonnegative.
///
/// Notes
/// -----
/// Coupons accrue over contractual observation periods on Actual/365 Fixed.
/// Coupon and knock-out barriers are inclusive; knock-in is strict. Past
/// coupons are settled. There is no coupon memory or maturity coupon.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PhoenixOption(pub core::PhoenixOption);
#[pymethods]
impl PhoenixOption {
    #[new]
    #[pyo3(signature = (*, coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation_mode, effective_date, expiry_date, observations, barrier_state=None, principal_ratio=core::DEFAULT_PRINCIPAL_RATIO))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        coupon_rate: &Bound<'_, PyAny>,
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
        let mut terms = core::PhoenixTerms::new(
            real(coupon_rate)?,
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
        Ok(Self(domain(py, core::PhoenixOption::new(terms))?))
    }
    /// Return a validated replacement without changing this value.
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
    fn replace(&self, py: Python<'_>, barrier_state: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.replace(state(barrier_state)?))?))
    }
    /// Return owned copies of the contractual observations.
    #[getter]
    fn observations(&self) -> Vec<AutocallableObservation> {
        self.0
            .observations()
            .iter()
            .copied()
            .map(AutocallableObservation)
            .collect()
    }
    /// Return a copy of the contractual observation dates.
    #[getter]
    fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .observations()
            .iter()
            .map(|r| python_date(py, r.date))
            .collect()
    }
    /// Return copied knock-out thresholds in observation order.
    #[getter]
    fn knock_out_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.knock_out_level)
            .collect()
    }
    /// Return copied Phoenix coupon thresholds in observation order.
    #[getter]
    fn coupon_barrier_levels(&self) -> Vec<f64> {
        self.0
            .observations()
            .iter()
            .map(|r| r.coupon_barrier_level)
            .collect()
    }
    /// Return finite annual coupon rate in decimal units.
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.0.terms().coupon_rate
    }
    /// Return positive reference spot used to normalize downside losses.
    #[getter]
    fn initial_spot(&self) -> f64 {
        self.0.terms().initial_spot
    }
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    }
    /// Return positive upper terminal settlement strike.
    #[getter]
    fn upper_strike(&self) -> f64 {
        self.0.terms().upper_strike
    }
    /// Return nonnegative lower terminal strike, at most the upper strike.
    #[getter]
    fn lower_strike(&self) -> f64 {
        self.0.terms().lower_strike
    }
    /// Return nonnegative normalized principal, independent of coupons and losses.
    #[getter]
    fn principal_ratio(&self) -> f64 {
        self.0.terms().principal_ratio
    }
    /// Return the knock-in monitoring choice string.
    #[getter]
    fn knock_in_observation_mode(&self) -> &'static str {
        mode_name(self.0.terms().knock_in_observation_mode)
    }
    /// Return strictly prior autocallable history, or None if unknown.
    #[getter]
    fn barrier_state(&self) -> Option<&'static str> {
        state_name(self.0.barrier_state())
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
            "PhoenixOption(coupon_rate={:?}, initial_spot={:?}, knock_in_level={:?}, upper_strike={:?}, lower_strike={:?}, knock_in_observation_mode={:?}, barrier_state={}, principal_ratio={:?}, effective_date={}, expiry_date={}, observations=[{}])",
            self.coupon_rate(),
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

macro_rules! with_note {
    ($instrument:expr, $note:ident, $body:expr) => {{
        if let Ok(note) =
            $instrument.extract::<PyRef<'_, $crate::instruments::autocallable::PhoenixOption>>()
        {
            let $note = note.0.clone();
            $body
        } else if let Ok(note) = $instrument
            .extract::<PyRef<'_, $crate::instruments::autocallable::BinarySnowballOption>>()
        {
            let $note = note.0.clone();
            $body
        } else if let Ok(note) = $instrument
            .extract::<PyRef<'_, $crate::instruments::autocallable::TernarySnowballOption>>()
        {
            let $note = note.0.clone();
            $body
        } else {
            let $note = $crate::conversions::instrument::<
                PyRef<'_, $crate::instruments::autocallable::SnowballOption>,
            >($instrument)?
            .0
            .clone();
            $body
        }
    }};
}
pub(crate) use with_note;
