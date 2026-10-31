mod accumulator;
pub use accumulator::QuadratureAccumulatorEngine;
mod autocallable;
pub use autocallable::QuadratureAutocallableEngine;
mod barrier;
pub(crate) mod gaussian;
pub use barrier::{QuadratureBarrierEngine, QuadratureSettings};

use crate::{PricingContext, Result};

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.model_parameters();
    Ok((model.risk_free_rate()?, model.dividend_yield()?))
}
