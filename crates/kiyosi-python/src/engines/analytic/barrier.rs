use crate::error::domain;
use crate::instruments::barrier::BarrierOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Analytic single-barrier pricing with flat rates.
    ///
    /// Scheduled monitoring uses the Broadie-Glasserman-Kou (BGK) continuity correction.
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    AnalyticBarrierEngine,
    BarrierOption
);
