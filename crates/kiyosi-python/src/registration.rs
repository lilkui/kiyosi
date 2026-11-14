use crate::calendar::TradingCalendar;
use crate::engines::analytic;
use crate::market::{BlackScholesMertonParameters, PricingContext};
use crate::{calendar, engines, error, instruments, market, risk, schedule};
use pyo3::prelude::*;

macro_rules! pricing_classes {
    (@engines [$($class:ty),+ $(,)?]) => {
        pricing_classes!(register_engines, is_engine, [$($class),+]);

        pub(crate) fn evaluate<R: Send, Q: engines::PricingRequest<R>>(
            py: Python<'_>,
            engine: &Bound<'_, PyAny>,
            instrument: &Bound<'_, PyAny>,
            context: &PricingContext,
            request: impl FnOnce() -> PyResult<Q>,
        ) -> PyResult<R> {
            $(if engine.is_instance_of::<$class>() {
                let engine = engine.extract::<PyRef<'_, $class>>()?;
                return engine.evaluate(py, crate::conversions::instrument(instrument)?, context, request()?);
            })+
            Err(pyo3::exceptions::PyTypeError::new_err("expected a Kiyosi pricing engine"))
        }
    };
    ($register:ident, $recognize:ident, [$($class:ty),+ $(,)?]) => {
        fn $register(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$class>()?;)+
            Ok(())
        }

        pub(crate) fn $recognize(value: &Bound<'_, PyAny>) -> bool {
            $(value.is_instance_of::<$class>())||+
        }
    };
}

pricing_classes!(
    register_instruments,
    is_instrument,
    [
        instruments::vanilla::EuropeanOption,
        instruments::american::AmericanOption,
        instruments::digital::CashOrNothingOption,
        instruments::digital::AssetOrNothingOption,
        instruments::asian::GeometricAveragePriceOption,
        instruments::asian::ArithmeticAveragePriceOption,
        instruments::barrier::BarrierOption,
        instruments::binary_barrier::BinaryBarrierOption,
        instruments::binary_barrier::TouchOption,
        instruments::double_barrier::DoubleBarrierOption,
        instruments::double_touch::DoubleTouchOption,
        instruments::autocallable::SnowballOption,
        instruments::autocallable::BinarySnowballOption,
        instruments::autocallable::TernarySnowballOption,
        instruments::autocallable::PhoenixOption,
        instruments::accumulator::Accumulator,
        instruments::range_accrual::RangeAccrualOption,
    ]
);

pricing_classes!(
    @engines
    [
        analytic::european::AnalyticEuropeanEngine,
        analytic::heston::AnalyticHestonEuropeanEngine,
        analytic::american::BjerksundStenslandVanillaEngine,
        analytic::digital::AnalyticDigitalEngine,
        analytic::asian::AnalyticGeometricAveragePriceEngine,
        analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine,
        analytic::barrier::AnalyticBarrierEngine,
        analytic::binary_barrier::AnalyticBinaryBarrierEngine,
        analytic::double_barrier::AnalyticDoubleBarrierEngine,
        analytic::double_touch::AnalyticDoubleTouchEngine,
        analytic::range_accrual::AnalyticRangeAccrualEngine,
        engines::binomial::CoxRossRubinsteinVanillaEngine,
        engines::binomial::CoxRossRubinsteinDigitalEngine,
        engines::binomial::CoxRossRubinsteinBarrierEngine,
        engines::quadrature::QuadratureBarrierEngine,
        engines::quadrature::QuadratureVanillaEngine,
        engines::quadrature::QuadratureDigitalEngine,
        engines::quadrature::QuadratureAutocallableEngine,
        engines::quadrature::QuadratureAccumulatorEngine,
        engines::finite_difference::FiniteDifferenceVanillaEngine,
        engines::finite_difference::heston::FiniteDifferenceHestonVanillaEngine,
        engines::finite_difference::FiniteDifferenceDigitalEngine,
        engines::finite_difference::FiniteDifferenceBarrierEngine,
        engines::finite_difference::FiniteDifferenceDoubleBarrierEngine,
        engines::finite_difference::FiniteDifferenceBinaryBarrierEngine,
        engines::finite_difference::FiniteDifferenceDoubleTouchEngine,
        engines::finite_difference::FiniteDifferenceAutocallableEngine,
        engines::finite_difference::FiniteDifferenceAccumulatorEngine,
        engines::monte_carlo::MonteCarloVanillaEngine,
        engines::monte_carlo::MonteCarloDigitalEngine,
        engines::monte_carlo::MonteCarloAsianEngine,
        engines::monte_carlo::MonteCarloBarrierEngine,
        engines::monte_carlo::MonteCarloAutocallableEngine,
        engines::monte_carlo::MonteCarloAccumulatorEngine,
    ]
);

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    macro_rules! register_functions {
        ($($function:path),+ $(,)?) => {
            $(module.add_function(wrap_pyfunction!($function, module)?)?;)+
        };
    }

    register_instruments(module)?;
    register_engines(module)?;
    module.add("__version__", kiyosi::VERSION)?;
    module.add("KiyosiError", py.get_type::<error::KiyosiError>())?;
    module.add(
        "GreekUnavailableError",
        py.get_type::<error::GreekUnavailableError>(),
    )?;
    // PyO3's exception macro accepts one base; Python supplies multiple inheritance.
    py.get_type::<error::InvalidInputError>().setattr(
        "__bases__",
        (
            py.get_type::<error::KiyosiError>(),
            py.get_type::<pyo3::exceptions::PyValueError>(),
        ),
    )?;
    module.add(
        "InvalidInputError",
        py.get_type::<error::InvalidInputError>(),
    )?;
    module.add(
        "MissingHistoryError",
        py.get_type::<error::MissingHistoryError>(),
    )?;
    module.add(
        "UnsupportedOperationError",
        py.get_type::<error::UnsupportedOperationError>(),
    )?;
    module.add(
        "ComputationFailureError",
        py.get_type::<error::ComputationFailureError>(),
    )?;
    module.add(
        "RootNotBracketedError",
        py.get_type::<error::RootNotBracketedError>(),
    )?;
    module.add(
        "NonConvergenceError",
        py.get_type::<error::NonConvergenceError>(),
    )?;
    let names = kiyosi::Greek::ALL.map(kiyosi::Greek::name);
    module.add(
        "Greek",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(pyo3::types::PyTuple::new(py, names)?)?,
    )?;
    register_functions!(
        instruments::autocallable::standard_snowball,
        instruments::autocallable::european_snowball,
        instruments::autocallable::step_down_snowball,
        instruments::autocallable::both_down_snowball,
        instruments::autocallable::dual_coupon_snowball,
        instruments::autocallable::parachute_snowball,
        instruments::autocallable::otm_snowball,
        instruments::autocallable::loss_capped_snowball,
    );
    module.add_class::<instruments::autocallable::AutocallableObservation>()?;
    module.add_class::<instruments::autocallable::KnockInObservation>()?;
    register_functions!(
        instruments::autocallable::fixed_coupon_note,
        instruments::autocallable::standard_phoenix,
        instruments::autocallable::step_down_phoenix,
        instruments::autocallable::digital_coupon_note,
        instruments::autocallable::early_profit_snowball,
        instruments::autocallable::butterfly_snowball,
        instruments::autocallable::autocallable_observations,
    );
    module.add_class::<engines::monte_carlo::MonteCarloEstimate>()?;
    register_functions!(engines::monte_carlo::configure_monte_carlo_pool,);
    module.add_class::<BlackScholesMertonParameters>()?;
    module.add_class::<crate::heston::HestonParameters>()?;
    module.add_class::<crate::local_volatility::LocalVolatilitySurface>()?;
    module.add_class::<crate::local_volatility::LocalVolatilityParameters>()?;
    module.add_class::<market::RateTermStructure>()?;
    module.add_class::<TradingCalendar>()?;
    module.add_class::<schedule::ObservationSchedule>()?;
    module.add_class::<PricingContext>()?;
    module.add_class::<risk::PricingResult>()?;
    register_functions!(
        risk::implied_volatility,
        risk::numerical_greeks,
        risk::implied_snowball_coupon,
        risk::implied_phoenix_coupon,
        calendar::weekdays_calendar,
        calendar::all_days_calendar,
        calendar::sse_calendar,
        calendar::custom_calendar,
        schedule::fixed_interval_schedule,
        schedule::monthly_schedule,
        calendar::_calendar_parts,
        risk::_pricing_result,
        engines::monte_carlo::_monte_carlo_estimate,
    );
    crate::signatures::register(py, module)
}
