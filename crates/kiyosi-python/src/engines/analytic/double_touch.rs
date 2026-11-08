use crate::error::domain;
use crate::instruments::double_touch::DoubleTouchOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Prices continuous cash double-touch and double-no-touch contracts with flat rates.
    ///
    /// Either inclusive barrier triggers a touch. Supports expiry payments and
    /// double-touch at-hit payments; past at-hit payments have zero remaining value.
    /// Uses image/spectral series and adaptive first-exit integration for at-hit payments.
    /// Nonconstant rate curves are rejected. At positive volatility, unresolved
    /// scheduled monitoring is unsupported. Resolved cash flows and zero-volatility
    /// paths support scheduled monitoring after calendar and history validation.
    /// Greeks use numerical differences; expiry and current barrier equality without
    /// a prior touch omit Greeks. Stencils crossing a barrier omit affected measures.
    ///
    /// See Also
    /// --------
    /// DoubleTouchOption : Supported cash contract.
    /// cash_double_touch : Double one-touch constructor.
    /// cash_double_no_touch : Double no-touch constructor.
    /// FiniteDifferenceDoubleTouchEngine : Supports schedules and nonconstant rate curves.
    AnalyticDoubleTouchEngine,
    DoubleTouchOption
);
