use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Analytic European vanilla pricing with deterministic rate curves.
    ///
    /// See Also
    /// --------
    /// EuropeanOption : Supported contract.
    ///
    AnalyticEuropeanEngine,
    PyRef<'_, EuropeanOption>,
    with_contract,
    |_option, _context| true,
    [
        ///
        /// Notes
        /// -----
        /// Uses analytic Greeks at positive volatility before expiry, scalar
        /// differences at zero volatility, and default terminal stencils at expiry.
        /// Use numerical_greeks for configurable scalar differences.
        ///
    ]
);
