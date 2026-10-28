//! Python representation adapters; all domain validation lives in kiyosi-core.

use kiyosi_core as core;
mod accumulator;
mod american;
mod analytics;
mod asian;
mod autocallable;
mod barrier;
mod binary_barrier;
mod binomial;
mod calendar;
mod conversions;
mod digital;
mod double_barrier;
mod double_touch;
mod error;
mod finite_difference;
mod market;
mod monte_carlo;
mod quadrature;
mod range_accrual;
mod schedule;
mod vanilla;

use calendar::TradingCalendar;
use conversions::{
    check_changes, check_integer, date, dates, direction, integer, python_date, real,
};
use error::domain;
use market::{BlackScholesMertonParameters, PricingContext};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDict};
use vanilla::EuropeanOption;

#[pymodule]
fn _native(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", core::VERSION)?;
    let categories = PyDict::new(py);
    for category in core::ErrorCategory::ALL {
        categories.set_item(category.name(), category as u8)?;
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("module", "kiyosi._native")?;
    module.add(
        "ErrorCategory",
        py.import("enum")?
            .getattr("IntEnum")?
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
    module.add_class::<accumulator::Accumulator>()?;
    module.add_class::<accumulator::QuadratureAccumulatorEngine>()?;
    module.add_class::<asian::GeometricAveragePriceOption>()?;
    module.add_class::<asian::ArithmeticAveragePriceOption>()?;
    module.add_class::<asian::AnalyticGeometricAveragePriceEngine>()?;
    module.add_class::<asian::TurnbullWakemanArithmeticAveragePriceEngine>()?;
    binary_barrier::register(module)?;
    double_touch::register(module)?;
    module.add_function(wrap_pyfunction!(autocallable::standard_snowball, module)?)?;
    module.add_function(wrap_pyfunction!(autocallable::european_snowball, module)?)?;
    module.add_function(wrap_pyfunction!(autocallable::step_down_snowball, module)?)?;
    module.add_function(wrap_pyfunction!(autocallable::both_down_snowball, module)?)?;
    module.add_function(wrap_pyfunction!(
        autocallable::dual_coupon_snowball,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(autocallable::parachute_snowball, module)?)?;
    module.add_function(wrap_pyfunction!(autocallable::otm_snowball, module)?)?;
    module.add_function(wrap_pyfunction!(
        autocallable::loss_capped_snowball,
        module
    )?)?;
    module.add_class::<autocallable::AutocallableObservation>()?;
    module.add_class::<autocallable::SnowballOption>()?;
    module.add_class::<autocallable::BinarySnowballOption>()?;
    module.add_class::<autocallable::TernarySnowballOption>()?;
    module.add_class::<autocallable::PhoenixOption>()?;
    module.add_class::<autocallable::QuadratureAutocallableEngine>()?;
    module.add_function(wrap_pyfunction!(
        autocallable::autocallable_observations,
        module
    )?)?;
    module.add_class::<range_accrual::RangeAccrualOption>()?;
    module.add_class::<range_accrual::AnalyticRangeAccrualEngine>()?;
    module.add_class::<double_barrier::DoubleBarrierOption>()?;
    module.add_class::<double_barrier::AnalyticDoubleBarrierEngine>()?;
    module.add_class::<barrier::BarrierOption>()?;
    module.add_class::<barrier::AnalyticBarrierEngine>()?;
    module.add_class::<barrier::QuadratureBarrierEngine>()?;
    module.add_class::<barrier::CoxRossRubinsteinBarrierEngine>()?;
    module.add_class::<EuropeanOption>()?;
    module.add_class::<american::AmericanOption>()?;
    module.add_class::<american::BjerksundStenslandVanillaEngine>()?;
    module.add_class::<digital::CashOrNothingOption>()?;
    module.add_class::<digital::AssetOrNothingOption>()?;
    module.add_class::<digital::AnalyticDigitalEngine>()?;
    module.add_class::<binomial::CoxRossRubinsteinVanillaEngine>()?;
    module.add_class::<binomial::CoxRossRubinsteinDigitalEngine>()?;
    module.add_class::<quadrature::QuadratureVanillaEngine>()?;
    module.add_class::<quadrature::QuadratureDigitalEngine>()?;
    module.add_class::<monte_carlo::MonteCarloVanillaEngine>()?;
    module.add_class::<monte_carlo::MonteCarloDigitalEngine>()?;
    module.add_class::<monte_carlo::MonteCarloBarrierEngine>()?;
    module.add_class::<monte_carlo::MonteCarloDoubleBarrierEngine>()?;
    module.add_class::<monte_carlo::MonteCarloAutocallableEngine>()?;
    module.add_class::<monte_carlo::MonteCarloAccumulatorEngine>()?;
    module.add_class::<monte_carlo::MonteCarloAsianEngine>()?;
    module.add_class::<monte_carlo::MonteCarloEstimate>()?;
    module.add(
        "MonteCarloSampling",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(("pseudo_random", "sobol"))?,
    )?;
    module.add_class::<finite_difference::FiniteDifferenceAutocallableEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceAccumulatorEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceVanillaEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceDigitalEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceBarrierEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceDoubleBarrierEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceDoubleTouchEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceBinaryBarrierEngine>()?;
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
    module.add_class::<vanilla::AnalyticVanillaEngine>()?;
    module.add_class::<analytics::PricingResult>()?;
    module.add_function(wrap_pyfunction!(analytics::implied_volatility, module)?)?;
    module.add_function(wrap_pyfunction!(autocallable::implied_coupon, module)?)?;
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
