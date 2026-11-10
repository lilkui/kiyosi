//! Prices derivatives under Black-Scholes-Merton, local-volatility, and Heston models.
//!
//! Construct a contract and a [`PricingContext`], then select an engine for the
//! contract and model. Engine methods return prices, selected Greeks, and Monte
//! Carlo sampling statistics. [`numerical_greeks`] selects scalar BSM differences;
//! [`implied_volatility`] and the coupon solvers invert an engine's price curve.
//! Engine documentation specifies model support and numerical controls.
//!
//! # Conventions
//!
//! Annual rates and annualized volatility use decimal units. Rates are continuously
//! compounded; rate curves use Actual/365 Fixed. [`Greek`] defines sensitivity units.
//! BSM volatility may be zero, selecting deterministic pricing. Check numerical
//! convergence separately from Monte Carlo sampling uncertainty and solver tolerances.
//!
//! Contract and market times use [`Timestamp`] in a caller-selected time basis.
//! Contract boundaries are inclusive. Calendars classify [`Date`] values without
//! moving those boundaries. Constructors validate terms and observation calendars;
//! pricing validates valuation times, model coverage, and required history.
//! Barrier and touch terms select continuous monitoring with `observation_times: None`
//! or scheduled monitoring with a nonempty `Some(times)`.
//! European vanilla and digital options permit pre-effective valuation; other
//! contracts require valuation within their life.
//!
//! Values own their data; accessors may borrow it. Replacement returns a validated
//! value and preserves the original on success and failure. See each contract's
//! `replace` method for history preservation rules.
//!
//! # History snapshots
//!
//! Supplied history requires `history_time` and covers events strictly before it.
//! Discrete history can be reused between observations. Continuous barrier and Asian
//! history require the same instant unless barrier monitoring is resolved. Refresh
//! payload and timestamp together with `replace_history`; clearing all history
//! permits clearing its timestamp. Numerical time sensitivities hold history fixed.
//! Changing a dated snapshot's timestamp requires explicitly supplying its history payloads.
//! Missing payloads or snapshot timestamps return [`Error::MissingHistory`]. History
//! invalidated by valuation or observation edits returns [`Error::StaleHistory`].
//! Impossible states and future timestamps return [`Error::InvalidInput`].
//!
//! ```rust
//! use kiyosi::*;
//! # fn main() -> Result<()> {
//! let start = Date::new(2025, 1, 2)?.start_of_day();
//! let at = Date::new(2025, 1, 6)?.at_time(TimeOfDay::new(12, 0, 0, 0)?);
//! let end = Date::new(2025, 1, 10)?.start_of_day();
//! let mut terms = RangeAccrualTerms::new(100.0, 0.1, 90.0, 110.0,
//!     start, end, TimeOfDay::new(15, 0, 0, 0)?);
//! terms.realized_hit_count = Some(1);
//! terms.history_time = Some(at);
//! let option = RangeAccrualOption::new(terms)?;
//! let context = PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, 100.0, at)?;
//! let price = AnalyticRangeAccrualEngine.price(&option, &context)?;
//! let later = context.replace(PricingContextChanges {
//!     valuation_time: Some(at.add_days(1)?), ..Default::default()
//! })?;
//! assert!(matches!(AnalyticRangeAccrualEngine.price(&option, &later), Err(Error::StaleHistory(_))));
//! let refreshed = option.replace(RangeAccrualOptionChanges {
//!     realized_hit_count: Some(Some(2)), history_time: Some(Some(later.valuation_time())),
//!     ..Default::default()
//! })?;
//! assert_eq!(AnalyticRangeAccrualEngine.price(&refreshed, &later)?, price);
//! # Ok(()) }
//! ```
//!
//! # Concurrency and errors
//!
//! Built-in contracts, market data, engines, and results are `Send + Sync`.
//! Distinct and shared instances support concurrent reads and pricing. Mutation
//! requires exclusive Rust access. Parallel Monte Carlo calls share a library-owned
//! Rayon pool with per-call buffers and worker caps; serial calls use the calling
//! thread. [`configure_monte_carlo_pool`] caps created workers before initialization.
//! The pool leaves the application's global Rayon pool unchanged.
//! [`PricingEngine::prepare_request`] handles support sequential use on the calling
//! thread and need not be `Send` or `Sync`; concurrent requests prepare their own.
//! Custom calendar predicates must return stable answers throughout the calendar's
//! lifetime, including clones. Custom engines define their own concurrency contract.
//!
//! Domain rejections return [`Result<T>`]. Operational failures retain native I/O
//! details through [`Error::Io`]. Calls are synchronous; [`CancellationToken::run`]
//! enables cooperative cancellation from another thread. Built-in loops check between
//! work units and return [`std::io::ErrorKind::Interrupted`] without a partial result.
//! Custom engines must check their token. Allocation and callbacks may delay checks.
//! Panics, including user callback panics, retain Rust behavior; allocation exhaustion
//! normally aborts. Python signal handling and exception mappings belong to the bindings.
//!
//! # Examples
//!
//! ```rust
//! use kiyosi::{AnalyticEuropeanEngine, BlackScholesMertonParameters, Date,
//!                   EuropeanOption, OptionType, PricingContext};
//! # fn main() -> kiyosi::Result<()> {
//! let start = Date::new(2025, 1, 1)?.start_of_day();
//! let expiry = Date::new(2026, 1, 1)?.start_of_day();
//! let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
//! let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
//! let context = PricingContext::new(model, 100.0, start.into())?;
//! assert!((AnalyticEuropeanEngine.price(&option, &context)? - 9.227005508154036).abs() < 1e-12);
//! let result = AnalyticEuropeanEngine.price_with_greeks(&option, &context,
//!     kiyosi::GreeksRequest::all())?;
//! assert!(result.require(kiyosi::Greek::Delta)? > 0.0);
//! let sigma = kiyosi::implied_volatility(&AnalyticEuropeanEngine, &option,
//!     &context, result.price(), kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0))?;
//! assert!((sigma - 0.2).abs() < 1e-8);
//! # Ok(()) }
//! ```

mod cancellation;
mod engines;
mod error;
mod instruments;
mod market;
mod numerics;
mod risk;
mod time;

pub use cancellation::CancellationToken;
pub use engines::analytic::american::BjerksundStenslandVanillaEngine;
pub use engines::analytic::asian::{
    AnalyticGeometricAveragePriceEngine, TurnbullWakemanArithmeticAveragePriceEngine,
};
pub use engines::analytic::barrier::AnalyticBarrierEngine;
pub use engines::analytic::binary_barrier::AnalyticBinaryBarrierEngine;
pub use engines::analytic::digital::AnalyticDigitalEngine;
pub use engines::analytic::double_barrier::AnalyticDoubleBarrierEngine;
pub use engines::analytic::double_touch::AnalyticDoubleTouchEngine;
pub use engines::analytic::european::AnalyticEuropeanEngine;
pub use engines::analytic::heston::{AnalyticHestonEuropeanEngine, HestonIntegrationSettings};
pub use engines::analytic::range_accrual::AnalyticRangeAccrualEngine;
pub use engines::binomial::{
    BinomialSettings, CoxRossRubinsteinBarrierEngine, CoxRossRubinsteinDigitalEngine,
    CoxRossRubinsteinVanillaEngine,
};
pub use engines::finite_difference::heston::{
    FiniteDifferenceHestonVanillaEngine, HestonFiniteDifferenceSettings,
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
    MonteCarloEstimate, MonteCarloGridSettings, MonteCarloSampling, MonteCarloSettings,
    MonteCarloUncertaintyStatus, MonteCarloVanillaEngine, configure_monte_carlo_pool,
};
pub use engines::quadrature::{
    QuadratureAccumulatorEngine, QuadratureAutocallableEngine, QuadratureBarrierEngine,
    QuadratureDigitalEngine, QuadratureSettings, QuadratureVanillaEngine,
    QuadratureVanillaSettings,
};
pub use error::{Error, Result};
pub use instruments::accumulator::{
    Accumulator, AccumulatorChanges, AccumulatorHistoryChanges, AccumulatorTerms,
};
pub use instruments::asian::{
    ArithmeticAveragePriceOption, AveragePriceOptionChanges, DEFAULT_REALIZED_AVERAGE,
    GeometricAveragePriceOption,
};
pub use instruments::autocallable::{
    AutocallableBarrierState, AutocallableNote, AutocallableObservation,
    AutocallableObservationChanges, AutocallableObservationKind, AutocallableTerms,
    BinarySnowballOption, BinarySnowballOptionChanges, BinarySnowballTerms,
    DEFAULT_PHOENIX_LOCKOUT_PERIODS, DEFAULT_PRINCIPAL_REDEMPTION_RATIO, KnockInObservationMode,
    PhoenixOption, PhoenixOptionChanges, PhoenixPreset, PhoenixPresetTerms, PhoenixTerms,
    SnowballCouponTerms, SnowballOption, SnowballOptionChanges, SnowballPreset,
    SnowballPresetTerms, SnowballTerms, TernarySnowballOption, TernarySnowballOptionChanges,
    TernarySnowballTerms, autocallable_observations,
};
pub use instruments::barrier::{
    BarrierOption, BarrierOptionChanges, BarrierOptionTerms, BarrierTouchState, BarrierType,
    ObservationMode, RebateTiming,
};
pub use instruments::binary_barrier::{
    BinaryBarrierOption, BinaryBarrierOptionChanges, BinaryBarrierTerms, BinaryPayoff,
    SettlementTiming, TouchOption, TouchOptionChanges, TouchOptionTerms,
};
pub use instruments::double_barrier::{
    DoubleBarrierOption, DoubleBarrierOptionChanges, DoubleBarrierOptionTerms, DoubleBarrierType,
};
pub use instruments::double_touch::{
    DoubleTouchOption, DoubleTouchOptionChanges, DoubleTouchOptionTerms,
};
pub use instruments::range_accrual::{
    RangeAccrualOption, RangeAccrualOptionChanges, RangeAccrualTerms,
};
pub use instruments::vanilla::{
    AmericanOption, AmericanOptionChanges, DigitalOption, DigitalOptionChanges, EuropeanOption,
    EuropeanOptionChanges, OptionContract, OptionType,
};
pub use market::{
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, HestonParameterChanges,
    HestonParameterInputs, HestonParameters, LocalVolatilityParameterChanges,
    LocalVolatilityParameters, LocalVolatilitySurface, ModelParameters, PricingContext,
    PricingContextChanges, Rate, RateTermStructure,
};
pub use risk::analytics::{
    Greek, GreekMethod, GreekStatus, GreeksRequest, NumericalShiftSettings, PricingEngine,
    PricingResult,
};
pub use risk::coupon::{
    CouponQuoteConvention, ImpliedCouponSettings, SnowballCouponQuote, implied_phoenix_coupon,
    implied_snowball_coupon,
};
pub use risk::implied::{ImpliedVolatilitySettings, implied_volatility};
pub use risk::numerical::numerical_greeks;
pub use time::{
    BusinessDayConvention, Date, ObservationSchedule, TimeOfDay, Timestamp, TradingCalendar,
};

/// Package version, shared with the Python bindings.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
