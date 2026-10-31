use crate::{DigitalOption, OptionType};

pub(crate) mod analytic;
pub(crate) mod binomial;
pub(crate) mod finite_difference;
pub(crate) mod monte_carlo;
pub(crate) mod quadrature;

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
