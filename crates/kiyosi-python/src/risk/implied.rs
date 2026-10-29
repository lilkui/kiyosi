use super::{integer_setting, real_setting};
use crate::conversions::{check_changes, real};
use crate::engines::analytic;
use crate::engines::analytic::vanilla::AnalyticVanillaEngine;
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
use pyo3::types::PyDict;
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, observed_price, **settings))]
pub(crate) fn implied_volatility(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &PricingContext,
    observed_price: &Bound<'_, PyAny>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<f64> {
    check_changes(
        settings,
        &[
            "lower_bound",
            "upper_bound",
            "price_tolerance",
            "parameter_tolerance",
            "max_iterations",
        ],
    )?;
    let observed = real(observed_price)?;
    let d = core::ImpliedVolatilitySettings::default();
    let controls = core::ImpliedVolatilitySettings {
        lower_bound: real_setting(settings, "lower_bound", d.lower_bound)?,
        upper_bound: real_setting(settings, "upper_bound", d.upper_bound)?,
        price_tolerance: real_setting(settings, "price_tolerance", d.price_tolerance)?,
        parameter_tolerance: real_setting(settings, "parameter_tolerance", d.parameter_tolerance)?,
        max_iterations: integer_setting(settings, "max_iterations", d.max_iterations)?,
    };
    let context = context.0.clone();
    if engine.is_instance_of::<analytic::range_accrual::AnalyticRangeAccrualEngine>() {
        let option = instrument
            .extract::<PyRef<'_, range_accrual::RangeAccrualOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticRangeAccrualEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine>() {
        let option = instrument
            .extract::<PyRef<'_, asian::ArithmeticAveragePriceOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::TurnbullWakemanArithmeticAveragePriceEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<analytic::asian::AnalyticGeometricAveragePriceEngine>() {
        let option = instrument
            .extract::<PyRef<'_, asian::GeometricAveragePriceOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticGeometricAveragePriceEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAsianEngine>>() {
        let engine = engine.0;
        return match instrument.extract::<asian::AsianInstrument<'_>>()? {
            asian::AsianInstrument::Geometric(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloDigitalEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloVanillaEngine>>() {
        let engine = engine.0;
        return match instrument.extract::<binomial::VanillaInstrument<'_>>()? {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceVanillaEngine>>()
    {
        let engine = engine.0;
        return match instrument.extract::<binomial::VanillaInstrument<'_>>()? {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDigitalEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceBarrierEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDoubleBarrierEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, double_barrier::DoubleBarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceDoubleTouchEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, double_touch::DoubleTouchOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if engine.is_instance_of::<quadrature::QuadratureVanillaEngine>() {
        let option = instrument.extract::<PyRef<'_, EuropeanOption>>()?.0.clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::QuadratureVanillaEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<quadrature::QuadratureDigitalEngine>() {
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::QuadratureDigitalEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinVanillaEngine>>() {
        let engine = engine.0;
        let option = instrument.extract::<binomial::VanillaInstrument<'_>>()?;
        return match option {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinDigitalEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<digital::DigitalInstrument<'_>>()?
            .contract()
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceBinaryBarrierEngine>>()
    {
        let engine = engine.0;
        return match instrument.extract::<binary_barrier::BinaryInstrument<'_>>()? {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(&engine, &option, &context, observed, controls)
                    }),
                )
            }
        };
    }
    if engine.is_instance_of::<analytic::binary_barrier::AnalyticBinaryBarrierEngine>() {
        return match instrument.extract::<binary_barrier::BinaryInstrument<'_>>()? {
            binary_barrier::BinaryInstrument::Binary(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(
                            &core::AnalyticBinaryBarrierEngine,
                            &option,
                            &context,
                            observed,
                            controls,
                        )
                    }),
                )
            }
            binary_barrier::BinaryInstrument::Touch(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| {
                        core::implied_volatility(
                            &core::AnalyticBinaryBarrierEngine,
                            &option,
                            &context,
                            observed,
                            controls,
                        )
                    }),
                )
            }
        };
    }
    if engine.is_instance_of::<analytic::double_barrier::AnalyticDoubleBarrierEngine>() {
        let option = instrument
            .extract::<PyRef<'_, double_barrier::DoubleBarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticDoubleBarrierEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloDoubleBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, double_barrier::DoubleBarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, binomial::CoxRossRubinsteinBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, quadrature::QuadratureAutocallableEngine>>() {
        let engine = engine.0;
        return autocallable::with_note!(
            instrument,
            option,
            domain(
                py,
                py.detach(|| core::implied_volatility(
                    &engine, &option, &context, observed, controls
                ))
            )
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAutocallableEngine>>() {
        let engine = engine.0;
        return autocallable::with_note!(
            instrument,
            option,
            domain(
                py,
                py.detach(|| core::implied_volatility(
                    &engine, &option, &context, observed, controls
                ))
            )
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceAutocallableEngine>>()
    {
        let engine = engine.0;
        return autocallable::with_note!(
            instrument,
            option,
            domain(
                py,
                py.detach(|| core::implied_volatility(
                    &engine, &option, &context, observed, controls
                ))
            )
        );
    }
    if let Ok(engine) =
        engine.extract::<PyRef<'_, finite_difference::FiniteDifferenceAccumulatorEngine>>()
    {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, accumulator::Accumulator>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, monte_carlo::MonteCarloAccumulatorEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, accumulator::Accumulator>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, quadrature::QuadratureAccumulatorEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, accumulator::Accumulator>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if let Ok(engine) = engine.extract::<PyRef<'_, quadrature::QuadratureBarrierEngine>>() {
        let engine = engine.0;
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| core::implied_volatility(&engine, &option, &context, observed, controls)),
        );
    }
    if engine.is_instance_of::<analytic::barrier::AnalyticBarrierEngine>() {
        let option = instrument
            .extract::<PyRef<'_, barrier::BarrierOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticBarrierEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<analytic::digital::AnalyticDigitalEngine>() {
        let instrument = instrument.extract::<digital::DigitalInstrument<'_>>()?;
        let option = instrument.contract().clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::AnalyticDigitalEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    if engine.is_instance_of::<analytic::american::BjerksundStenslandVanillaEngine>() {
        let option = instrument
            .extract::<PyRef<'_, american::AmericanOption>>()?
            .0
            .clone();
        return domain(
            py,
            py.detach(|| {
                core::implied_volatility(
                    &core::BjerksundStenslandVanillaEngine,
                    &option,
                    &context,
                    observed,
                    controls,
                )
            }),
        );
    }
    engine.extract::<PyRef<'_, AnalyticVanillaEngine>>()?;
    let option = instrument.extract::<PyRef<'_, EuropeanOption>>()?.0.clone();
    domain(
        py,
        py.detach(|| {
            core::implied_volatility(
                &core::AnalyticVanillaEngine,
                &option,
                &context,
                observed,
                controls,
            )
        }),
    )
}
