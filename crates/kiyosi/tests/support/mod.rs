use kiyosi::{OptionContract, PricingContext};

pub fn dated<I: OptionContract + Clone>(instrument: &I, context: &PricingContext) -> I {
    if instrument.history_time().is_none()
        && context.valuation_time() >= instrument.effective_time()
        && context.valuation_time() <= instrument.expiry_time()
    {
        instrument
            .clone()
            .with_history_time(Some(context.valuation_time()))
            .expect("test history timestamp must be within contract life")
    } else {
        instrument.clone()
    }
}
