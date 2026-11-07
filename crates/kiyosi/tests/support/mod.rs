use kiyosi::{OptionContract, PricingContext};

pub fn dated<I: OptionContract + Clone>(instrument: &I, context: &PricingContext) -> I {
    if instrument.history_time().is_none() {
        instrument
            .clone()
            .with_history_time(Some(context.valuation_time()))
    } else {
        instrument.clone()
    }
}
