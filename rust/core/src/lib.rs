//! Calendars, observation schedules and European vanilla and digital BSM pricing,
//! independent of Python.
//!
//! All public values own their data and are `Send + Sync`. Distinct and shared
//! instances support concurrent reads and pricing without synchronization.
//! Mutation requires exclusive Rust access; Python exposes immutable values.
//! Custom calendar predicates must also uphold [`TradingCalendar`]'s determinism
//! contract; panics in user predicates retain native Rust behavior.
//! User-supplied [`EuropeanPricingEngine`] implementations retain their own
//! thread-safety and operational-failure contracts.
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
//! let result = AnalyticVanillaEngine.price_with_greeks(&option, &context,
//!     kiyosi_core::GreeksRequest::all(), Default::default())?;
//! assert!(result.require(kiyosi_core::Greek::Delta)? > 0.0);
//! let sigma = kiyosi_core::implied_volatility(&AnalyticVanillaEngine, &option,
//!     &context, result.price(), Default::default())?;
//! assert!((sigma - 0.2).abs() < 1e-8);
//! # Ok(()) }
//! ```

mod analytics;
mod calendar;
mod digital;
mod error;
mod implied;
mod instruments;
mod market;
mod numerical;
mod pricing;
mod schedule;
mod term_structure;
mod time;

pub use analytics::{
    EuropeanPricingEngine, Greek, GreeksRequest, NumericalShiftSettings, PricingResult,
};
pub use calendar::{BusinessDayConvention, TradingCalendar};
pub use digital::AnalyticDigitalEngine;
pub use error::{Error, ErrorCategory, Result};
pub use implied::{ImpliedVolatilitySettings, implied_volatility};
pub use instruments::{
    DigitalOption, DigitalOptionChanges, EuropeanContract, EuropeanOption, OptionType,
};
pub use market::{
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, PricingContext,
    PricingContextChanges, Rate,
};
pub use numerical::numerical_greeks;
pub use pricing::AnalyticVanillaEngine;
pub use schedule::ObservationSchedule;
pub use term_structure::RateTermStructure;
pub use time::{Date, Timestamp};

/// Package version, shared by the candidate Python extension.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
