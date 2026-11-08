use crate::error::domain;
use crate::instruments::asian::{ArithmeticAveragePriceOption, GeometricAveragePriceOption};
use crate::market::PricingContext;
use pyo3::prelude::*;

analytic_engine!(
    /// Prices continuously monitored geometric average-price Asian options.
    ///
    /// Supports deterministic risk-free-rate and dividend-yield curves.
    ///
    /// See Also
    /// --------
    /// GeometricAveragePriceOption : Supported contract.
    AnalyticGeometricAveragePriceEngine,
    GeometricAveragePriceOption
);
analytic_engine!(
    /// Approximates continuously monitored arithmetic average-price Asian option prices.
    ///
    /// Requires flat rates before expiry. Uses the Turnbull-Wakeman
    /// (Levy-compatible) lognormal moment-matching approximation.
    ///
    /// See Also
    /// --------
    /// ArithmeticAveragePriceOption : Supported contract.
    TurnbullWakemanArithmeticAveragePriceEngine,
    ArithmeticAveragePriceOption
);
