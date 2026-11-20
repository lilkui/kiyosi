use std::borrow::Cow;

use super::{Payoff, curve::Curve};
use crate::instruments::autocallable::pricing::{
    Initial, event_times_through, fixed_value, initial, validate,
};
use crate::instruments::{
    binary_barrier::{Contract, Settlement},
    monitoring::{Monitoring, validate_life},
};
use crate::numerics::{
    Sum, checked, checked_with_scale, discounted_product, finite_price as finite, log_ratio,
    scaled_cash, scaled_signed_cash,
};
use crate::{
    Accumulator, AutocallableNote, AutocallableTerms, BarrierOption, DoubleBarrierOption, Error,
    ModelParameters, ObservationMode, OptionContract, PricingContext, PricingContextChanges,
    RateTermStructure, RebateTiming, Result, SettlementTiming, Timestamp,
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
    Ok(clock.calendar().is_some() && clock.variance_start(context.valuation_time(), end)? == end)
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
    let model = context.model_parameters();
    let start = context.valuation_time();
    let end = option.expiry_time();
    let spot = context.spot_price();
    if payoff.is_american() {
        let risk = Curve::new(model.risk_free_curve(), start, end)?;
        let dividend = Curve::new(model.dividend_curve(), start, end)?;
        return market_exercise_value(
            payoff,
            context,
            end,
            &risk,
            &dividend,
            &market_exercise_times(context, end),
            None,
        );
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
    risk: Curve,
    dividend: Curve,
    expiry: Timestamp,
    times: Vec<Timestamp>,
    boundary: f64,
}

impl AmericanExercise {
    pub(super) fn new(payoff: Payoff, context: &PricingContext, expiry: Timestamp) -> Result<Self> {
        let model = context.model_parameters();
        let start = context.valuation_time();
        let risk = Curve::new(model.risk_free_curve(), start, expiry)?;
        let dividend = Curve::new(model.dividend_curve(), start, expiry)?;
        let times = market_exercise_times(context, expiry);
        let boundary = exercise_boundary(payoff, &risk, &dividend)?;
        Ok(Self {
            payoff,
            risk,
            dividend,
            expiry,
            times,
            boundary,
        })
    }

    pub(super) fn interval_count(&self) -> usize {
        self.times.len()
    }

    pub(super) fn price(&self, context: &PricingContext) -> Result<f64> {
        market_exercise_value(
            self.payoff,
            context,
            self.expiry,
            &self.risk,
            &self.dividend,
            &self.times,
            None,
        )
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
        let mut best = 0.0;
        let mut best_log_delta = 0.0;
        let mut alternative = 0.0;
        let mut observe = |value, log_delta: f64| {
            let distinct = (log_delta - best_log_delta).abs()
                > 4.0 * f64::EPSILON * log_delta.abs().max(best_log_delta.abs()).max(1.0);
            if value > best {
                if distinct {
                    alternative = best;
                }
                best = value;
                best_log_delta = log_delta;
            } else if distinct {
                alternative = f64::max(alternative, value);
            }
        };
        let price = market_exercise_value(
            self.payoff,
            context,
            self.expiry,
            &self.risk,
            &self.dividend,
            &self.times,
            Some(&mut observe),
        )?;
        Ok(alternative > 0.0 && price - alternative <= 4.0 * f64::EPSILON * price)
    }
}

fn market_exercise_times(context: &PricingContext, expiry: Timestamp) -> Vec<Timestamp> {
    let start = context.valuation_time();
    let model = context.model_parameters();
    let mut times: Vec<_> = [model.risk_free_curve(), model.dividend_curve()]
        .into_iter()
        .flat_map(|curve| {
            curve
                .nodes_after(start)
                .iter()
                .copied()
                .take_while(|&t| t < expiry)
        })
        .collect();
    times.sort_unstable();
    times.dedup();
    times.push(expiry);
    times
}

fn market_exercise_value(
    payoff: Payoff,
    context: &PricingContext,
    expiry: Timestamp,
    risk: &Curve,
    dividend: &Curve,
    times: &[Timestamp],
    observe: Option<&mut dyn FnMut(f64, f64)>,
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
pub(super) fn exercise_value(
    payoff: Payoff,
    spot: f64,
    start: f64,
    end: f64,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    let knots = if payoff.is_american() && start < end && (spot > 0.0 || payoff.sign < 0.0) {
        exercise_knots(start, end, risk, dividend)
    } else {
        Vec::new()
    };
    exercise_value_on_knots(
        payoff,
        spot,
        start,
        end,
        [risk, dividend],
        knots.into_iter().chain([end]).map(|time| (time, None)),
        None,
    )
}

fn exercise_knots(start: f64, end: f64, risk: &Curve, dividend: &Curve) -> Vec<f64> {
    let mut knots: Vec<_> = risk
        .ends_between(start, end)
        .chain(dividend.ends_between(start, end))
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    knots
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
    mut observe: Option<&mut dyn FnMut(f64, f64)>,
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
            observe(best, 0.0);
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
                    if payoff.sign * r.signum() * (q - r) < 0.0 {
                        observe(value, d);
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
                observe(value, d);
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
    pub spot: f64,
    pub log_discount: f64,
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
    let mut advance = forward_factors(context);
    if monitoring.observation_mode() == ObservationMode::Scheduled {
        let times = monitoring.observation_times();
        let first = times.partition_point(|&time| time <= start);
        for &time in times[first..].iter().take_while(|&&time| time <= end) {
            let (value, log_discount) = advance(time)?;
            if let Some((level, distance)) = hit(time, value) {
                return Ok(Some(Touch {
                    spot: if distance == 0.0 {
                        level
                    } else {
                        spot_after_carry(initial, value)?
                    },
                    log_discount,
                }));
            }
        }
        return Ok(None);
    }
    let times = continuous_knots(context, end);
    for pair in times.windows(2) {
        let (x, a) = advance(pair[0])?;
        if let Some((level, distance)) = hit(pair[0], x) {
            return Ok(Some(Touch {
                spot: if distance == 0.0 {
                    level
                } else {
                    spot_after_carry(initial, x)?
                },
                log_discount: a,
            }));
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
                spot: level,
                log_discount,
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
                return checked(scaled_cash(
                    contract.payout.unwrap_or(touch.spot),
                    touch.log_discount,
                ));
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
    validate(note, context)?;
    let state = autocallable_through(note, context, note.expiry_time())?;
    finite(state.settlement.ok_or_else(invalid)?)
}

pub(super) fn autocallable_through<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    horizon: Timestamp,
) -> Result<Initial> {
    let mut state = initial(note, context);
    if state.settlement.is_some() || horizon == context.valuation_time() {
        return Ok(state);
    }
    if horizon == note.expiry_time() {
        if let Some(value) = fixed_value(note, context, state.knocked_in) {
            state.settlement = Some(finite(state.coupon + value)?);
            return Ok(state);
        }
    }
    let daily = note.view().daily_knock_in();
    let mut next = note
        .events()
        .partition_point(|record| record.time() <= context.valuation_time());
    let mut forward_to = forward_factors(context);
    for time in event_times_through(note, context, daily, horizon)? {
        let daily_fixing = daily && note.daily_time(time, note.calendar());
        let record = note
            .events()
            .get(next)
            .filter(|record| record.time() == time);
        if !daily_fixing && record.is_none() && time != note.expiry_time() {
            continue;
        }
        let (carry, log_discount) = forward_to(time)?;
        let initial = context.spot_price();
        let spot = spot_after_carry(initial, carry)?;
        let compare = |level| context.forward_spot_cmp(time, initial, level, carry);
        let below_knock_in = || {
            note.view()
                .knock_in
                .is_some_and(|ki| compare(ki.level).is_some_and(std::cmp::Ordering::is_lt))
        };
        if daily_fixing {
            state.knocked_in |= below_knock_in();
        }
        if let Some(record) = record {
            next += 1;
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
    let (mut quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return Ok((quantity, Some(finite(value)?)));
    }
    if end == context.valuation_time() {
        return Ok((quantity, None));
    }
    let initial = context.spot_price();
    let strike_distance = log_ratio(initial, option.strike());
    let mut forward_to = forward_factors(context);
    for time in option.future_observation_times_through(context, end)? {
        let (carry, log_discount) = forward_to(time)?;
        let spot = spot_after_carry(initial, carry)?;
        let knocked_out = option.observes_knock_out(time)
            && context
                .forward_spot_cmp(time, initial, option.knock_out_level(), carry)
                .is_some_and(|ordering| match option.option_type() {
                    crate::OptionType::Call => ordering.is_ge(),
                    crate::OptionType::Put => ordering.is_le(),
                });
        if !knocked_out && option.accumulates_at(time, option.calendar()) {
            let distance = quoted_spot_distance(context, time, option.strike())
                .unwrap_or(strike_distance + carry);
            quantity += option.purchase_from_strike_distance(distance);
        }
        if knocked_out || time == option.expiry_time() {
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
                    discounted_product(quantity, unit_payoff, log_discount)
                } else {
                    discounted_product(quantity, difference, strike.ln() + log_discount)
                }
            } else {
                discounted_product(quantity, option.unit_payoff(spot), log_discount)
            };
            return Ok((quantity, Some(finite(value)?)));
        }
    }
    Ok((quantity, None))
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
