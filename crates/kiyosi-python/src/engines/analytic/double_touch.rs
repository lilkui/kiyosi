use crate::instruments::double_touch::DoubleTouchOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Flat-rate continuous cash double-touch and double-no-touch pricing.
    ///
    /// Either inclusive barrier triggers a touch. Supports expiry payments and
    /// double-touch at-hit payments; past at-hit payments have zero remaining
    /// value. Uses image/spectral series and adaptive first-exit integration for
    /// at-hit payments. Nonconstant rate curves are rejected. At positive
    /// volatility, unresolved scheduled monitoring is unsupported. Resolved cash
    /// flows and zero-volatility paths support scheduled monitoring after calendar
    /// and history validation. Greeks use numerical differences. At expiry they
    /// follow terminal payoffs, with time Greeks absent. Before expiry at positive
    /// volatility, barrier equality without a prior touch leaves spot Greeks
    /// absent; rate and volatility Greeks follow the resolved payoff. Stencils
    /// crossing a barrier omit affected measures.
    ///
    /// See Also
    /// --------
    /// DoubleTouchOption : Supported cash contract.
    /// FiniteDifferenceDoubleTouchEngine : Supports schedules and nonconstant rate curves.
    ///
    AnalyticDoubleTouchEngine,
    PyRef<'_, DoubleTouchOption>,
    with_contract
);
