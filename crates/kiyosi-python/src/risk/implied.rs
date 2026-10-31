use crate::conversions::{instrument as extract_instrument, integer, real};
use crate::engines::analytic;
use crate::engines::analytic::european::AnalyticEuropeanEngine;
use crate::engines::binomial;
use crate::engines::finite_difference;
use crate::engines::monte_carlo;
use crate::engines::quadrature;
use crate::error::domain;
use crate::instruments::accumulator;
use crate::instruments::american;
use crate::instruments::asian;
use crate::instruments::autocallable;
use crate::instruments::barrier;
use crate::instruments::binary_barrier;
use crate::instruments::digital;
use crate::instruments::double_barrier;
use crate::instruments::double_touch;
use crate::instruments::range_accrual;
use crate::instruments::vanilla::EuropeanOption;
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
    let observed = observed_price;
    let controls = kiyosi::ImpliedVolatilitySettings {
        lower_bound,
        upper_bound,
        price_tolerance,
        parameter_tolerance,
        max_iterations,
    };
    let context = &context.0;
    macro_rules! solve {
        ($engine:expr, $option:expr) => {
            domain(py.detach(|| {
                kiyosi::implied_volatility(&$engine, $option, context, observed, controls)
            }))
        };
    }
    macro_rules! solve_with {
        ($binding:ty, $option:ty) => {
            if let Ok(engine) = engine.extract::<PyRef<'_, $binding>>() {
                let engine = engine.0;
                let option = &extract_instrument::<PyRef<'_, $option>>(instrument)?.0;
                return solve!(engine, option);
            }
        };
        ($binding:ty, $option:ty, $core:expr) => {
            if engine.is_instance_of::<$binding>() {
                let option = &extract_instrument::<PyRef<'_, $option>>(instrument)?.0;
                return solve!($core, option);
            }
        };
    }
    solve_with!(
        analytic::range_accrual::AnalyticRangeAccrualEngine,
        range_accrual::RangeAccrualOption,
        kiyosi::AnalyticRangeAccrualEngine
    );
    solve_with!(
        analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine,
        asian::ArithmeticAveragePriceOption,
        kiyosi::TurnbullWakemanArithmeticAveragePriceEngine
    );
    solve_with!(
        analytic::asian::AnalyticGeometricAveragePriceEngine,
        asian::GeometricAveragePriceOption,
        kiyosi::AnalyticGeometricAveragePriceEngine
    );
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAsianEngine>>() {
        let engine = engine.0;
        return asian::with_asian!(
            extract_instrument::<asian::AsianInstrument<'_>>(instrument)?,
            option,
            solve!(engine, option)
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloDigitalEngine>>() {
        let engine = engine.0;
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloVanillaEngine>>() {
        let engine = engine.0;
        return binomial::with_vanilla!(
            extract_instrument::<binomial::VanillaInstrument<'_>>(instrument)?,
            option,
            solve!(engine, option)
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceVanillaEngine>>()
    {
        let engine = engine.0;
        return binomial::with_vanilla!(
            extract_instrument::<binomial::VanillaInstrument<'_>>(instrument)?,
            option,
            solve!(engine, option)
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDigitalEngine>>()
    {
        let engine = engine.0;
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(engine, option);
    }
    solve_with!(
        finite_difference::FiniteDifferenceBarrierEngine,
        barrier::BarrierOption
    );
    solve_with!(
        finite_difference::FiniteDifferenceDoubleBarrierEngine,
        double_barrier::DoubleBarrierOption
    );
    solve_with!(
        finite_difference::FiniteDifferenceDoubleTouchEngine,
        double_touch::DoubleTouchOption
    );
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinVanillaEngine>>() {
        let engine = engine.0;
        let option = extract_instrument::<binomial::VanillaInstrument<'_>>(instrument)?;
        return binomial::with_vanilla!(option, option, solve!(engine, option));
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinDigitalEngine>>() {
        let engine = engine.0;
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(engine, option);
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceBinaryBarrierEngine>>()
    {
        let engine = engine.0;
        return binary_barrier::with_binary!(
            extract_instrument::<binary_barrier::BinaryInstrument<'_>>(instrument)?,
            option,
            solve!(engine, option)
        );
    }
    if engine.is_instance_of::<analytic::binary_barrier::AnalyticBinaryBarrierEngine>() {
        return binary_barrier::with_binary!(
            extract_instrument::<binary_barrier::BinaryInstrument<'_>>(instrument)?,
            option,
            solve!(kiyosi::AnalyticBinaryBarrierEngine, option)
        );
    }
    solve_with!(
        analytic::double_barrier::AnalyticDoubleBarrierEngine,
        double_barrier::DoubleBarrierOption,
        kiyosi::AnalyticDoubleBarrierEngine
    );
    solve_with!(monte_carlo::MonteCarloBarrierEngine, barrier::BarrierOption);
    solve_with!(
        monte_carlo::MonteCarloDoubleBarrierEngine,
        double_barrier::DoubleBarrierOption
    );
    solve_with!(
        binomial::CoxRossRubinsteinBarrierEngine,
        barrier::BarrierOption
    );
    if let Ok(engine) = engine.extract::<PyRef<'_, quadrature::QuadratureAutocallableEngine>>() {
        let engine = engine.0;
        return autocallable::with_note!(instrument, option, solve!(engine, option));
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAutocallableEngine>>() {
        let engine = engine.0;
        return autocallable::with_note!(instrument, option, solve!(engine, option));
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceAutocallableEngine>>()
    {
        let engine = engine.0;
        return autocallable::with_note!(instrument, option, solve!(engine, option));
    }
    solve_with!(
        finite_difference::FiniteDifferenceAccumulatorEngine,
        accumulator::Accumulator
    );
    solve_with!(
        monte_carlo::MonteCarloAccumulatorEngine,
        accumulator::Accumulator
    );
    solve_with!(
        quadrature::QuadratureAccumulatorEngine,
        accumulator::Accumulator
    );
    solve_with!(quadrature::QuadratureBarrierEngine, barrier::BarrierOption);
    solve_with!(
        analytic::barrier::AnalyticBarrierEngine,
        barrier::BarrierOption,
        kiyosi::AnalyticBarrierEngine
    );
    if engine.is_instance_of::<analytic::digital::AnalyticDigitalEngine>() {
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(kiyosi::AnalyticDigitalEngine, option);
    }
    solve_with!(
        analytic::american::BjerksundStenslandVanillaEngine,
        american::AmericanOption,
        kiyosi::BjerksundStenslandVanillaEngine
    );
    engine.extract::<PyRef<'_, AnalyticEuropeanEngine>>()?;
    let option = &extract_instrument::<PyRef<'_, EuropeanOption>>(instrument)?.0;
    solve!(kiyosi::AnalyticEuropeanEngine, option)
}
