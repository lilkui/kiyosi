use pyo3::prelude::*;
use pyo3::types::PyDict;

pub(crate) fn register(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    let defaults = PyDict::new(py);
    let shifts = kiyosi::NumericalShiftSettings::default();
    defaults.set_item("spot_shift", shifts.spot_shift)?;
    defaults.set_item("fixed_spot_shift", shifts.spot_shift())?;
    defaults.set_item("volatility_shift", shifts.volatility_shift)?;
    defaults.set_item("rate_shift", shifts.rate_shift)?;
    defaults.set_item("time_shift_days", shifts.time_shift_days)?;
    defaults.set_item("principal_ratio", kiyosi::DEFAULT_PRINCIPAL_RATIO)?;
    defaults.set_item("realized_average", kiyosi::DEFAULT_REALIZED_AVERAGE)?;
    defaults.set_item(
        "accumulated_quantity",
        kiyosi::AccumulatorTerms::DEFAULT_ACCUMULATED_QUANTITY,
    )?;
    defaults.set_item("rebate", kiyosi::BarrierOptionTerms::DEFAULT_REBATE)?;
    defaults.set_item(
        "coupon_rate",
        kiyosi::AutocallableObservation::DEFAULT_COUPON_RATE,
    )?;
    defaults.set_item(
        "coupon_barrier_level",
        kiyosi::AutocallableObservation::DEFAULT_COUPON_BARRIER_LEVEL,
    )?;
    defaults.set_item(
        "observation_mode",
        match kiyosi::ObservationMode::default() {
            kiyosi::ObservationMode::Continuous => "continuous",
            kiyosi::ObservationMode::Scheduled => "scheduled",
        },
    )?;
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
    solver!(
        "implied_volatility",
        kiyosi::ImpliedVolatilitySettings::default()
    );
    solver!("implied_coupon", kiyosi::ImpliedCouponSettings::default());
    module.add("_solver_signature_defaults", solvers)?;
    Ok(())
}
