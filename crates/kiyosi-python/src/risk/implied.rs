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
use kiyosi_core as core;
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
#[pyo3(signature = (engine, instrument, context, observed_price, *, lower_bound=core::ImpliedVolatilitySettings::default().lower_bound, upper_bound=core::ImpliedVolatilitySettings::default().upper_bound, price_tolerance=core::ImpliedVolatilitySettings::default().price_tolerance, parameter_tolerance=core::ImpliedVolatilitySettings::default().parameter_tolerance, max_iterations=core::ImpliedVolatilitySettings::default().max_iterations))]
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
    let controls = core::ImpliedVolatilitySettings {
        lower_bound,
        upper_bound,
        price_tolerance,
        parameter_tolerance,
        max_iterations,
    };
    let context = &context.0;
    macro_rules! solve {
        ($engine:expr, $option:expr) => {
            domain(
                py,
                py.detach(|| {
                    core::implied_volatility(&$engine, $option, context, observed, controls)
                }),
            )
        };
    }
    if engine.is_instance_of::<analytic::range_accrual::AnalyticRangeAccrualEngine>() {
        let option =
            &extract_instrument::<PyRef<'_, range_accrual::RangeAccrualOption>>(instrument)?.0;
        return solve!(core::AnalyticRangeAccrualEngine, option);
    }
    if engine.is_instance_of::<analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine>() {
        let option =
            &extract_instrument::<PyRef<'_, asian::ArithmeticAveragePriceOption>>(instrument)?.0;
        return solve!(core::TurnbullWakemanArithmeticAveragePriceEngine, option);
    }
    if engine.is_instance_of::<analytic::asian::AnalyticGeometricAveragePriceEngine>() {
        let option =
            &extract_instrument::<PyRef<'_, asian::GeometricAveragePriceOption>>(instrument)?.0;
        return solve!(core::AnalyticGeometricAveragePriceEngine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAsianEngine>>() {
        let engine = engine.0;
        return match extract_instrument::<asian::AsianInstrument<'_>>(instrument)? {
            asian::AsianInstrument::Geometric(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
        };
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloDigitalEngine>>() {
        let engine = engine.0;
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloVanillaEngine>>() {
        let engine = engine.0;
        return match extract_instrument::<binomial::VanillaInstrument<'_>>(instrument)? {
            binomial::VanillaInstrument::European(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
            binomial::VanillaInstrument::American(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
        };
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceVanillaEngine>>()
    {
        let engine = engine.0;
        return match extract_instrument::<binomial::VanillaInstrument<'_>>(instrument)? {
            binomial::VanillaInstrument::European(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
            binomial::VanillaInstrument::American(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
        };
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDigitalEngine>>()
    {
        let engine = engine.0;
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(engine, option);
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceBarrierEngine>>()
    {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, barrier::BarrierOption>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDoubleBarrierEngine>>()
    {
        let engine = engine.0;
        let option =
            &extract_instrument::<PyRef<'_, double_barrier::DoubleBarrierOption>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDoubleTouchEngine>>()
    {
        let engine = engine.0;
        let option =
            &extract_instrument::<PyRef<'_, double_touch::DoubleTouchOption>>(instrument)?.0;
        return solve!(engine, option);
    }
    if engine.is_instance_of::<quadrature::QuadratureVanillaEngine>() {
        let option = &extract_instrument::<PyRef<'_, EuropeanOption>>(instrument)?.0;
        return solve!(core::QuadratureVanillaEngine, option);
    }
    if engine.is_instance_of::<quadrature::QuadratureDigitalEngine>() {
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(core::QuadratureDigitalEngine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinVanillaEngine>>() {
        let engine = engine.0;
        let option = extract_instrument::<binomial::VanillaInstrument<'_>>(instrument)?;
        return match option {
            binomial::VanillaInstrument::European(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
            binomial::VanillaInstrument::American(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
        };
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
        return match extract_instrument::<binary_barrier::BinaryInstrument<'_>>(instrument)? {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = &option.0;
                solve!(engine, option)
            }
        };
    }
    if engine.is_instance_of::<analytic::binary_barrier::AnalyticBinaryBarrierEngine>() {
        return match extract_instrument::<binary_barrier::BinaryInstrument<'_>>(instrument)? {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = &option.0;
                solve!(core::AnalyticBinaryBarrierEngine, option)
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = &option.0;
                solve!(core::AnalyticBinaryBarrierEngine, option)
            }
        };
    }
    if engine.is_instance_of::<analytic::double_barrier::AnalyticDoubleBarrierEngine>() {
        let option =
            &extract_instrument::<PyRef<'_, double_barrier::DoubleBarrierOption>>(instrument)?.0;
        return solve!(core::AnalyticDoubleBarrierEngine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloBarrierEngine>>() {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, barrier::BarrierOption>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloDoubleBarrierEngine>>() {
        let engine = engine.0;
        let option =
            &extract_instrument::<PyRef<'_, double_barrier::DoubleBarrierOption>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinBarrierEngine>>() {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, barrier::BarrierOption>>(instrument)?.0;
        return solve!(engine, option);
    }
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
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceAccumulatorEngine>>()
    {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, accumulator::Accumulator>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAccumulatorEngine>>() {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, accumulator::Accumulator>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, quadrature::QuadratureAccumulatorEngine>>() {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, accumulator::Accumulator>>(instrument)?.0;
        return solve!(engine, option);
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, quadrature::QuadratureBarrierEngine>>() {
        let engine = engine.0;
        let option = &extract_instrument::<PyRef<'_, barrier::BarrierOption>>(instrument)?.0;
        return solve!(engine, option);
    }
    if engine.is_instance_of::<analytic::barrier::AnalyticBarrierEngine>() {
        let option = &extract_instrument::<PyRef<'_, barrier::BarrierOption>>(instrument)?.0;
        return solve!(core::AnalyticBarrierEngine, option);
    }
    if engine.is_instance_of::<analytic::digital::AnalyticDigitalEngine>() {
        let instrument = extract_instrument::<digital::DigitalInstrument<'_>>(instrument)?;
        let option = instrument.contract();
        return solve!(core::AnalyticDigitalEngine, option);
    }
    if engine.is_instance_of::<analytic::american::BjerksundStenslandVanillaEngine>() {
        let option = &extract_instrument::<PyRef<'_, american::AmericanOption>>(instrument)?.0;
        return solve!(core::BjerksundStenslandVanillaEngine, option);
    }
    engine.extract::<PyRef<'_, AnalyticEuropeanEngine>>()?;
    let option = &extract_instrument::<PyRef<'_, EuropeanOption>>(instrument)?.0;
    solve!(core::AnalyticEuropeanEngine, option)
}
