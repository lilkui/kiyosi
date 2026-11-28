use crate::conversions::{integer, real};
use crate::engines::finite_difference;
use crate::engines::monte_carlo;
use crate::engines::quadrature::QuadratureAutocallableEngine;
use crate::error::domain;
use crate::instruments::autocallable::{AutocallableInstrument, coupon_convention};
use crate::market::PricingContext;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

macro_rules! with_coupon_engine {
    ($value:expr, |$engine:ident| $solve:expr) => {{
        if let Ok(engine) = $value.extract::<PyRef<'_, QuadratureAutocallableEngine>>() {
            let $engine = engine.0;
            $solve
        } else if let Ok(engine) =
            $value.extract::<PyRef<'_, monte_carlo::MonteCarloAutocallableEngine>>()
        {
            let $engine = engine.0;
            $solve
        } else {
            let engine = crate::conversions::engine::<
                PyRef<'_, finite_difference::FiniteDifferenceAutocallableEngine>,
            >($value)?;
            let $engine = engine.0;
            $solve
        }
    }};
}

/// Solve the coupon that reproduces the observed price of an autocallable note.
///
/// Parameters
/// ----------
/// engine : object
///     QuadratureAutocallableEngine, FiniteDifferenceAutocallableEngine or
///     MonteCarloAutocallableEngine.
/// instrument : object
///     SnowballOption, BinarySnowballOption, TernarySnowballOption or PhoenixOption.
/// context : PricingContext
///     Market snapshot.
/// observed_price : float
///     Finite target price in the engine's payment units.
/// quote_convention : CouponQuoteConvention, optional
///     Required for snowballs. Use shift_maturity_coupon to shift the maturity
///     coupon with observation coupons or preserve_maturity_coupon to keep it fixed.
///     Omit for Phoenix.
/// lower_bound : float, optional
///     Finite lower coupon in the contract's units; defaults to 0. Signed
///     bounds are accepted when the resulting contract terms are valid. For
///     shifted ternary maturity coupons, the bracket is intersected with the
///     valid domain that keeps maturity at or above the fixed minimum.
/// upper_bound : float, optional
///     Finite upper coupon in the same units, above lower_bound; defaults to
///     2.
/// price_tolerance : float, optional
///     Positive finite absolute price residual tolerance; defaults to 1e-8.
/// parameter_tolerance : float, optional
///     Positive finite bracket-width tolerance in the contract's coupon units;
///     defaults to 1e-8. Bracket convergence without a price match raises
///     NonConvergenceError.
/// max_iterations : int, optional
///     Positive iteration budget; defaults to 100. Endpoint and
///     affine-interpolation evaluations do not use this budget.
///
/// Returns
/// -------
/// SnowballCouponQuote or PhoenixCouponQuote
///     Apply with instrument.with_coupon(**quote). A snowball quote contains
///     first_observation_coupon, an annual decimal rate, and quote_convention.
///     A Phoenix quote contains coupon_per_period, a decimal rate per
///     qualifying observation.
///
/// Raises
/// ------
/// TypeError
///     If an argument has the wrong representation or the quote convention
///     is missing for a snowball or supplied for Phoenix.
/// ValueError
///     If the quote convention is unknown.
/// InvalidInputError
///     If the quote, settings or repriced terms are invalid.
/// UnsupportedOperationError
///     If the combination is unsupported or the parameter is not identifiable.
/// RootNotBracketedError
///     If the supplied endpoints do not bracket the target. Also raised when
///     the bracket has no nondegenerate interval in the valid domain.
/// NonConvergenceError
///     If the iteration budget is exhausted or the bracket converges without a
///     price match.
/// ComputationFailureError
///     If a price, residual or numerical evaluation is non-finite.
///
/// Notes
/// -----
/// Pricing failures propagate. Monte Carlo solves require an explicit seed;
/// call with_resolved_seed() first; tolerances constrain the sampled price
/// curve, not its sampling error. Snowballs shift all observation coupons
/// relative to the first one, including past observations. The maturity
/// convention controls the final coupon. The ternary minimum is a fixed lower bound.
/// Rates, brackets and parameter tolerances use annual decimal units for
/// snowballs and periodic decimal units for Phoenix.
/// Trials use affine interpolation, then bisection.
/// Model support follows the engine, including local volatility with finite differences.
///
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, *, quote_convention=crate::conversions::OMITTED, lower_bound=kiyosi::ImpliedCouponSettings::default().lower_bound, upper_bound=kiyosi::ImpliedCouponSettings::default().upper_bound, price_tolerance=kiyosi::ImpliedCouponSettings::default().price_tolerance, parameter_tolerance=kiyosi::ImpliedCouponSettings::default().parameter_tolerance, max_iterations=kiyosi::ImpliedCouponSettings::default().max_iterations))]
#[expect(clippy::too_many_arguments, reason = "explicit Python solver controls")]
pub(crate) fn implied_coupon(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    #[pyo3(from_py_with = real)] observed_price: f64,
    #[pyo3(from_py_with = crate::conversions::supplied)] quote_convention: Option<
        &Bound<'_, PyAny>,
    >,
    #[pyo3(from_py_with = real)] lower_bound: f64,
    #[pyo3(from_py_with = real)] upper_bound: f64,
    #[pyo3(from_py_with = real)] price_tolerance: f64,
    #[pyo3(from_py_with = real)] parameter_tolerance: f64,
    #[pyo3(from_py_with = integer)] max_iterations: i32,
) -> PyResult<Py<PyAny>> {
    let controls = kiyosi::ImpliedCouponSettings {
        lower_bound,
        upper_bound,
        price_tolerance,
        parameter_tolerance,
        max_iterations,
    };
    with_coupon_engine!(engine, |engine| solve_coupon(
        py,
        &engine,
        crate::conversions::instrument(instrument)?,
        &context.0,
        observed_price,
        quote_convention,
        controls,
    ))
}

fn solve_coupon<E>(
    py: Python<'_>,
    engine: &E,
    instrument: AutocallableInstrument<'_>,
    context: &kiyosi::PricingContext,
    observed: f64,
    convention: Option<&Bound<'_, PyAny>>,
    controls: kiyosi::ImpliedCouponSettings,
) -> PyResult<Py<PyAny>>
where
    E: kiyosi::PricingEngine<kiyosi::SnowballOption>
        + kiyosi::PricingEngine<kiyosi::BinarySnowballOption>
        + kiyosi::PricingEngine<kiyosi::TernarySnowballOption>
        + kiyosi::PricingEngine<kiyosi::PhoenixOption>
        + Sync,
{
    match instrument {
        AutocallableInstrument::Phoenix(note) => {
            solve_periodic_coupon(py, engine, &note.0, context, observed, convention, controls)
        }
        AutocallableInstrument::Binary(note) => {
            solve_annual_coupon(py, engine, &note.0, context, observed, convention, controls)
        }
        AutocallableInstrument::Ternary(note) => {
            solve_annual_coupon(py, engine, &note.0, context, observed, convention, controls)
        }
        AutocallableInstrument::Snowball(note) => {
            solve_annual_coupon(py, engine, &note.0, context, observed, convention, controls)
        }
    }
}

fn solve_periodic_coupon(
    py: Python<'_>,
    engine: &(impl kiyosi::PricingEngine<kiyosi::PhoenixOption> + Sync),
    note: &kiyosi::PhoenixOption,
    context: &kiyosi::PricingContext,
    observed: f64,
    convention: Option<&Bound<'_, PyAny>>,
    controls: kiyosi::ImpliedCouponSettings,
) -> PyResult<Py<PyAny>> {
    if convention.is_some() {
        return Err(PyTypeError::new_err(
            "Phoenix does not take quote_convention",
        ));
    }
    let coupon = domain(crate::cancellation::run(py, || {
        kiyosi::implied_phoenix_coupon(engine, note, context, observed, controls)
    })?)?;
    let quote = PyDict::new(py);
    quote.set_item("coupon_per_period", coupon)?;
    Ok(quote.into_any().unbind())
}

fn solve_annual_coupon<T: kiyosi::instruments::SnowballCouponTerms>(
    py: Python<'_>,
    engine: &(impl kiyosi::PricingEngine<kiyosi::instruments::AutocallableNote<T>> + Sync),
    note: &kiyosi::instruments::AutocallableNote<T>,
    context: &kiyosi::PricingContext,
    observed: f64,
    convention: Option<&Bound<'_, PyAny>>,
    controls: kiyosi::ImpliedCouponSettings,
) -> PyResult<Py<PyAny>> {
    let convention = convention
        .ok_or_else(|| PyTypeError::new_err("snowball coupon quotes require quote_convention"))?;
    let convention = coupon_convention(convention)?;
    let coupon = domain(crate::cancellation::run(py, || {
        kiyosi::implied_snowball_coupon(engine, note, context, observed, convention, controls)
    })?)?;
    let quote = PyDict::new(py);
    quote.set_item("first_observation_coupon", coupon.first_observation_coupon)?;
    quote.set_item(
        "quote_convention",
        match coupon.quote_convention {
            kiyosi::CouponQuoteConvention::ShiftMaturityCoupon => "shift_maturity_coupon",
            kiyosi::CouponQuoteConvention::PreserveMaturityCoupon => "preserve_maturity_coupon",
        },
    )?;
    Ok(quote.into_any().unbind())
}
