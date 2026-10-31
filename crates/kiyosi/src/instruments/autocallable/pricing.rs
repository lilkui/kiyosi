use super::*;

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
        knocked_in |= context.spot_price() < note.view().knock_in_level;
    }
    let current = note
        .observations()
        .iter()
        .find(|r| r.date.start_of_day() == context.valuation_time());
    let coupon = current.map_or(0.0, |r| note.observation_coupon(r, context.spot_price()));
    let settlement =
        if let Some(record) = current.filter(|r| context.spot_price() >= r.knock_out_level) {
            Some(note.knock_out_payment(record) + coupon)
        } else if context.valuation_time() == note.expiry_date().start_of_day() {
            Some(
                coupon
                    + note.terminal(
                        context.spot_price(),
                        knocked_in
                            || note.view().has_knock_in
                                && context.spot_price() < note.view().knock_in_level,
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
pub(crate) fn validate<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> Result<()> {
    let valuation = context.valuation_time();
    if valuation < note.effective_date().start_of_day()
        || valuation > note.expiry_date().start_of_day()
    {
        return Err(Error::InvalidInput(
            "valuation must be within contract life".into(),
        ));
    }
    if note
        .observations()
        .iter()
        .any(|r| !context.calendar().is_trading_day(r.date))
    {
        return Err(Error::InvalidInput(
            "observation is not a trading day".into(),
        ));
    }
    note.prior(context.valuation_time(), context.calendar())?;
    if !context.calendar().is_trading_day(note.expiry_date()) {
        return Err(Error::InvalidInput("expiry is not a trading day".into()));
    }
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

pub(crate) fn fixed_value<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    knocked_in: bool,
) -> Option<f64> {
    let t = note.view();
    if t.fixed_knocked_in_coupon.is_some() {
        if !knocked_in && note.knock_in_affects_payoff() {
            return None;
        }
    } else if t.lower_strike != t.upper_strike || !knocked_in && t.maturity_coupon_rate != 0.0 {
        return None;
    }
    let discount = |date: Date| {
        context
            .model_parameters()
            .risk_free_curve()
            .factor(context.valuation_time(), date.into())
            .ok()
    };
    let value = note.terminal(context.spot_price(), knocked_in) * discount(note.expiry_date())?;
    if !value.is_finite() {
        return None;
    }
    let mut coupons = 0.0;
    for record in note
        .observations()
        .iter()
        .rev()
        .filter(|r| r.date.start_of_day() > context.valuation_time())
    {
        let discount = discount(record.date)?;
        if !discount.is_normal() || note.knock_out_payment(record) * discount != value + coupons {
            return None;
        }
        if t.phoenix {
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
    let discount = discount(note.expiry_date())?;
    discount.is_normal().then_some(value + coupons)
}
