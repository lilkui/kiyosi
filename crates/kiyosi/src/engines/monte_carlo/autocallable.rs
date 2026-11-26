use std::ops::RangeInclusive;

use super::{LazyPath, Moments, MonteCarloEstimate, MonteCarloSettings, SkippingPath};
use super::{
    check_path_step, estimate, path_samples_lazy, path_samples_skipping, scaled_value, transition,
    unstable,
};
use crate::instruments::autocallable::pricing::{
    Initial, fixed_value, future_event_times, initial, validate,
};
use crate::numerics::log_ratio;
use crate::{
    AutocallableNote, AutocallableTerms, Error, Greek, GreeksRequest, NumericalShiftSettings,
    OptionContract, PricingContext, PricingContextChanges, PricingEngine, PricingResult, Result,
    Timestamp,
};

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
        match self.prepare_sample::<_, 1>(note, context)? {
            Sample::Fixed(value) => {
                let mut sample = Moments::default();
                sample.add(value, false);
                Ok((sample, sample, true, None))
            }
            Sample::Path(plan) => {
                let ([moments], [pairs], seed) =
                    plan.sample(self.settings, [&plan], statistics, false)?;
                Ok((moments, pairs, false, Some(seed)))
            }
        }
    }

    fn spot_prices<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        [base, high, low]: [&PricingContext; 3],
    ) -> Result<Option<([f64; 3], u64)>> {
        let Sample::Path(base) = self.prepare_sample::<_, 3>(note, base)? else {
            return Ok(None);
        };
        let Sample::Path(high) = self.prepare_sample::<_, 3>(note, high)? else {
            return Ok(None);
        };
        let Sample::Path(low) = self.prepare_sample::<_, 3>(note, low)? else {
            return Ok(None);
        };
        if !base.shares_transitions(&high)? || !base.shares_transitions(&low)? {
            return Ok(None);
        }
        let ([base_price, high_price, low_price], _, seed) =
            base.sample(self.settings, [&base, &high, &low], false, true)?;
        let price = |moment: Moments| scaled_value(moment.scale, moment.mean, 0.0);
        Ok(Some((
            [price(base_price)?, price(high_price)?, price(low_price)?],
            seed,
        )))
    }

    fn prepare_sample<'a, T: AutocallableTerms, const N: usize>(
        &self,
        note: &'a AutocallableNote<T>,
        context: &PricingContext,
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
        if N > 1 {
            self.settings
                .validate_scheduled_workspace::<[PathStep; N]>(dates.len())?;
        }
        let mut steps = self.settings.scheduled_steps(dates.len())?;
        let mut next = note.events().partition_point(|r| r.time() <= valuation);
        let mut excursion = 0.0;
        for &date in &dates {
            crate::cancellation::check()?;
            let (drift, diffusion, discount) = transition(
                rate,
                dividend,
                sigma,
                previous.years_until(date),
                valuation.years_until(date),
            )?;
            excursion += drift.abs() + 20.0 * diffusion;
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
        let bridge =
            if daily && safe_returns.contains(&excursion) && safe_returns.contains(&-excursion) {
                self.settings
                    .validate_scheduled_workspace::<[(PathStep, BridgeInterval, [f64; 2]); N]>(
                        steps.len(),
                    )?;
                BridgePlan::new(valuation, &dates, &steps, [rate, dividend, sigma])?
            } else {
                None
            };
        Ok(Sample::Path(PathPlan {
            note,
            state,
            steps,
            initial_spot,
            safe_returns,
            knock_in,
            expiry_knock_in: note.observes_knock_in(note.expiry_time()),
            bridge,
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
    bridge: Option<BridgePlan>,
}

struct BridgePlan {
    intervals: Vec<BridgeInterval>,
    daily: Vec<[f64; 2]>,
}

struct BridgeInterval {
    end: usize,
    drift: f64,
    diffusion: f64,
}

impl BridgePlan {
    fn new(
        valuation: Timestamp,
        dates: &[Timestamp],
        steps: &[PathStep],
        [rate, dividend, sigma]: [f64; 3],
    ) -> Result<Option<Self>> {
        let mut intervals = Vec::new();
        let mut first = 0;
        let mut has_gap = false;
        for (end, step) in steps.iter().enumerate() {
            check_path_step(end)?;
            if step.event.is_some() || end + 1 == steps.len() {
                has_gap |= end - first >= 8;
                let start = if first == 0 {
                    valuation
                } else {
                    dates[first - 1]
                };
                let (drift, diffusion, _) =
                    transition(rate, dividend, sigma, start.years_until(dates[end]), 0.0)?;
                intervals.push(BridgeInterval {
                    end,
                    drift,
                    diffusion,
                });
                first = end + 1;
            }
        }
        if !has_gap {
            return Ok(None);
        }
        let mut daily = vec![[0.0; 2]; steps.len()];
        let mut first = 0;
        let mut previous = valuation;
        for interval in &intervals {
            let end = dates[interval.end];
            for i in first..interval.end {
                check_path_step(i)?;
                let dt = previous.years_until(dates[i]);
                let fraction = dt / previous.years_until(end);
                if !(0.0 < fraction && fraction < 1.0) {
                    return Ok(None);
                }
                daily[i] = [fraction, sigma * (dt * (1.0 - fraction)).sqrt()];
                previous = dates[i];
            }
            first = interval.end + 1;
            previous = end;
        }
        Ok(Some(Self { intervals, daily }))
    }
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

impl CashEvent {
    fn discounted_payment(self, coupon_due: bool, knocked_out: bool, discount: f64) -> Result<f64> {
        let coupon = if coupon_due { self.period_coupon } else { 0.0 };
        let amount = if knocked_out {
            self.redemption + coupon
        } else {
            coupon
        };
        scaled_value(1.0, amount, discount)
    }
}

impl<T: AutocallableTerms> PathPlan<'_, T> {
    fn sample<const N: usize>(
        &self,
        settings: MonteCarloSettings,
        plans: [&Self; N],
        statistics: bool,
        spot_stencil: bool,
    ) -> Result<([Moments; N], [Moments; N], u64)> {
        if let Some(bridge) = &self.bridge {
            return path_samples_skipping(settings, self.steps.len(), statistics, |normals| {
                self.bridge_payoffs(bridge, plans, normals)
            });
        }
        path_samples_lazy(settings, self.steps.len(), statistics, |normals| {
            if spot_stencil {
                self.spot_payoffs(plans, normals)
            } else {
                self.payoff(plans, normals)
            }
        })
    }

    fn shares_transitions(&self, other: &Self) -> Result<bool> {
        if self.steps.len() != other.steps.len() || self.bridge.is_some() != other.bridge.is_some()
        {
            return Ok(false);
        }
        for (left, right) in self.steps.chunks(1024).zip(other.steps.chunks(1024)) {
            crate::cancellation::check()?;
            if left
                .iter()
                .zip(right)
                .any(|(a, b)| a.transition.map(f64::to_bits) != b.transition.map(f64::to_bits))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn validate_return(&self, log_return: f64) -> Result<()> {
        if !self.safe_returns.contains(&log_return)
            && scaled_value(self.initial_spot, 1.0, log_return)? <= 0.0
        {
            return Err(unstable());
        }
        Ok(())
    }

    fn bridge_payoffs<const N: usize>(
        &self,
        bridge: &BridgePlan,
        plans: [&Self; N],
        normals: &mut SkippingPath<'_, '_>,
    ) -> Result<[f64; N]> {
        let mut cash = plans.map(|plan| plan.state.coupon);
        let mut monitoring =
            plans.map(|plan| (!plan.state.knocked_in).then_some(plan.knock_in).flatten());
        let mut knocked_in = plans.map(|plan| plan.state.knocked_in);
        let mut active = [true; N];
        let mut start = 0;
        let mut log_return = 0.0;
        let mut discount = 0.0;
        for (index, interval) in bridge.intervals.iter().enumerate() {
            check_path_step(index)?;
            let target = log_return + interval.diffusion.mul_add(normals.normal(), interval.drift);
            for i in start..interval.end {
                if monitoring.iter().all(Option::is_none) {
                    normals.skip(interval.end - i);
                    break;
                }
                check_path_step(i)?;
                let [weight, diffusion] = bridge.daily[i];
                log_return += weight.mul_add(target - log_return, diffusion * normals.normal());
                for (level, hit) in monitoring.iter_mut().zip(&mut knocked_in) {
                    if level.is_some_and(|level| log_return < level) {
                        *level = None;
                        *hit = true;
                    }
                }
            }
            log_return = target;
            start = interval.end + 1;
            discount = self.steps[interval.end].transition[2];
            for lane in 0..N {
                if !active[lane] {
                    continue;
                }
                let step = &plans[lane].steps[interval.end];
                if step.observe_knock_in && monitoring[lane].is_some_and(|level| log_return < level)
                {
                    monitoring[lane] = None;
                    knocked_in[lane] = true;
                }
                if let Some(event) = step.event {
                    let hit = event.knock_out.is_some_and(|level| log_return >= level);
                    cash[lane] += event.discounted_payment(
                        log_return >= event.coupon_barrier,
                        hit,
                        discount,
                    )?;
                    if hit {
                        active[lane] = false;
                        monitoring[lane] = None;
                    }
                }
            }
            if active.iter().all(|active| !active) {
                return finite_payoffs(cash);
            }
        }
        for (lane, plan) in plans.into_iter().enumerate() {
            if active[lane] {
                let hit = knocked_in[lane]
                    || (plan.expiry_knock_in
                        && plan.knock_in.is_some_and(|level| log_return < level));
                cash[lane] += plan.discounted_terminal(log_return, hit, discount)?;
            }
        }
        finite_payoffs(cash)
    }

    fn spot_payoffs<const N: usize>(
        &self,
        plans: [&Self; N],
        normals: &mut LazyPath<'_, '_>,
    ) -> Result<[f64; N]> {
        let mut log_return = 0.0;
        let mut discount = 0.0;
        let mut cash = plans.map(|plan| plan.state.coupon);
        let mut knocked_in = plans.map(|plan| plan.state.knocked_in);
        let mut active = [true; N];
        for (index, step) in self.steps.iter().enumerate() {
            check_path_step(index)?;
            let [drift, diffusion, log_discount] = step.transition;
            discount = log_discount;
            log_return += diffusion.mul_add(normals.normal(), drift);
            for lane in 0..N {
                if !active[lane] {
                    continue;
                }
                let plan = plans[lane];
                plan.validate_return(log_return)?;
                let step = &plan.steps[index];
                if step.observe_knock_in {
                    knocked_in[lane] |= plan.knock_in.is_some_and(|level| log_return < level);
                }
                if let Some(event) = step.event {
                    let hit = event.knock_out.is_some_and(|level| log_return >= level);
                    cash[lane] += event.discounted_payment(
                        log_return >= event.coupon_barrier,
                        hit,
                        discount,
                    )?;
                    active[lane] = !hit;
                }
            }
            if active.iter().all(|active| !active) {
                return finite_payoffs(cash);
            }
        }
        for lane in 0..N {
            if active[lane] {
                let plan = plans[lane];
                knocked_in[lane] |=
                    plan.expiry_knock_in && plan.knock_in.is_some_and(|level| log_return < level);
                cash[lane] += plan.discounted_terminal(log_return, knocked_in[lane], discount)?;
            }
        }
        finite_payoffs(cash)
    }

    fn discounted_terminal(&self, log_return: f64, knocked_in: bool, discount: f64) -> Result<f64> {
        let spot = scaled_value(self.initial_spot, 1.0, log_return)?;
        scaled_value(1.0, self.note.terminal(spot, knocked_in), discount)
    }

    fn shares_thresholds(&self, other: &Self) -> Result<bool> {
        if self.state.knocked_in != other.state.knocked_in
            || self.knock_in != other.knock_in
            || self.expiry_knock_in != other.expiry_knock_in
            || !self.shares_transitions(other)?
        {
            return Ok(false);
        }
        for (left, right) in self.steps.chunks(1024).zip(other.steps.chunks(1024)) {
            crate::cancellation::check()?;
            let event_shape = |event: CashEvent| (event.knock_out, event.coupon_barrier);
            if left.iter().zip(right).any(|(a, b)| {
                a.observe_knock_in != b.observe_knock_in
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
            self.validate_return(log_return)?;
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
                    *cash += payment.discounted_payment(coupon_due, knocked_out, discount)?;
                }
                if knocked_out {
                    return finite_payoffs(coupons);
                }
            }
        }
        knocked_in |= self.expiry_knock_in && below_knock_in(log_return);
        for (cash, plan) in coupons.iter_mut().zip(plans) {
            *cash += plan.discounted_terminal(log_return, knocked_in, discount)?;
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
    fn price_with_greeks_with_settings(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        context.bsm_parameters()?;
        <Self as PricingEngine<AutocallableNote<T>>>::validate_applicability(self, context)?;
        settings.validate()?;
        let spot = context.spot_price();
        let shift = crate::risk::numerical::symmetric_shift(spot, settings.spot_shift_at(spot));
        if !(request.has(Greek::Delta) || request.has(Greek::Gamma))
            || shift <= 0.0
            || spot <= shift
            || context.valuation_time() == option.expiry_time()
            || option.all_greeks_unavailable(context)
            || option.at_spot_discontinuity(context, shift)
        {
            return crate::numerical_greeks(self, option, context, request, settings);
        }
        let _sampling = super::RequestCache::new();
        let engine = self.with_resolved_seed()?;
        let shifted = |spot| {
            context.replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
        };
        let high = shifted(spot + shift)?;
        let low = shifted(spot - shift)?;
        let (base, up, down, seed) = match engine.spot_prices(option, [context, &high, &low]) {
            Ok(Some(([base, up, down], seed))) => (base, up, down, seed),
            Ok(None) | Err(Error::InvalidInput(_)) => {
                return crate::numerical_greeks(&engine, option, context, request, settings);
            }
            Err(error) => return Err(error),
        };
        let native =
            |context: &PricingContext, _| PricingResult::new(engine.price(option, context)?);
        crate::risk::numerical::complete_greeks_with_pair(
            option,
            context,
            request,
            settings,
            false,
            PricingResult::new(base)?,
            native,
            |a, b, _| {
                if *a == high && *b == low {
                    Ok(Some((PricingResult::new(up)?, PricingResult::new(down)?)))
                } else {
                    Ok(None)
                }
            },
        )
        .map(|result| result.with_seed(Some(seed)))
    }

    fn price_coupon_bounds(
        &self,
        lower: &AutocallableNote<T>,
        upper: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<Option<[f64; 2]>> {
        self.settings.validate_solver_seed()?;
        let Sample::Path(lower) = self.prepare_sample::<_, 2>(lower, context)? else {
            return Ok(None);
        };
        let Sample::Path(upper) = self.prepare_sample::<_, 2>(upper, context)? else {
            return Ok(None);
        };
        if !lower.shares_thresholds(&upper)? {
            return Ok(None);
        }
        let ([low, high], _, _) = lower.sample(self.settings, [&lower, &upper], false, false)?;
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
