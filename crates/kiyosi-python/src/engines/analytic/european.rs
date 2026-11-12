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
        /// Uses analytic Greeks at positive volatility before expiry and scalar
        /// differences at zero volatility. At expiry, supported Greeks use exact
        /// payoff derivatives; spot derivatives are undefined at the strike and
        /// time Greeks are absent. Use numerical_greeks for scalar differences.
        ///
    ]
);
