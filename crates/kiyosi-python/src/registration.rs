use crate::calendar::TradingCalendar;
use crate::engines::analytic;
use crate::market::{BlackScholesMertonParameters, PricingContext};
use crate::{calendar, engines, error, instruments, market, risk, schedule};
use pyo3::prelude::*;

macro_rules! pricing_classes {
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
    register_engines,
    is_engine,
    [
        analytic::european::AnalyticEuropeanEngine,
        analytic::american::BjerksundStenslandVanillaEngine,
        analytic::digital::AnalyticDigitalEngine,
        analytic::asian::AnalyticGeometricAveragePriceEngine,
        analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine,
        analytic::barrier::AnalyticBarrierEngine,
        analytic::binary_barrier::AnalyticBinaryBarrierEngine,
        analytic::double_barrier::AnalyticDoubleBarrierEngine,
        analytic::range_accrual::AnalyticRangeAccrualEngine,
        engines::binomial::CoxRossRubinsteinVanillaEngine,
        engines::binomial::CoxRossRubinsteinDigitalEngine,
        engines::binomial::CoxRossRubinsteinBarrierEngine,
        engines::quadrature::QuadratureBarrierEngine,
        engines::quadrature::QuadratureAutocallableEngine,
        engines::quadrature::QuadratureAccumulatorEngine,
        engines::finite_difference::FiniteDifferenceVanillaEngine,
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
        engines::monte_carlo::MonteCarloDoubleBarrierEngine,
        engines::monte_carlo::MonteCarloAutocallableEngine,
        engines::monte_carlo::MonteCarloAccumulatorEngine,
    ]
);

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_instruments(module)?;
    register_engines(module)?;
    module.add("__version__", kiyosi::VERSION)?;
    module.add("KiyosiError", py.get_type::<error::KiyosiError>())?;
    module.add(
        "InvalidInputError",
        py.get_type::<error::InvalidInputError>(),
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
    let names = kiyosi::Greek::ALL.map(|g| g.name());
    module.add(
        "Greek",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(pyo3::types::PyTuple::new(py, names)?)?,
    )?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::cash_binary_barrier_option,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::asset_binary_barrier_option,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::cash_one_touch_up,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::cash_one_touch_down,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::cash_no_touch_up,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::cash_no_touch_down,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::asset_one_touch_up,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::asset_one_touch_down,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::asset_no_touch_up,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::binary_barrier::asset_no_touch_down,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::double_touch::cash_double_touch,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::double_touch::cash_double_no_touch,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::standard_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::european_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::step_down_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::both_down_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::dual_coupon_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::parachute_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::otm_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::loss_capped_snowball,
        module
    )?)?;
    module.add_class::<instruments::autocallable::AutocallableObservation>()?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::autocallable_observations,
        module
    )?)?;
    module.add_class::<engines::monte_carlo::MonteCarloEstimate>()?;
    module.add(
        "MonteCarloSampling",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(("pseudo_random", "sobol"))?,
    )?;
    module.add(
        "FiniteDifferenceScheme",
        py.import("typing")?.getattr("Literal")?.get_item((
            "explicit_euler",
            "implicit_euler",
            "crank_nicolson",
        ))?,
    )?;
    module.add_class::<BlackScholesMertonParameters>()?;
    module.add_class::<market::RateTermStructure>()?;
    module.add_class::<TradingCalendar>()?;
    module.add_class::<schedule::ObservationSchedule>()?;
    module.add_class::<PricingContext>()?;
    module.add_class::<risk::PricingResult>()?;
    module.add_function(wrap_pyfunction!(risk::implied_volatility, module)?)?;
    module.add_function(wrap_pyfunction!(risk::implied_coupon, module)?)?;
    module.add(
        "CouponQuoteConvention",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(("shift_maturity_coupon", "preserve_maturity_coupon"))?,
    )?;
    module.add_function(wrap_pyfunction!(calendar::weekdays_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(calendar::all_days_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(calendar::sse_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(calendar::custom_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(schedule::fixed_interval_schedule, module)?)?;
    module.add_function(wrap_pyfunction!(schedule::monthly_schedule, module)?)?;
    Ok(())
}
