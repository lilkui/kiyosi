use crate::{
    BinaryPayoff, DigitalOption, Greek, GreeksRequest, NumericalShiftSettings, OptionContract,
    OptionType, PricingContext, PricingEngine, PricingResult, Result,
};

pub(crate) mod analytic;
pub(crate) mod binomial;
mod curve;
pub(crate) mod deterministic;
pub(crate) mod finite_difference;
pub(crate) mod monte_carlo;
pub(crate) mod quadrature;

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.bsm_parameters()?;
    Ok((model.flat_risk_free_rate()?, model.flat_dividend_yield()?))
}

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
    kind: PayoffKind,
}

#[derive(Clone, Copy)]
enum PayoffKind {
    European,
    American,
    Digital(BinaryPayoff),
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
            kind: if american {
                PayoffKind::American
            } else {
                PayoffKind::European
            },
        }
    }

    fn digital(option: &DigitalOption) -> Self {
        Self {
            kind: PayoffKind::Digital(
                option
                    .payout()
                    .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash),
            ),
            ..Self::vanilla(option.option_type(), option.strike(), false)
        }
    }

    fn is_american(self) -> bool {
        matches!(self.kind, PayoffKind::American)
    }

    fn is_digital(self) -> bool {
        matches!(self.kind, PayoffKind::Digital(_))
    }

    fn payout(self) -> Option<f64> {
        match self.kind {
            PayoffKind::Digital(BinaryPayoff::Cash(amount)) => Some(amount),
            _ => None,
        }
    }

    fn value(self, spot: f64) -> f64 {
        match self.kind {
            PayoffKind::Digital(payment) => {
                if self.sign * (spot - self.strike) > 0.0 {
                    match payment {
                        BinaryPayoff::Cash(amount) => amount,
                        BinaryPayoff::Asset => spot,
                    }
                } else {
                    0.0
                }
            }
            PayoffKind::European | PayoffKind::American => {
                (self.sign * (spot - self.strike)).max(0.0)
            }
        }
    }
}
