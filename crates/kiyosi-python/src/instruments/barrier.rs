use crate::conversions::{date, dates, direction, real};
use crate::error::domain;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) fn kind(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::BarrierType> {
    match value.extract::<&str>()? {
        "up_and_in" => Ok(kiyosi::BarrierType::UpAndIn),
        "up_and_out" => Ok(kiyosi::BarrierType::UpAndOut),
        "down_and_in" => Ok(kiyosi::BarrierType::DownAndIn),
        "down_and_out" => Ok(kiyosi::BarrierType::DownAndOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
    }
}
pub(crate) fn mode(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::ObservationMode> {
    match value.extract::<&str>()? {
        "continuous" => Ok(kiyosi::ObservationMode::Continuous),
        "scheduled" => Ok(kiyosi::ObservationMode::Scheduled),
        _ => Err(PyValueError::new_err("unknown observation_mode")),
    }
}
pub(crate) fn timing(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::RebateTiming> {
    match value.extract::<&str>()? {
        "at_hit" => Ok(kiyosi::RebateTiming::AtHit),
        "at_expiry" => Ok(kiyosi::RebateTiming::AtExpiry),
        _ => Err(PyValueError::new_err("unknown rebate_timing")),
    }
}
pub(crate) fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<kiyosi::BarrierTouchState>> {
    if value.is_none() {
        return Ok(None);
    }
    match value.extract::<&str>()? {
        "untouched" => Ok(Some(kiyosi::BarrierTouchState::Untouched)),
        "touched" => Ok(Some(kiyosi::BarrierTouchState::Touched)),
        _ => Err(PyValueError::new_err("unknown touch_state")),
    }
}

/// Single-barrier vanilla option with inclusive barrier touches.
///
/// Parameters
/// ----------
/// option_type : {'call', 'put'}
///     Call or put payoff direction.
/// strike : float
///     Positive finite strike price.
/// effective_midnight_utc : datetime.date
///     Contract starts at 00:00 UTC on this date.
/// expiry_midnight_utc : datetime.date
///     Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
/// barrier_level : float
///     Positive finite trigger level; equality counts as a touch.
/// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
///     Trigger direction and knock-in or knock-out behavior.
/// rebate : float, optional
///     Nonnegative finite cash rebate; defaults to zero.
/// rebate_timing : {'at_hit', 'at_expiry'}, optional
///     Payment timing; at-hit rebates are valid only for knock-outs.
/// observation_mode : {'continuous', 'scheduled'}, optional
///     Continuous monitoring or observations on the supplied dates.
/// observation_dates : iterable of date, optional
///     Strictly increasing observation dates within the contract life.
/// touch_state : {'untouched', 'touched'} or None, optional
///     Strictly prior history. None means unknown; untouched means known no
///     hit.
///
/// Raises
/// ------
/// InvalidInputError
///     If levels, rebate, life dates, monitoring schedule or rebate timing
///     are invalid. Trading-day and valuation-history checks occur at pricing.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BarrierOption(pub kiyosi::BarrierOption);

monitoring_properties!(
    BarrierOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_midnight_utc : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_midnight_utc : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// rebate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// rebate_timing : {'at_hit', 'at_expiry'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_mode : {'continuous', 'scheduled'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// observation_dates : Iterable[date], optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// touch_state : {'untouched', 'touched'} or None, optional
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
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_midnight_utc=crate::conversions::OMITTED, expiry_midnight_utc=crate::conversions::OMITTED, barrier_level=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, rebate=crate::conversions::OMITTED, rebate_timing=crate::conversions::OMITTED, observation_mode=crate::conversions::OMITTED, observation_dates=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_midnight_utc: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_midnight_utc: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_level: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] barrier_type: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] rebate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] rebate_timing: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] observation_mode: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] observation_dates: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::BarrierOptionChanges {
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            effective_midnight_utc: effective_midnight_utc
                .map(crate::conversions::date)
                .transpose()?,
            expiry_midnight_utc: expiry_midnight_utc
                .map(crate::conversions::date)
                .transpose()?,
            barrier_level: barrier_level.map(crate::conversions::real).transpose()?,
            barrier_type: barrier_type
                .map(crate::instruments::barrier::kind)
                .transpose()?,
            rebate: rebate.map(crate::conversions::real).transpose()?,
            rebate_timing: rebate_timing
                .map(crate::instruments::barrier::timing)
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
        };
        Ok(Self(domain(self.0.replace(changes))?))
    },
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_midnight_utc, expiry_midnight_utc, barrier_level, barrier_type, rebate=kiyosi::BarrierOptionTerms::DEFAULT_REBATE, rebate_timing=kiyosi::RebateTiming::default(), observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_midnight_utc: &Bound<'_, PyAny>,
        expiry_midnight_utc: &Bound<'_, PyAny>,
        barrier_level: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] rebate: f64,
        #[pyo3(from_py_with = timing)] rebate_timing: kiyosi::RebateTiming,
        #[pyo3(from_py_with = mode)] observation_mode: kiyosi::ObservationMode,
        #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
        #[pyo3(from_py_with = history)] touch_state: Option<kiyosi::BarrierTouchState>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::BarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            date(effective_midnight_utc)?,
            date(expiry_midnight_utc)?,
            real(barrier_level)?,
            kind(barrier_type)?,
        );
        terms.rebate = rebate;
        terms.rebate_timing = rebate_timing;
        terms.observation_mode = observation_mode;
        terms.observation_dates = observation_dates;
        terms.touch_state = touch_state;
        Ok(Self(domain(kiyosi::BarrierOption::new(terms))?))
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
    },
    /// Return positive finite trigger level; equality counts as a touch.
    #[getter]
    fn barrier_level(&self) -> f64 {
        self.0.barrier_level()
    },
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
    /// Return nonnegative finite cash rebate; defaults to zero.
    #[getter]
    fn rebate(&self) -> f64 {
        self.0.rebate()
    },
    /// Return payment timing; at-hit rebates are valid only for knock-outs.
    #[getter]
    fn rebate_timing(&self) -> &'static str {
        match self.0.rebate_timing() {
            kiyosi::RebateTiming::AtHit => "at_hit",
            kiyosi::RebateTiming::AtExpiry => "at_expiry",
        }
    },
    /// Return mean fixing spacing in Actual/365 Fixed years.
    #[getter]
    fn mean_observation_year_fraction(&self) -> f64 {
        self.0.mean_observation_year_fraction()
    }
);
