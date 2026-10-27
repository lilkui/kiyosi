//! European Black–Scholes–Merton pricing, independent of Python.
//!
//! All public values own their data and are `Send + Sync`. Distinct and shared
//! instances support concurrent reads and pricing without synchronization.
//! Mutation requires exclusive Rust access; Python exposes immutable values.
//! Calls are synchronous, with no cancellation or signal polling. Domain
//! rejections use [`Result`], never panics. Allocation exhaustion follows Rust's
//! allocator behavior (normally process abort); internal panics are not domain errors.
//!
//! ```
//! use kiyosi_core::{AnalyticVanillaEngine, BlackScholesMertonParameters, Date,
//!                   EuropeanOption, OptionType, PricingContext};
//! # fn main() -> kiyosi_core::Result<()> {
//! let start = Date::new(2025, 1, 1)?;
//! let expiry = Date::new(2026, 1, 1)?;
//! let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
//! let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
//! let context = PricingContext::new(model, 100.0, start.into())?;
//! assert!((AnalyticVanillaEngine.price(&option, &context)? - 9.227005508154036).abs() < 1e-12);
//! # Ok(()) }
//! ```

mod error;
mod instruments;
mod market;
mod pricing;
mod term_structure;
mod time;

pub use error::{Error, ErrorCategory, Result};
pub use instruments::{EuropeanOption, OptionType};
pub use market::{
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, PricingContext,
    PricingContextChanges, Rate, TradingCalendar,
};
pub use pricing::AnalyticVanillaEngine;
pub use term_structure::RateTermStructure;
pub use time::{Date, Timestamp};

/// Package version, shared by the candidate Python extension.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
