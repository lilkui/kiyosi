use crate::conversions::{check_changes, real};
use crate::engines::finite_difference;
use crate::engines::monte_carlo;
use crate::engines::quadrature::QuadratureAutocallableEngine;
use crate::error::domain;
use crate::instruments::autocallable::{
    BinarySnowballOption, PhoenixOption, SnowballOption, TernarySnowballOption,
};
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, **settings))]
pub(crate) fn implied_coupon(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed_price: &Bound<'_, PyAny>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<f64> {
    if let Ok(engine) = engine.extract::<PyRef<'_, QuadratureAutocallableEngine>>() {
        return solve_coupon(py, engine.0, instrument, context, observed_price, settings);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAutocallableEngine>>() {
        return solve_coupon(py, engine.0, instrument, context, observed_price, settings);
    }
    let engine = crate::conversions::engine::<
        PyRef<'_, finite_difference::FiniteDifferenceAutocallableEngine>,
    >(engine)?;
    solve_coupon(py, engine.0, instrument, context, observed_price, settings)
}

fn solve_coupon<E>(
    py: Python<'_>,
    engine: E,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed_price: &Bound<'_, PyAny>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<f64>
where
    E: core::PricingEngine<core::SnowballOption>
        + core::PricingEngine<core::BinarySnowballOption>
        + core::PricingEngine<core::TernarySnowballOption>
        + core::PricingEngine<core::PhoenixOption>
        + Sync,
{
    check_changes(
        settings,
        &[
            "quote_convention",
            "lower_bound",
            "upper_bound",
            "price_tolerance",
            "parameter_tolerance",
            "max_iterations",
        ],
    )?;
    let observed = real(observed_price)?;
    let d = core::ImpliedCouponSettings::default();
    let controls = core::ImpliedCouponSettings {
        lower_bound: risk::real_setting(settings, "lower_bound", d.lower_bound)?,
        upper_bound: risk::real_setting(settings, "upper_bound", d.upper_bound)?,
        price_tolerance: risk::real_setting(settings, "price_tolerance", d.price_tolerance)?,
        parameter_tolerance: risk::real_setting(
            settings,
            "parameter_tolerance",
            d.parameter_tolerance,
        )?,
        max_iterations: risk::integer_setting(settings, "max_iterations", d.max_iterations)?,
    };
    let convention = settings
        .map(|s| s.get_item("quote_convention"))
        .transpose()?
        .flatten();
    let context = context.0.clone();
    if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
        if convention.is_some() {
            return Err(PyTypeError::new_err(
                "Phoenix has no quote_convention argument",
            ));
        }
        let note = note.0.clone();
        return domain(
            py,
            py.detach(|| core::implied_coupon(&engine, &note, &context, observed, controls)),
        );
    }
    let convention =
        convention.ok_or_else(|| PyTypeError::new_err("snowballs require quote_convention"))?;
    let convention = match convention.extract::<&str>()? {
        "shift_maturity_coupon" => core::CouponQuoteConvention::ShiftMaturityCoupon,
        "preserve_maturity_coupon" => core::CouponQuoteConvention::PreserveMaturityCoupon,
        _ => return Err(PyValueError::new_err("unknown coupon quote convention")),
    };
    macro_rules! solve {
        ($note:expr) => {{
            let note = $note.0.clone();
            domain(
                py,
                py.detach(|| {
                    core::implied_snowball_coupon(
                        &engine, &note, &context, observed, convention, controls,
                    )
                }),
            )
        }};
    }
    if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
        return solve!(note);
    }
    if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
        return solve!(note);
    }
    let note = crate::conversions::instrument::<PyRef<'_, SnowballOption>>(instrument)?;
    solve!(note)
}
