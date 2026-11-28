use std::{ops::RangeInclusive, sync::Arc};

use super::{LazyPath, Moments, MonteCarloEstimate, MonteCarloSettings, SkippingPath};
use super::{
    check_path_step, estimate, path_samples_lazy, path_samples_skipping, scaled_value, transition,
    unstable,
};
use crate::instruments::autocallable::ObservationEvent;
use crate::instruments::autocallable::pricing::{
    Initial, fixed_value, future_event_times, initial, validate_with_work_check,
};
use crate::numerics::log_ratio;
use crate::risk::analytics::CouponPricer;
use crate::{
    Error, Greek, GreeksRequest, NumericalShiftSettings, PricingContext, PricingContextChanges,
    PricingEngine, PricingResult, Result, Timestamp,
    instruments::{AutocallableNote, AutocallableTerms, OptionContract},
};

mod portfolio;

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
    /// The returned engine copies all other controls, including any integration or
    /// exercise grid. Save it before pricing to replay the calculation within the
    /// same build. See
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
    /// use kiyosi::engines::MonteCarloSettings;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2025, 4, 1)?.start_of_day();
    /// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.02, start, end,
    ///     SnowballObservation::from_columns(&[end], &[110.0], &[0.08])?, kiyosi::TradingCalendar::weekdays()))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
    /// let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
    ///     seed: Some(42), path_count: 4096, ..Default::default()
    /// })?;
    /// let estimate = engine.price_with_statistics(&note, &context)?;
    /// assert!((estimate.price() - 1.0).abs() < 0.02);
    /// assert!(estimate.standard_error().is_some());
    /// assert_eq!(engine.settings().path_count, 4096);
    /// let result = engine.price_with_greeks(&note, &context,
    ///     GreeksRequest::new([Greek::Rho]))?;
    /// assert!(result.get(Greek::Rho).is_some());
    /// # Ok(()) }
    /// ```
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned sampling settings. See [`Self::new`].
    pub fn settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Prices a batch of notes of one product type in input order.
    ///
    /// Compatible observation grids share simulated paths within the batch
    /// workspace. All simulations in the batch use one seed. Configure a
    /// seed or call [`Self::with_resolved_seed`] to replay the results.
    ///
    /// # Examples
    ///
    /// With the note and market snapshot from [`Self::new`], pass
    /// `&[&note]` to [`Self::price_many`] to obtain a one-price vector.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] above 1,048,576 notes or one billion
    /// estimated path and note visits, including deterministic schedule work,
    /// and [`Error::ComputationFailure`] if output reservation fails.
    /// Propagates scalar pricing, entropy and cancellation errors.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the batch's parallel work finishes.
    pub fn price_many<T: AutocallableTerms>(
        &self,
        notes: &[&AutocallableNote<T>],
        context: &PricingContext,
    ) -> Result<Vec<f64>> {
        portfolio::price_many(*self, notes, context, false)
    }

    /// Prices notes on a common daily grid across cash-flow dates and maturities.
    ///
    /// Notes of one product type can share simulated paths when stochastic
    /// transitions and daily knock-in monitoring dates match through their
    /// common life. Different maturities can share the grid when they fall on
    /// its observation dates. Cash events keep their contractual dates. The
    /// batch is reproducible with a fixed seed.
    /// Notes that differ only by maturity, periodic coupon or knock-in level can share
    /// cash-flow exposures on the common grid.
    /// Shared paths can differ from the seeded scalar Brownian bridge. See
    /// [`Self::price_many`] for an example of pricing a batch of notes.
    ///
    /// # Errors
    ///
    /// Propagates the pricing errors of [`Self::price_many`].
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the batch's parallel work finishes.
    pub fn price_many_on_shared_grid<T: AutocallableTerms>(
        &self,
        notes: &[&AutocallableNote<T>],
        context: &PricingContext,
    ) -> Result<Vec<f64>> {
        portfolio::price_many(*self, notes, context, true)
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

    fn market_prices<T: AutocallableTerms, const N: usize>(
        &self,
        note: &AutocallableNote<T>,
        contexts: [&PricingContext; N],
    ) -> Result<Option<([f64; N], u64)>> {
        let mut plans = Vec::with_capacity(N);
        for context in contexts {
            let Sample::Path(plan) = self.prepare_sample::<_, N>(note, context)? else {
                return Ok(None);
            };
            plans.push(plan);
        }
        let plans: [&PathPlan<'_, T>; N] = std::array::from_fn(|lane| &plans[lane]);
        let base = plans[0];
        // For the same note and valuation, equal monitoring modes select the same dates.
        let mut shared = true;
        for plan in &plans[1..] {
            if base.steps.len() != plan.steps.len()
                || base.state.knocked_in != plan.state.knocked_in
                || base.bridge.is_some() != plan.bridge.is_some()
            {
                return Ok(None);
            }
            shared = shared && base.shares_transitions(plan)?;
        }
        let (moments, _, seed) = if shared {
            base.sample(self.settings, plans, false, true)?
        } else if let Some(bridge) = &base.bridge {
            path_samples_skipping(self.settings, base.steps.len(), false, |normals| {
                PathPlan::bridge_payoffs::<N, false>(bridge, plans, normals)
            })?
        } else {
            path_samples_lazy(self.settings, base.steps.len(), false, false, |normals| {
                base.spot_payoffs::<N, false>(plans, normals)
            })?
        };
        let mut prices = [0.0; N];
        for (price, moment) in prices.iter_mut().zip(moments) {
            *price = scaled_value(moment.scale, moment.mean, 0.0)?;
        }
        Ok(Some((prices, seed)))
    }

    fn market_greek_prices<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<Vec<(PricingContext, PricingResult)>> {
        let greeks = [Greek::Vega, Greek::Rho, Greek::DividendRho]
            .into_iter()
            .filter(|&greek| {
                request.has(greek) || greek == Greek::Vega && request.has(Greek::Volga)
            });
        let greeks = greeks.collect::<Vec<_>>();
        if greeks.len() < 2 || context.valuation_time() == note.expiry_time() {
            return Ok(Vec::new());
        }
        let scenarios = crate::risk::numerical::greek_scenarios(
            note,
            context,
            GreeksRequest::new(greeks),
            shifts,
        )?;
        let batch = match &scenarios[1..] {
            [a, b, c, d] => self
                .market_prices(note, [a, b, c, d])?
                .map(|(p, seed)| (p.to_vec(), seed)),
            [a, b, c, d, e, f] => self
                .market_prices(note, [a, b, c, d, e, f])?
                .map(|(p, seed)| (p.to_vec(), seed)),
            _ => None,
        };
        let Some((prices, seed)) = batch else {
            return Ok(Vec::new());
        };
        scenarios
            .into_iter()
            .skip(1)
            .zip(prices)
            .map(|(context, price)| Ok((context, PricingResult::new(price)?.with_seed(Some(seed)))))
            .collect()
    }

    fn prepare_sample<'a, T: AutocallableTerms, const N: usize>(
        &self,
        note: &'a AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<Sample<'a, T>> {
        self.prepare_sample_on_grid::<T, N>(note, context, None, |_| Ok(()))
    }

    fn prepare_sample_on_grid<'a, T: AutocallableTerms, const N: usize>(
        &self,
        note: &'a AutocallableNote<T>,
        context: &PricingContext,
        shared: Option<&mut portfolio::DailyGrids<'a>>,
        charge: impl Fn(u64) -> Result<()>,
    ) -> Result<Sample<'a, T>> {
        let model = context.natural_bsm_parameters()?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::autocallable_with_work_check(
                note, context, charge,
            )
            .map(Sample::Fixed);
        }
        validate_with_work_check(note, context, &charge)?;
        let state = initial(note, context);
        let terms = note.view();
        let valuation = context.valuation_time();
        let expiry = note.expiry_time();
        if state.settlement.is_none() {
            charge(note.future_schedule_visits(valuation, terms.daily_knock_in()))?;
        }
        if let Some(value) = state.settlement.or_else(|| {
            fixed_value(note, context, state.knocked_in).map(|value| value + state.coupon)
        }) {
            return if value.is_finite() {
                Ok(Sample::Fixed(value))
            } else {
                Err(unstable())
            };
        }
        let curve_knots = [model.risk_free_curve(), model.dividend_curve()]
            .into_iter()
            .map(|curve| {
                curve
                    .nodes_after(valuation)
                    .partition_point(|&node| node <= expiry) as u64
            })
            .sum();
        charge(curve_knots)?;
        context.validate_curve_horizon(expiry)?;
        let daily = terms.daily_knock_in() && !state.knocked_in && note.knock_in_affects_payoff();
        let sigma = model.volatility();
        let initial_spot = context.spot_price();
        let safe_returns = super::safe_log_returns(initial_spot);
        let knock_in = terms.knock_in.map(|ki| log_ratio(ki.level, initial_spot));
        let expiry_knock_in = note.observes_knock_in(note.expiry_time());
        if let Some(shared) = shared.filter(|_| daily) {
            if let Some(portfolio::SparsePath {
                steps,
                events,
                excursion,
            }) = shared.prepare(
                note,
                context,
                self.settings,
                [rate, dividend, sigma],
                &charge,
            )? {
                self.settings
                    .validate_scheduled_workspace::<[PathStep; N]>(steps.len())?;
                if safe_returns.contains(&excursion) && safe_returns.contains(&-excursion) {
                    self.settings
                        .validate_scheduled_workspace::<[(PathStep, BridgeInterval, [f64; 2]); N]>(
                            steps.len(),
                        )?;
                }
                return Ok(Sample::Path(PathPlan {
                    note,
                    state,
                    steps,
                    initial_spot,
                    safe_returns,
                    knock_in,
                    expiry_knock_in,
                    bridge: None,
                    shared_events: Some(events),
                }));
            }
        }
        // Construct the combined schedule, then turn it into path transitions.
        charge(2 * note.future_schedule_visits(valuation, daily))?;
        let dates = future_event_times(note, context, daily)?;
        let mut previous = valuation;
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
            let event = record.map(|record| CashEvent::new(note, record, initial_spot));
            steps.push(PathStep {
                transition: [drift, diffusion, discount],
                event,
                observe_knock_in: daily && note.daily_time(date, note.calendar()),
            });
            previous = date;
        }
        let bridge =
            if daily && safe_returns.contains(&excursion) && safe_returns.contains(&-excursion) {
                charge(2 * steps.len() as u64)?;
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
            steps: Arc::new(steps),
            initial_spot,
            safe_returns,
            knock_in,
            expiry_knock_in,
            bridge,
            shared_events: None,
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
    steps: Arc<Vec<PathStep>>,
    initial_spot: f64,
    safe_returns: RangeInclusive<f64>,
    knock_in: Option<f64>,
    expiry_knock_in: bool,
    bridge: Option<BridgePlan>,
    shared_events: Option<portfolio::SparseEvents>,
}

#[derive(PartialEq)]
struct BridgePlan {
    intervals: Vec<BridgeInterval>,
    daily: Vec<[f64; 2]>,
}

#[derive(PartialEq)]
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
    fn new<T: AutocallableTerms>(
        note: &AutocallableNote<T>,
        record: &ObservationEvent,
        initial_spot: f64,
    ) -> Self {
        let knock_out = record
            .has_knock_out()
            .then(|| log_ratio(record.knock_out_level(), initial_spot));
        let coupon_barrier = record.coupon_barrier_level();
        Self {
            knock_out,
            coupon_barrier: log_ratio(coupon_barrier.unwrap_or(0.0), initial_spot),
            period_coupon: if coupon_barrier.is_some() {
                note.period_coupon()
            } else {
                0.0
            },
            redemption: knock_out.map_or(0.0, |_| note.knock_out_payment(record)),
        }
    }

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
    fn shares_portfolio_grid(&self, other: &Self) -> Result<bool> {
        if !self.shares_transitions(other)? || self.bridge != other.bridge {
            return Ok(false);
        }
        for (left, right) in self.steps.chunks(1024).zip(other.steps.chunks(1024)) {
            crate::cancellation::check()?;
            if left.iter().zip(right).any(|(a, b)| {
                a.observe_knock_in != b.observe_knock_in || a.event.is_some() != b.event.is_some()
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn sample<const N: usize>(
        &self,
        settings: MonteCarloSettings,
        plans: [&Self; N],
        statistics: bool,
        spot_stencil: bool,
    ) -> Result<([Moments; N], [Moments; N], u64)> {
        if let Some(bridge) = &self.bridge {
            return path_samples_skipping(settings, self.steps.len(), statistics, |normals| {
                Self::bridge_payoffs::<N, true>(bridge, plans, normals)
            });
        }
        path_samples_lazy(settings, self.steps.len(), statistics, false, |normals| {
            if spot_stencil {
                self.spot_payoffs::<N, true>(plans, normals)
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

    fn bridge_payoffs<const N: usize, const SHARED_GROWTH: bool>(
        bridge: &BridgePlan,
        plans: [&Self; N],
        normals: &mut SkippingPath<'_, '_>,
    ) -> Result<[f64; N]> {
        let bridges = plans.map(|plan| {
            if SHARED_GROWTH {
                bridge
            } else {
                plan.bridge.as_ref().expect("shared bridge dates")
            }
        });
        let mut cash = plans.map(|plan| plan.state.coupon);
        let mut monitoring =
            plans.map(|plan| (!plan.state.knocked_in).then_some(plan.knock_in).flatten());
        let mut knocked_in = plans.map(|plan| plan.state.knocked_in);
        let mut active = [true; N];
        let mut start = 0;
        let mut log_returns = [0.0; N];
        let mut discounts = [0.0; N];
        for (index, interval) in bridge.intervals.iter().enumerate() {
            check_path_step(index)?;
            let normal = normals.normal();
            let targets: [f64; N] = std::array::from_fn(|lane| {
                let lane = if SHARED_GROWTH { 0 } else { lane };
                let interval = &bridges[lane].intervals[index];
                log_returns[lane] + interval.diffusion.mul_add(normal, interval.drift)
            });
            let floor = monitoring.iter().flatten().copied().reduce(f64::min);
            let mut minimum = f64::INFINITY;
            for (offset, &daily) in bridge.daily[start..interval.end].iter().enumerate() {
                let i = start + offset;
                if if SHARED_GROWTH {
                    floor.is_none_or(|level| minimum < level)
                } else {
                    monitoring.iter().all(Option::is_none)
                } {
                    normals.skip(interval.end - i);
                    break;
                }
                check_path_step(i)?;
                let normal = normals.normal();
                for lane in 0..if SHARED_GROWTH { 1 } else { N } {
                    if SHARED_GROWTH || monitoring[lane].is_some() {
                        let [weight, diffusion] = if SHARED_GROWTH {
                            daily
                        } else {
                            bridges[lane].daily[i]
                        };
                        let value = &mut log_returns[lane];
                        *value += weight.mul_add(targets[lane] - *value, diffusion * normal);
                    }
                }
                if SHARED_GROWTH {
                    minimum = minimum.min(log_returns[0]);
                } else {
                    for (lane, level) in monitoring.iter_mut().enumerate() {
                        if level.is_some_and(|level| log_returns[lane] < level) {
                            *level = None;
                            knocked_in[lane] = true;
                        }
                    }
                }
            }
            log_returns = targets;
            start = interval.end + 1;
            for lane in 0..N {
                if !active[lane] {
                    continue;
                }
                let step = &plans[lane].steps[interval.end];
                let log_return = log_returns[lane];
                let discount = step.transition[2];
                discounts[lane] = discount;
                if monitoring[lane].is_some_and(|level| {
                    minimum < level || (step.observe_knock_in && log_return < level)
                }) {
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
                let log_return = log_returns[lane];
                let discount = discounts[lane];
                let hit = knocked_in[lane]
                    || (plan.expiry_knock_in
                        && plan.knock_in.is_some_and(|level| log_return < level));
                cash[lane] += plan.discounted_terminal(log_return, hit, discount)?;
            }
        }
        finite_payoffs(cash)
    }

    fn spot_payoffs<const N: usize, const SHARED_GROWTH: bool>(
        &self,
        plans: [&Self; N],
        normals: &mut LazyPath<'_, '_>,
    ) -> Result<[f64; N]> {
        let mut log_returns = [0.0; N];
        let mut discounts = [0.0; N];
        let mut cash = plans.map(|plan| plan.state.coupon);
        let mut knocked_in = plans.map(|plan| plan.state.knocked_in);
        let mut active = [true; N];
        for (index, step) in self.steps.iter().enumerate() {
            check_path_step(index)?;
            let normal = normals.normal();
            if SHARED_GROWTH {
                log_returns[0] += step.transition[1].mul_add(normal, step.transition[0]);
            }
            for lane in 0..N {
                if !active[lane] {
                    continue;
                }
                let plan = plans[lane];
                let step = &plan.steps[index];
                let [drift, diffusion, discount] = step.transition;
                discounts[lane] = discount;
                if !SHARED_GROWTH {
                    log_returns[lane] += diffusion.mul_add(normal, drift);
                }
                let log_return = log_returns[if SHARED_GROWTH { 0 } else { lane }];
                plan.validate_return(log_return)?;
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
                let log_return = log_returns[if SHARED_GROWTH { 0 } else { lane }];
                let discount = discounts[lane];
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
    fn price_with_statistics(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (sample, pairs, known, seed) = self.sample(option, context, true)?;
        estimate(self.settings, sample, pairs, 0.0, known, seed)
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
        let _sampling = super::RequestCache::new();
        let engine = self.with_resolved_seed()?;
        let mut spot_batch = None;
        if (request.has(Greek::Delta) || request.has(Greek::Gamma))
            && shift > 0.0
            && spot > shift
            && context.valuation_time() != option.expiry_time()
            && !option.all_greeks_unavailable(context)
            && !option.at_spot_discontinuity(context, shift)
        {
            let shifted = |spot| {
                context.replace(PricingContextChanges {
                    spot_price: Some(spot),
                    ..Default::default()
                })
            };
            let high = shifted(spot + shift)?;
            let low = shifted(spot - shift)?;
            match engine.spot_prices(option, [context, &high, &low]) {
                Ok(Some((prices, seed))) => spot_batch = Some((high, low, prices, seed)),
                Ok(None) | Err(Error::InvalidInput(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let seed = std::cell::Cell::new(None);
        let native = |context: &PricingContext, _| {
            let (price, used_seed) = engine.price_with_seed(option, context)?;
            if used_seed.is_some() {
                seed.set(used_seed);
            }
            PricingResult::new(price)
        };
        let base = if let Some((_, _, [base, ..], used_seed)) = &spot_batch {
            seed.set(Some(*used_seed));
            PricingResult::new(*base)?
        } else {
            native(context, request)?
        };
        let market_batch = match engine.market_greek_prices(option, context, request, settings) {
            Ok(prices) => prices,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted => {
                return Err(error.into());
            }
            Err(_) => Vec::new(),
        };
        crate::risk::numerical::complete_greeks_with_pair(
            option,
            context,
            request,
            settings,
            false,
            base,
            native,
            |a, b, _| {
                if let Some((high, low, [_, up, down], _)) = &spot_batch {
                    if a == high && b == low {
                        return Ok(Some((PricingResult::new(*up)?, PricingResult::new(*down)?)));
                    }
                }
                if a.valuation_time() == b.valuation_time() {
                    let cached = |context| {
                        market_batch
                            .iter()
                            .find(|(scenario, _)| scenario == context)
                            .map(|(_, result)| result.clone())
                    };
                    if let (Some(up), Some(down)) = (cached(a), cached(b)) {
                        seed.set(up.seed());
                        return Ok(Some((up, down)));
                    }
                    match engine.market_prices(option, [a, b]) {
                        Ok(Some(([up, down], used_seed))) => {
                            seed.set(Some(used_seed));
                            return Ok(Some((PricingResult::new(up)?, PricingResult::new(down)?)));
                        }
                        Err(error @ Error::Io(_)) => return Err(error),
                        Ok(None) | Err(_) => {}
                    }
                }
                Ok(None)
            },
        )
        .map(|result| result.with_seed(seed.get()))
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

    fn prepare_coupon_pricer<'a>(
        &'a self,
        lower: &AutocallableNote<T>,
        upper: &AutocallableNote<T>,
        context: &'a PricingContext,
    ) -> Result<Option<CouponPricer<'a, AutocallableNote<T>>>> {
        let affected = lower.knock_in_affects_payoff();
        if affected != upper.knock_in_affects_payoff() {
            return Ok(None);
        }
        let Some([low, high]) = self.price_coupon_bounds(lower, upper, context)? else {
            return Ok(None);
        };
        Ok(Some(Box::new(move |note, fraction| {
            if note.knock_in_affects_payoff() != affected {
                return Ok(None);
            }
            if let Sample::Fixed(value) = self.prepare_sample::<_, 1>(note, context)? {
                return Ok(Some(value));
            }
            let price = low * (1.0 - fraction) + high * fraction;
            Ok(Some(price))
        })))
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        crate::engines::flat_rates(context).map(|_| ())
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.settings().validate_solver_seed()
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AutocallableNote<T>> + '_>>> {
        prepare_seeded_request!(self, self.settings)
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
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        Ok(self.price_with_seed(option, context)?.0)
    }
}
