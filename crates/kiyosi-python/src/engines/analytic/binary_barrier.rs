use crate::instruments::binary_barrier::{BinaryInstrument, with_binary};
use crate::market::PricingContext;
use pyo3::prelude::*;

fn short(instrument: &BinaryInstrument<'_>, context: &kiyosi::PricingContext) -> bool {
    let (observations, at_expiry) = match instrument {
        BinaryInstrument::Binary(option) => (option.0.observation_times(), true),
        BinaryInstrument::Touch(option) => (
            option.0.observation_times(),
            option.0.settlement_timing() == kiyosi::SettlementTiming::AtExpiry,
        ),
    };
    super::short_barrier(observations, at_expiry, context)
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
