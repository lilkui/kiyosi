use crate::error::domain;
use crate::instruments::asian::{ArithmeticAveragePriceOption, GeometricAveragePriceOption};
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

analytic_engine!(
    /// Prices geometric continuous-average contracts with deterministic rate curves.
    ///
    /// See Also
    /// --------
    /// GeometricAveragePriceOption : Supported contract.
    AnalyticGeometricAveragePriceEngine,
    GeometricAveragePriceOption
);
analytic_engine!(
    /// Approximates arithmetic continuous-average prices with flat rates before expiry.
    ///
    /// Uses Levy-compatible moments.
    ///
    /// See Also
    /// --------
    /// ArithmeticAveragePriceOption : Supported contract.
    TurnbullWakemanArithmeticAveragePriceEngine,
    ArithmeticAveragePriceOption
);
