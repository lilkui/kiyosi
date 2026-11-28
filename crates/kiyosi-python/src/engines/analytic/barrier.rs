use crate::instruments::barrier::BarrierOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Analytic single-barrier pricing with flat rates.
    ///
    /// Scheduled monitoring uses the Broadie-Glasserman-Kou (BGK) continuity
    /// correction. Continuous knock-outs use survival-weighted quadrature when
    /// closed-form payoff terms nearly cancel.
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    ///
    AnalyticBarrierEngine,
    PyRef<'_, BarrierOption>,
    with_contract,
    |option, context| super::short_barrier(
        option
            .0
            .observation_schedule()
            .map(kiyosi::ObservationSchedule::times),
        option.0.rebate() == 0.0 || option.0.rebate_timing() == kiyosi::RebateTiming::AtExpiry,
        context,
    )
);
