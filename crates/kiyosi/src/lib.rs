//! Prices derivatives under Black-Scholes-Merton, local-volatility, and Heston models.
//!
//! Construct a contract and a [`PricingContext`], then choose an engine that
//! supports the contract and model. Use [`PricingEngine::price`] for a price
//! and [`PricingEngine::price_with_greeks`] for selected sensitivities. Import
//! [`PricingEngine`] to call its methods. Each engine documents its supported
//! combinations and numerical controls.
//!
//! Use [`numerical_greeks`] to select scalar BSM differences explicitly.
//! [`implied_volatility`], [`implied_snowball_coupon`] and
//! [`implied_phoenix_coupon`] solve for inputs that reproduce a quoted price.
//! Monte Carlo engines can also report the sampling seed and, for supported
//! contracts, [sampling statistics][`PricingEngine::price_with_statistics`].
//! Find algorithm-specific controls and results in [`engines`] and contract
//! presets and extension types in [`instruments`].
//!
//! For batches, choose the operation by the input that varies. Engines with
//! `price_many` accept existing contracts. Use `price_strikes` to vary the
//! strike, or [`engines::VanillaBatchEngine::price_surface`] to vary strike and expiry.
//! Use [`engines::FiniteDifferenceEngine::solve`] to reuse a solution across spot prices.
//!
//! # Conventions
//!
//! Annual rates and annualized volatility use decimal units. Rates are continuously
//! compounded and accrue on Actual/365 Fixed by default. Use
//! [`RateTermStructure::bounded_flat_with_calendar`] for calendar-based accrual.
//! [`VarianceTimeConvention`] defines volatility units and variance accumulation.
//! BSM and local-volatility PDE models support business time over a calendar's
//! fixed basis, with no variance accumulation on closed dates. Rate curves govern
//! carry and discounting independently of variance and contractual dates. Other engines
//! reject business time with [`Error::UnsupportedOperation`]. Heston variance
//! parameters use Actual/365 Fixed. [`Greek`] defines sensitivity units.
//! BSM volatility may be zero, selecting deterministic pricing. Check numerical
//! convergence separately from Monte Carlo sampling uncertainty and solver tolerances.
//!
//! Contract and market times use [`Timestamp`] in a caller-selected time basis.
//! Contract boundaries are inclusive. Calendars classify [`Date`] values without
//! moving those boundaries. Constructors validate terms and observation calendars;
//! pricing validates valuation times, model coverage, and required history.
//! Barrier and touch terms select continuous monitoring with `observation_schedule: None`
//! or scheduled monitoring with a nonempty schedule. Terms inherit the schedule's
//! calendar unless they specify an override. Borrow the schedule with the
//! contract's `observation_schedule()` method, which returns `None` for continuous
//! monitoring. Clone the schedule to pass it to another constructor or replacement.
//! An [`Accumulator`]'s explicit [knock-out schedule][`Accumulator::knock_out_observation_schedule`]
//! may be empty to disable early knock-out. With `None`, knock-out follows the
//! [daily accumulation schedule][`Accumulator::accumulation_schedule`].
//! European vanilla and digital options permit pre-effective valuation; other
//! contracts require valuation within their life.
//!
//! Values own their data, and accessors may borrow it. Replacement reads an
//! existing value and returns a separate validated value. See each contract's
//! `replace` method for its history rules.
//!
//! # Caller-supplied history
//!
//! Supplied history describes events strictly before [`PricingContext::valuation_time`].
//! Observations exactly at valuation use the current spot and are processed once.
//! Callers maintain lifecycle events, historical market data and settlement status,
//! keeping supplied state consistent with valuation time and contract terms.
//! Kiyosi does not check state age, reconstruct historical paths or advance supplied
//! state. Callers must prevent stale or future state from being supplied.
//!
//! Absent history is unknown; explicit zero, false, untouched, and no-event states
//! are known history where supported. [`AutocallableBarrierState::KnockedIn`]
//! means prior knock-in on an outstanding note without prior knock-out;
//! [`AutocallableBarrierState::NoEvent`] means neither occurred.
//! [`AutocallableBarrierState::KnockedOut`] denotes a settled contract.
//! Missing required history returns [`Error::MissingHistory`]; intrinsically invalid
//! inputs and impossible states return [`Error::InvalidInput`]. Constructors and
//! replacement validate constraints known from their inputs; pricing validates
//! constraints relative to valuation, including whether prior events are possible.
//!
//! `replace` validates terms and history together. Omitted fields, including
//! history, default to their values on the source contract. Numerical time
//! sensitivities hold supplied history fixed. Observation boundaries determine
//! whether a time stencil is available.
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
//! let option = RangeAccrualOption::new(terms)?;
//! let context = PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, 100.0, at)?;
//! let price = AnalyticRangeAccrualEngine.price(&option, &context)?;
//! let later = context.replace(PricingContextChanges {
//!     valuation_time: Some(at.add_days(1)?), ..Default::default()
//! })?;
//! assert!(AnalyticRangeAccrualEngine.price(&option, &later)? < price);
//! let refreshed = option.replace(RangeAccrualOptionChanges {
//!     realized_hit_count: Some(Some(2)),
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
//! thread. [`engines::configure_monte_carlo_pool`] caps created workers before initialization.
//! Kiyosi's Monte Carlo pool is independent of the application's global Rayon pool.
//! [`engines::FiniteDifferenceSolution`] owns immutable query state; clones and shared
//! instances permit concurrent spot queries after the source inputs are dropped.
//! Its queries do not perform rollback or mutate the saved numerical solution.
//! Custom calendar predicates must return stable answers throughout the calendar's
//! lifetime, including clones. See [`PricingEngine`] for custom engine contracts.
//!
//! Domain rejections return [`Result<T>`]. Operational failures retain native I/O
//! details through [`Error::Io`]. Calls are synchronous; [`CancellationToken::run`]
//! enables cooperative cancellation from another thread. Built-in loops check between
//! work units and return [`std::io::ErrorKind::Interrupted`] without a partial result.
//! Custom engines must check their token. Allocation and callbacks may delay checks.
//! Panics, including user callback panics, unwind or abort according to the build's
//! panic strategy. Allocation exhaustion normally aborts. Python signal handling
//! and exception mappings belong to the bindings.
//!
//! # Examples
//!
//! ```rust
//! use kiyosi::{AnalyticEuropeanEngine, BlackScholesMertonParameters, Date,
//!                   EuropeanOption, OptionType, PricingContext, PricingEngine};
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
pub mod engines;
mod error;
pub mod instruments;
mod market;
mod numerics;
mod risk;
mod time;

/// Shares pricing input limits, defaults, and work estimates with language bindings.
#[doc(hidden)]
pub mod binding_support {
    pub use crate::engines::analytic::heston::validate_heston_strike_batch_size;
    pub use crate::engines::finite_difference::{
        validate_finite_difference_solution_batch_size,
        validate_finite_difference_strike_batch_size, validate_finite_difference_surface_size,
    };
    pub use crate::engines::monte_carlo::validate_monte_carlo_strike_batch_size;
    pub use crate::engines::quadrature::validate_quadrature_strike_batch_size;
    pub use crate::instruments::asian::DEFAULT_REALIZED_AVERAGE;
    pub use crate::instruments::autocallable::{
        DEFAULT_AUTOCALLABLE_REBATE_RATE, DEFAULT_PHOENIX_LOCKOUT_PERIODS,
        DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
    };
    pub use crate::instruments::monitoring::binding as barrier_monitoring;
    pub use crate::instruments::monitoring::binding::{new_accumulator, replace_accumulator};
    pub use crate::market::{
        validate_local_volatility_surface_dimension, validate_local_volatility_time_node,
        validate_rate_curve_node, validate_rate_curve_shape,
    };
    pub use crate::time::{
        validate_schedule_bounds, validate_schedule_time, variance_time_convention_from_inputs,
    };

    /// Checks the input count for CRR and autocallable Monte Carlo contract batches.
    ///
    /// See [`crate::CoxRossRubinsteinVanillaEngine::price_many`] for a batch example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] above 1,048,576 contracts.
    pub fn validate_contract_batch_size(count: usize) -> crate::Result<()> {
        if count > 1 << 20 {
            return Err(crate::Error::InvalidInput(
                "contract batch exceeds 1,048,576 instruments".into(),
            ));
        }
        Ok(())
    }

    /// Estimates per-spot work for dispatching cancellable solution queries.
    pub fn finite_difference_solution_query_work(
        solution: &crate::engines::FiniteDifferenceSolution,
    ) -> usize {
        solution.query_work()
    }
}

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
pub use engines::analytic::heston::AnalyticHestonEuropeanEngine;
pub use engines::analytic::range_accrual::AnalyticRangeAccrualEngine;
pub use engines::binomial::{
    CoxRossRubinsteinBarrierEngine, CoxRossRubinsteinDigitalEngine, CoxRossRubinsteinVanillaEngine,
};
pub use engines::finite_difference::heston::FiniteDifferenceHestonVanillaEngine;
pub use engines::finite_difference::{
    FiniteDifferenceAccumulatorEngine, FiniteDifferenceAutocallableEngine,
    FiniteDifferenceBarrierEngine, FiniteDifferenceBinaryBarrierEngine,
    FiniteDifferenceDigitalEngine, FiniteDifferenceDoubleBarrierEngine,
    FiniteDifferenceDoubleTouchEngine, FiniteDifferenceVanillaEngine,
};
pub use engines::monte_carlo::{
    MonteCarloAccumulatorEngine, MonteCarloAsianEngine, MonteCarloAutocallableEngine,
    MonteCarloBarrierEngine, MonteCarloDigitalEngine, MonteCarloVanillaEngine,
};
pub use engines::quadrature::{
    QuadratureAccumulatorEngine, QuadratureAutocallableEngine, QuadratureBarrierEngine,
    QuadratureDigitalEngine, QuadratureVanillaEngine,
};
pub use error::{Error, Result};
pub use instruments::accumulator::{Accumulator, AccumulatorChanges, AccumulatorTerms};
pub use instruments::asian::{
    ArithmeticAveragePriceOption, AveragePriceOptionChanges, GeometricAveragePriceOption,
};
pub use instruments::autocallable::{
    AutocallableBarrierState, BinarySnowballOption, BinarySnowballOptionChanges,
    BinarySnowballTerms, CouponObservation, KnockInObservation, KnockOutObservation, PhoenixOption,
    PhoenixOptionChanges, PhoenixTerms, SnowballObservation, SnowballObservationChanges,
    SnowballOption, SnowballOptionChanges, SnowballTerms, TernarySnowballOption,
    TernarySnowballOptionChanges, TernarySnowballTerms,
};
pub(crate) use instruments::barrier::ObservationMode;
pub use instruments::barrier::{
    BarrierOption, BarrierOptionChanges, BarrierOptionTerms, BarrierTouchState, BarrierType,
    RebateTiming,
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
    AmericanOption, DigitalOption, DigitalOptionChanges, EuropeanOption, OptionType,
    VanillaOptionChanges,
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
    VarianceTimeConvention,
};

/// Reports the package version shared with the Python bindings.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
