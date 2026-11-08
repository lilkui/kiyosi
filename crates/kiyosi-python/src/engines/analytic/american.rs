use crate::error::domain;
use crate::instruments::american::AmericanOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Bjerksund-Stensland 2002 American approximation with flat rates.
    ///
    /// See Also
    /// --------
    /// AmericanOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Requires BSM with constant rates. At positive volatility before expiry,
    /// negative transformed interest rates (risk-free rate for calls, dividend
    /// yield for puts) and exercise boundaries at or below strike raise
    /// UnsupportedOperationError. Zero volatility uses exact deterministic
    /// exercise; expiry returns intrinsic value.
    ///
    BjerksundStenslandVanillaEngine,
    AmericanOption,
    |_option, _context| true
);
