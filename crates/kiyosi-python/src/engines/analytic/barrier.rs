use crate::error::domain;
use crate::instruments::barrier::BarrierOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

analytic_engine!(
    /// Flat-rate single-barrier analytics with BGK scheduled monitoring.
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    AnalyticBarrierEngine,
    BarrierOption
);
