use crate::conversions::{date, dates, direction, real};
use crate::error::domain;
use crate::instruments::barrier::{history, mode, timing};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn kind(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::DoubleBarrierType> {
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
///     Positive finite exercise or purchase price.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
/// lower_barrier : float
///     Positive finite lower trigger; equality counts as a touch.
/// upper_barrier : float
///     Finite upper trigger, strictly above the lower trigger.
/// barrier_type : {'knock_in', 'knock_out'}
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
///     If boundaries, strike, rebate, life dates, monitoring schedule or
///     rebate timing are invalid. Calendar and history checks occur at pricing.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct DoubleBarrierOption(pub kiyosi::DoubleBarrierOption);

monitoring_properties!(
    DoubleBarrierOption,
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
    /// Return the exercise or purchase price in underlying price units.
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
    /// Return trigger direction and knock-in or knock-out behavior.
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
