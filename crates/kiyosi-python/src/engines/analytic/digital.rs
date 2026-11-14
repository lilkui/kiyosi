use crate::instruments::digital::{DigitalInstrument, with_digital};
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Analytic European digital pricing with deterministic rate curves.
    ///
    /// See Also
    /// --------
    /// CashOrNothingOption : Supported contract.
    /// AssetOrNothingOption : Supported contract.
    ///
    AnalyticDigitalEngine,
    DigitalInstrument<'_>,
    with_digital,
    |_option, _context| true,
    [
        ///
        /// Notes
        /// -----
        /// Delta and gamma are analytic at positive volatility before expiry. At zero
        /// volatility they use scalar differences. Other Greeks use numerical
        /// differences with the supplied shifts. At expiry spot Greeks use the
        /// supplied terminal-payoff stencils. Use numerical_greeks for scalar-only
        /// differences.
        ///
    ]
);
