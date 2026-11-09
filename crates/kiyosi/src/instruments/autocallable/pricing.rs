use super::{
    AutocallableBarrierState, AutocallableNote, AutocallableTerms, Coupon, Error, PricingContext,
    Result,
};
use crate::Timestamp;

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
    let current = note
        .observations()
        .iter()
        .find(|r| r.time == context.valuation_time());
    let coupon = current.map_or(0.0, |r| note.observation_coupon(r, context.spot_price()));
    let settlement =
        if let Some(record) = current.filter(|r| context.spot_price() >= r.knock_out_level) {
            Some(note.knock_out_payment(record) + coupon)
        } else if context.valuation_time() == note.expiry_time() {
            Some(
                coupon
                    + note.terminal(
                        context.spot_price(),
                        knocked_in
                            || note.observes_knock_in(context.valuation_time())
                                && note.view().below_knock_in(context.spot_price()),
                    ),
            )
        } else {
            None
        };
    Initial {
        knocked_in,
        settlement,
        coupon,
    }
}

pub(crate) fn future_event_times<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    daily: bool,
) -> Result<Vec<Timestamp>> {
    let mut times: Vec<_> = note
        .observations()
        .iter()
        .map(|r| r.time)
        .filter(|&time| time > context.valuation_time())
        .collect();
    if daily {
        times.extend(
            note.daily_observation_schedule()?
                .times()
                .iter()
                .copied()
                .filter(|&time| time > context.valuation_time()),
        );
    }
    times.push(note.expiry_time());
    times.sort_unstable();
    times.dedup();
    Ok(times)
}

pub(crate) fn validate<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> Result<()> {
    let valuation = context.valuation_time();
    if valuation < note.effective_time() || valuation > note.expiry_time() {
        return Err(Error::InvalidInput(
            "valuation must be within contract life".into(),
        ));
    }
    note.prior(context.valuation_time())?;
    if let Some(time) = crate::OptionContract::history_time(note) {
        note.prior(time)?;
    }
    crate::instruments::validate_history_time(
        note,
        note.barrier_state().is_some(),
        context,
        false,
    )?;
    Ok(())
}

/// Reports whether volatility affects a validated note's remaining payoff.
pub(crate) fn volatility_affects_price<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> bool {
    let state = initial(note, context);
    state.settlement.is_none() && fixed_value(note, context, state.knocked_in).is_none()
}

#[expect(
    clippy::float_cmp,
    reason = "Exact payoff terms and exactly recoverable discounted payments determine whether valuation is constant."
)]
pub(crate) fn fixed_value<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    knocked_in: bool,
) -> Option<f64> {
    let t = note.view();
    let pending_knock_in = if t.daily_knock_in() {
        context.valuation_time() < note.expiry_time()
            && note
                .has_daily_observation(
                    context.valuation_time().add_micros(1).ok()?,
                    note.expiry_time(),
                )
                .ok()?
    } else {
        t.knock_in.is_some() && context.valuation_time() < note.expiry_time()
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
            .factor(context.valuation_time(), date)
            .ok()
    };
    let value = note.terminal(context.spot_price(), knocked_in) * discount(note.expiry_time())?;
    if !value.is_finite() {
        return None;
    }
    let mut coupons = 0.0;
    for record in note
        .observations()
        .iter()
        .rev()
        .filter(|r| r.time > context.valuation_time())
    {
        let discount = discount(record.time)?;
        if !discount.is_normal() || note.knock_out_payment(record) * discount != value + coupons {
            return None;
        }
        if matches!(t.coupon, Coupon::Phoenix(_)) {
            let coupon = note.period_coupon(record);
            if coupon != 0.0 && record.coupon_barrier_level != 0.0 {
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
