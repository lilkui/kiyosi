use pyo3::prelude::*;
use pyo3::types::PyDict;

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    let defaults = PyDict::new(py);
    defaults.set_item("lockout_periods", kiyosi::DEFAULT_PHOENIX_LOCKOUT_PERIODS)?;
    defaults.set_item(
        "principal_redemption_ratio",
        kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
    )?;
    defaults.set_item("realized_average", kiyosi::DEFAULT_REALIZED_AVERAGE)?;
    defaults.set_item(
        "accumulated_quantity",
        kiyosi::AccumulatorTerms::DEFAULT_ACCUMULATED_QUANTITY,
    )?;
    defaults.set_item(
        "knock_out_settled",
        kiyosi::AccumulatorTerms::DEFAULT_KNOCK_OUT_SETTLED,
    )?;
    defaults.set_item("rebate", kiyosi::BarrierOptionTerms::DEFAULT_REBATE)?;
    defaults.set_item(
        "rebate_timing",
        match kiyosi::RebateTiming::default() {
            kiyosi::RebateTiming::AtHit => "at_hit",
            kiyosi::RebateTiming::AtExpiry => "at_expiry",
        },
    )?;
    defaults.set_item(
        "settlement_timing",
        match kiyosi::SettlementTiming::default() {
            kiyosi::SettlementTiming::AtHit => "at_hit",
            kiyosi::SettlementTiming::AtExpiry => "at_expiry",
        },
    )?;
    defaults.set_item(
        "option_type",
        match kiyosi::AccumulatorTerms::DEFAULT_OPTION_TYPE {
            kiyosi::OptionType::Call => "call",
            kiyosi::OptionType::Put => "put",
        },
    )?;
    defaults.set_item(
        "knock_in_observation_mode",
        match kiyosi::KnockInObservationMode::default() {
            kiyosi::KnockInObservationMode::EveryTradingDay => "every_trading_day",
            kiyosi::KnockInObservationMode::AtExpiry => "at_expiry",
        },
    )?;
    module.add("_signature_defaults", defaults)?;
    let solvers = PyDict::new(py);
    macro_rules! solver {
        ($name:literal, $settings:expr) => {
            let settings = $settings;
            let values = PyDict::new(py);
            values.set_item("lower_bound", settings.lower_bound)?;
            values.set_item("upper_bound", settings.upper_bound)?;
            values.set_item("price_tolerance", settings.price_tolerance)?;
            values.set_item("parameter_tolerance", settings.parameter_tolerance)?;
            values.set_item("max_iterations", settings.max_iterations)?;
            solvers.set_item($name, values)?;
        };
    }
    let volatility = PyDict::new(py);
    volatility.set_item(
        "price_tolerance",
        kiyosi::ImpliedVolatilitySettings::DEFAULT_PRICE_TOLERANCE,
    )?;
    volatility.set_item(
        "parameter_tolerance",
        kiyosi::ImpliedVolatilitySettings::DEFAULT_PARAMETER_TOLERANCE,
    )?;
    volatility.set_item(
        "max_iterations",
        kiyosi::ImpliedVolatilitySettings::DEFAULT_MAX_ITERATIONS,
    )?;
    solvers.set_item("implied_volatility", volatility)?;
    solver!(
        "implied_snowball_coupon",
        kiyosi::ImpliedCouponSettings::default()
    );
    solver!(
        "implied_phoenix_coupon",
        kiyosi::ImpliedCouponSettings::default()
    );
    module.add("_solver_signature_defaults", solvers)?;
    Ok(())
}
