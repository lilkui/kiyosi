use crate::error::domain;
use crate::instruments::binary_barrier::{BinaryInstrument, with_binary};
use crate::market::PricingContext;
use pyo3::prelude::*;

fn short(instrument: &BinaryInstrument<'_>, context: &kiyosi::PricingContext) -> bool {
    let (continuous, at_expiry) = match instrument {
        BinaryInstrument::Binary(option) => (option.0.observation_times().is_none(), true),
        BinaryInstrument::Touch(option) => (
            option.0.observation_times().is_none(),
            option.0.settlement_timing() == kiyosi::SettlementTiming::AtExpiry,
        ),
    };
    continuous
        && at_expiry
        && matches!(context.model_parameters(), kiyosi::ModelParameters::BlackScholesMerton(model)
            if model.volatility() > 0.0)
}

analytic_engine!(
    /// Flat-rate binary-barrier and single-touch analytics.
    ///
    /// See Also
    /// --------
    /// BinaryBarrierOption : Supported contract.
    /// TouchOption : Supported contract.
    ///
    AnalyticBinaryBarrierEngine,
    BinaryInstrument<'_>,
    with_binary,
    |instrument, context| short(instrument, context)
);
