use crate::error::domain;
use crate::instruments::double_barrier::DoubleBarrierOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Flat-rate continuous double-barrier analytics.
    ///
    /// Zero-volatility BSM paths are evaluated exactly, including scheduled
    /// monitoring.
    ///
    /// See Also
    /// --------
    /// DoubleBarrierOption : Supported contract.
    ///
    AnalyticDoubleBarrierEngine,
    PyRef<'_, DoubleBarrierOption>,
    with_contract,
    |option, context| option.0.touch_state() == Some(kiyosi::BarrierTouchState::Touched)
        && matches!(context.model_parameters(), kiyosi::ModelParameters::BlackScholesMerton(model)
            if model.volatility() > 0.0)
);
