use crate::error::domain;
use crate::instruments::range_accrual::RangeAccrualOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

analytic_engine!(
    /// Exact BSM range-coupon marginals with deterministic rate curves.
    ///
    /// See Also
    /// --------
    /// RangeAccrualOption : Supported contract.
    AnalyticRangeAccrualEngine,
    RangeAccrualOption
);
