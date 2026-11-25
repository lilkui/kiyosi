use std::ops::RangeInclusive;

use super::{LazyPath, Moments, MonteCarloEstimate, MonteCarloSettings};
use super::{check_path_step, estimate, path_samples_lazy, scaled_value, transition, unstable};
use crate::instruments::autocallable::pricing::{
    Initial, fixed_value, future_event_times, initial, validate,
};
use crate::numerics::log_ratio;
use crate::{AutocallableNote, AutocallableTerms, PricingContext, PricingEngine, Result};

/// Prices snowball variants and Phoenix notes with trading-day CPU Monte Carlo.
///
/// Uses exact lognormal transitions on trading days when knock-in matters, or
/// remaining observation dates and expiry otherwise. Time is Actual/365 Fixed.
/// Current events precede simulation. Future knock-in, knock-out and coupon tests
/// preserve log-price movements even when the simulated spot rounds to a threshold.
/// Early knock-out preserves the normal draws assigned to later paths.
/// Sampling, antithetic counts and thread
/// reproducibility follow [`crate::MonteCarloVanillaEngine`]. Shared and distinct
/// instances permit concurrent calls with request-owned buffers and shared workers.
/// Greeks pin one request-local seed. Implied-parameter solvers require an explicit seed and
/// constrain the sampled price curve, not its statistical error. Coupon trials
/// retain their own event-grid shortcuts when knock-in exposure changes.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloAutocallableEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloAutocallableEngine {
    /// Returns a replayable engine, drawing a seed only when none is configured.
    ///
    /// All other controls, including any integration or exercise grid, are preserved.
    /// The original engine remains unchanged. Save this engine before pricing to
    /// replay the calculation within the same build; see
    /// [`crate::MonteCarloVanillaEngine::with_resolved_seed`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Io`] if system entropy is unavailable.
    pub fn with_resolved_seed(&self) -> crate::Result<Self> {
        Ok(Self {
            settings: self.settings.with_resolved_seed()?,
        })
    }

    /// Creates an engine with validated CPU sampling settings.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for invalid paths, threads or sampling.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2025, 4, 1)?.start_of_day();
    /// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.02, start, end,
    ///     autocallable_observations(&[end], &[110.0], &[0.08])?, kiyosi::TradingCalendar::weekdays()))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
    /// let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
    ///     seed: Some(42), path_count: 4096, ..Default::default()
    /// })?;
    /// let estimate = engine.price_with_statistics(&note, &context)?;
    /// assert!((estimate.price() - 1.0).abs() < 0.02);
    /// assert!(estimate.standard_error().is_some());
    /// assert_eq!(engine.sampling_settings().path_count, 4096);
    /// let result = engine.price_with_greeks(&note, &context,
    ///     GreeksRequest::new([Greek::Rho]))?;
    /// assert!(result.has(Greek::Rho));
    /// # Ok(()) }
    /// ```
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned sampling settings. See [`Self::new`].
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns the mean discounted remaining cash flows. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price_with_statistics`] sampling and pricing errors,
    /// without computing or validating uncertainty.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    ///
    /// # Greeks
    ///
    /// Computes requested finite-difference Greeks with shared normal draws.
    ///
    /// Pins a request-local seed when none is configured. At expiry Greeks follow
    /// terminal payoffs, with time Greeks absent. Stencils crossing active
    /// spot/time events are unavailable, following
    /// [`crate::numerical_greeks`]. Discontinuous payoffs can make small bumps
    /// noisy; shift sizes control the sampled curve, not statistical accuracy.
    /// See [`Self::new`] for an example.
    ///
    /// ## Errors
    ///
    /// Checks flat rates before shift validation, then propagates
    /// [`crate::numerical_greeks`] and [`Self::price`] errors.
    ///
    /// ## Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// ## Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        Ok(self.price_with_seed(option, context)?.0)
    }

    /// Returns a price with optional standard error and asymptotic 95% confidence interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; known pseudorandom cash flows
    /// have zero uncertainty even before expiry. Coupons are discounted at their
    /// contractual dates, including a current Phoenix coupon exactly once.
    /// Cancellation follows [`crate::CancellationToken`]. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Checks flat rates ([`crate::Error::UnsupportedOperation`]), then inclusive
    /// life and prior history. Invalid life or impossible history returns
    /// [`crate::Error::InvalidInput`]. Calendar compatibility follows
    /// [`AutocallableNote::new`]. Sobol dimensions above 21,201 or transition and
    /// sampling workspace above 1 GiB per request
    /// return [`crate::Error::InvalidInput`]. Non-finite paths,
    /// payments or uncertainty return [`crate::Error::ComputationFailure`].
    /// Entropy and thread failures preserve native I/O details through
    /// [`crate::Error::Io`].
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_statistics<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (sample, pairs, known, seed) = self.sample(note, context, true)?;
        estimate(self.settings, sample, pairs, 0.0, known, seed)
    }

    fn sample<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        statistics: bool,
    ) -> Result<(Moments, Moments, bool, Option<u64>)> {
        match self.prepare_sample(note, context, false)? {
            Sample::Fixed(value) => {
                let mut sample = Moments::default();
                sample.add(value, false);
                Ok((sample, sample, true, None))
            }
            Sample::Path(plan) => {
                let ([moments], [pairs], seed) =
                    path_samples_lazy(self.settings, plan.steps.len(), statistics, |normals| {
                        plan.payoff([&plan], normals)
                    })?;
                Ok((moments, pairs, false, Some(seed)))
            }
        }
    }

    fn prepare_sample<'a, T: AutocallableTerms>(
        &self,
        note: &'a AutocallableNote<T>,
        context: &PricingContext,
        shared_bounds: bool,
    ) -> Result<Sample<'a, T>> {
        let model = context.natural_bsm_parameters()?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::autocallable(note, context).map(Sample::Fixed);
        }
        validate(note, context)?;
        let state = initial(note, context);
        if let Some(value) = state.settlement.or_else(|| {
            fixed_value(note, context, state.knocked_in).map(|value| value + state.coupon)
        }) {
            return if value.is_finite() {
                Ok(Sample::Fixed(value))
            } else {
                Err(unstable())
            };
        }
        context.validate_curve_horizon(note.expiry_time())?;
        let terms = note.view();
        let daily = terms.daily_knock_in() && !state.knocked_in && note.knock_in_affects_payoff();
        let valuation = context.valuation_time();
        let dates = future_event_times(note, context, daily)?;
        let mut previous = valuation;
        let sigma = model.volatility();
        let initial_spot = context.spot_price();
        let log_spot = initial_spot.ln();
        // Leave room for rounding in both the exponential and the scaled spot.
        let low = f64::MIN_POSITIVE.ln() + 1.0;
        let high = f64::MAX.ln() - 1.0;
        let safe_returns = low.max(low - log_spot)..=high.min(high - log_spot);
        let knock_in = terms.knock_in.map(|ki| log_ratio(ki.level, initial_spot));
        if shared_bounds {
            self.settings
                .validate_scheduled_workspace::<[PathStep; 2]>(dates.len())?;
        }
        let mut steps = self.settings.scheduled_steps(dates.len())?;
        let mut next = note.events().partition_point(|r| r.time() <= valuation);
        for date in dates {
            crate::cancellation::check()?;
            let (drift, diffusion, discount) = transition(
                rate,
                dividend,
                sigma,
                previous.years_until(date),
                valuation.years_until(date),
            )?;
            let record = note.events().get(next).filter(|r| r.time() == date);
            if record.is_some() {
                next += 1;
            }
            let event = record.map(|record| {
                let knock_out = record
                    .has_knock_out()
                    .then(|| log_ratio(record.knock_out_level(), initial_spot));
                let coupon_barrier = record.coupon_barrier_level();
                CashEvent {
                    knock_out,
                    coupon_barrier: log_ratio(coupon_barrier.unwrap_or(0.0), initial_spot),
                    period_coupon: if coupon_barrier.is_some() {
                        note.period_coupon()
                    } else {
                        0.0
                    },
                    redemption: knock_out.map_or(0.0, |_| note.knock_out_payment(record)),
                }
            });
            steps.push(PathStep {
                transition: [drift, diffusion, discount],
                event,
                observe_knock_in: daily && note.daily_time(date, note.calendar()),
            });
            previous = date;
        }
        Ok(Sample::Path(PathPlan {
            note,
            state,
            steps,
            initial_spot,
            safe_returns,
            knock_in,
            expiry_knock_in: note.observes_knock_in(note.expiry_time()),
        }))
    }
}

enum Sample<'a, T: AutocallableTerms> {
    Fixed(f64),
    Path(PathPlan<'a, T>),
}

struct PathPlan<'a, T: AutocallableTerms> {
    note: &'a AutocallableNote<T>,
    state: Initial,
    steps: Vec<PathStep>,
    initial_spot: f64,
    safe_returns: RangeInclusive<f64>,
    knock_in: Option<f64>,
    expiry_knock_in: bool,
}

struct PathStep {
    transition: [f64; 3],
    event: Option<CashEvent>,
    observe_knock_in: bool,
}

#[derive(Clone, Copy)]
struct CashEvent {
    knock_out: Option<f64>,
    coupon_barrier: f64,
    period_coupon: f64,
    redemption: f64,
}

impl<T: AutocallableTerms> PathPlan<'_, T> {
    fn shares_thresholds(&self, other: &Self) -> Result<bool> {
        if self.state.knocked_in != other.state.knocked_in
            || self.knock_in != other.knock_in
            || self.expiry_knock_in != other.expiry_knock_in
            || self.steps.len() != other.steps.len()
        {
            return Ok(false);
        }
        for (left, right) in self.steps.chunks(1024).zip(other.steps.chunks(1024)) {
            crate::cancellation::check()?;
            let event_shape = |event: CashEvent| (event.knock_out, event.coupon_barrier);
            if left.iter().zip(right).any(|(a, b)| {
                a.transition.map(f64::to_bits) != b.transition.map(f64::to_bits)
                    || a.observe_knock_in != b.observe_knock_in
                    || a.event.map(event_shape) != b.event.map(event_shape)
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn payoff<const N: usize>(
        &self,
        plans: [&Self; N],
        normals: &mut LazyPath<'_, '_>,
    ) -> Result<[f64; N]> {
        let below_knock_in = |log_return| self.knock_in.is_some_and(|level| log_return < level);
        let mut log_return = 0.0;
        let mut knocked_in = self.state.knocked_in;
        let mut coupons = plans.map(|plan| plan.state.coupon);
        let mut discount = 0.0;
        for (index, step) in self.steps.iter().enumerate() {
            check_path_step(index)?;
            let [drift, diffusion, log_discount] = step.transition;
            discount = log_discount;
            // Keep threshold decisions before rounding the simulated spot.
            log_return += diffusion.mul_add(normals.normal(), drift);
            if !self.safe_returns.contains(&log_return)
                && scaled_value(self.initial_spot, 1.0, log_return)? <= 0.0
            {
                return Err(unstable());
            }
            if step.observe_knock_in {
                knocked_in |= below_knock_in(log_return);
            }
            if let Some(event) = step.event {
                let coupon_due = log_return >= event.coupon_barrier;
                let knocked_out = event.knock_out.is_some_and(|level| log_return >= level);
                for (cash, plan) in coupons.iter_mut().zip(plans) {
                    let payment = plan.steps[index]
                        .event
                        .expect("shared paths identify the same cash events");
                    let coupon = if coupon_due {
                        payment.period_coupon
                    } else {
                        0.0
                    };
                    let amount = if knocked_out {
                        payment.redemption + coupon
                    } else {
                        coupon
                    };
                    *cash += scaled_value(1.0, amount, discount)?;
                }
                if knocked_out {
                    return finite_payoffs(coupons);
                }
            }
        }
        knocked_in |= self.expiry_knock_in && below_knock_in(log_return);
        let spot = scaled_value(self.initial_spot, 1.0, log_return)?;
        for (cash, plan) in coupons.iter_mut().zip(plans) {
            *cash += scaled_value(1.0, plan.note.terminal(spot, knocked_in), discount)?;
        }
        finite_payoffs(coupons)
    }
}

fn finite_payoffs<const N: usize>(values: [f64; N]) -> Result<[f64; N]> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(values)
    } else {
        Err(unstable())
    }
}

impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>> for MonteCarloAutocallableEngine {
    fn price_with_seed(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        let (sample, _, _, seed) = self.sample(option, context, false)?;
        Ok((scaled_value(sample.scale, sample.mean, 0.0)?, seed))
    }
    fn price_with_statistics(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        self.price_with_statistics(option, context)
    }
    fn price_coupon_bounds(
        &self,
        lower: &AutocallableNote<T>,
        upper: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<Option<[f64; 2]>> {
        self.settings.validate_solver_seed()?;
        let Sample::Path(lower) = self.prepare_sample(lower, context, true)? else {
            return Ok(None);
        };
        let Sample::Path(upper) = self.prepare_sample(upper, context, true)? else {
            return Ok(None);
        };
        if !lower.shares_thresholds(&upper)? {
            return Ok(None);
        }
        let ([low, high], _, _) =
            path_samples_lazy(self.settings, lower.steps.len(), false, |normals| {
                lower.payoff([&lower, &upper], normals)
            })?;
        Ok(Some([
            scaled_value(low.scale, low.mean, 0.0)?,
            scaled_value(high.scale, high.mean, 0.0)?,
        ]))
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        crate::engines::flat_rates(context).map(|_| ())
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.sampling_settings().validate_solver_seed()
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AutocallableNote<T>> + '_>>> {
        prepare_seeded_request!(self, self.settings)
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
}
