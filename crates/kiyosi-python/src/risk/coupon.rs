use crate::conversions::{integer, real};
use crate::engines::finite_difference;
use crate::engines::monte_carlo;
use crate::engines::quadrature::QuadratureAutocallableEngine;
use crate::error::UnsupportedOperationError;
use crate::error::domain;
use crate::instruments::autocallable::{AutocallableInstrument, coupon_convention};
use crate::market::PricingContext;
use pyo3::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::PyDict;
/// Solves the annual snowball coupon that reproduces an observed price.
///
/// Parameters
/// ----------
/// engine : object
///     QuadratureAutocallableEngine, FiniteDifferenceAutocallableEngine or
///     MonteCarloAutocallableEngine.
/// instrument : object
///     SnowballOption, BinarySnowballOption or TernarySnowballOption; the original is unchanged.
/// context : PricingContext
///     Market snapshot; the original is unchanged.
/// observed_price : float
///     Finite target price in the engine's payment units.
/// quote_convention : CouponQuoteConvention
///     Required: shift_maturity_coupon shifts the maturity coupon with observation
///     coupons; preserve_maturity_coupon keeps it fixed.
/// lower_bound : float, optional
///     Finite lower coupon in the contract's units; defaults to 0. Signed
///     bounds are accepted when the resulting contract terms are valid.
///     For shifted ternary maturity coupons, the bracket is intersected with
///     the valid domain that keeps maturity at or above the fixed minimum.
/// upper_bound : float, optional
///     Finite upper coupon in the same units, above lower_bound; defaults to 2.
/// price_tolerance : float, optional
///     Positive finite absolute price residual tolerance; defaults to 1e-8.
/// parameter_tolerance : float, optional
///     Positive finite bracket-width tolerance in the contract's coupon units; defaults to
///     1e-8. Bracket convergence without a price match raises NonConvergenceError.
/// max_iterations : int, optional
///     Positive iteration budget; defaults to 100.
///     Endpoint and affine-interpolation evaluations do not use this budget.
///
/// Returns
/// -------
/// float
///     Annual coupon rate in decimal units.
///
/// Raises
/// ------
/// InvalidInputError
///     If the quote, settings or repriced terms are invalid.
/// UnsupportedOperationError
///     If the combination is unsupported or the parameter is not identifiable.
/// RootNotBracketedError
///     If the supplied endpoints do not bracket the target.
///     Also raised when the bracket has no nondegenerate interval in the valid domain.
/// NonConvergenceError
///     If the iteration budget is exhausted or the bracket converges without a price match.
/// ComputationFailureError
///     If a price, residual or numerical evaluation is non-finite.
///
/// Notes
/// -----
/// Pricing failures propagate. Monte Carlo solves pin one seed per request;
/// tolerances constrain the sampled price curve, not its sampling error.
/// Snowballs shift all observation coupons relative to the first one,
/// including past observations. The maturity convention controls the final
/// coupon; the ternary minimum remains fixed. Bounds, parameter tolerance
/// and result use annual decimal units. Trials use affine interpolation, then bisection.
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, *, quote_convention, lower_bound=kiyosi::ImpliedCouponSettings::default().lower_bound, upper_bound=kiyosi::ImpliedCouponSettings::default().upper_bound, price_tolerance=kiyosi::ImpliedCouponSettings::default().price_tolerance, parameter_tolerance=kiyosi::ImpliedCouponSettings::default().parameter_tolerance, max_iterations=kiyosi::ImpliedCouponSettings::default().max_iterations))]
#[expect(clippy::too_many_arguments, reason = "explicit Python solver controls")]
pub(crate) fn implied_snowball_coupon(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    #[pyo3(from_py_with = real)] observed_price: f64,
    quote_convention: &Bound<'_, PyAny>,
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
    dispatch_coupon(
        py,
        engine,
        instrument,
        context,
        observed_price,
        Some(quote_convention),
        controls,
    )
}

/// Solves a Phoenix coupon without changing its annual or per-period units.
///
/// Parameters
/// ----------
/// engine : object
///     QuadratureAutocallableEngine, FiniteDifferenceAutocallableEngine or
///     MonteCarloAutocallableEngine.
/// instrument : PhoenixOption
///     Contract to reprice; the original is unchanged.
/// context : PricingContext
///     Market snapshot; the original is unchanged.
/// observed_price : float
///     Finite target price in the engine's payment units.
/// lower_bound, upper_bound : float, optional
///     Finite coupon bracket in the contract's units, defaulting to [0, 2].
/// price_tolerance : float, optional
///     Positive finite price residual tolerance; defaults to 1e-8.
/// parameter_tolerance : float, optional
///     Positive finite bracket-width tolerance in coupon units; defaults to 1e-8.
///     Bracket convergence without a price match raises NonConvergenceError.
/// max_iterations : int, optional
///     Positive iteration budget, default 100; endpoint and affine evaluations are extra.
///
/// Returns
/// -------
/// PhoenixCouponQuote
///     Dictionary with exactly one key: coupon_rate for an annual rate or
///     coupon_per_period for a fixed proportion per observation.
///     Apply with instrument.replace(**quote) to retain the coupon convention.
///
/// Raises
/// ------
/// InvalidInputError
///     If the quote, settings or repriced terms are invalid.
/// UnsupportedOperationError
///     If the combination is unsupported or the coupon is not identifiable.
/// RootNotBracketedError
///     If endpoints do not bracket the target.
/// NonConvergenceError
///     If the iteration budget is exhausted or the bracket converges without a price match.
/// ComputationFailureError
///     If a price, residual or numerical evaluation is non-finite.
///
/// Notes
/// -----
/// Replaces the conditional coupon, preserving its units. Monte Carlo solves pin
/// a seed per request; tolerances constrain the sampled curve, not sampling error.
/// Pricing failures propagate. Trials use affine interpolation, then bisection.
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, *, lower_bound=kiyosi::ImpliedCouponSettings::default().lower_bound, upper_bound=kiyosi::ImpliedCouponSettings::default().upper_bound, price_tolerance=kiyosi::ImpliedCouponSettings::default().price_tolerance, parameter_tolerance=kiyosi::ImpliedCouponSettings::default().parameter_tolerance, max_iterations=kiyosi::ImpliedCouponSettings::default().max_iterations))]
#[expect(clippy::too_many_arguments, reason = "explicit Python solver controls")]
pub(crate) fn implied_phoenix_coupon(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    #[pyo3(from_py_with = real)] observed_price: f64,
    #[pyo3(from_py_with = real)] lower_bound: f64,
    #[pyo3(from_py_with = real)] upper_bound: f64,
    #[pyo3(from_py_with = real)] price_tolerance: f64,
    #[pyo3(from_py_with = real)] parameter_tolerance: f64,
    #[pyo3(from_py_with = integer)] max_iterations: i32,
) -> PyResult<Py<PyAny>> {
    dispatch_coupon(
        py,
        engine,
        instrument,
        context,
        observed_price,
        None,
        kiyosi::ImpliedCouponSettings {
            lower_bound,
            upper_bound,
            price_tolerance,
            parameter_tolerance,
            max_iterations,
        },
    )
}

fn dispatch_coupon(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed_price: f64,
    quote_convention: Option<&Bound<'_, PyAny>>,
    controls: kiyosi::ImpliedCouponSettings,
) -> PyResult<Py<PyAny>> {
    if let Ok(engine) = engine.extract::<PyRef<'_, QuadratureAutocallableEngine>>() {
        return solve_coupon(
            py,
            engine.0,
            instrument,
            context,
            observed_price,
            quote_convention,
            controls,
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAutocallableEngine>>() {
        return solve_coupon(
            py,
            engine.0,
            instrument,
            context,
            observed_price,
            quote_convention,
            controls,
        );
    }
    let engine = crate::conversions::engine::<
        PyRef<'_, finite_difference::FiniteDifferenceAutocallableEngine>,
    >(engine)?;
    solve_coupon(
        py,
        engine.0,
        instrument,
        context,
        observed_price,
        quote_convention,
        controls,
    )
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "own the engine snapshot for the detached solve"
)]
fn solve_coupon<E>(
    py: Python<'_>,
    engine: E,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
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
    let context = &context.0;
    let instrument = crate::conversions::instrument::<AutocallableInstrument<'_>>(instrument)?;
    macro_rules! solve {
        ($note:expr) => {{
            let convention = convention.ok_or_else(|| {
                UnsupportedOperationError::new_err(
                    "Phoenix coupon solving requires a PhoenixOption",
                )
            })?;
            let convention = coupon_convention(convention)?;
            let note = &$note.0;
            domain(crate::cancellation::run(py, || {
                kiyosi::implied_snowball_coupon(
                    &engine, note, context, observed, convention, controls,
                )
            })?)?
            .into_py_any(py)
        }};
    }
    match instrument {
        AutocallableInstrument::Phoenix(note) => {
            if convention.is_some() {
                return Err(UnsupportedOperationError::new_err(
                    "snowball coupon solving requires a snowball contract",
                ));
            }
            let note = &note.0;
            let coupon = domain(crate::cancellation::run(py, || {
                kiyosi::implied_phoenix_coupon(&engine, note, context, observed, controls)
            })?)?;
            let (field, value) = match coupon {
                kiyosi::PhoenixCoupon::Annual(value) => ("coupon_rate", value),
                kiyosi::PhoenixCoupon::PerPeriod(value) => ("coupon_per_period", value),
            };
            let quote = PyDict::new(py);
            quote.set_item(field, value)?;
            Ok(quote.into_any().unbind())
        }
        AutocallableInstrument::Binary(note) => solve!(note),
        AutocallableInstrument::Ternary(note) => solve!(note),
        AutocallableInstrument::Snowball(note) => solve!(note),
    }
}
