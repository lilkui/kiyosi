mod accumulator;
pub use accumulator::QuadratureAccumulatorEngine;
mod autocallable;
pub use autocallable::QuadratureAutocallableEngine;
mod barrier;
pub(crate) mod gaussian;
pub use barrier::{QuadratureBarrierEngine, QuadratureSettings};
mod digital;
pub use digital::QuadratureDigitalEngine;
mod vanilla;
pub use vanilla::{QuadratureVanillaEngine, QuadratureVanillaSettings};

use crate::{PricingContext, Result};

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.bsm_parameters()?;
    Ok((model.flat_risk_free_rate()?, model.flat_dividend_yield()?))
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "Validated positive i32 panel counts and their indices convert exactly."
)]
fn simpson(breaks: &[f64], steps: i32, integrand: impl Fn(f64) -> f64) -> Result<f64> {
    let panels = steps as usize + steps as usize % 2;
    let mut result = crate::numerics::Sum::default();
    for section in breaks.windows(2) {
        let step = (section[1] - section[0]) / panels as f64;
        let mut sum = crate::numerics::Sum::default();
        sum.add(integrand(section[0]));
        sum.add(integrand(section[1]));
        for index in 1..panels {
            if index % 256 == 0 {
                crate::cancellation::check()?;
            }
            sum.add(
                if index % 2 == 0 { 2.0 } else { 4.0 }
                    * integrand(section[0] + index as f64 * step),
            );
        }
        result.add(sum.total() * step / 3.0);
    }
    Ok(result.total())
}
