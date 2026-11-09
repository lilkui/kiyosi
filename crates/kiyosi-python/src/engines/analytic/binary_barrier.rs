use crate::error::domain;
use crate::instruments::binary_barrier::{BinaryInstrument, with_binary};
use crate::market::PricingContext;
use pyo3::prelude::*;

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
    with_binary
);
