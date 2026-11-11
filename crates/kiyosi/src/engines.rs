use crate::{
    DigitalOption, Greek, GreeksRequest, NumericalShiftSettings, OptionContract, OptionType,
    PricingContext, PricingEngine, PricingResult, Result,
};

pub(crate) mod analytic;
pub(crate) mod binomial;
mod curve;
pub(crate) mod deterministic;
pub(crate) mod finite_difference;
pub(crate) mod monte_carlo;
pub(crate) mod quadrature;

fn greeks<I: OptionContract>(
    engine: &impl PricingEngine<I>,
    option: &I,
    context: &PricingContext,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    native: impl FnOnce() -> Result<PricingResult>,
) -> Result<PricingResult> {
    engine.validate_applicability(context)?;
    shifts.validate()?;
    let supported = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    if deterministic::enabled(context) {
        let supported_request = GreeksRequest::new(
            [Greek::Delta, Greek::Gamma]
                .into_iter()
                .filter(|&greek| request.has(greek)),
        );
        return crate::numerical_greeks(engine, option, context, supported_request, shifts)
            .map(|result| result.with_request(request, supported));
    }
    let result = native()?.with_request(request, supported);
    crate::risk::numerical::expiry_greeks(option, context, shifts, result, |context| {
        engine.price(option, context)
    })
}

#[derive(Clone, Copy)]
struct Payoff {
    strike: f64,
    sign: f64,
    american: bool,
    digital: bool,
    payout: Option<f64>,
}

impl Payoff {
    fn vanilla(option_type: OptionType, strike: f64, american: bool) -> Self {
        Self {
            strike,
            sign: if option_type == OptionType::Call {
                1.0
            } else {
                -1.0
            },
            american,
            digital: false,
            payout: None,
        }
    }

    fn digital(option: &DigitalOption) -> Self {
        Self {
            digital: true,
            payout: option.payout(),
            ..Self::vanilla(option.option_type(), option.strike(), false)
        }
    }

    fn value(self, spot: f64) -> f64 {
        if self.digital {
            if self.sign * (spot - self.strike) > 0.0 {
                self.payout.unwrap_or(spot)
            } else {
                0.0
            }
        } else {
            (self.sign * (spot - self.strike)).max(0.0)
        }
    }
}
