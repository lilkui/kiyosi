use super::{
    AutocallableBarrierState, AutocallableNote, AutocallableTerms, PricingContext, Result,
};
use crate::Timestamp;
use crate::instruments::autocallable::ObservationEvent;

pub(crate) struct Initial {
    pub knocked_in: bool,
    pub settlement: Option<f64>,
    pub coupon: f64,
}
pub(crate) fn initial<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> Initial {
    if note.barrier_state() == Some(AutocallableBarrierState::KnockedOut) {
        return Initial {
            knocked_in: false,
            settlement: Some(0.0),
            coupon: 0.0,
        };
    }
    let mut knocked_in = note.barrier_state() == Some(AutocallableBarrierState::KnockedIn);
    if note.daily_at(context) {
        knocked_in |= note.view().below_knock_in(context.spot_price());
    }
    let current = note.event_at(context.valuation_time());
    let coupon = current.map_or(0.0, |r| {
        note.observation_coupon(r, |level| context.spot_price() >= level)
    });
    let settlement = if context.valuation_time() == note.expiry_time() {
        Some(expiry_payment(
            note,
            context.spot_price(),
            knocked_in,
            current,
        ))
    } else {
        current
            .filter(|r| r.has_knock_out() && context.spot_price() >= r.knock_out_level())
            .map(|record| note.knock_out_payment(record) + coupon)
    };
    Initial {
        knocked_in,
        settlement,
        coupon,
    }
}

pub(crate) fn expiry_payment<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    spot: f64,
    knocked_in: bool,
    record: Option<&ObservationEvent>,
) -> f64 {
    let coupon = record.map_or(0.0, |r| note.observation_coupon(r, |level| spot >= level));
    coupon
        + if let Some(r) = record.filter(|r| r.has_knock_out() && spot >= r.knock_out_level()) {
            note.knock_out_payment(r)
        } else {
            note.terminal(
                spot,
                knocked_in
                    || note.observes_knock_in(note.expiry_time())
                        && note.view().below_knock_in(spot),
            )
        }
}

pub(crate) fn future_event_times<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    daily: bool,
) -> Result<Vec<Timestamp>> {
    event_times_through(note, context, daily, note.expiry_time())
}

pub(crate) fn event_times_through<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    daily: bool,
    horizon: Timestamp,
) -> Result<Vec<Timestamp>> {
    crate::cancellation::check()?;
    let events = note.events();
    let first = events.partition_point(|record| record.time <= context.valuation_time());
    let count = events[first..].partition_point(|record| record.time <= horizon);
    let mut times = Vec::with_capacity(count + 1);
    for chunk in events[first..first + count].chunks(1024) {
        crate::cancellation::check()?;
        times.extend(chunk.iter().map(|r| r.time));
    }
    if let Some(clock) = note.observation_clock_time().filter(|_| daily) {
        times.extend(crate::ObservationSchedule::daily_times(
            context.valuation_time(),
            horizon,
            note.calendar(),
            false,
            clock,
        )?);
    }
    times.push(horizon);
    times.sort_unstable();
    times.dedup();
    Ok(times)
}

pub(crate) fn validate<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> Result<()> {
    validate_with_work_check(note, context, |_| Ok(()))
}

pub(crate) fn validate_with_work_check<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    charge: impl Fn(u64) -> Result<()>,
) -> Result<()> {
    crate::instruments::monitoring::validate_life(note, context)?;
    note.prior_with_work_check(context.valuation_time(), charge)?;
    Ok(())
}

pub(crate) fn fixed_value<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    knocked_in: bool,
) -> Option<f64> {
    fixed_value_at(note, context, context.valuation_time(), knocked_in)
}

#[expect(
    clippy::float_cmp,
    reason = "Exact payoff terms and exactly recoverable discounted payments determine whether valuation is constant."
)]
pub(crate) fn fixed_value_at<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    time: Timestamp,
    knocked_in: bool,
) -> Option<f64> {
    let t = note.view();
    let pending_knock_in = if t.daily_knock_in() {
        time < note.expiry_time()
            && note
                .has_daily_observation(time.add_micros(1).ok()?, note.expiry_time())
                .ok()?
    } else {
        t.knock_in.is_some() && time < note.expiry_time()
    };
    if knocked_in || pending_knock_in {
        if let Some(d) = t.downside() {
            if d.lower_strike != d.upper_strike || !knocked_in && t.maturity_coupon_rate() != 0.0 {
                return None;
            }
        } else if !knocked_in && note.knock_in_affects_payoff() {
            return None;
        }
    }
    let discount = |date: Timestamp| {
        context
            .model_parameters()
            .risk_free_curve()
            .factor(time, date)
            .ok()
    };
    let value = note.terminal(context.spot_price(), knocked_in) * discount(note.expiry_time())?;
    if !value.is_finite() {
        return None;
    }
    let mut coupons = 0.0;
    for record in note.events().iter().rev().take_while(|r| r.time > time) {
        crate::cancellation::check().ok()?;
        let discount = discount(record.time)?;
        if !discount.is_normal()
            || record.has_knock_out()
                && note.knock_out_payment(record) * discount != value + coupons
        {
            return None;
        }
        if let Some(level) = record.coupon_barrier_level() {
            let coupon = note.period_coupon();
            if coupon != 0.0 && level != 0.0 {
                return None;
            }
            coupons += coupon * discount;
            if !coupons.is_finite() {
                return None;
            }
        }
    }
    let discount = discount(note.expiry_time())?;
    discount.is_normal().then_some(value + coupons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlackScholesMertonParameters, Date, KnockInObservation, SnowballObservation,
        SnowballOption, SnowballTerms, TimeOfDay, TradingCalendar,
    };

    #[test]
    fn future_events_merge_exact_daily_autocall_and_expiry_times() -> Result<()> {
        let clock = TimeOfDay::new(15, 0, 0, 123_456)?;
        let now = Date::new(2025, 1, 3)?.at_time(clock);
        let sunday = now.add_days(2)?;
        let monday = now.add_days(3)?;
        let tuesday = now.add_days(4)?;
        let expiry = tuesday.add_micros(1)?;
        let calendar = TradingCalendar::custom(
            TradingCalendar::weekdays(),
            vec![monday.date()],
            vec![sunday.date()],
            None,
        )?;
        let note = SnowballOption::new(SnowballTerms::new(
            0.1,
            100.0,
            80.0,
            100.0,
            0.0,
            Date::new(2020, 1, 1)?.start_of_day(),
            expiry,
            [now, sunday, expiry]
                .map(|at| SnowballObservation::new(at, 110.0, 0.1))
                .into_iter()
                .collect::<Result<_>>()?,
            KnockInObservation::EveryTradingDay {
                observation_clock_time: clock,
            },
            calendar,
        ))?;
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
            100.0,
            now,
        )?;
        assert_eq!(
            future_event_times(&note, &context, true)?,
            [sunday, tuesday, expiry]
        );
        assert_eq!(
            future_event_times(&note, &context, false)?,
            [sunday, expiry]
        );
        let before = context.replace(crate::PricingContextChanges {
            valuation_time: Some(now.add_micros(-1)?),
            ..Default::default()
        })?;
        assert_eq!(
            future_event_times(&note, &before, true)?,
            [now, sunday, tuesday, expiry]
        );
        Ok(())
    }
}
