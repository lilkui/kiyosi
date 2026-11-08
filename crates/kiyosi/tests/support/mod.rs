use kiyosi::{OptionContract, PricingContext};

pub fn dated<I: OptionContract + Clone>(instrument: &I, context: &PricingContext) -> I {
    if context.valuation_time() >= instrument.effective_time()
        && context.valuation_time() <= instrument.expiry_time()
    {
        match instrument
            .clone()
            .with_history_time(Some(context.valuation_time()))
        {
            Ok(dated) => dated,
            Err(kiyosi::Error::UnsupportedOperation(_)) => instrument.clone(),
            Err(error) => panic!("invalid test history timestamp: {error}"),
        }
    } else {
        instrument.clone()
    }
}
