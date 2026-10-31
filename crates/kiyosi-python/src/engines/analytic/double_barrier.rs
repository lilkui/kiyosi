use crate::error::domain;
use crate::instruments::double_barrier::DoubleBarrierOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

analytic_engine!(
    /// Flat-rate continuous double-barrier analytics.
    ///
    /// See Also
    /// --------
    /// DoubleBarrierOption : Supported contract.
    AnalyticDoubleBarrierEngine,
    DoubleBarrierOption
);
