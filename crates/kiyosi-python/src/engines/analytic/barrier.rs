use crate::error::domain;
use crate::instruments::barrier::BarrierOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Analytic single-barrier pricing with flat rates.
    ///
    /// Scheduled monitoring uses the Broadie-Glasserman-Kou (BGK) continuity
    /// correction.
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    ///
    AnalyticBarrierEngine,
    PyRef<'_, BarrierOption>,
    with_contract,
    |option, context| option.0.rebate() == 0.0
        && option.0.observation_times().is_none()
        && matches!(context.model_parameters(), kiyosi::ModelParameters::BlackScholesMerton(model)
            if model.volatility() > 0.0)
);
