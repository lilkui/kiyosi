mod accumulator;
pub use accumulator::QuadratureAccumulatorEngine;
mod autocallable;
pub use autocallable::QuadratureAutocallableEngine;
mod barrier;
pub(crate) mod gaussian;
pub use barrier::{QuadratureBarrierEngine, QuadratureSettings};

use crate::{Error, OptionContract, PricingContext, Result};

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.model_parameters();
    Ok((model.risk_free_rate()?, model.dividend_yield()?))
}

fn time_to_expiry(option: &impl OptionContract, context: &PricingContext) -> Result<f64> {
    let valuation = context.valuation_time();
    let expiry = option.expiry_date().start_of_day();
    if valuation < option.effective_date().start_of_day() || valuation > expiry {
        return Err(Error::InvalidInput(
            "valuation must be within contract life".into(),
        ));
    }
    Ok(valuation.years_until(expiry))
}
