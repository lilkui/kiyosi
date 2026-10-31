//! Prices derivatives under the Black–Scholes–Merton model.
//!
//! Supports vanilla and exotic options, autocallable notes and accumulators with
//! analytic, binomial, quadrature, finite-difference and Monte Carlo engines.
//! Provides calendars, schedules, Greeks and implied-parameter solvers.
//! The core builds with Cargo alone; Python bindings live in a separate crate.
//!
//! Construct a contract and a [`PricingContext`], then choose an engine for that
//! contract. [`PricingEngine`] also supports [`numerical_greeks`] and the implied
//! parameter solvers. Contract constructors validate terms; pricing validates
//! valuation dates, calendar consistency and required prior history.
//! Annual rates and volatility use decimal units. Rate curves use Actual/365
//! Fixed time; [`TradingCalendar::trading_year_fraction`] uses its own day-count
//! basis. [`Greek`] documents sensitivity reporting units.
//!
//! # Concurrency and errors
//!
//! Built-in public types own their data and are `Send + Sync`. Distinct and shared
//! instances support concurrent reads and pricing without synchronization.
//! Mutation requires exclusive Rust access; Python exposes immutable values.
//! Custom calendar predicates must also uphold [`TradingCalendar`]'s determinism
//! contract; panics in user predicates retain native Rust behavior.
//! User-supplied [`PricingEngine`] implementations retain their own
//! thread-safety and operational-failure contracts.
//! Calls are synchronous, with no cancellation or signal polling. Domain
//! rejections use [`Result`], never panics. Allocation exhaustion follows Rust's
//! allocator behavior (normally process abort); internal panics are not domain errors.
//! Monte Carlo entropy-source and thread-creation failures retain native I/O
//! details through [`Error::Io`] and the standard error source.
//!
//! # Examples
//!
//! ```rust
//! use kiyosi::{AnalyticEuropeanEngine, BlackScholesMertonParameters, Date,
//!                   EuropeanOption, OptionType, PricingContext};
//! # fn main() -> kiyosi::Result<()> {
//! let start = Date::new(2025, 1, 1)?;
//! let expiry = Date::new(2026, 1, 1)?;
//! let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
//! let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
//! let context = PricingContext::new(model, 100.0, start.into())?;
//! assert!((AnalyticEuropeanEngine.price(&option, &context)? - 9.227005508154036).abs() < 1e-12);
//! let result = AnalyticEuropeanEngine.price_with_greeks(&option, &context,
//!     kiyosi::GreeksRequest::all())?;
//! assert!(result.require(kiyosi::Greek::Delta)? > 0.0);
//! let sigma = kiyosi::implied_volatility(&AnalyticEuropeanEngine, &option,
//!     &context, result.price(), Default::default())?;
//! assert!((sigma - 0.2).abs() < 1e-8);
//! # Ok(()) }
//! ```

mod engines;
mod error;
mod instruments;
mod market;
mod risk;
mod time;

pub use engines::analytic::american::BjerksundStenslandVanillaEngine;
pub use engines::analytic::asian::{
    AnalyticGeometricAveragePriceEngine, TurnbullWakemanArithmeticAveragePriceEngine,
};
pub use engines::analytic::barrier::AnalyticBarrierEngine;
pub use engines::analytic::binary_barrier::AnalyticBinaryBarrierEngine;
pub use engines::analytic::digital::AnalyticDigitalEngine;
pub use engines::analytic::double_barrier::AnalyticDoubleBarrierEngine;
pub use engines::analytic::european::AnalyticEuropeanEngine;
pub use engines::analytic::range_accrual::AnalyticRangeAccrualEngine;
pub use engines::binomial::{
    BinomialSettings, CoxRossRubinsteinBarrierEngine, CoxRossRubinsteinDigitalEngine,
    CoxRossRubinsteinVanillaEngine,
};
pub use engines::finite_difference::{
    FiniteDifferenceAccumulatorEngine, FiniteDifferenceAutocallableEngine,
    FiniteDifferenceBarrierEngine, FiniteDifferenceBinaryBarrierEngine,
    FiniteDifferenceDigitalEngine, FiniteDifferenceDoubleBarrierEngine,
    FiniteDifferenceDoubleTouchEngine, FiniteDifferenceScheme, FiniteDifferenceSettings,
    FiniteDifferenceVanillaEngine,
};
pub use engines::monte_carlo::{
    MonteCarloAccumulatorEngine, MonteCarloAsianEngine, MonteCarloAutocallableEngine,
    MonteCarloBarrierEngine, MonteCarloDigitalEngine, MonteCarloDoubleBarrierEngine,
    MonteCarloEstimate, MonteCarloSampling, MonteCarloSettings, MonteCarloVanillaEngine,
};
pub use engines::quadrature::{
    QuadratureAccumulatorEngine, QuadratureAutocallableEngine, QuadratureBarrierEngine,
    QuadratureSettings,
};
pub use error::{Error, Result};
pub use instruments::accumulator::{Accumulator, AccumulatorTerms};
pub use instruments::asian::{
    ArithmeticAveragePriceOption, DEFAULT_REALIZED_AVERAGE, GeometricAveragePriceOption,
};
pub use instruments::autocallable::{
    AutocallableBarrierState, AutocallableNote, AutocallableObservation, AutocallableTerms,
    BinarySnowballOption, BinarySnowballTerms, DEFAULT_PRINCIPAL_RATIO, KnockInObservationMode,
    PhoenixOption, PhoenixTerms, SnowballCouponTerms, SnowballOption, SnowballPreset,
    SnowballPresetTerms, SnowballTerms, TernarySnowballOption, TernarySnowballTerms,
    autocallable_observations,
};
pub use instruments::barrier::{
    BarrierOption, BarrierOptionTerms, BarrierTouchState, BarrierType, ObservationMode,
    RebateTiming,
};
pub use instruments::binary_barrier::{
    BinaryBarrierOption, BinaryBarrierTerms, SettlementTiming, TouchOption, TouchOptionTerms,
};
pub use instruments::double_barrier::{
    DoubleBarrierOption, DoubleBarrierOptionTerms, DoubleBarrierType,
};
pub use instruments::double_touch::{DoubleTouchOption, DoubleTouchOptionTerms};
pub use instruments::range_accrual::{RangeAccrualOption, RangeAccrualTerms};
pub use instruments::vanilla::{
    AmericanOption, AmericanOptionChanges, DigitalOption, DigitalOptionChanges, EuropeanOption,
    OptionContract, OptionType,
};
pub use market::{
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, PricingContext,
    PricingContextChanges, Rate, RateTermStructure,
};
pub use risk::analytics::{
    Greek, GreeksRequest, NumericalShiftSettings, PricingEngine, PricingResult,
};
pub use risk::coupon::{
    CouponQuoteConvention, ImpliedCouponSettings, implied_coupon, implied_snowball_coupon,
};
pub use risk::implied::{ImpliedVolatilitySettings, implied_volatility};
pub use risk::numerical::numerical_greeks;
pub use time::{BusinessDayConvention, Date, ObservationSchedule, Timestamp, TradingCalendar};

/// Package version, shared with the Python bindings.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
