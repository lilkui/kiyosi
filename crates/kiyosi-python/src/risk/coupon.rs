use crate::conversions::{integer, real, supplied};
use crate::engines::finite_difference;
use crate::engines::monte_carlo;
use crate::engines::quadrature::QuadratureAutocallableEngine;
use crate::error::domain;
use crate::instruments::autocallable::AutocallableInstrument;
use crate::market::PricingContext;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
/// Solve the annual coupon that reproduces an observed price.
///
/// Parameters
/// ----------
/// engine : object
///     QuadratureAutocallableEngine, FiniteDifferenceAutocallableEngine or
///     MonteCarloAutocallableEngine.
/// instrument : object
///     SnowballOption, BinarySnowballOption, TernarySnowballOption or
///     PhoenixOption; the original is unchanged.
/// context : PricingContext
///     Market snapshot; the original is unchanged.
/// observed_price : float
///     Finite target price in the engine's payment units.
/// quote_convention : CouponQuoteConvention
///     Required for snowballs: shift_maturity_coupon shifts the maturity
///     coupon with observation coupons; preserve_maturity_coupon keeps it
///     fixed. Omit this keyword for Phoenix, or pass Ellipsis (the displayed default).
/// lower_bound : float, optional
///     Finite lower annual coupon in decimal units; defaults to 0. Signed
///     bounds are accepted when the resulting contract terms are valid.
/// upper_bound : float, optional
///     Finite upper annual coupon, above lower_bound; defaults to 2.
/// price_tolerance : float, optional
///     Positive finite absolute price residual tolerance; defaults to 1e-8.
/// parameter_tolerance : float, optional
///     Positive finite bracket-width tolerance in decimal units; defaults to
///     1e-8. Either tolerance can stop iteration.
/// max_iterations : int, optional
///     Positive iteration budget; defaults to 100.
///     Endpoint and affine-interpolation evaluations do not use this budget.
///
/// Returns
/// -------
/// float
///     Annual parameter in decimal units.
///
/// Raises
/// ------
/// InvalidInputError
///     If the quote, settings or repriced terms are invalid.
/// UnsupportedOperationError
///     If the combination is unsupported or the parameter is not identifiable.
/// RootNotBracketedError
///     If the supplied endpoints do not bracket the target.
/// NonConvergenceError
///     If the iteration budget is exhausted.
/// ComputationFailureError
///     If a price, residual or numerical evaluation is non-finite.
///
/// Notes
/// -----
/// Pricing failures propagate. Monte Carlo solves pin one seed per request;
/// tolerances constrain the sampled price curve, not its sampling error.
/// Snowballs shift all observation coupons relative to the first one,
/// including past observations. The maturity convention controls the final
/// coupon; the ternary minimum remains fixed. Phoenix replaces its
/// conditional annual coupon. Trials use affine interpolation, then bisection.
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, *, quote_convention=crate::conversions::OMITTED, lower_bound=kiyosi::ImpliedCouponSettings::default().lower_bound, upper_bound=kiyosi::ImpliedCouponSettings::default().upper_bound, price_tolerance=kiyosi::ImpliedCouponSettings::default().price_tolerance, parameter_tolerance=kiyosi::ImpliedCouponSettings::default().parameter_tolerance, max_iterations=kiyosi::ImpliedCouponSettings::default().max_iterations))]
#[expect(clippy::too_many_arguments, reason = "explicit Python solver controls")]
pub(crate) fn implied_coupon(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    #[pyo3(from_py_with = real)] observed_price: f64,
    #[pyo3(from_py_with = supplied)] quote_convention: Option<&Bound<'_, PyAny>>,
    #[pyo3(from_py_with = real)] lower_bound: f64,
    #[pyo3(from_py_with = real)] upper_bound: f64,
    #[pyo3(from_py_with = real)] price_tolerance: f64,
    #[pyo3(from_py_with = real)] parameter_tolerance: f64,
    #[pyo3(from_py_with = integer)] max_iterations: i32,
) -> PyResult<f64> {
    let controls = kiyosi::ImpliedCouponSettings {
        lower_bound,
        upper_bound,
        price_tolerance,
        parameter_tolerance,
        max_iterations,
    };
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

fn solve_coupon<E>(
    py: Python<'_>,
    engine: E,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed: f64,
    convention: Option<&Bound<'_, PyAny>>,
    controls: kiyosi::ImpliedCouponSettings,
) -> PyResult<f64>
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
            let convention = convention
                .ok_or_else(|| PyTypeError::new_err("snowballs require quote_convention"))?;
            let convention = match convention.extract::<&str>()? {
                "shift_maturity_coupon" => kiyosi::CouponQuoteConvention::ShiftMaturityCoupon,
                "preserve_maturity_coupon" => kiyosi::CouponQuoteConvention::PreserveMaturityCoupon,
                _ => return Err(PyValueError::new_err("unknown coupon quote convention")),
            };
            let note = &$note.0;
            domain(py.detach(|| {
                kiyosi::implied_snowball_coupon(
                    &engine, note, context, observed, convention, controls,
                )
            }))
        }};
    }
    match instrument {
        AutocallableInstrument::Phoenix(note) => {
            if convention.is_some() {
                return Err(PyTypeError::new_err(
                    "Phoenix has no quote_convention argument",
                ));
            }
            let note = &note.0;
            domain(py.detach(|| kiyosi::implied_coupon(&engine, note, context, observed, controls)))
        }
        AutocallableInstrument::Binary(note) => solve!(note),
        AutocallableInstrument::Ternary(note) => solve!(note),
        AutocallableInstrument::Snowball(note) => solve!(note),
    }
}
