use crate::instruments::binary_barrier::{BinaryInstrument, with_binary};
use crate::market::PricingContext;
use pyo3::prelude::*;

fn short(instrument: &BinaryInstrument<'_>, context: &kiyosi::PricingContext) -> bool {
    let (schedule, at_expiry) = match instrument {
        BinaryInstrument::Binary(option) => (option.0.observation_schedule(), true),
        BinaryInstrument::Touch(option) => (
            option.0.observation_schedule(),
            option.0.settlement_timing() == kiyosi::SettlementTiming::AtExpiry,
        ),
    };
    super::short_barrier(
        schedule.map(kiyosi::ObservationSchedule::times),
        at_expiry,
        context,
    )
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
