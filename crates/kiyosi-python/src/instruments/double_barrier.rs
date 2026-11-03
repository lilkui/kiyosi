use crate::conversions::{date, dates, direction, real};
use crate::error::domain;
use crate::instruments::barrier::{history, mode, timing};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) fn kind(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::DoubleBarrierType> {
    match value.extract::<&str>()? {
        "knock_in" => Ok(kiyosi::DoubleBarrierType::KnockIn),
        "knock_out" => Ok(kiyosi::DoubleBarrierType::KnockOut),
        _ => Err(PyValueError::new_err("unknown barrier_type")),
    }
}

/// Vanilla option activated or terminated by either inclusive boundary.
///
/// Parameters
/// ----------
/// option_type : {'call', 'put'}
///     Call or put payoff direction.
/// strike : float
///     Positive finite strike price.
/// effective_date : datetime.date
///     Contract starts at 00:00 UTC on this date.
/// expiry_date : datetime.date
///     Expiry boundary at 00:00 UTC; later instants on this date are outside contract life.
/// lower_barrier : float
///     Positive finite lower trigger; equality counts as a touch.
/// upper_barrier : float
///     Finite upper trigger, strictly above the lower trigger.
/// barrier_type : {'knock_in', 'knock_out'}
///     Knock-in or knock-out activation by either barrier.
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
///     If boundaries, strike, rebate, life dates, monitoring schedule or
///     rebate timing are invalid. Calendar and history checks occur at pricing.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleBarrierOption(pub kiyosi::DoubleBarrierOption);

monitoring_properties!(
    DoubleBarrierOption,
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_date : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_date : datetime.date, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// lower_barrier : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// upper_barrier : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// barrier_type : {'knock_in', 'knock_out'}, optional
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
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_date=crate::conversions::OMITTED, expiry_date=crate::conversions::OMITTED, lower_barrier=crate::conversions::OMITTED, upper_barrier=crate::conversions::OMITTED, barrier_type=crate::conversions::OMITTED, rebate=crate::conversions::OMITTED, rebate_timing=crate::conversions::OMITTED, observation_mode=crate::conversions::OMITTED, observation_dates=crate::conversions::OMITTED, touch_state=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_date: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_date: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] lower_barrier: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] upper_barrier: Option<
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
        let changes = kiyosi::DoubleBarrierOptionChanges {
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            effective_date: effective_date.map(crate::conversions::date).transpose()?,
            expiry_date: expiry_date.map(crate::conversions::date).transpose()?,
            lower_barrier: lower_barrier.map(crate::conversions::real).transpose()?,
            upper_barrier: upper_barrier.map(crate::conversions::real).transpose()?,
            barrier_type: barrier_type
                .map(crate::instruments::double_barrier::kind)
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
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date, lower_barrier, upper_barrier, barrier_type, rebate=kiyosi::BarrierOptionTerms::DEFAULT_REBATE, rebate_timing=kiyosi::RebateTiming::default(), observation_mode=kiyosi::ObservationMode::default(), observation_dates=Vec::new(), touch_state=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
        lower_barrier: &Bound<'_, PyAny>,
        upper_barrier: &Bound<'_, PyAny>,
        barrier_type: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = real)] rebate: f64,
        #[pyo3(from_py_with = timing)] rebate_timing: kiyosi::RebateTiming,
        #[pyo3(from_py_with = mode)] observation_mode: kiyosi::ObservationMode,
        #[pyo3(from_py_with = dates)] observation_dates: Vec<kiyosi::Date>,
        #[pyo3(from_py_with = history)] touch_state: Option<kiyosi::BarrierTouchState>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::DoubleBarrierOptionTerms::new(
            direction(option_type)?,
            real(strike)?,
            date(effective_date)?,
            date(expiry_date)?,
            real(lower_barrier)?,
            real(upper_barrier)?,
            kind(barrier_type)?,
        );
        terms.rebate = rebate;
        terms.rebate_timing = rebate_timing;
        terms.observation_mode = observation_mode;
        terms.observation_dates = observation_dates;
        terms.touch_state = touch_state;
        Ok(Self(domain(kiyosi::DoubleBarrierOption::new(terms))?))
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
    /// Return knock-in or knock-out activation by either barrier.
    #[getter]
    fn barrier_type(&self) -> &'static str {
        match self.0.barrier_type() {
            kiyosi::DoubleBarrierType::KnockIn => "knock_in",
            kiyosi::DoubleBarrierType::KnockOut => "knock_out",
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
    }
);
