use std::borrow::Cow;

use super::{Payoff, curve::Curve};
use crate::instruments::autocallable::pricing::{
    Initial, event_times_through, fixed_value, initial, validate_with_work_check,
};
use crate::instruments::{
    autocallable::ObservationEvent,
    binary_barrier::{Contract, Settlement},
    monitoring::{Monitoring, validate_life},
};
use crate::numerics::{
    Sum, checked, checked_with_scale, discounted_product, finite_price as finite, log_ratio,
    scaled_cash, scaled_signed_cash,
};
use crate::{
    Accumulator, BarrierOption, DoubleBarrierOption, Error, ModelParameters, ObservationMode,
    PricingContext, PricingContextChanges, RateTermStructure, RebateTiming, Result,
    SettlementTiming, Timestamp,
    instruments::{AutocallableNote, AutocallableTerms, OptionContract},
};

pub(super) fn enabled(context: &PricingContext) -> bool {
    matches!(context.model_parameters(), ModelParameters::BlackScholesMerton(model) if model.volatility() == 0.0)
}

pub(crate) fn enabled_until(context: &PricingContext, end: Timestamp) -> Result<bool> {
    if enabled(context) {
        return Ok(true);
    }
    if end < context.valuation_time() {
        return Ok(false);
    }
    let clock = context.model_parameters().variance_time_convention();
    Ok(
        !clock.uses_actual365_clock()
            && clock.variance_start(context.valuation_time(), end)? == end,
    )
}

fn invalid() -> Error {
    Error::ComputationFailure("deterministic valuation is not representable".into())
}

pub(super) fn vanilla(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
) -> Result<f64> {
    validate_life(option, context)?;
    vanilla_to_expiry(payoff, context, option.expiry_time())
}

pub(super) fn vanilla_to_expiry(
    payoff: Payoff,
    context: &PricingContext,
    end: Timestamp,
) -> Result<f64> {
    let model = context.model_parameters();
    let start = context.valuation_time();
    let spot = context.spot_price();
    if payoff.is_american() {
        return AmericanExerciseMarket::new(context, end)?.price(payoff, context);
    }
    context.validate_curve_coverage(end)?;
    let risk_log = model.risk_free_curve().log_factor(start, end)?;
    let dividend_log = model.dividend_curve().log_factor(start, end)?;
    let carry = dividend_log - risk_log;
    if payoff.is_digital() {
        let forward = quoted_spot_distance(context, end, payoff.strike)
            .unwrap_or_else(|| log_ratio(spot, payoff.strike) + carry);
        return checked(if payoff.sign * forward <= 0.0 {
            0.0
        } else if let Some(payout) = payoff.payout() {
            scaled_cash(payout, risk_log)
        } else {
            scaled_cash(spot, dividend_log)
        });
    }
    let moneyness = context.log_forward_moneyness(end, payoff.strike, carry);
    discounted_intrinsic(payoff, spot, dividend_log, risk_log, moneyness)
}

pub(crate) fn american_exercise_discontinuity(
    option: &crate::AmericanOption,
    context: &PricingContext,
    radius: f64,
) -> Result<bool> {
    let payoff = Payoff::vanilla(option.option_type(), option.strike(), true);
    AmericanExercise::new(payoff, context, option.expiry_time())?
        .at_spot_discontinuity(context, radius)
}

fn exercise_boundary(payoff: Payoff, risk: &Curve, dividend: &Curve) -> Result<f64> {
    // The most favorable carry occurs at a knot and sets the first paying spot.
    let best = if payoff.sign > 0.0 {
        f64::max
    } else {
        f64::min
    };
    let mut carry = 0.0;
    for t in risk.ends().chain(dividend.ends()) {
        crate::cancellation::check()?;
        carry = best(carry, dividend.log_factor(0.0, t) - risk.log_factor(0.0, t));
    }
    Ok(scaled_cash(payoff.strike, -carry))
}

#[derive(PartialEq)]
pub(super) struct AmericanExercise {
    payoff: Payoff,
    market: AmericanExerciseMarket,
    boundary: f64,
}

#[derive(PartialEq)]
pub(super) struct AmericanExerciseMarket {
    risk: Curve,
    dividend: Curve,
    expiry: Timestamp,
    times: Vec<Timestamp>,
}

impl AmericanExerciseMarket {
    pub(super) fn estimate_intervals(context: &PricingContext, expiry: Timestamp) -> u64 {
        let model = context.model_parameters();
        let start = context.valuation_time();
        Curve::interval_count(model.risk_free_curve(), start, expiry)
            + Curve::interval_count(model.dividend_curve(), start, expiry)
            - 1
    }

    pub(super) fn new(context: &PricingContext, expiry: Timestamp) -> Result<Self> {
        let model = context.model_parameters();
        let start = context.valuation_time();
        let risk = Curve::new(model.risk_free_curve(), start, expiry)?;
        let dividend = Curve::new(model.dividend_curve(), start, expiry)?;
        let times = market_exercise_times(context, expiry)?;
        Ok(Self {
            risk,
            dividend,
            expiry,
            times,
        })
    }

    pub(super) fn interval_count(&self) -> usize {
        self.times.len()
    }

    pub(super) fn price(&self, payoff: Payoff, context: &PricingContext) -> Result<f64> {
        self.evaluate(payoff, context, None)
    }

    fn evaluate(
        &self,
        payoff: Payoff,
        context: &PricingContext,
        observe: Option<&mut dyn FnMut(f64, f64, f64)>,
    ) -> Result<f64> {
        market_exercise_value(
            payoff,
            context,
            self.expiry,
            &self.risk,
            &self.dividend,
            &self.times,
            observe,
        )
    }
}

impl AmericanExercise {
    pub(super) fn new(payoff: Payoff, context: &PricingContext, expiry: Timestamp) -> Result<Self> {
        let market = AmericanExerciseMarket::new(context, expiry)?;
        let boundary = exercise_boundary(payoff, &market.risk, &market.dividend)?;
        Ok(Self {
            payoff,
            market,
            boundary,
        })
    }

    pub(super) fn interval_count(&self) -> usize {
        self.market.interval_count()
    }

    pub(super) fn price(&self, context: &PricingContext) -> Result<f64> {
        self.market.price(self.payoff, context)
    }

    pub(super) fn at_spot_discontinuity(
        &self,
        context: &PricingContext,
        radius: f64,
    ) -> Result<bool> {
        let spot = context.spot_price();
        if spot - radius <= self.boundary && self.boundary <= spot + radius {
            return Ok(true);
        }
        if radius == 0.0 {
            return self.exercise_at(context).map(|(_, _, tied)| tied);
        }
        let at_spot = |spot| {
            context.replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
        };
        let (low_time, low_delta, low_tied) = self.exercise_at(&at_spot(spot - radius)?)?;
        let (high_time, high_delta, high_tied) = self.exercise_at(&at_spot(spot + radius)?)?;
        if low_tied || high_tied {
            return Ok(true);
        }
        if !distinct_exercise_slopes(low_delta, high_delta) {
            return Ok(false);
        }
        let (from, to) = (low_time.min(high_time), low_time.max(high_time));
        let middle = from.midpoint(to);
        let r = self.market.risk.rate(middle);
        let q = self.market.dividend.rate(middle);
        // A stationary optimum can move smoothly inside a single rate interval.
        // Across different intervals the stencil can mix distinct exercise branches.
        if !stationary_exercise_maximum(self.payoff, r, q)
            || self.market.risk.ends_between(from, to).next().is_some()
            || self.market.dividend.ends_between(from, to).next().is_some()
        {
            return Ok(true);
        }
        // With monotone dividend discounts, exercise times outside this interval
        // cannot have slopes between the two winners and take over inside the bump.
        let mut start = 0.0;
        for end in self.market.dividend.ends() {
            crate::cancellation::check()?;
            if q.signum() * self.market.dividend.rate(start) < 0.0 {
                return Ok(true);
            }
            start = end;
        }
        Ok(false)
    }

    fn exercise_at(&self, context: &PricingContext) -> Result<(f64, f64, bool)> {
        let (mut best, mut best_time, mut best_log_delta) = (0.0, 0.0, 0.0);
        let mut alternative = 0.0;
        let mut observe = |value, log_delta, time| {
            let distinct = distinct_exercise_slopes(log_delta, best_log_delta);
            if value > best {
                if distinct {
                    alternative = best;
                }
                best = value;
                best_time = time;
                best_log_delta = log_delta;
            } else if distinct {
                alternative = f64::max(alternative, value);
            }
        };
        let price = self
            .market
            .evaluate(self.payoff, context, Some(&mut observe))?;
        let tied = alternative > 0.0 && price - alternative <= 4.0 * f64::EPSILON * price;
        Ok((best_time, best_log_delta, tied))
    }
}

fn distinct_exercise_slopes(left: f64, right: f64) -> bool {
    (left - right).abs() > 4.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
}

fn stationary_exercise_maximum(payoff: Payoff, r: f64, q: f64) -> bool {
    r != 0.0
        && q != 0.0
        && r.signum() * q.signum() > 0.0
        && payoff.sign * r.signum() * (q - r) < 0.0
}

fn market_exercise_times(context: &PricingContext, expiry: Timestamp) -> Result<Vec<Timestamp>> {
    let start = context.valuation_time();
    let model = context.model_parameters();
    let mut times = Vec::new();
    for curve in [model.risk_free_curve(), model.dividend_curve()] {
        let nodes = curve.nodes_after(start);
        let end = nodes.partition_point(|&node| node < expiry);
        for chunk in nodes[..end].chunks(1024) {
            crate::cancellation::check()?;
            times.extend_from_slice(chunk);
        }
    }
    times.sort_unstable();
    times.dedup();
    times.push(expiry);
    Ok(times)
}

fn market_exercise_value(
    payoff: Payoff,
    context: &PricingContext,
    expiry: Timestamp,
    risk: &Curve,
    dividend: &Curve,
    times: &[Timestamp],
    observe: Option<&mut dyn FnMut(f64, f64, f64)>,
) -> Result<f64> {
    let start = context.valuation_time();
    let spot = context.spot_price();
    if start == expiry {
        return Ok(payoff.value(spot));
    }
    let model = context.model_parameters();
    let quoted_start = [model.risk_free_curve(), model.dividend_curve()]
        .iter()
        .all(|curve| curve.unshifted_factor_quotes(start, start).is_some());
    let knots = times.iter().map(|&time| {
        let moneyness = if quoted_start {
            context.quoted_log_forward_moneyness(time, payoff.strike)
        } else {
            None
        };
        (start.years_until(time), moneyness)
    });
    exercise_value_on_knots(
        payoff,
        spot,
        0.0,
        start.years_until(expiry),
        [risk, dividend],
        knots,
        observe,
    )
}

fn discounted_intrinsic(
    payoff: Payoff,
    spot: f64,
    dividend_log: f64,
    risk_log: f64,
    moneyness: f64,
) -> Result<f64> {
    let price = if spot == 0.0 {
        if payoff.sign < 0.0 {
            scaled_cash(payoff.strike, risk_log)
        } else {
            0.0
        }
    } else if payoff.strike == 0.0 {
        if payoff.sign > 0.0 {
            scaled_cash(spot, dividend_log)
        } else {
            0.0
        }
    } else {
        if moneyness.is_nan() {
            return Err(invalid());
        }
        if payoff.sign * moneyness <= 0.0 {
            0.0
        } else if payoff.sign > 0.0 {
            scaled_cash(spot, dividend_log + (-(-moneyness).exp_m1()).ln())
        } else {
            scaled_cash(payoff.strike, risk_log + (-moneyness.exp_m1()).ln())
        }
    };
    checked_with_scale(price, 0.0)
}

// Discounted deterministic intrinsic values also supply the far-asset asymptote.
pub(super) struct ExerciseBoundaries<'a> {
    payoff: Payoff,
    assets: [f64; 2],
    end: f64,
    curves: [&'a Curve; 2],
    knots: Vec<f64>,
    rules: [BoundaryRule; 2],
    envelope: Option<KnotEnvelope>,
}

#[derive(Clone, Copy)]
enum BoundaryRule {
    Zero,
    Immediate,
    Expiry,
    Scan,
}

impl<'a> ExerciseBoundaries<'a> {
    pub fn new(
        payoff: Payoff,
        assets: [f64; 2],
        end: f64,
        [risk, dividend]: [&'a Curve; 2],
        steps: usize,
    ) -> Result<Self> {
        let curves = [risk, dividend];
        let mut knots = Vec::new();
        let mut rules = [BoundaryRule::Expiry; 2];
        let mut envelope = None;
        if payoff.is_american() {
            let mut carry = [0.0_f64; 2];
            for t in risk.ends().chain(dividend.ends()) {
                crate::cancellation::check()?;
                let log_carry = dividend.log_factor(0.0, t) - risk.log_factor(0.0, t);
                if !log_carry.is_finite() {
                    carry = [f64::NEG_INFINITY, f64::INFINITY];
                    break;
                }
                carry[0] = carry[0].min(log_carry);
                carry[1] = carry[1].max(log_carry);
            }
            let [r, q] = curves.map(Curve::rate_bounds);
            let thresholds = if r[0] > 0.0 && q[0] > 0.0 {
                [log_ratio(r[0], q[1]), log_ratio(r[1], q[0])]
            } else {
                [f64::NEG_INFINITY, f64::INFINITY]
            };
            let width = carry[1] - carry[0];
            rules = assets.map(|asset| {
                let center = log_ratio(asset, payoff.strike);
                // Stay away from ties and preserve the general path for extreme discounts.
                let margin = 1e-10 * (1.0 + center.abs() + carry[0].abs() + carry[1].abs());
                let low = center - width - margin;
                let high = center + width + margin;
                let call = payoff.sign > 0.0;
                if (call && high < 0.0) || (!call && low > 0.0) {
                    BoundaryRule::Zero
                } else if (call && low > thresholds[1]) || (!call && high < thresholds[0]) {
                    BoundaryRule::Immediate
                } else if (call && high < thresholds[0]) || (!call && low > thresholds[1]) {
                    BoundaryRule::Expiry
                } else {
                    BoundaryRule::Scan
                }
            });
            let scans = rules
                .iter()
                .filter(|rule| matches!(rule, BoundaryRule::Scan))
                .count();
            if scans != 0 {
                let count = risk.ends().len() + dividend.ends().len() - 2;
                knots.extend(risk.ends_between(0.0, end));
                knots.extend(dividend.ends_between(0.0, end));
                knots.sort_unstable_by(f64::total_cmp);
                knots.dedup();
                let mut domain = [f64::INFINITY, f64::NEG_INFINITY];
                for (spot, rule) in assets.into_iter().zip(rules) {
                    if matches!(rule, BoundaryRule::Scan) {
                        let moneyness = log_ratio(spot, payoff.strike);
                        domain[0] = domain[0].min(moneyness - carry[1]);
                        domain[1] = domain[1].max(moneyness - carry[0]);
                    }
                }
                envelope = KnotEnvelope::new(payoff, domain, &knots, end, curves)?;
                if envelope.is_none()
                    && count.saturating_mul(steps).saturating_mul(scans) > 25_000_000
                {
                    return Err(crate::Error::InvalidInput(
                        "American exercise boundary exceeds 25 million curve-knot visits; reduce time steps or curve nodes".into(),
                    ));
                }
            }
        }
        Ok(Self {
            payoff,
            assets,
            end,
            curves,
            knots,
            rules,
            envelope,
        })
    }

    pub fn at(&mut self, start: f64) -> Result<[f64; 2]> {
        let [risk, dividend] = self.curves;
        let prefix = self
            .envelope
            .as_mut()
            .map(|envelope| {
                envelope.advance(start)?;
                Ok::<_, crate::Error>([
                    risk.log_factor(0.0, start),
                    dividend.log_factor(0.0, start),
                ])
            })
            .transpose()?;
        let mut values = [0.0; 2];
        for ((value, spot), rule) in values.iter_mut().zip(self.assets).zip(self.rules) {
            *value = match rule {
                BoundaryRule::Zero => 0.0,
                BoundaryRule::Immediate => self.payoff.value(spot),
                BoundaryRule::Expiry => {
                    let r = risk.log_factor(start, self.end);
                    let d = dividend.log_factor(start, self.end);
                    discounted_intrinsic(
                        self.payoff,
                        spot,
                        d,
                        r,
                        log_ratio(spot, self.payoff.strike) + (d - r),
                    )?
                }
                BoundaryRule::Scan => {
                    if let Some((envelope, [r, q])) = self.envelope.as_ref().zip(prefix) {
                        let initial = log_ratio(spot, self.payoff.strike);
                        *value = self.payoff.value(spot);
                        if let Some([risk, dividend]) = envelope.best(initial + (r - q)) {
                            let r = risk - r;
                            let q = dividend - q;
                            let future =
                                discounted_intrinsic(self.payoff, spot, q, r, initial + (q - r))?;
                            *value = value.max(future);
                        }
                        continue;
                    }
                    let first = self.knots.partition_point(|&knot| knot <= start);
                    exercise_value_on_knots(
                        self.payoff,
                        spot,
                        start,
                        self.end,
                        self.curves,
                        self.knots[first..]
                            .iter()
                            .copied()
                            .chain([self.end])
                            .map(|time| (time, None)),
                        None,
                    )?
                }
            };
        }
        Ok(values)
    }
}

struct KnotEnvelope {
    knots: Vec<ExerciseKnot>,
    remaining: usize,
    domain: [f64; 2],
    nodes: Vec<EnvelopeNode>,
}

struct ExerciseKnot {
    time: f64,
    logs: [f64; 2],
    line: [f64; 2],
}

struct EnvelopeNode {
    knot: usize,
    children: [Option<usize>; 2],
}

impl KnotEnvelope {
    fn new(
        payoff: Payoff,
        [low, high]: [f64; 2],
        interior: &[f64],
        end: f64,
        [risk, dividend]: [&Curve; 2],
    ) -> Result<Option<Self>> {
        // Bound both workspace and the loss of precision from changing discount origin.
        if !(16..=100_000).contains(&interior.len())
            || !low.is_finite()
            || !high.is_finite()
            || low < -16.0
            || high > 16.0
            || low > high
            || [risk, dividend]
                .into_iter()
                .flat_map(Curve::rate_bounds)
                .any(|rate| rate.abs() * end > 1.0)
        {
            return Ok(None);
        }
        let mut previous = 0.0_f64;
        let mut knots = Vec::with_capacity(interior.len() + 1);
        for time in interior.iter().copied().chain([end]) {
            crate::cancellation::check()?;
            let middle = previous.midpoint(time);
            // With no interior maximum, every future optimum is at a knot or at start.
            if stationary_exercise_maximum(payoff, risk.rate(middle), dividend.rate(middle)) {
                return Ok(None);
            }
            let logs = [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)];
            knots.push(ExerciseKnot {
                time,
                logs,
                line: [payoff.sign * logs[1].exp(), -payoff.sign * logs[0].exp()],
            });
            previous = time;
        }
        let margin = 64.0 * f64::EPSILON * (1.0 + low.abs() + high.abs());
        Ok(Some(Self {
            remaining: knots.len(),
            knots,
            domain: [low - margin, high + margin],
            nodes: Vec::new(),
        }))
    }

    fn value(&self, knot: usize, spot_ratio: f64) -> f64 {
        let [slope, intercept] = self.knots[knot].line;
        spot_ratio.mul_add(slope, intercept)
    }

    fn advance(&mut self, start: f64) -> Result<()> {
        while self.remaining > 0 && self.knots[self.remaining - 1].time > start {
            crate::cancellation::check()?;
            self.remaining -= 1;
            self.insert(self.remaining);
        }
        Ok(())
    }

    // Two payoff lines cross at most once. Keep the middle winner at the
    // current node and descend only into the half where the other can win.
    fn insert(&mut self, mut knot: usize) {
        if self.nodes.is_empty() {
            self.nodes.push(EnvelopeNode {
                knot,
                children: [None; 2],
            });
            return;
        }
        let mut node = 0;
        let [mut low, mut high] = self.domain;
        for _ in 0..64 {
            let middle = low.midpoint(high);
            let incumbent = self.nodes[node].knot;
            let [left_ratio, middle_ratio, right_ratio] = [low, middle, high].map(f64::exp);
            let wins_left = self.value(knot, left_ratio) > self.value(incumbent, left_ratio);
            let wins_middle = self.value(knot, middle_ratio) > self.value(incumbent, middle_ratio);
            let wins_right = self.value(knot, right_ratio) > self.value(incumbent, right_ratio);
            if wins_middle {
                std::mem::swap(&mut knot, &mut self.nodes[node].knot);
            }
            let side = if wins_left != wins_middle {
                high = middle;
                0
            } else if wins_right != wins_middle {
                low = middle;
                1
            } else {
                return;
            };
            if let Some(child) = self.nodes[node].children[side] {
                node = child;
            } else {
                self.nodes[node].children[side] = Some(self.nodes.len());
                self.nodes.push(EnvelopeNode {
                    knot,
                    children: [None; 2],
                });
                return;
            }
        }
    }

    fn best(&self, moneyness: f64) -> Option<[f64; 2]> {
        let mut node = self.nodes.first()?;
        let [mut low, mut high] = self.domain;
        let mut best = node.knot;
        let spot_ratio = moneyness.exp();
        let mut value = self.value(best, spot_ratio);
        loop {
            let middle = low.midpoint(high);
            let side = if moneyness < middle {
                high = middle;
                0
            } else {
                low = middle;
                1
            };
            let Some(child) = node.children[side] else {
                break;
            };
            node = &self.nodes[child];
            let candidate = self.value(node.knot, spot_ratio);
            if candidate > value {
                best = node.knot;
                value = candidate;
            }
        }
        Some(self.knots[best].logs)
    }
}

#[expect(
    clippy::float_cmp,
    reason = "Equal forward rates have no isolated stationary point."
)]
fn exercise_value_on_knots(
    payoff: Payoff,
    spot: f64,
    start: f64,
    end: f64,
    [risk, dividend]: [&Curve; 2],
    knots: impl Iterator<Item = (f64, Option<f64>)>,
    mut observe: Option<&mut dyn FnMut(f64, f64, f64)>,
) -> Result<f64> {
    let moneyness = |dividend_log, risk_log, quoted: Option<f64>| {
        quoted.unwrap_or_else(|| log_ratio(spot, payoff.strike) + (dividend_log - risk_log))
    };
    if !payoff.is_american() || start >= end {
        let d = dividend.log_factor(start, end);
        let r = risk.log_factor(start, end);
        return discounted_intrinsic(payoff, spot, d, r, moneyness(d, r, None));
    }
    if spot == 0.0 && payoff.sign > 0.0 {
        return Ok(0.0);
    }
    let mut best = payoff.value(spot);
    // Check only local maxima so a nearby knot on the same smooth branch cannot look like a tie.
    let time_slope = |r: f64, q: f64, moneyness: f64| {
        let slope = if r != 0.0 && q != 0.0 && r.signum() == q.signum() {
            r.signum() * (log_ratio(r.abs(), q.abs()) - moneyness)
        } else {
            r - q
        };
        payoff.sign * slope
    };
    if let Some(observe) = &mut observe {
        if time_slope(
            risk.rate(start),
            dividend.rate(start),
            moneyness(0.0, 0.0, None),
        ) <= 0.0
        {
            observe(best, 0.0, start);
        }
    }
    let mut a = start;
    let mut risk_log = Sum::default();
    let mut dividend_log = Sum::default();
    for (b, quoted) in knots {
        crate::cancellation::check()?;
        let middle = a.midpoint(b);
        let r = risk.rate(middle);
        let q = dividend.rate(middle);
        if spot > 0.0 && r != 0.0 && q != 0.0 && r.signum() == q.signum() && r != q {
            let log =
                log_ratio(r.abs(), q.abs()) + log_ratio(payoff.strike, spot) + risk_log.total()
                    - dividend_log.total();
            let t = a + log / (r - q);
            if t > a && t < b {
                let d = dividend_log.total() + dividend.log_factor(a, t);
                let r_log = risk_log.total() + risk.log_factor(a, t);
                let value =
                    discounted_intrinsic(payoff, spot, d, r_log, moneyness(d, r_log, None))?;
                best = best.max(value);
                if let Some(observe) = &mut observe {
                    if stationary_exercise_maximum(payoff, r, q) {
                        observe(value, d, t);
                    }
                }
            }
        }
        // Advance by each interval to preserve short widths without rescanning prior knots.
        risk_log.add(risk.log_factor(a, b));
        dividend_log.add(dividend.log_factor(a, b));
        let (d, r_log) = (dividend_log.total(), risk_log.total());
        let forward = moneyness(d, r_log, quoted);
        let value = discounted_intrinsic(payoff, spot, d, r_log, forward)?;
        best = best.max(value);
        if let Some(observe) = &mut observe {
            if time_slope(r, q, forward) >= 0.0
                && (b >= end || time_slope(risk.rate(b), dividend.rate(b), forward) <= 0.0)
            {
                observe(value, d, b);
            }
        }
        a = b;
    }
    Ok(best)
}

pub(super) fn log_return(context: &PricingContext, time: Timestamp) -> Result<f64> {
    finite(context.forward_carry(time)?)
}

fn spot_after_carry(spot: f64, carry: f64) -> Result<f64> {
    let value = scaled_cash(spot, carry);
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(invalid())
    }
}

pub(super) fn quoted_spot_distance(
    context: &PricingContext,
    time: Timestamp,
    level: f64,
) -> Option<f64> {
    context
        .quoted_spot_cmp(time, context.spot_price(), level)
        .map(|ordering| f64::from(ordering as i8))
}

pub(super) fn discount(context: &PricingContext, time: Timestamp) -> Result<f64> {
    context
        .model_parameters()
        .risk_free_curve()
        .log_factor(context.valuation_time(), time)
}

pub(super) fn continuation_context(
    context: &PricingContext,
    start: Timestamp,
) -> Result<(Cow<'_, PricingContext>, f64, f64)> {
    if start == context.valuation_time() {
        return Ok((Cow::Borrowed(context), 0.0, 0.0));
    }
    let carry = log_return(context, start)?;
    let discount = discount(context, start)?;
    let spot = scaled_cash(context.spot_price(), carry);
    if !spot.is_finite() || spot <= 0.0 {
        return Err(invalid());
    }
    let rolled = context.replace(PricingContextChanges {
        valuation_time: Some(start),
        spot_price: Some(spot),
        ..Default::default()
    })?;
    Ok((Cow::Owned(rolled), carry, discount))
}

pub(super) struct Touch {
    pub log_discount: f64,
    discounted_asset: f64,
}

impl Touch {
    pub(super) fn discounted_payout(&self, payout: Option<f64>) -> f64 {
        payout.map_or(self.discounted_asset, |cash| {
            scaled_cash(cash, self.log_discount)
        })
    }
}

pub(super) struct BarrierPrefix {
    pub prior: bool,
    pub touched: bool,
    pub finished: bool,
    pub hit: Option<Touch>,
    pub end: Timestamp,
}

fn monitoring_end(monitoring: &Monitoring) -> Timestamp {
    monitoring
        .observation_times()
        .last()
        .copied()
        .unwrap_or(monitoring.expiry_time())
}

fn monitoring_prefix_end(monitoring: &Monitoring, context: &PricingContext) -> Result<Timestamp> {
    let start = context.valuation_time();
    context
        .model_parameters()
        .variance_time_convention()
        .variance_start(start, monitoring_end(monitoring))
        .map(|end| end.max(start))
}

pub(super) fn barrier_prefix(
    monitoring: &Monitoring,
    upper: Option<f64>,
    context: &PricingContext,
) -> Result<BarrierPrefix> {
    let (prior, touched, finished) = monitoring.state(context, upper)?;
    let mut prefix = BarrierPrefix {
        prior,
        touched,
        finished,
        hit: None,
        end: context.valuation_time(),
    };
    if !touched && !finished {
        prefix.end = monitoring_prefix_end(monitoring, context)?;
        if prefix.end > context.valuation_time() {
            prefix.hit = first_touch(monitoring, upper, context, prefix.end)?;
            prefix.touched = prefix.hit.is_some();
            prefix.finished = prefix.end == monitoring_end(monitoring);
        }
    }
    Ok(prefix)
}

pub(super) fn barrier_probe_levels(
    monitoring: &Monitoring,
    upper: Option<f64>,
    context: &PricingContext,
    range: &std::ops::RangeInclusive<f64>,
) -> Result<Vec<f64>> {
    let level = monitoring.barrier_level();
    let mut levels = vec![level];
    levels.extend(upper);
    let start = context.valuation_time();
    if monitoring.prior(start)? {
        return Ok(levels);
    }
    let up = monitoring.barrier_type().is_up();
    if monitoring.monitored(start)
        && (up && *range.start() >= level
            || !up && *range.end() <= level
            || upper.is_some_and(|upper| *range.start() >= upper))
    {
        return Ok(levels);
    }
    let end = monitoring_prefix_end(monitoring, context)?;
    if end <= start {
        return Ok(levels);
    }
    let knots;
    let times = if monitoring.continuous() {
        knots = continuous_knots(context, end);
        &knots
    } else {
        monitoring.observation_times()
    };
    let mut forward_to = forward_factors(context);
    let mut lower = 0.0_f64;
    let mut higher = f64::INFINITY;
    let first = times.partition_point(|&time| time <= start);
    for &time in times[first..].iter().take_while(|&&time| time <= end) {
        let (carry, _) = forward_to(time)?;
        if up {
            higher = higher.min(scaled_cash(level, -carry));
        } else {
            lower = lower.max(scaled_cash(level, -carry));
            if let Some(upper) = upper {
                higher = higher.min(scaled_cash(upper, -carry));
            }
        }
        if lower >= *range.end() || higher <= *range.start() || lower >= higher {
            break;
        }
    }
    levels.extend([lower, higher]);
    Ok(levels)
}

fn continuous_knots(context: &PricingContext, end: Timestamp) -> Vec<Timestamp> {
    let start = context.valuation_time();
    let model = context.model_parameters();
    let mut times = vec![start, end];
    for curve in [model.risk_free_curve(), model.dividend_curve()] {
        let nodes = curve.nodes_after(start);
        times.extend_from_slice(&nodes[..nodes.partition_point(|&time| time < end)]);
        times.extend(curve.max_time().filter(|&time| time > start && time < end));
    }
    times.sort_unstable();
    times.dedup();
    times
}

pub(super) fn forward_factors(
    context: &PricingContext,
) -> impl FnMut(Timestamp) -> Result<(f64, f64)> + '_ {
    let model = context.model_parameters();
    let start = context.valuation_time();
    let mut previous = start;
    let [mut risk, mut dividend] = [Sum::default(), Sum::default()];
    let advance = move |curve: &RateTermStructure, total: &mut Sum, previous, time| {
        if curve.nodes().is_empty() {
            return curve.log_factor(start, time);
        }
        // Re-anchor at quoted endpoints so earlier fixings cannot move a payoff boundary.
        if let Some(quoted) = curve.quoted_log_factor(start, time) {
            *total = Sum::default();
            total.add(finite(quoted)?);
        } else {
            total.add(curve.log_factor(previous, time)?);
        }
        finite(total.total())
    };
    move |time| {
        crate::cancellation::check()?;
        let risk_log = advance(model.risk_free_curve(), &mut risk, previous, time)?;
        let dividend_log = advance(model.dividend_curve(), &mut dividend, previous, time)?;
        previous = time;
        Ok((finite(dividend_log - risk_log)?, risk_log))
    }
}

fn first_touch(
    monitoring: &Monitoring,
    upper: Option<f64>,
    context: &PricingContext,
    end: Timestamp,
) -> Result<Option<Touch>> {
    let initial = context.spot_price();
    let lower = log_ratio(monitoring.barrier_level(), initial);
    let upper_log = upper.map(|level| log_ratio(level, initial));
    let hit = |time, carry: f64| {
        let level = monitoring.barrier_level();
        let distance = quoted_spot_distance(context, time, level).unwrap_or(carry - lower);
        if monitoring.barrier_type().is_up() {
            (distance >= 0.0).then_some((level, distance))
        } else if distance <= 0.0 {
            Some((level, distance))
        } else {
            upper.zip(upper_log).and_then(|(level, log)| {
                let distance = quoted_spot_distance(context, time, level).unwrap_or(carry - log);
                (distance >= 0.0).then_some((level, distance))
            })
        }
    };
    let start = context.valuation_time();
    let fixing_touch = |time, level, distance: f64, log_discount| -> Result<Touch> {
        let discounted_asset = if distance == 0.0 {
            scaled_cash(level, log_discount)
        } else {
            // An observed asset is worth spot discounted by dividends, even if its forward overflows.
            let dividend = context
                .model_parameters()
                .dividend_curve()
                .log_factor(start, time)?;
            scaled_cash(initial, dividend)
        };
        Ok(Touch {
            log_discount,
            discounted_asset,
        })
    };
    let mut advance = forward_factors(context);
    if monitoring.observation_mode() == ObservationMode::Scheduled {
        let times = monitoring.observation_times();
        let first = times.partition_point(|&time| time <= start);
        for &time in times[first..].iter().take_while(|&&time| time <= end) {
            let (value, log_discount) = advance(time)?;
            if let Some((level, distance)) = hit(time, value) {
                return fixing_touch(time, level, distance, log_discount).map(Some);
            }
        }
        return Ok(None);
    }
    let times = continuous_knots(context, end);
    for pair in times.windows(2) {
        let (x, a) = advance(pair[0])?;
        if let Some((level, distance)) = hit(pair[0], x) {
            return fixing_touch(pair[0], level, distance, a).map(Some);
        }
        let (y, b) = advance(pair[1])?;
        if let Some((level, distance)) = hit(pair[1], y) {
            let log_discount = if distance == 0.0 || (y - x).abs() == 0.0 {
                b
            } else {
                let fraction = ((log_ratio(level, initial) - x) / (y - x)).clamp(0.0, 1.0);
                fraction.mul_add(b - a, a)
            };
            return Ok(Some(Touch {
                log_discount,
                discounted_asset: scaled_cash(level, log_discount),
            }));
        }
    }
    Ok(None)
}

pub(super) fn barrier(option: &BarrierOption, context: &PricingContext) -> Result<f64> {
    validate_life(option, context)?;
    if let Some(value) = super::analytic::barrier::resolved_price(option, context)? {
        return Ok(value);
    }
    barrier_value(option, None, context)
}

pub(super) fn double_barrier(
    option: &DoubleBarrierOption,
    context: &PricingContext,
) -> Result<f64> {
    validate_life(option, context)?;
    if let Some(value) = super::analytic::double_barrier::resolved_price(option, context)? {
        return Ok(value);
    }
    barrier_value(&option.lower, Some(option.upper_barrier()), context)
}

pub(super) fn barrier_value(
    option: &BarrierOption,
    upper: Option<f64>,
    context: &PricingContext,
) -> Result<f64> {
    let touch = first_touch(&option.monitoring, upper, context, option.expiry_time())?;
    if touch.is_some() == option.barrier_type().is_knock_in() {
        let vanilla = option.vanilla();
        return self::vanilla(
            vanilla,
            Payoff::vanilla(vanilla.option_type(), vanilla.strike(), false),
            context,
        );
    }
    if option.rebate() == 0.0 {
        return Ok(0.0);
    }
    let log_discount =
        if let Some(touch) = touch.filter(|_| option.rebate_timing() == RebateTiming::AtHit) {
            touch.log_discount
        } else {
            discount(context, option.expiry_time())?
        };
    checked(scaled_cash(option.rebate(), log_discount))
}

pub(super) fn binary(contract: &Contract<'_>, context: &PricingContext) -> Result<f64> {
    let monitoring = contract.monitoring;
    validate_life(monitoring, context)?;
    match contract.settlement(context)? {
        Settlement::Value(value) => return checked(scaled_cash(value, 0.0)),
        Settlement::AtExpiry => {}
        Settlement::Pending => {
            let touch = first_touch(
                monitoring,
                contract.upper_barrier,
                context,
                monitoring.expiry_time(),
            )?;
            if touch.is_some() != monitoring.barrier_type().is_knock_in() {
                return Ok(0.0);
            }
            if contract.timing == SettlementTiming::AtHit {
                let touch = touch.ok_or_else(invalid)?;
                return checked(touch.discounted_payout(contract.payout));
            }
        }
    }
    if let Some(digital) = contract.digital {
        return vanilla(digital, Payoff::digital(digital), context);
    }
    touch_value_at_expiry(contract, context)
}

pub(super) fn touch_value_at_expiry(
    contract: &Contract<'_>,
    context: &PricingContext,
) -> Result<f64> {
    let model = context.model_parameters();
    let start = context.valuation_time();
    let end = contract.monitoring.expiry_time();
    checked(if let Some(amount) = contract.payout {
        scaled_cash(amount, model.risk_free_curve().log_factor(start, end)?)
    } else {
        scaled_cash(
            context.spot_price(),
            model.dividend_curve().log_factor(start, end)?,
        )
    })
}

pub(super) fn autocallable<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> Result<f64> {
    autocallable_with_work_check(note, context, |_| Ok(()))
}

pub(super) fn autocallable_with_work_check<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    charge: impl Fn(u64) -> Result<()>,
) -> Result<f64> {
    validate_with_work_check(note, context, &charge)?;
    let state = autocallable_through_checked(note, context, note.expiry_time(), None, || {
        // Fixed-flow detection, schedule construction, and replay can each visit the dates.
        charge(
            3 * note.future_schedule_visits(context.valuation_time(), note.view().daily_knock_in()),
        )
    })?;
    finite(state.settlement.ok_or_else(invalid)?)
}

pub(super) fn autocallable_through<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    horizon: Timestamp,
) -> Result<Initial> {
    autocallable_through_prepared(note, context, horizon, None)
}

#[derive(Clone, Copy, PartialEq)]
struct ForwardFixing {
    time: Timestamp,
    carry: f64,
    discount: f64,
}

fn prepare_fixings<F>(fixings: Result<impl Iterator<Item = Result<F>>>) -> Result<Option<Vec<F>>> {
    let load = || {
        let fixings = fixings?;
        let mut prepared = Vec::new();
        if let Some(count) = fixings.size_hint().1 {
            // Allow room for the accumulator's two observation indices as well.
            if count.saturating_mul(size_of::<F>() + 2 * size_of::<usize>()) > 256 << 20
                || prepared.try_reserve_exact(count).is_err()
            {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
        for fixing in fixings {
            prepared.push(fixing?);
        }
        Ok(Some(prepared))
    };
    match load() {
        // A spot may settle before a bad forward factor or an uncovered date.
        // Preserve the scalar evaluation order when eager preparation fails.
        Err(error @ Error::Io(_)) => Err(error),
        Err(_) => Ok(None),
        prepared => prepared,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct AutocallableFixing {
    forward: ForwardFixing,
    daily: bool,
    record: Option<usize>,
}

fn autocallable_fixings<'a, T: AutocallableTerms>(
    note: &'a AutocallableNote<T>,
    context: &'a PricingContext,
    horizon: Timestamp,
) -> Result<impl Iterator<Item = Result<AutocallableFixing>> + 'a> {
    let daily = note.view().daily_knock_in();
    let mut next = note
        .events()
        .partition_point(|record| record.time() <= context.valuation_time());
    let mut forward_to = forward_factors(context);
    Ok(event_times_through(note, context, daily, horizon)?
        .into_iter()
        .filter_map(move |time| {
            let daily = daily && note.daily_time(time, note.calendar());
            let record = note
                .events()
                .get(next)
                .map(ObservationEvent::time)
                .filter(|&at| at == time)
                .map(|_| {
                    let index = next;
                    next += 1;
                    index
                });
            if !daily && record.is_none() && time != note.expiry_time() {
                return None;
            }
            Some(
                forward_to(time).map(|(carry, discount)| AutocallableFixing {
                    forward: ForwardFixing {
                        time,
                        carry,
                        discount,
                    },
                    daily,
                    record,
                }),
            )
        }))
}

pub(super) fn prepare_autocallable_prefix<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    horizon: Timestamp,
) -> Result<Option<Vec<AutocallableFixing>>> {
    if horizon <= context.valuation_time()
        || note.barrier_state() == Some(crate::AutocallableBarrierState::KnockedOut)
    {
        return Ok(None);
    }
    prepare_fixings(autocallable_fixings(note, context, horizon))
}

pub(super) fn autocallable_through_prepared<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    horizon: Timestamp,
    prepared: Option<&[AutocallableFixing]>,
) -> Result<Initial> {
    autocallable_through_checked(note, context, horizon, prepared, || Ok(()))
}

fn autocallable_through_checked<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    horizon: Timestamp,
    prepared: Option<&[AutocallableFixing]>,
    check_work: impl FnOnce() -> Result<()>,
) -> Result<Initial> {
    let mut state = initial(note, context);
    if state.settlement.is_some() || horizon == context.valuation_time() {
        return Ok(state);
    }
    check_work()?;
    if horizon == note.expiry_time() {
        if let Some(value) = fixed_value(note, context, state.knocked_in) {
            state.settlement = Some(finite(state.coupon + value)?);
            return Ok(state);
        }
    }
    match prepared {
        Some(fixings) => replay_autocallable(note, context, state, fixings.iter().copied().map(Ok)),
        None => replay_autocallable(
            note,
            context,
            state,
            autocallable_fixings(note, context, horizon)?,
        ),
    }
}

fn replay_autocallable<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    mut state: Initial,
    fixings: impl Iterator<Item = Result<AutocallableFixing>>,
) -> Result<Initial> {
    for fixing in fixings {
        crate::cancellation::check()?;
        let AutocallableFixing {
            forward:
                ForwardFixing {
                    time,
                    carry,
                    discount: log_discount,
                },
            daily,
            record,
        } = fixing?;
        let initial = context.spot_price();
        let spot = spot_after_carry(initial, carry)?;
        let compare = |level| context.forward_spot_cmp(time, initial, level, carry);
        let below_knock_in = || {
            note.view()
                .knock_in
                .is_some_and(|ki| compare(ki.level).is_some_and(std::cmp::Ordering::is_lt))
        };
        if daily {
            state.knocked_in |= below_knock_in();
        }
        if let Some(record) = record.map(|index| &note.events()[index]) {
            let at_or_above = |level| compare(level).is_some_and(std::cmp::Ordering::is_ge);
            let coupon = note.observation_coupon(record, at_or_above);
            if record.has_knock_out() && at_or_above(record.knock_out_level()) {
                state.settlement = Some(finite(
                    state.coupon
                        + scaled_signed_cash(note.knock_out_payment(record) + coupon, log_discount),
                )?);
                break;
            }
            state.coupon += scaled_signed_cash(coupon, log_discount);
        }
        if time == note.expiry_time() {
            state.knocked_in |= note.observes_knock_in(time) && below_knock_in();
            let terminal = note.terminal(spot, state.knocked_in);
            state.settlement = Some(finite(
                state.coupon + scaled_signed_cash(terminal, log_discount),
            )?);
        }
    }
    Ok(state)
}

pub(super) fn accumulator(option: &Accumulator, context: &PricingContext) -> Result<f64> {
    option.validate_pricing(context)?;
    accumulator_through(option, context, option.expiry_time())?
        .1
        .ok_or_else(invalid)
}

pub(super) fn accumulator_through(
    option: &Accumulator,
    context: &PricingContext,
    end: Timestamp,
) -> Result<(f64, Option<f64>)> {
    accumulator_through_prepared(option, context, end, None)
}

#[derive(Clone, Copy, PartialEq)]
struct AccumulatorFixing {
    forward: ForwardFixing,
    knock_out: bool,
    purchase: bool,
}

fn accumulator_fixings<'a>(
    option: &'a Accumulator,
    context: &'a PricingContext,
    end: Timestamp,
) -> Result<impl Iterator<Item = Result<AccumulatorFixing>> + 'a> {
    let mut forward_to = forward_factors(context);
    Ok(option
        .future_observation_times_through(context, end)?
        .into_iter()
        .map(move |time| {
            let (carry, discount) = forward_to(time)?;
            Ok(AccumulatorFixing {
                forward: ForwardFixing {
                    time,
                    carry,
                    discount,
                },
                knock_out: option.observes_knock_out(time),
                purchase: option.accumulates_at(time, option.calendar()),
            })
        }))
}

#[derive(PartialEq)]
pub(super) struct AccumulatorPrefix {
    fixings: Vec<AccumulatorFixing>,
    index: Option<AccumulatorIndex>,
    accrues: bool,
}

#[derive(PartialEq)]
struct AccumulatorIndex {
    knock_outs: Vec<usize>,
    purchases: Vec<usize>,
    increasing: bool,
}

pub(super) fn prepare_accumulator_prefix(
    option: &Accumulator,
    context: &PricingContext,
    end: Timestamp,
) -> Result<Option<AccumulatorPrefix>> {
    if end <= context.valuation_time() || option.knock_out_settled() == Some(true) {
        return Ok(None);
    }
    let Some(fixings) = prepare_fixings(accumulator_fixings(option, context, end))? else {
        return Ok(None);
    };
    let model = context.model_parameters();
    let flat = [model.risk_free_curve(), model.dividend_curve()]
        .iter()
        .all(|curve| curve.nodes().is_empty() && curve.exact_flat_rate().is_some());
    let mut index = None;
    if flat {
        let mut increasing = true;
        let mut decreasing = true;
        let mut knock_outs = Vec::new();
        let mut purchases = Vec::new();
        let mut previous = None;
        for (i, fixing) in fixings.iter().enumerate() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let carry = fixing.forward.carry;
            if let Some(prior) = previous {
                increasing &= prior <= carry;
                decreasing &= prior >= carry;
            }
            previous = Some(carry);
            if fixing.knock_out {
                knock_outs.push(i);
            }
            if fixing.purchase {
                purchases.push(i);
            }
        }
        if increasing || decreasing {
            index = Some(AccumulatorIndex {
                knock_outs,
                purchases,
                increasing,
            });
        }
    }
    let accrues = option.accrues(context);
    crate::cancellation::check()?;
    Ok(Some(AccumulatorPrefix {
        fixings,
        index,
        accrues,
    }))
}

pub(super) fn accumulator_through_prepared(
    option: &Accumulator,
    context: &PricingContext,
    end: Timestamp,
    prepared: Option<&AccumulatorPrefix>,
) -> Result<(f64, Option<f64>)> {
    let (quantity, settlement) = option.initial_with_accrual(context, || {
        prepared.map_or_else(|| option.accrues(context), |prefix| prefix.accrues)
    });
    if let Some(value) = settlement {
        return Ok((quantity, Some(finite(value)?)));
    }
    if end == context.valuation_time() {
        return Ok((quantity, None));
    }
    match prepared {
        Some(prefix) => {
            if let Some(settlement) = prefix.indexed(option, context, quantity) {
                settlement
            } else {
                replay_accumulator(
                    option,
                    context,
                    quantity,
                    prefix.fixings.iter().copied().map(Ok),
                )
            }
        }
        None => replay_accumulator(
            option,
            context,
            quantity,
            accumulator_fixings(option, context, end)?,
        ),
    }
}

impl AccumulatorPrefix {
    #[expect(
        clippy::cast_precision_loss,
        reason = "The bounded fixing arrays have far fewer than 2^53 events."
    )]
    fn indexed(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        quantity: f64,
    ) -> Option<Result<(f64, Option<f64>)>> {
        let index = self.index.as_ref()?;
        let first = self.fixings.first()?.forward;
        let last = self.fixings.last()?.forward;
        let spot = context.spot_price();
        // Every intermediate spot is representable when both monotone endpoints are.
        spot_after_carry(spot, first.carry).ok()?;
        spot_after_carry(spot, last.carry).ok()?;
        let call = option.option_type() == crate::OptionType::Call;
        let forward_increasing = call == index.increasing;
        let hit = |i: usize| accumulator_knocks_out(option, context, self.fixings[i].forward);
        let knock_out = if forward_increasing {
            index
                .knock_outs
                .get(index.knock_outs.partition_point(|&i| !hit(i)))
                .copied()
        } else {
            index.knock_outs.first().copied().filter(|&i| hit(i))
        };
        let purchases = &index.purchases[..index
            .purchases
            .partition_point(|&i| knock_out.is_none_or(|stop| i < stop))];
        let strike_distance = log_ratio(spot, option.strike());
        let accelerated = |i: usize| {
            let fixing = self.fixings[i].forward;
            let distance = quoted_spot_distance(context, fixing.time, option.strike())
                .unwrap_or(strike_distance + fixing.carry);
            if call { distance < 0.0 } else { distance > 0.0 }
        };
        let count = if forward_increasing {
            purchases.partition_point(|&i| accelerated(i))
        } else {
            purchases.len() - purchases.partition_point(|&i| !accelerated(i))
        };
        let mut quantity = quantity + (purchases.len() - count) as f64 * option.daily_quantity();
        if count != 0 {
            quantity += count as f64 * (option.daily_quantity() * option.acceleration_factor());
        }
        // The scalar addition order governs behavior near the overflow boundary.
        if !quantity.is_finite() || quantity > f64::MAX / 2.0 {
            return None;
        }
        let settlement = knock_out
            .map(|i| self.fixings[i].forward)
            .or_else(|| (last.time == option.expiry_time()).then_some(last));
        Some(settlement.map_or(Ok((quantity, None)), |fixing| {
            let spot = spot_after_carry(spot, fixing.carry)?;
            Ok((
                quantity,
                Some(accumulator_settlement(
                    option, context, fixing, spot, quantity,
                )?),
            ))
        }))
    }
}

fn accumulator_knocks_out(
    option: &Accumulator,
    context: &PricingContext,
    fixing: ForwardFixing,
) -> bool {
    context
        .forward_spot_cmp(
            fixing.time,
            context.spot_price(),
            option.knock_out_level(),
            fixing.carry,
        )
        .is_some_and(|ordering| match option.option_type() {
            crate::OptionType::Call => ordering.is_ge(),
            crate::OptionType::Put => ordering.is_le(),
        })
}

fn replay_accumulator(
    option: &Accumulator,
    context: &PricingContext,
    mut quantity: f64,
    fixings: impl Iterator<Item = Result<AccumulatorFixing>>,
) -> Result<(f64, Option<f64>)> {
    let initial = context.spot_price();
    let strike_distance = log_ratio(initial, option.strike());
    for fixing in fixings {
        crate::cancellation::check()?;
        let AccumulatorFixing {
            forward,
            knock_out,
            purchase,
        } = fixing?;
        let spot = spot_after_carry(initial, forward.carry)?;
        let knocked_out = knock_out && accumulator_knocks_out(option, context, forward);
        if !knocked_out && purchase {
            let distance = quoted_spot_distance(context, forward.time, option.strike())
                .unwrap_or(strike_distance + forward.carry);
            quantity += option.purchase_from_strike_distance(distance);
        }
        if knocked_out || forward.time == option.expiry_time() {
            return Ok((
                quantity,
                Some(accumulator_settlement(
                    option, context, forward, spot, quantity,
                )?),
            ));
        }
    }
    Ok((quantity, None))
}

fn accumulator_settlement(
    option: &Accumulator,
    context: &PricingContext,
    fixing: ForwardFixing,
    spot: f64,
    quantity: f64,
) -> Result<f64> {
    let ForwardFixing {
        time,
        carry,
        discount,
    } = fixing;
    let strike = option.strike();
    let distance = context.log_forward_moneyness(time, strike, carry);
    let unchanged_spot = carry == 0.0
        && context
            .quoted_spot_cmp(time, 1.0, 1.0)
            .is_none_or(std::cmp::Ordering::is_eq);
    let value = if !unchanged_spot && distance.abs() < 0.5 {
        let difference = match option.option_type() {
            crate::OptionType::Call => distance.exp_m1(),
            crate::OptionType::Put => -distance.exp_m1(),
        };
        let unit_payoff = strike * difference;
        if unit_payoff.is_normal() {
            discounted_product(quantity, unit_payoff, discount)
        } else {
            discounted_product(quantity, difference, strike.ln() + discount)
        }
    } else {
        discounted_product(quantity, option.unit_payoff(spot), discount)
    };
    finite(value)
}

pub(super) fn average(
    terms: &crate::instruments::asian::AverageTerms,
    context: &PricingContext,
    geometric: bool,
) -> Result<f64> {
    let tau = terms.validate(context)?;
    if tau == 0.0 {
        return terms.settlement(context);
    }
    if terms.period() == 0.0 {
        let option = &terms.terms;
        return vanilla(
            option,
            Payoff::vanilla(option.option_type(), option.strike(), false),
            context,
        );
    }
    terms.coverage(context)?;
    let average = terms.deterministic_average(context, geometric)?;
    checked(scaled_cash(
        (terms.sign() * (average - terms.terms.strike())).max(0.0),
        discount(context, terms.terms.expiry_time())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Date, OptionType, RateTermStructure};

    #[test]
    fn indexed_exercise_edges_match_the_full_future_knot_scan() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(320)?;
        let dates = (1..=32)
            .map(|i| start.add_days(i * 10))
            .collect::<Result<Vec<_>>>()?;
        let factors = (1..=32)
            .map(|i| (-0.001 * f64::from(i) - 0.002 * f64::from(i % 2)).exp())
            .collect();
        let risk = Curve::new(&RateTermStructure::new(start, dates, factors)?, start, end)?;
        let dividend = Curve::new(&RateTermStructure::flat(0.02, None)?, start, end)?;
        let end = start.years_until(end);
        let payoff = Payoff::vanilla(OptionType::Put, 100.0, true);
        let mut indexed =
            ExerciseBoundaries::new(payoff, [95.0, 105.0], end, [&risk, &dividend], 100)?;
        let mut scan =
            ExerciseBoundaries::new(payoff, [95.0, 105.0], end, [&risk, &dividend], 100)?;
        assert!(
            indexed.envelope.is_some(),
            "the rate segments have no interior maxima"
        );
        scan.envelope = None;
        for i in (0..=100).rev() {
            let t = end * f64::from(i) / 100.0;
            for (actual, expected) in indexed.at(t)?.into_iter().zip(scan.at(t)?) {
                assert!(
                    (actual - expected).abs() < 5e-13,
                    "{t}: {actual} != {expected}"
                );
            }
        }
        Ok(())
    }
}
