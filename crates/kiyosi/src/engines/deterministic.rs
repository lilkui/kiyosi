use super::{Payoff, curve::Curve};
use crate::instruments::autocallable::pricing::{future_event_times, initial, validate};
use crate::instruments::{
    binary_barrier::Contract,
    monitoring::{Monitoring, validate_life},
};
use crate::numerics::{checked, checked_with_scale, log_ratio, scaled_cash};
use crate::{
    Accumulator, AutocallableNote, AutocallableTerms, BarrierOption, DoubleBarrierOption, Error,
    ModelParameters, ObservationMode, OptionContract, PricingContext, RebateTiming, Result,
    SettlementTiming, Timestamp,
};

pub(super) fn enabled(context: &PricingContext) -> bool {
    matches!(context.model_parameters(), ModelParameters::BlackScholesMerton(model) if model.volatility() == 0.0)
}

fn invalid() -> Error {
    Error::ComputationFailure("deterministic valuation is not representable".into())
}

fn finite(value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid())
    }
}

pub(super) fn vanilla(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
) -> Result<f64> {
    validate_life(option, context)?;
    let model = context.bsm_parameters()?;
    let start = context.valuation_time();
    let end = option.expiry_time();
    let risk = Curve::new(model.risk_free_curve(), start, end)?;
    let dividend = Curve::new(model.dividend_curve(), start, end)?;
    let time = start.years_until(end);
    if payoff.digital {
        let r = model.risk_free_curve().log_factor(start, end)?;
        let q = model.dividend_curve().log_factor(start, end)?;
        let forward = log_ratio(context.spot_price(), payoff.strike) + q - r;
        return checked(if payoff.sign * forward <= 0.0 {
            0.0
        } else if let Some(payout) = payoff.payout {
            scaled_cash(payout, r)
        } else {
            scaled_cash(context.spot_price(), q)
        });
    }
    exercise_value(payoff, context.spot_price(), 0.0, time, &risk, &dividend)
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
            let m = log_ratio(spot, payoff.strike) + dividend_log - risk_log;
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
    if !payoff.american {
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

fn log_return(context: &PricingContext, time: Timestamp) -> Result<f64> {
    let model = context.bsm_parameters()?;
    let start = context.valuation_time();
    finite(
        model.dividend_curve().log_factor(start, time)?
            - model.risk_free_curve().log_factor(start, time)?,
    )
}

fn spot(context: &PricingContext, time: Timestamp) -> Result<f64> {
    let value = scaled_cash(context.spot_price(), log_return(context, time)?);
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(invalid())
    }
}

fn discount(context: &PricingContext, time: Timestamp) -> Result<f64> {
    context
        .bsm_parameters()?
        .risk_free_curve()
        .log_factor(context.valuation_time(), time)
}

struct Touch {
    spot: f64,
    log_discount: f64,
}

fn first_touch(
    monitoring: &Monitoring,
    upper: Option<f64>,
    context: &PricingContext,
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
    if monitoring.observation_mode() == ObservationMode::Scheduled {
        for &time in monitoring
            .observation_times()
            .iter()
            .filter(|&&time| time > start)
        {
            if hit(log_return(context, time)?) {
                return Ok(Some(Touch {
                    spot: spot(context, time)?,
                    log_discount: discount(context, time)?,
                }));
            }
        }
        return Ok(None);
    }
    let model = context.bsm_parameters()?;
    let end = monitoring.expiry_time();
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
    for pair in times.windows(2) {
        let x = log_return(context, pair[0])?;
        if hit(x) {
            return Ok(Some(Touch {
                spot: spot(context, pair[0])?,
                log_discount: discount(context, pair[0])?,
            }));
        }
        let y = log_return(context, pair[1])?;
        if hit(y) {
            let level = if monitoring.barrier_type().is_up() || y <= lower {
                monitoring.barrier_level()
            } else {
                upper.ok_or_else(invalid)?
            };
            let fraction = (log_ratio(level, initial) - x) / (y - x);
            let a = discount(context, pair[0])?;
            let b = discount(context, pair[1])?;
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
    let touch = first_touch(&option.monitoring, upper, context)?;
    if touch.is_some() == option.barrier_type().is_knock_in() {
        let vanilla = option.vanilla();
        return super::analytic::european::AnalyticEuropeanEngine.price(vanilla, context);
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
    let (prior, touched, finished) = contract.state(context)?;
    let at_hit = contract.timing == SettlementTiming::AtHit;
    if prior && at_hit {
        return Ok(0.0);
    }
    let touch = if touched {
        Some(Touch {
            spot: context.spot_price(),
            log_discount: 0.0,
        })
    } else if finished {
        None
    } else {
        first_touch(monitoring, contract.upper_barrier, context)?
    };
    if touch.is_some() != monitoring.barrier_type().is_knock_in() {
        return Ok(0.0);
    }
    if at_hit {
        let touch = touch.ok_or_else(invalid)?;
        return checked(scaled_cash(
            contract.payout.unwrap_or(touch.spot),
            touch.log_discount,
        ));
    }
    if let Some(digital) = contract.digital {
        return vanilla(digital, Payoff::digital(digital), context);
    }
    let model = context.bsm_parameters()?;
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
    let state = initial(note, context);
    if let Some(value) = state.settlement {
        return finite(value);
    }
    let mut knocked_in = state.knocked_in;
    let mut coupons = state.coupon;
    let mut next = note
        .observations()
        .partition_point(|record| record.time() <= context.valuation_time());
    for time in future_event_times(note, context, note.view().daily_knock_in())? {
        let spot = spot(context, time)?;
        if note.view().daily_knock_in() && note.daily_time(time, note.calendar()) {
            knocked_in |= note.view().below_knock_in(spot);
        }
        if let Some(record) = note
            .observations()
            .get(next)
            .filter(|record| record.time() == time)
        {
            next += 1;
            let coupon = note.observation_coupon(record, spot);
            if note.knock_out_enabled(record) && spot >= record.knock_out_level() {
                return finite(
                    coupons
                        + scaled_cash(
                            (note.knock_out_payment(record) + coupon).abs(),
                            discount(context, time)?,
                        )
                        .copysign(note.knock_out_payment(record) + coupon),
                );
            }
            coupons += scaled_cash(coupon.abs(), discount(context, time)?).copysign(coupon);
        }
        if time == note.expiry_time() {
            knocked_in |= note.observes_knock_in(time) && note.view().below_knock_in(spot);
            let terminal = note.terminal(spot, knocked_in);
            return finite(
                coupons + scaled_cash(terminal.abs(), discount(context, time)?).copysign(terminal),
            );
        }
    }
    Err(invalid())
}

pub(super) fn accumulator(option: &Accumulator, context: &PricingContext) -> Result<f64> {
    option.validate_pricing(context)?;
    let (mut quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return finite(value);
    }
    for time in option.future_observation_times(context)? {
        let spot = spot(context, time)?;
        if option.observes_knock_out(time) && option.is_knock_out(spot) {
            return finite(quantity * option.unit_payoff(spot) * discount(context, time)?.exp());
        }
        if option.accumulates_at(time, option.calendar()) {
            quantity += option.purchase(spot);
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
