use super::{
    BinomialSettings, Error, Greek, GreeksRequest, ImpliedVolatilitySettings, OptionContract,
    OptionType, PricingContext, PricingEngine, PricingResult, Result, european_levels, flat_rates,
    parameters, prepare_implied, scaled, time_step, unstable,
};
use crate::{AnalyticEuropeanEngine, BarrierOption, RebateTiming};
use rustfft::{FftPlanner, num_complex::Complex64};
use std::ops::Range;

const GAP_ROLLBACK_BUDGET: usize = 128_000_000;

/// Prices single-barrier vanilla contracts with a flat-rate CRR tree.
///
/// Continuous monitoring checks every tree level. Scheduled fixings must align exactly
/// with time steps; dates are never moved. Only node delta/gamma are returned.
/// Values own their settings and shared instances support concurrent pricing.
/// See [`Self::price`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoxRossRubinsteinBarrierEngine {
    settings: BinomialSettings,
}

impl CoxRossRubinsteinBarrierEngine {
    /// Creates an engine after validating its step count.
    ///
    /// See [`CoxRossRubinsteinVanillaEngine::new`][crate::CoxRossRubinsteinVanillaEngine::new] for a settings example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside 1 through 1,000,000 steps.
    pub fn new(settings: BinomialSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> BinomialSettings {
        self.settings
    }

    /// Prices after validating flat rates, inclusive life and prior history.
    ///
    /// Resolved payoffs settle before grid validation; resolved vanilla payoffs use
    /// analytic pricing. Prior at-hit rebates are already paid.
    /// Settled cash flows do not require curve coverage through expiry.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves or unsupported
    /// fixings. Returns [`Error::InvalidInput`] outside life, for impossible history,
    /// outside required curve coverage, above 16,000 steps when full rollback is needed,
    /// or when scheduled rollback exceeds its work budget.
    /// Unstable tree parameters, nodes or results return [`Error::ComputationFailure`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BarrierOption, BarrierOptionTerms, BarrierType, BinomialSettings,
    ///     BlackScholesMertonParameters, CoxRossRubinsteinBarrierEngine, Date, OptionType,
    ///     PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let engine = CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 256 })?;
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        context.natural_bsm_parameters()?;
        Ok(self
            .native(option, context, GreeksRequest::new([]))?
            .price())
    }

    /// Returns requested node delta/gamma; other Greeks are absent.
    ///
    /// Uses engine-default estimation controls; see [`crate::numerical_greeks`] for custom bumps.
    /// Terminal stencils use engine-default spot bumps.
    /// One-step gamma is absent before expiry. Expiry spot Greeks follow terminal payoffs. Current barrier equality also
    /// suppresses Greeks without a prior touch.
    /// See [`Self::price`] for a pricing example.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before shift validation, then reuses
    /// [`Self::price`] errors. Requested measures other than delta and gamma
    /// have [`crate::GreekStatus::Unsupported`]; empty requests select price only.
    pub fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.greeks_with_settings(option, context, request, Default::default())
    }

    fn greeks_with_settings(
        self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        super::super::greeks(&self, option, context, request, settings, || {
            self.native(option, context, request)
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "Validated step counts and aligned in-life observations bound indices to the tree and fit exactly in f64."
    )]
    fn native(
        self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let (rate, _) = flat_rates(context)?;
        if crate::engines::deterministic::enabled(context) {
            return PricingResult::new(crate::engines::deterministic::barrier(option, context)?);
        }
        crate::instruments::monitoring::validate_life(option, context)?;
        let steps = self.settings.step_count as usize;
        let request = if context.valuation_time() == option.expiry_time()
            || option.at_spot_discontinuity(context, 0.0)
        {
            GreeksRequest::new([])
        } else {
            GreeksRequest::new(
                [Greek::Delta, Greek::Gamma]
                    .into_iter()
                    .filter(|&g| request.has(g) && (g != Greek::Gamma || steps >= 2)),
            )
        };
        if let Some(value) = crate::engines::analytic::barrier::resolved_price(option, context)? {
            if option.volatility_affects_price(context)
                && (request.has(Greek::Delta) || request.has(Greek::Gamma))
            {
                return AnalyticEuropeanEngine.price_with_greeks(
                    option.vanilla(),
                    context,
                    request,
                );
            }
            let mut result = PricingResult::new(value)?;
            result.set(request, Greek::Delta, 0.0)?;
            result.set(request, Greek::Gamma, 0.0)?;
            return Ok(result);
        }
        context.validate_curve_horizon(option.expiry_time())?;
        let time = context.valuation_time();
        let observations = option.observation_times().unwrap_or(&[]);
        let future = &observations[observations.partition_point(|&at| at <= time)..];
        let terminal_only = option.observation_mode() == crate::ObservationMode::Scheduled
            && future.iter().all(|&at| at == option.expiry_time());
        let continuous = option.observation_mode() == crate::ObservationMode::Continuous;
        let sparse = !continuous && future.len() <= steps / 128;
        if !terminal_only && !continuous && !sparse {
            self.settings.validate_rollback()?;
        }
        let duration = time.micros_until(option.expiry_time());
        let direct = terminal_only && steps >= 256;
        let mut observed = if direct {
            Vec::new()
        } else {
            vec![continuous; steps + 1]
        };
        if !continuous && !direct {
            // Integer arithmetic preserves exact microsecond alignment without overflowing i64.
            for &date in observations {
                let offset = time.micros_until(date);
                if offset < 0 {
                    continue;
                }
                let scaled = i128::from(offset) * steps as i128;
                if scaled % i128::from(duration) != 0 {
                    return Err(Error::UnsupportedOperation(
                        "scheduled barrier observation does not coincide with a CRR tree step"
                            .into(),
                    ));
                }
                observed[(scaled / i128::from(duration)) as usize] = true;
            }
        }
        let dt = time_step(option, context, self.settings)?;
        let (width, up, down, discount, p) = parameters(context, dt)?;
        let spot = context.spot_price();
        let log_barrier = crate::numerics::log_ratio(spot, option.barrier_level());
        let upper = option.barrier_type().is_up();
        let knock_in = option.barrier_type().is_knock_in();
        let breached = |level: usize, node: usize| {
            let offset = (2.0 * node as f64 - level as f64) * width;
            let distance = log_barrier + offset;
            if distance.abs() <= 4.0 * f64::EPSILON * (log_barrier.abs() + offset.abs()).max(1.0) {
                let asset = scaled(spot, offset);
                if upper {
                    asset >= option.barrier_level()
                } else {
                    asset <= option.barrier_level()
                }
            } else if upper {
                distance >= 0.0
            } else {
                distance <= 0.0
            }
        };
        let reflection = continuous
            && steps >= 1024
            && (1e-6..=1.0 - 1e-6).contains(&p)
            && (discount.log_factor * f64::from(self.settings.step_count)).abs() < 100.0
            && width * f64::from(self.settings.step_count) < 100.0
            && (1e-100..=1e100).contains(&spot)
            && (1e-100..=1e100).contains(&option.strike())
            && option.rebate() <= 1e100;
        let scheduled_gaps =
            sparse && (discount.log_factor * f64::from(self.settings.step_count)).abs() < 100.0;
        if !terminal_only && !reflection && !scheduled_gaps {
            self.settings.validate_rollback()?;
        }
        let mut values = vec![0.0; steps + 1];
        let mut vanilla = if knock_in && !reflection && !direct {
            vec![0.0; steps + 1]
        } else {
            Vec::new()
        };
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let observes_expiry = if direct {
            future.last() == Some(&option.expiry_time())
        } else {
            observed[steps]
        };
        for node in 0..=steps {
            if node % 256 == 0 {
                crate::cancellation::check()?;
            }
            let asset = scaled(spot, (2.0 * node as f64 - steps as f64) * width);
            if !asset.is_normal() {
                return Err(unstable());
            }
            let payoff = (sign * (asset - option.strike())).max(0.0);
            values[node] = if reflection || (observes_expiry && breached(steps, node)) == knock_in {
                payoff
            } else {
                option.rebate()
            };
            if !vanilla.is_empty() {
                vanilla[node] = payoff;
            }
        }
        let mut first = [0.0; 2];
        let mut second = [0.0; 3];
        if direct {
            let level = if request.has(Greek::Gamma) {
                2
            } else {
                usize::from(request.has(Greek::Delta))
            };
            let (price, nodes) = european_levels(&values, p, discount.log_factor, level)?;
            if !price.is_finite() || nodes.iter().any(|value| !value.is_finite()) {
                return Err(unstable());
            }
            second = nodes;
            first = if level == 2 {
                [
                    discount.rollback(nodes[0], nodes[1], p),
                    discount.rollback(nodes[1], nodes[2], p),
                ]
            } else {
                [nodes[0], nodes[1]]
            };
            values[0] = price;
        } else if reflection {
            // Check each integer walk state, including the opposite parity from expiry.
            let boundary = barrier_boundary(2 * steps + 1, |index| {
                let level = index.abs_diff(steps);
                breached(level, if index >= steps { level } else { 0 }) != upper
            });
            let away = if upper { 1.0 - p } else { p };
            let walk = BarrierWalk {
                away,
                log_discount: discount.log_factor,
                rebate: option.rebate(),
                at_hit: option.rebate_timing() == RebateTiming::AtHit,
                knock_in,
                upper,
            };
            let price_at = |level: usize, node: usize| {
                let index = steps + 2 * node - level;
                let distance = if upper {
                    boundary.saturating_sub(index)
                } else {
                    (index + 1).saturating_sub(boundary)
                };
                walk.price(&values[node..=node + steps - level], distance)
            };
            let price = price_at(0, 0)?;
            if request.has(Greek::Delta) {
                for (node, value) in first.iter_mut().enumerate() {
                    *value = price_at(1, node)?;
                }
            }
            if request.has(Greek::Gamma) {
                for (node, value) in second.iter_mut().enumerate() {
                    *value = price_at(2, node)?;
                }
            }
            values[0] = price;
        } else if scheduled_gaps {
            let mut planner = FftPlanner::new();
            let mut work = GAP_ROLLBACK_BUDGET * (1 + usize::from(!vanilla.is_empty()));
            let mut upper_level = steps;
            for level in (0..steps).rev().filter(|&i| i <= 2 || observed[i]) {
                crate::cancellation::check()?;
                let count = level + 1;
                let gap = upper_level - level;
                if upper_level == steps
                    && gap >= 128
                    && count > 3
                    && (1e-6..=1.0 - 1e-6).contains(&p)
                    && steps as f64 * width + spot.ln().abs() < 600.0
                {
                    let terminal_asset =
                        |node: usize| scaled(spot, (2.0 * node as f64 - steps as f64) * width);
                    let strike = option.strike();
                    let strike_node = barrier_boundary(steps + 1, |node| {
                        if sign > 0.0 {
                            terminal_asset(node) <= strike
                        } else {
                            terminal_asset(node) < strike
                        }
                    });
                    let paying = if sign > 0.0 {
                        strike_node..steps + 1
                    } else {
                        0..strike_node
                    };
                    let boundary = if observes_expiry {
                        barrier_boundary(steps + 1, |node| breached(steps, node) != upper)
                    } else if upper {
                        steps + 1
                    } else {
                        0
                    };
                    let (live, rebate) = if upper == knock_in {
                        (boundary..steps + 1, 0..boundary)
                    } else {
                        (0..boundary, boundary..steps + 1)
                    };
                    let terminal = TerminalGap::new(gap, p, width, &mut work)?;
                    terminal.rollback(
                        &mut values,
                        [
                            paying.start.max(live.start)..paying.end.min(live.end),
                            rebate,
                        ],
                        [spot, strike, sign, option.rebate(), discount.log_factor],
                        &mut work,
                    )?;
                    if !vanilla.is_empty() {
                        terminal.rollback(
                            &mut vanilla,
                            [paying, 0..0],
                            [spot, strike, sign, 0.0, discount.log_factor],
                            &mut work,
                        )?;
                    }
                } else {
                    rollback_gap(
                        &mut values[..=upper_level],
                        count,
                        p,
                        discount,
                        &mut planner,
                        &mut work,
                    )?;
                    if !vanilla.is_empty() {
                        rollback_gap(
                            &mut vanilla[..=upper_level],
                            count,
                            p,
                            discount,
                            &mut planner,
                            &mut work,
                        )?;
                    }
                }
                if observed[level] {
                    let boundary = barrier_boundary(count, |node| breached(level, node) != upper);
                    let hit = if upper { boundary..count } else { 0..boundary };
                    if knock_in {
                        values[hit.clone()].copy_from_slice(&vanilla[hit]);
                    } else {
                        let log_discount = if option.rebate_timing() == RebateTiming::AtHit {
                            0.0
                        } else {
                            -rate * (steps - level) as f64 * dt
                        };
                        values[hit].fill(scaled(option.rebate(), log_discount));
                    }
                }
                if values[..count].iter().any(|x| !x.is_finite()) {
                    return Err(unstable());
                }
                if level == 1 {
                    first.copy_from_slice(&values[..2]);
                }
                if level == 2 {
                    second.copy_from_slice(&values[..3]);
                }
                upper_level = level;
            }
        } else {
            for level in (0..=steps).rev() {
                crate::cancellation::check()?;
                if level < steps {
                    let rebate = if observed[level] && !knock_in {
                        if option.rebate_timing() == RebateTiming::AtHit {
                            option.rebate()
                        } else {
                            scaled(option.rebate(), -rate * (steps - level) as f64 * dt)
                        }
                    } else {
                        0.0
                    };
                    let nodes = &mut values[..level + 2];
                    let live = if observed[level] {
                        let boundary =
                            barrier_boundary(level + 1, |node| breached(level, node) != upper);
                        if upper {
                            0..boundary
                        } else {
                            boundary..level + 1
                        }
                    } else {
                        0..level + 1
                    };
                    if discount.factor.is_normal() {
                        rollback_level(nodes, &mut vanilla, rebate, live, |lower, upper| {
                            discount.rollback_normal(lower, upper, p)
                        })?;
                    } else {
                        rollback_level(nodes, &mut vanilla, rebate, live, |lower, upper| {
                            discount.rollback_scaled(lower, upper, p)
                        })?;
                    }
                }
                if level == 1 {
                    first.copy_from_slice(&values[..2]);
                }
                if level == 2 {
                    second.copy_from_slice(&values[..3]);
                }
            }
        }
        let mut result = PricingResult::new(values[0])?;
        let denominator = spot * (up - down);
        if denominator.is_finite() && denominator != 0.0 {
            result.set(request, Greek::Delta, (first[1] - first[0]) / denominator)?;
        }
        if steps >= 2 {
            let high = spot * (up * up - 1.0);
            let low = spot * (1.0 - down * down);
            let denominator = 0.5 * spot * (up * up - down * down);
            if high.is_finite()
                && low.is_finite()
                && denominator.is_finite()
                && high != 0.0
                && low != 0.0
                && denominator != 0.0
            {
                result.set(
                    request,
                    Greek::Gamma,
                    ((second[2] - second[1]) / high - (second[1] - second[0]) / low) / denominator,
                )?;
            }
        }
        Ok(result)
    }
}

struct BarrierWalk {
    away: f64,
    log_discount: f64,
    rebate: f64,
    at_hit: bool,
    knock_in: bool,
    upper: bool,
}

impl BarrierWalk {
    #[expect(
        clippy::cast_precision_loss,
        reason = "Validated tree sizes fit exactly in f64."
    )]
    fn price(&self, payoffs: &[f64], distance: usize) -> Result<f64> {
        let steps = payoffs.len() - 1;
        let expiry_discount = steps as f64 * self.log_discount;
        if distance == 0 && !self.knock_in {
            return Ok(if self.at_hit {
                self.rebate
            } else {
                scaled(self.rebate, expiry_discount)
            });
        }
        let log_towards = (-self.away).ln_1p();
        let log_away = self.away.ln();
        let log_odds = log_away - log_towards;
        let mut log_probability = steps as f64 * log_towards;
        let mut hit_lower = usize::MAX;
        let mut log_hit = f64::NEG_INFINITY;
        if (1..=steps).contains(&distance) {
            // First live terminal state. Reflecting a path that hits the barrier
            // requires distance extra away steps, with the original path weights.
            hit_lower = (steps - distance) / 2 + 1;
            if hit_lower + distance <= steps {
                log_hit = (hit_lower..hit_lower + distance)
                    .map(|k| ((steps - k) as f64 / (k + 1) as f64).ln())
                    .sum();
            }
        }
        let mut sum = crate::numerics::Sum::default();
        for k in 0..=steps {
            if k % 256 == 0 {
                crate::cancellation::check()?;
            }
            let payoff = payoffs[if self.upper { steps - k } else { k }];
            let hit_log = if distance == 0 || (distance <= steps && k < hit_lower) {
                0.0
            } else {
                log_hit.min(0.0)
            };
            let survival_log = (-hit_log.exp_m1()).ln();
            let payoff_log = if self.knock_in { hit_log } else { survival_log };
            if payoff > 0.0 && payoff_log.is_finite() {
                sum.add(scaled(
                    payoff,
                    log_probability + payoff_log + expiry_discount,
                ));
            }
            if self.rebate > 0.0 && (self.knock_in || !self.at_hit) {
                let rebate_log = if self.knock_in { survival_log } else { hit_log };
                if rebate_log.is_finite() {
                    sum.add(scaled(
                        self.rebate,
                        log_probability + rebate_log + expiry_discount,
                    ));
                }
            }
            if k < steps {
                log_probability += ((steps - k) as f64 / (k + 1) as f64).ln() + log_odds;
                if k >= hit_lower && k + distance < steps {
                    log_hit += (((steps - k - distance) as f64 / (steps - k) as f64)
                        * ((k + 1) as f64 / (k + distance + 1) as f64))
                        .ln();
                } else if k >= hit_lower {
                    log_hit = f64::NEG_INFINITY;
                }
            }
        }
        if !self.knock_in && self.at_hit && self.rebate > 0.0 && distance <= steps {
            // The first-passage mass at distance + 2k follows the ballot recurrence.
            let mut first_hit = distance as f64 * log_towards;
            for k in 0..=(steps - distance) / 2 {
                if k % 256 == 0 {
                    crate::cancellation::check()?;
                }
                let step = (distance + 2 * k) as f64;
                sum.add(scaled(self.rebate, first_hit + step * self.log_discount));
                first_hit += (step * (step + 1.0) / ((k + 1) as f64 * (k + distance + 1) as f64))
                    .ln()
                    + log_away
                    + log_towards;
            }
        }
        let price = sum.total();
        if price.is_finite() {
            Ok(price)
        } else {
            Err(unstable())
        }
    }
}

fn barrier_boundary(count: usize, below: impl Fn(usize) -> bool) -> usize {
    let mut low = 0;
    let mut high = count;
    while low < high {
        let middle = low.midpoint(high);
        if below(middle) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

fn charge_gap_work(work: &mut usize, visits: usize) -> Result<()> {
    *work = work.checked_sub(visits).ok_or_else(|| {
        Error::InvalidInput("scheduled binomial rollback exceeds its work budget".into())
    })?;
    Ok(())
}

struct BinomialTails {
    prefix: Vec<f64>,
    suffix: Vec<f64>,
}

impl BinomialTails {
    #[expect(
        clippy::cast_precision_loss,
        reason = "Bounded tree counts fit exactly in f64."
    )]
    fn new(weights: &[f64], width: f64) -> Result<Self> {
        let len = weights.len();
        let mut prefix = vec![f64::NEG_INFINITY; len + 1];
        let mut suffix = vec![f64::NEG_INFINITY; len + 1];
        let add_logs = |a: f64, b: f64| a.max(b) + (a.min(b) - a.max(b)).exp().ln_1p();
        let term = |i: usize| weights[i] + (2.0 * i as f64 - (len - 1) as f64) * width;
        for i in 0..len {
            if i % 256 == 0 {
                crate::cancellation::check()?;
            }
            prefix[i + 1] = add_logs(prefix[i], term(i));
            let j = len - 1 - i;
            suffix[j] = add_logs(suffix[j + 1], term(j));
        }
        Ok(Self { prefix, suffix })
    }

    fn log_mass(&self, range: Range<usize>) -> f64 {
        if range.is_empty() {
            return f64::NEG_INFINITY;
        }
        let prefix = [self.prefix[range.end], self.prefix[range.start]];
        let suffix = [self.suffix[range.start], self.suffix[range.end]];
        // Take the better-conditioned difference. Far tails remain in log space.
        let [high, low] = if prefix[0] - prefix[1] >= suffix[0] - suffix[1] {
            prefix
        } else {
            suffix
        };
        crate::numerics::log_difference(high, low)
    }
}

struct TerminalGap {
    cash: BinomialTails,
    asset: BinomialTails,
    log_weights: Vec<f64>,
    log_normalization: f64,
    width: f64,
}

impl TerminalGap {
    fn new(gap: usize, p: f64, width: f64, work: &mut usize) -> Result<Self> {
        charge_gap_work(work, 7 * (gap + 1))?;
        let (log_weights, normalization) = super::binomial_log_weights(gap, p)?;
        Ok(Self {
            cash: BinomialTails::new(&log_weights, 0.0)?,
            asset: BinomialTails::new(&log_weights, width)?,
            log_weights,
            log_normalization: normalization.ln(),
            width,
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "Bounded tree counts fit exactly in f64."
    )]
    fn rollback(
        &self,
        values: &mut [f64],
        [paying, rebate_nodes]: [Range<usize>; 2],
        [spot, strike, sign, rebate, log_discount]: [f64; 5],
        work: &mut usize,
    ) -> Result<()> {
        let dimensions = self.log_weights.len();
        let level = values.len() - dimensions;
        charge_gap_work(work, level + 1)?;
        let weight_scale = (dimensions - 1) as f64 * log_discount - self.log_normalization;
        let log_spot = spot.ln();
        let log_strike = strike.ln();
        let log_rebate = rebate.ln();
        for i in 0..=level {
            if i % 256 == 0 {
                crate::cancellation::check()?;
            }
            let relative = |range: &Range<usize>| {
                range.start.saturating_sub(i).min(dimensions)
                    ..range.end.saturating_sub(i).min(dimensions)
            };
            let paid = relative(&paying);
            let payoff = if paid.is_empty() {
                0.0
            } else {
                let cash = log_strike + self.cash.log_mass(paid.clone()) + weight_scale;
                let asset = log_spot
                    + (2.0 * i as f64 - level as f64) * self.width
                    + self.asset.log_mass(paid.clone())
                    + weight_scale;
                let [high, low] = if sign > 0.0 {
                    [asset, cash]
                } else {
                    [cash, asset]
                };
                if high - low > 1e-4 {
                    crate::numerics::log_difference(high, low).exp()
                } else {
                    direct_gap_sum(&values[i..], &self.log_weights, paid, weight_scale, work)?
                }
            };
            values[i] = payoff
                + (log_rebate + self.cash.log_mass(relative(&rebate_nodes)) + weight_scale).exp();
            if !values[i].is_finite() {
                return Err(unstable());
            }
        }
        Ok(())
    }
}

fn direct_gap_sum(
    values: &[f64],
    log_weights: &[f64],
    range: Range<usize>,
    weight_scale: f64,
    work: &mut usize,
) -> Result<f64> {
    charge_gap_work(work, 2 * range.len())?;
    let term = |i: usize| values[i].ln() + log_weights[i];
    let mut maximum = f64::NEG_INFINITY;
    for i in range.clone() {
        if i % 1024 == 0 {
            crate::cancellation::check()?;
        }
        maximum = maximum.max(term(i));
    }
    if maximum == f64::NEG_INFINITY {
        return Ok(0.0);
    }
    let mut sum = 0.0;
    for i in range {
        if i % 1024 == 0 {
            crate::cancellation::check()?;
        }
        sum += (term(i) - maximum).exp();
    }
    Ok((maximum + sum.ln() + weight_scale).exp())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Bounded rollback indices fit exactly in f64."
)]
fn rollback_gap(
    values: &mut [f64],
    count: usize,
    probability: f64,
    discount: super::StepDiscount,
    planner: &mut FftPlanner<f64>,
    work: &mut usize,
) -> Result<()> {
    let gap = values.len() - count;
    if gap < 128 {
        charge_gap_work(work, gap.saturating_mul(values.len() + count - 1) / 2)?;
        for end in (count..values.len()).rev() {
            crate::cancellation::check()?;
            rollback_level(&mut values[..=end], &mut [], 0.0, 0..end, |lower, upper| {
                discount.rollback(lower, upper, probability)
            })?;
        }
        return Ok(());
    }
    let log_discount = gap as f64 * discount.log_factor;
    if count <= 3 {
        charge_gap_work(work, 3 * count * (gap + 1))?;
        let mut next = [0.0; 3];
        for (i, value) in next[..count].iter_mut().enumerate() {
            *value = european_levels(&values[i..=i + gap], probability, discount.log_factor, 0)?.0;
        }
        values[..count].copy_from_slice(&next[..count]);
        return Ok(());
    }
    if probability <= 0.0 || probability >= 1.0 {
        charge_gap_work(work, count)?;
        let offset = if probability >= 1.0 { gap } else { 0 };
        for i in 0..count {
            values[i] = scaled(values[i + offset], log_discount);
        }
        return Ok(());
    }

    let scale = values.iter().copied().fold(0.0, f64::max);
    if scale == 0.0 {
        return Ok(());
    }
    let size = (values.len() + gap).next_power_of_two();
    charge_gap_work(work, 3 * size * size.ilog2() as usize)?;
    let (weights, normalization) = super::binomial_log_weights(gap, probability)?;
    let mut payoff = vec![Complex64::default(); size];
    let mut kernel = vec![Complex64::default(); size];
    for (slot, &value) in payoff.iter_mut().zip(values.iter()) {
        slot.re = value / scale;
    }
    for (slot, &weight) in kernel.iter_mut().zip(weights.iter().rev()) {
        slot.re = weight.exp() / normalization;
    }
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let mut scratch = vec![
        Complex64::default();
        forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len())
    ];
    forward.process_with_scratch(&mut payoff, &mut scratch);
    crate::cancellation::check()?;
    forward.process_with_scratch(&mut kernel, &mut scratch);
    for (sample, weight) in payoff.iter_mut().zip(kernel) {
        *sample *= weight;
    }
    inverse.process_with_scratch(&mut payoff, &mut scratch);

    // Recover tails below the FFT's absolute accuracy with positive log-weighted sums.
    let last = values.iter().rposition(|&value| value > 0.0).unwrap_or(0);
    let first = values
        .iter()
        .position(|&value| value > 0.0)
        .unwrap_or(values.len());
    let log_normalization = normalization.ln();
    for i in 0..count {
        if i % 256 == 0 {
            crate::cancellation::check()?;
        }
        let value = payoff[i + gap].re / size as f64;
        values[i] = if value > 1e-8 {
            scaled(scale, value.ln() + log_discount)
        } else if i > last || i + gap < first {
            0.0
        } else {
            let start = first.saturating_sub(i);
            let end = (last - i).min(gap);
            direct_gap_sum(
                &values[i..],
                &weights,
                start..end + 1,
                log_discount - log_normalization,
                work,
            )?
        };
        if !values[i].is_finite() {
            return Err(unstable());
        }
    }
    Ok(())
}

fn rollback_level(
    values: &mut [f64],
    vanilla: &mut [f64],
    rebate: f64,
    live: std::ops::Range<usize>,
    rollback: impl Fn(f64, f64) -> f64,
) -> Result<()> {
    let count = values.len() - 1;
    let pass = |nodes: &mut [f64]| {
        for i in 0..nodes.len() - 1 {
            nodes[i] = rollback(nodes[i], nodes[i + 1]);
        }
    };
    pass(&mut values[live.start..=live.end]);
    if vanilla.is_empty() {
        values[..live.start].fill(rebate);
        values[live.end..count].fill(rebate);
    } else {
        pass(&mut vanilla[..=count]);
        values[..live.start].copy_from_slice(&vanilla[..live.start]);
        values[live.end..count].copy_from_slice(&vanilla[live.end..count]);
    }
    if values[..count]
        .iter()
        .chain(vanilla.iter().take(count))
        .any(|value| !value.is_finite())
    {
        return Err(unstable());
    }
    Ok(())
}

impl PricingEngine<BarrierOption> for CoxRossRubinsteinBarrierEngine {
    fn greek_method(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<BarrierOption>>::validate_applicability(self, context)?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            if crate::engines::deterministic::enabled(context) {
                crate::GreekMethod::NativeWithSpotControl
            } else {
                crate::GreekMethod::Native
            },
        ))
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.natural_bsm_parameters()?;
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.greeks_with_settings(option, context, request, settings)
    }
    fn prepare_implied_volatility(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        settings: ImpliedVolatilitySettings,
    ) -> Result<ImpliedVolatilitySettings> {
        prepare_implied(option, context, self.settings, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_gap_tails_match_full_rollback_for_narrow_and_one_sided_payoffs() -> Result<()> {
        let steps = 512;
        let level = 128;
        let gap = steps - level;
        let probability = 0.42;
        let width = 0.025;
        let spot = 100.0;
        let strike = 101.0;
        let rebate = 2.0;
        let discount = super::super::StepDiscount {
            factor: (-0.001_f64).exp(),
            log_factor: -0.001,
        };
        let mut work = GAP_ROLLBACK_BUDGET;
        let terminal = TerminalGap::new(gap, probability, width, &mut work)?;
        for (sign, paying, rebate_nodes) in [
            (1.0, 257..330, 330..513),
            (-1.0, 160..257, 0..160),
            (1.0, 257..258, 0..0),
            (-1.0, 0..257, 0..0),
        ] {
            let mut values: Vec<_> = (0..=steps)
                .map(|i| {
                    if paying.contains(&i) {
                        let spot =
                            scaled(spot, f64::from(2 * i32::try_from(i).unwrap() - 512) * width);
                        (sign * (spot - strike)).max(0.0)
                    } else if rebate_nodes.contains(&i) {
                        rebate
                    } else {
                        0.0
                    }
                })
                .collect();
            let mut reference = values.clone();
            for end in (level + 1..=steps).rev() {
                rollback_level(
                    &mut reference[..=end],
                    &mut [],
                    0.0,
                    0..end,
                    |lower, upper| discount.rollback(lower, upper, probability),
                )?;
            }
            terminal.rollback(
                &mut values,
                [paying, rebate_nodes],
                [spot, strike, sign, rebate, discount.log_factor],
                &mut work,
            )?;
            for (i, (&value, &want)) in values[..=level].iter().zip(&reference).enumerate() {
                assert!(
                    (value - want).abs() <= 1e-9 * want,
                    "node {i}: {value} != {want}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn scheduled_gap_convolution_matches_single_step_rollback_in_the_tails() -> Result<()> {
        let mut prices = vec![0.0; 513];
        prices[250..300].fill(2.0);
        prices[280] = 9.0;
        let mut reference = prices.clone();
        let discount = super::super::StepDiscount {
            factor: (-0.001_f64).exp(),
            log_factor: -0.001,
        };
        let probability = 0.42;
        for end in (257..prices.len()).rev() {
            rollback_level(
                &mut reference[..=end],
                &mut [],
                0.0,
                0..end,
                |lower, upper| discount.rollback(lower, upper, probability),
            )?;
        }
        let mut work = GAP_ROLLBACK_BUDGET;
        rollback_gap(
            &mut prices,
            257,
            probability,
            discount,
            &mut FftPlanner::new(),
            &mut work,
        )?;
        for (i, (&value, &expected)) in prices[..257].iter().zip(&reference).enumerate() {
            assert!(
                (value - expected).abs() <= 1e-7 * expected,
                "node {i}: {value} vs {expected}"
            );
        }
        Ok(())
    }
}
