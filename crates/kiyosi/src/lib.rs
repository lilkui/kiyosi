//! Prices derivatives under Black-Scholes-Merton (BSM), local-volatility and Heston models.
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
//! Contract dates denote midnight UTC boundaries, not whole trading days. The
//! effective-date and expiry-date midnights are included; later instants on the
//! expiry date are outside contract life. Calendars do not move these boundaries.
//! Contract dates cannot specify an intraday expiry. Python converts aware
//! valuation datetimes to UTC before applying the same boundaries.
//!
//! Annual rates and annualized volatility use decimal units. Rates and dividend
//! yields are continuously compounded. Rate curves use the Actual/365 Fixed
//! day-count convention; [`TradingCalendar::trading_year_fraction`] uses its own
//! basis. [`Greek`] documents sensitivity reporting units.
//!
//! Local volatility uses [`LocalVolatilitySurface`] with fixed absolute time/spot
//! nodes, bilinear interpolation of local volatility and constant extrapolation
//! in the spot dimension. Time extrapolation is rejected. Finite-difference
//! engines support this model for European and American vanilla options, digitals,
//! single and double barriers, binary barriers, touch and
//! no-touch options, autocallable notes and accumulators. They provide price,
//! delta and gamma with the surface held fixed. Other Greeks remain absent. Other engine families
//! and implied-parameter solvers reject this model. Digital pricing requires
//! explicitly flat risk-free rates and dividend yields.
//! Future coverage follows remaining stochastic evolution and required discounts;
//! settled cash flows do not require volatility coverage through contractual expiry.
//! [`ModelParameters`] selects the concrete model at the pricing entry point.
//! [`HestonParameters`] supplies constant risk-neutral variance parameters and
//! deterministic rate curves. [`AnalyticHestonEuropeanEngine`] supports European call/put
//! prices by Fourier integration; [`FiniteDifferenceHestonVanillaEngine`] supports
//! European and American call and put prices using the modified Craig-Sneyd scheme
//! with Ikonen-Toivanen splitting for American exercise. Both accept zero variance
//! parameters, correlation endpoints and violations of the Feller condition.
//! All Heston Greeks remain absent. Other engines and implied-parameter solvers reject Heston.
//! See each engine's constructor for examples, numerical controls and convergence checks.
//!
//! # Concurrency and errors
//!
//! Built-in contracts, market data, engines and results own their data and are
//! `Send + Sync`. Distinct and shared instances support concurrent reads and
//! pricing without synchronization.
//! Mutation requires exclusive Rust access; Python exposes immutable values.
//! Parallel Monte Carlo calls reuse one library-owned Rayon pool, with per-call
//! worker caps and request-owned buffers; [`MonteCarloSettings::thread_count`]
//! controls participation. Serial calls stay on the calling thread. The pool is
//! initialized lazily and leaves the application's global Rayon pool unchanged.
//! Handles returned by [`PricingEngine::prepare_request`] support sequential use
//! within the calling thread and need not be `Send` or `Sync`. Concurrent requests
//! prepare independent handles from the same engine.
//! Custom calendar predicates must also uphold [`TradingCalendar`]'s determinism
//! contract; panics in user predicates retain native Rust behavior.
//! User-supplied [`PricingEngine`] implementations retain their own
//! thread-safety and operational-failure contracts.
//! Calls are synchronous, with no cancellation or signal polling. Domain
//! rejections use [`Result`], never panics. Allocation exhaustion follows Rust's
//! allocator behavior (normally process abort); internal panics are not domain errors.
//! Monte Carlo entropy-source and pool thread-creation failures retain native I/O
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
//! assert!((result.require(kiyosi::Greek::Volga)? - 0.000236882234437609).abs() < 1e-12);
//! assert!(result.require(kiyosi::Greek::DividendRho)? < 0.0);
//! let sigma = kiyosi::implied_volatility(&AnalyticEuropeanEngine, &option,
//!     &context, result.price(), Default::default())?;
//! assert!((sigma - 0.2).abs() < 1e-8);
//! # Ok(()) }
//! ```

mod engines;
mod error;
mod instruments;
mod market;
mod numerics;
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
    MonteCarloEstimate, MonteCarloSampling, MonteCarloSettings, MonteCarloVanillaEngine,
};
pub use engines::quadrature::{
    QuadratureAccumulatorEngine, QuadratureAutocallableEngine, QuadratureBarrierEngine,
    QuadratureDigitalEngine, QuadratureSettings, QuadratureVanillaEngine,
    QuadratureVanillaSettings,
};
pub use error::{Error, Result};
pub use instruments::accumulator::{Accumulator, AccumulatorTerms};
pub use instruments::asian::{
    ArithmeticAveragePriceOption, DEFAULT_REALIZED_AVERAGE, GeometricAveragePriceOption,
};
pub use instruments::autocallable::{
    AutocallableBarrierState, AutocallableNote, AutocallableObservation, AutocallableTerms,
    BinarySnowballOption, BinarySnowballTerms, DEFAULT_PRINCIPAL_RATIO, KnockInObservationMode,
    PhoenixCoupon, PhoenixOption, PhoenixPreset, PhoenixPresetTerms, PhoenixTerms,
    SnowballCouponTerms, SnowballOption, SnowballPreset, SnowballPresetTerms, SnowballTerms,
    TernarySnowballOption, TernarySnowballTerms, autocallable_observations,
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
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, HestonParameterChanges,
    HestonParameters, LocalVolatilityParameterChanges, LocalVolatilityParameters,
    LocalVolatilitySurface, ModelParameters, PricingContext, PricingContextChanges, Rate,
    RateTermStructure,
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
