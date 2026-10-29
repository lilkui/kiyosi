use crate::conversions::{check_changes, date, dates, direction, python_date, real};
use crate::error::domain;
use crate::instruments::barrier;
use crate::schedule::ObservationSchedule;
use kiyosi_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinaryBarrierOption(pub core::BinaryBarrierOption);

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TouchOption(pub core::TouchOption);

macro_rules! monitoring_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pymethods]
        impl $name {
            $($extra)*
            #[pyo3(signature = (**fields))]
            fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                check_changes(fields, &["touch_state"])?;
                let state = match fields.map(|f| f.get_item("touch_state")).transpose()?.flatten() {
                    Some(value) => barrier::history(&value)?,
                    None => self.0.touch_state(),
                };
                Ok(Self(domain(py, self.0.replace(state))?))
            }
            #[getter]
            fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.effective_date())
            }
            #[getter]
            fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
                python_date(py, self.0.expiry_date())
            }
            #[getter]
            fn barrier_level(&self) -> f64 { self.0.barrier_level() }
            #[getter]
            fn barrier_type(&self) -> &'static str {
                match self.0.barrier_type() {
                    core::BarrierType::UpAndIn => "up_and_in",
                    core::BarrierType::UpAndOut => "up_and_out",
                    core::BarrierType::DownAndIn => "down_and_in",
                    core::BarrierType::DownAndOut => "down_and_out",
                }
            }
            #[getter]
            fn payoff_type(&self) -> &'static str { if self.0.payout().is_some() { "cash" } else { "asset" } }
            #[getter]
            fn payout(&self) -> Option<f64> { self.0.payout() }
            #[getter]
            fn observation_mode(&self) -> &'static str {
                match self.0.observation_mode() {
                    core::ObservationMode::Continuous => "continuous",
                    core::ObservationMode::Scheduled => "scheduled",
                }
            }
            #[getter]
            fn observation_schedule(&self) -> ObservationSchedule {
                ObservationSchedule(self.0.observation_schedule().clone())
            }
            #[getter]
            fn observation_dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
                self.0.observation_dates().iter().map(|&d| python_date(py, d)).collect()
            }
            #[getter]
            fn mean_observation_year_fraction(&self) -> f64 { self.0.mean_observation_year_fraction() }
            #[getter]
            fn touch_state(&self) -> Option<&'static str> {
                self.0.touch_state().map(|s| match s {
                    core::BarrierTouchState::Untouched => "untouched",
                    core::BarrierTouchState::Touched => "touched",
                })
            }
        }
    };
}
monitoring_properties!(
    BinaryBarrierOption,
    #[getter]
    fn option_type(&self) -> &'static str {
        if self.0.option_type() == core::OptionType::Call {
            "call"
        } else {
            "put"
        }
    },
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
);
monitoring_properties!(
    TouchOption,
    #[getter]
    fn is_one_touch(&self) -> bool {
        self.0.is_one_touch()
    },
    #[getter]
    fn is_up(&self) -> bool {
        self.0.is_up()
    },
    #[getter]
    fn settlement_timing(&self) -> &'static str {
        match self.0.settlement_timing() {
            core::SettlementTiming::AtHit => "at_hit",
            core::SettlementTiming::AtExpiry => "at_expiry",
        }
    }
);

macro_rules! binary_factory {
    ($name:ident, $constructor:ident $(, $payout:ident)?) => {
        #[pyfunction]
        #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date, barrier_level, barrier_type, $($payout,)? **settings))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        pub(crate) fn $name(py: Python<'_>, option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
            effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, barrier_type: &Bound<'_, PyAny>,
            $($payout: &Bound<'_, PyAny>,)? settings: Option<&Bound<'_, PyDict>>) -> PyResult<BinaryBarrierOption> {
            check_changes(settings, &["observation_mode", "observation_dates", "touch_state"])?;
            let mut terms = core::BinaryBarrierTerms::new(direction(option_type)?, real(strike)?,
                date(effective_date)?, date(expiry_date)?, real(barrier_level)?, barrier::kind(barrier_type)?);
            $(let $payout = real($payout)?;)?
            if let Some(settings) = settings {
                if let Some(v) = settings.get_item("observation_mode")? { terms.observation_mode = barrier::mode(&v)?; }
                if let Some(v) = settings.get_item("observation_dates")? { terms.observation_dates = dates(&v)?; }
                if let Some(v) = settings.get_item("touch_state")? { terms.touch_state = barrier::history(&v)?; }
            }
            Ok(BinaryBarrierOption(domain(py, core::BinaryBarrierOption::$constructor(terms $(, $payout)?))?))
        }
    };
}
binary_factory!(cash_binary_barrier_option, cash, payout);
binary_factory!(asset_binary_barrier_option, asset);

macro_rules! touch_factory {
    ($name:ident, $constructor:ident, $kind:ident $(, $payout:ident)?) => {
        #[pyfunction]
        #[pyo3(signature = (*, effective_date, expiry_date, barrier_level, $($payout,)? **settings))]
        pub(crate) fn $name(py: Python<'_>, effective_date: &Bound<'_, PyAny>, expiry_date: &Bound<'_, PyAny>,
            barrier_level: &Bound<'_, PyAny>, $($payout: &Bound<'_, PyAny>,)?
            settings: Option<&Bound<'_, PyDict>>) -> PyResult<TouchOption> {
            let kind = core::BarrierType::$kind;
            let one_touch = matches!(kind, core::BarrierType::UpAndIn | core::BarrierType::DownAndIn);
            check_changes(settings, if one_touch {
                &["settlement_timing", "observation_mode", "observation_dates", "touch_state"]
            } else { &["observation_mode", "observation_dates", "touch_state"] })?;
            let mut terms = core::TouchOptionTerms::new(date(effective_date)?, date(expiry_date)?, real(barrier_level)?, kind);
            $(let $payout = real($payout)?;)?
            if let Some(settings) = settings {
                if let Some(v) = settings.get_item("settlement_timing")? {
                    terms.settlement_timing = match v.extract::<&str>()? {
                        "at_hit" => core::SettlementTiming::AtHit,
                        "at_expiry" => core::SettlementTiming::AtExpiry,
                        _ => return Err(PyValueError::new_err("unknown settlement_timing")),
                    };
                }
                if let Some(v) = settings.get_item("observation_mode")? { terms.observation_mode = barrier::mode(&v)?; }
                if let Some(v) = settings.get_item("observation_dates")? { terms.observation_dates = dates(&v)?; }
                if let Some(v) = settings.get_item("touch_state")? { terms.touch_state = barrier::history(&v)?; }
            }
            Ok(TouchOption(domain(py, core::TouchOption::$constructor(terms $(, $payout)?))?))
        }
    };
}
touch_factory!(cash_one_touch_up, cash, UpAndIn, payout);
touch_factory!(cash_one_touch_down, cash, DownAndIn, payout);
touch_factory!(cash_no_touch_up, cash, UpAndOut, payout);
touch_factory!(cash_no_touch_down, cash, DownAndOut, payout);
touch_factory!(asset_one_touch_up, asset, UpAndIn);
touch_factory!(asset_one_touch_down, asset, DownAndIn);
touch_factory!(asset_no_touch_up, asset, UpAndOut);
touch_factory!(asset_no_touch_down, asset, DownAndOut);

#[derive(FromPyObject)]
pub(crate) enum BinaryInstrument<'py> {
    Binary(PyRef<'py, BinaryBarrierOption>),
    Touch(PyRef<'py, TouchOption>),
}
