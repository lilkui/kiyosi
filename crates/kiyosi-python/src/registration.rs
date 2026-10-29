use crate::calendar::TradingCalendar;
use crate::engines::analytic;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::{BlackScholesMertonParameters, PricingContext};
use crate::{calendar, engines, error, instruments, market, risk, schedule};
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", core::VERSION)?;
    let categories = PyDict::new(py);
    for category in core::ErrorCategory::ALL {
        categories.set_item(category.name(), category.name())?;
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("module", "kiyosi._native")?;
    module.add(
        "ErrorCategory",
        py.import("enum")?
            .getattr("Enum")?
            .call(("ErrorCategory", categories), Some(&kwargs))?,
    )?;
    module.add("KiyosiError", py.get_type::<error::KiyosiError>())?;
    let names = core::Greek::ALL.map(|g| g.name());
    module.add(
        "Greek",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(pyo3::types::PyTuple::new(py, names)?)?,
    )?;
    module.add_class::<instruments::accumulator::Accumulator>()?;
    module.add_class::<engines::quadrature::QuadratureAccumulatorEngine>()?;
    module.add_class::<instruments::asian::GeometricAveragePriceOption>()?;
    module.add_class::<instruments::asian::ArithmeticAveragePriceOption>()?;
    module.add_class::<analytic::asian::AnalyticGeometricAveragePriceEngine>()?;
    module.add_class::<analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine>()?;
    module.add_class::<instruments::binary_barrier::BinaryBarrierOption>()?;
    module.add_class::<instruments::binary_barrier::TouchOption>()?;
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
    module.add_class::<analytic::binary_barrier::AnalyticBinaryBarrierEngine>()?;
    module.add_class::<instruments::double_touch::DoubleTouchOption>()?;
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
    module.add_class::<instruments::autocallable::SnowballOption>()?;
    module.add_class::<instruments::autocallable::BinarySnowballOption>()?;
    module.add_class::<instruments::autocallable::TernarySnowballOption>()?;
    module.add_class::<instruments::autocallable::PhoenixOption>()?;
    module.add_class::<engines::quadrature::QuadratureAutocallableEngine>()?;
    module.add_function(wrap_pyfunction!(
        instruments::autocallable::autocallable_observations,
        module
    )?)?;
    module.add_class::<instruments::range_accrual::RangeAccrualOption>()?;
    module.add_class::<analytic::range_accrual::AnalyticRangeAccrualEngine>()?;
    module.add_class::<instruments::double_barrier::DoubleBarrierOption>()?;
    module.add_class::<analytic::double_barrier::AnalyticDoubleBarrierEngine>()?;
    module.add_class::<instruments::barrier::BarrierOption>()?;
    module.add_class::<analytic::barrier::AnalyticBarrierEngine>()?;
    module.add_class::<engines::quadrature::QuadratureBarrierEngine>()?;
    module.add_class::<engines::binomial::CoxRossRubinsteinBarrierEngine>()?;
    module.add_class::<EuropeanOption>()?;
    module.add_class::<instruments::american::AmericanOption>()?;
    module.add_class::<analytic::american::BjerksundStenslandVanillaEngine>()?;
    module.add_class::<instruments::digital::CashOrNothingOption>()?;
    module.add_class::<instruments::digital::AssetOrNothingOption>()?;
    module.add_class::<analytic::digital::AnalyticDigitalEngine>()?;
    module.add_class::<engines::binomial::CoxRossRubinsteinVanillaEngine>()?;
    module.add_class::<engines::binomial::CoxRossRubinsteinDigitalEngine>()?;
    module.add_class::<engines::quadrature::QuadratureVanillaEngine>()?;
    module.add_class::<engines::quadrature::QuadratureDigitalEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloVanillaEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloDigitalEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloBarrierEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloDoubleBarrierEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloAutocallableEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloAccumulatorEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloAsianEngine>()?;
    module.add_class::<engines::monte_carlo::MonteCarloEstimate>()?;
    module.add(
        "MonteCarloSampling",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(("pseudo_random", "sobol"))?,
    )?;
    module.add_class::<engines::finite_difference::FiniteDifferenceAutocallableEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceAccumulatorEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceVanillaEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceDigitalEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceBarrierEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceDoubleBarrierEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceDoubleTouchEngine>()?;
    module.add_class::<engines::finite_difference::FiniteDifferenceBinaryBarrierEngine>()?;
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
    module.add_class::<analytic::vanilla::AnalyticVanillaEngine>()?;
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
