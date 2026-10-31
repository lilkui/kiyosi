use crate::conversions::{integer, real};
use crate::engines::ScalarRequest;
use crate::market::PricingContext;
use pyo3::prelude::*;

/// Solve the annual volatility that reproduces an observed price.
///
/// Parameters
/// ----------
/// engine : object
///     Supported pricing engine for the instrument.
/// instrument : object
///     Contract to reprice; the original is unchanged.
/// context : PricingContext
///     Market snapshot; the original is unchanged.
/// observed_price : float
///     Finite target price in the engine's payment units.
/// lower_bound : float, optional
///     Positive finite lower annual volatility in decimal units; defaults to 0.0001.
/// upper_bound : float, optional
///     Finite upper annual volatility, above lower_bound; defaults to 4.
/// price_tolerance : float, optional
///     Positive finite absolute price residual tolerance; defaults to 1e-8.
/// parameter_tolerance : float, optional
///     Positive finite bracket-width tolerance in decimal units; defaults to
///     1e-8. Either tolerance can stop iteration.
/// max_iterations : int, optional
///     Positive iteration budget; defaults to 100.
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
/// Digital prices can have several volatility roots. The supplied bracket
/// selects a root; CRR engines restrict it to their admissible tree domain.
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, *, lower_bound=kiyosi::ImpliedVolatilitySettings::default().lower_bound, upper_bound=kiyosi::ImpliedVolatilitySettings::default().upper_bound, price_tolerance=kiyosi::ImpliedVolatilitySettings::default().price_tolerance, parameter_tolerance=kiyosi::ImpliedVolatilitySettings::default().parameter_tolerance, max_iterations=kiyosi::ImpliedVolatilitySettings::default().max_iterations))]
#[expect(clippy::too_many_arguments, reason = "explicit solver controls")]
pub(crate) fn implied_volatility(
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
) -> PyResult<f64> {
    let controls = kiyosi::ImpliedVolatilitySettings {
        lower_bound,
        upper_bound,
        price_tolerance,
        parameter_tolerance,
        max_iterations,
    };
    crate::registration::evaluate(
        py,
        engine,
        instrument,
        context,
        ScalarRequest::ImpliedVolatility {
            observed_price,
            settings: controls,
        },
    )
}
