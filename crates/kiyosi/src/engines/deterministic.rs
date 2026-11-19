use std::borrow::Cow;

use super::{Payoff, curve::Curve};
use crate::instruments::autocallable::pricing::{Initial, event_times_through, initial, validate};
use crate::instruments::{
    binary_barrier::{Contract, Settlement},
    monitoring::{Monitoring, validate_life},
};
use crate::numerics::{
    Sum, checked, checked_with_scale, finite_price as finite, log_ratio, scaled_cash,
    scaled_signed_cash,
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
    let stopped =
        clock.calendar().is_some() && clock.year_fraction(context.valuation_time(), end)? == 0.0;
    if stopped {
        if let ModelParameters::LocalVolatility(model) = context.model_parameters() {
            model.surface().validate_time(end)?;
        }
    }
    Ok(stopped)
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
    if payoff.is_digital() {
        context.validate_curve_horizon(end)?;
        let r = model.risk_free_curve().log_factor(start, end)?;
        let q = model.dividend_curve().log_factor(start, end)?;
        let forward = log_ratio(context.spot_price(), payoff.strike) + (q - r);
        return checked(if payoff.sign * forward <= 0.0 {
            0.0
        } else if let Some(payout) = payoff.payout() {
            scaled_cash(payout, r)
        } else {
            scaled_cash(context.spot_price(), q)
        });
    }
    let risk = Curve::new(model.risk_free_curve(), start, end)?;
    let dividend = Curve::new(model.dividend_curve(), start, end)?;
    let time = start.years_until(end);
    exercise_value(payoff, context.spot_price(), 0.0, time, &risk, &dividend)
}

pub(crate) fn american_exercise_boundary(
    option: &crate::AmericanOption,
    context: &PricingContext,
) -> Result<f64> {
    let model = context.model_parameters();
    let start = context.valuation_time();
    let end = option.expiry_time();
    let risk = Curve::new(model.risk_free_curve(), start, end)?;
    let dividend = Curve::new(model.dividend_curve(), start, end)?;
    // Carry is linear between curve knots. The most favorable exercise carry
    // determines where the discounted payoff first becomes positive.
    let best = match option.option_type() {
        crate::OptionType::Call => f64::max,
        crate::OptionType::Put => f64::min,
    };
    let carry = risk.ends().chain(dividend.ends()).fold(0.0, |carry, t| {
        best(carry, dividend.log_factor(0.0, t) - risk.log_factor(0.0, t))
    });
    Ok(scaled_cash(option.strike(), -carry))
}

// Discounted deterministic intrinsic values also supply the far-asset asymptote.
#[expect(
    clippy::float_cmp,
    reason = "Equal forward rates have no isolated stationary point."
)]
pub(super) fn exercise_value(
    payoff: Payoff,
    spot: f64,
    start: f64,
    end: f64,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    let value = |t| {
        let dividend_log = dividend.log_factor(start, t);
        let risk_log = risk.log_factor(start, t);
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
            let m = log_ratio(spot, payoff.strike) + (dividend_log - risk_log);
            if m.is_nan() {
                return Err(invalid());
            }
            if payoff.sign * m <= 0.0 {
                0.0
            } else if payoff.sign > 0.0 {
                scaled_cash(spot, dividend_log + (-(-m).exp_m1()).ln())
            } else {
                scaled_cash(payoff.strike, risk_log + (-m.exp_m1()).ln())
            }
        };
        checked_with_scale(price, 0.0)
    };
    if !payoff.is_american() {
        return value(end);
    }
    let mut knots: Vec<_> = risk
        .ends()
        .chain(dividend.ends())
        .filter(|&t| t > start && t < end)
        .chain([start, end])
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let mut best = value(start)?.max(value(end)?);
    for pair in knots.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        best = best.max(value(b)?);
        let middle = a.midpoint(b);
        let r = risk.rate(middle);
        let q = dividend.rate(middle);
        if spot > 0.0 && r != 0.0 && q != 0.0 && r.signum() == q.signum() && r != q {
            let log = log_ratio(r.abs(), q.abs())
                + log_ratio(payoff.strike, spot)
                + risk.log_factor(start, a)
                - dividend.log_factor(start, a);
            let t = a + log / (r - q);
            if t > a && t < b {
                best = best.max(value(t)?);
            }
        }
    }
    Ok(best)
}

pub(super) fn log_return(context: &PricingContext, time: Timestamp) -> Result<f64> {
    let model = context.model_parameters();
    let start = context.valuation_time();
    finite(
        model.dividend_curve().log_factor(start, time)?
            - model.risk_free_curve().log_factor(start, time)?,
    )
}

pub(super) fn spot(context: &PricingContext, time: Timestamp) -> Result<f64> {
    spot_after_carry(context.spot_price(), log_return(context, time)?)
}

fn spot_after_carry(spot: f64, carry: f64) -> Result<f64> {
    let value = scaled_cash(spot, carry);
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(invalid())
    }
}

fn quoted_strike_distance(context: &PricingContext, time: Timestamp, strike: f64) -> Option<f64> {
    let model = context.model_parameters();
    let start = context.valuation_time();
    let [risk_start, risk_end] = model
        .risk_free_curve()
        .unshifted_factor_quotes(start, time)?;
    let [dividend_start, dividend_end] = model
        .dividend_curve()
        .unshifted_factor_quotes(start, time)?;
    let ordering = crate::numerics::compare_positive_products(
        [context.spot_price(), dividend_end, risk_start],
        [strike, risk_end, dividend_start],
    );
    Some(f64::from(ordering as i8))
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
    for &time in times
        .iter()
        .skip_while(|&&time| time <= start)
        .take_while(|&&time| time <= end)
    {
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
        times.extend(
            curve
                .nodes()
                .iter()
                .copied()
                .chain(curve.max_time())
                .filter(|&time| time > start && time < end),
        );
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
    let hit = |x: f64| {
        if monitoring.barrier_type().is_up() {
            x >= lower
        } else {
            x <= lower || upper_log.is_some_and(|upper| x >= upper)
        }
    };
    let start = context.valuation_time();
    let mut advance = forward_factors(context);
    if monitoring.observation_mode() == ObservationMode::Scheduled {
        for &time in monitoring
            .observation_times()
            .iter()
            .skip_while(|&&time| time <= start)
            .take_while(|&&time| time <= end)
        {
            let (value, log_discount) = advance(time)?;
            if hit(value) {
                return Ok(Some(Touch {
                    spot: spot(context, time)?,
                    log_discount,
                }));
            }
        }
        return Ok(None);
    }
    let times = continuous_knots(context, end);
    for pair in times.windows(2) {
        let (x, a) = advance(pair[0])?;
        if hit(x) {
            return Ok(Some(Touch {
                spot: spot(context, pair[0])?,
                log_discount: a,
            }));
        }
        let (y, b) = advance(pair[1])?;
        if hit(y) {
            let level = if monitoring.barrier_type().is_up() || y <= lower {
                monitoring.barrier_level()
            } else {
                upper.ok_or_else(invalid)?
            };
            let fraction = (log_ratio(level, initial) - x) / (y - x);
            return Ok(Some(Touch {
                spot: level,
                log_discount: fraction.mul_add(b - a, a),
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
    let model = context.model_parameters();
    let start = context.valuation_time();
    let end = monitoring.expiry_time();
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
    let daily = note.view().daily_knock_in();
    let mut next = note
        .events()
        .partition_point(|record| record.time() <= context.valuation_time());
    for time in event_times_through(note, context, daily, horizon)? {
        crate::cancellation::check()?;
        let daily_fixing = daily && note.daily_time(time, note.calendar());
        let record = note
            .events()
            .get(next)
            .filter(|record| record.time() == time);
        if !daily_fixing && record.is_none() && time != note.expiry_time() {
            continue;
        }
        let spot = spot(context, time)?;
        if daily_fixing {
            state.knocked_in |= note.view().below_knock_in(spot);
        }
        if let Some(record) = record {
            next += 1;
            let coupon = note.observation_coupon(record, spot);
            if record.has_knock_out() && spot >= record.knock_out_level() {
                state.settlement = Some(finite(
                    state.coupon
                        + scaled_signed_cash(
                            note.knock_out_payment(record) + coupon,
                            discount(context, time)?,
                        ),
                )?);
                break;
            }
            state.coupon += scaled_signed_cash(coupon, discount(context, time)?);
        }
        if time == note.expiry_time() {
            state.knocked_in |= note.observes_knock_in(time) && note.view().below_knock_in(spot);
            let terminal = note.terminal(spot, state.knocked_in);
            state.settlement = Some(finite(
                state.coupon + scaled_signed_cash(terminal, discount(context, time)?),
            )?);
        }
    }
    Ok(state)
}

pub(super) fn accumulator(option: &Accumulator, context: &PricingContext) -> Result<f64> {
    option.validate_pricing(context)?;
    let (mut quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return finite(value);
    }
    let initial = context.spot_price();
    let strike_distance = log_ratio(initial, option.strike());
    for time in option.future_observation_times(context)? {
        let carry = log_return(context, time)?;
        let spot = spot_after_carry(initial, carry)?;
        if option.observes_knock_out(time) && option.is_knock_out(spot) {
            return finite(quantity * option.unit_payoff(spot) * discount(context, time)?.exp());
        }
        if option.accumulates_at(time, option.calendar()) {
            let distance = quoted_strike_distance(context, time, option.strike())
                .unwrap_or(strike_distance + carry);
            quantity += option.purchase_from_strike_distance(distance);
        }
        if time == option.expiry_time() {
            return finite(quantity * option.unit_payoff(spot) * discount(context, time)?.exp());
        }
    }
    Err(invalid())
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
    terms.coverage(context)?;
    let average = terms.deterministic_average(context, geometric)?;
    checked(scaled_cash(
        (terms.sign() * (average - terms.terms.strike())).max(0.0),
        discount(context, terms.terms.expiry_time())?,
    ))
}
