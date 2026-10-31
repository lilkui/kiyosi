use std::f64::consts::{FRAC_1_SQRT_2, PI};

use crate::{Error, Result};

const INF: f64 = f64::INFINITY;
const LOG_DENSITY: f64 = -0.9189385332046727;

pub(crate) fn checked(value: f64) -> Result<f64> {
    checked_with_scale(value, 0.0)
}

pub(crate) fn checked_with_scale(value: f64, scale: f64) -> Result<f64> {
    if value.is_finite()
        && (value >= 0.0 || scale.is_finite() && value >= -(64.0 * f64::EPSILON * scale))
    {
        Ok(value.max(0.0))
    } else {
        Err(Error::ComputationFailure(
            "negative or non-finite price".into(),
        ))
    }
}

pub(crate) fn log_ratio(numerator: f64, denominator: f64) -> f64 {
    let relative = (numerator - denominator) / denominator;
    if relative.abs() < 0.5 {
        relative.ln_1p()
    } else {
        let ratio = numerator / denominator;
        if ratio.is_normal() {
            ratio.ln()
        } else {
            numerator.ln() - denominator.ln()
        }
    }
}
pub(crate) fn standardize(value: f64, volatility: f64, root: f64) -> f64 {
    let width = volatility * root;
    if width == 0.0 {
        (value / volatility) / root
    } else {
        value / width
    }
}
pub(crate) fn scaled_cdf(amount: f64, exponent: f64, argument: f64) -> f64 {
    if amount == 0.0 {
        0.0
    } else {
        (amount.ln() + exponent + log_normal_cdf(argument)).exp()
    }
}
pub(crate) fn scaled_cash(amount: f64, exponent: f64) -> f64 {
    if amount == 0.0 {
        return 0.0;
    }
    let discount = exponent.exp();
    let value = amount * discount;
    if discount.is_normal() && value.is_finite() {
        value
    } else {
        (amount.ln() + exponent).exp()
    }
}

fn normal_cdf(value: f64) -> f64 {
    0.5 * libm::erfc(-value * FRAC_1_SQRT_2)
}

pub(crate) fn log_normal_cdf(value: f64) -> f64 {
    if value >= -10.0 {
        return normal_cdf(value).ln();
    }
    // Use the erfc tail expansion when exponential weights offset underflow.
    let inverse_square = (1.0 / value) / value;
    let mut term = 1.0;
    let mut sum = 1.0;
    for index in 1..=20 {
        term *= -f64::from(2 * index - 1) * inverse_square;
        sum += term;
    }
    -0.5 * value * value - (-value).ln() + (sum / (2.0 * PI).sqrt()).ln()
}

pub(crate) fn weighted_cdf(amount: f64, exponent: f64, argument: f64) -> f64 {
    let discount = exponent.exp();
    let discounted = amount * discount;
    if argument < -10.0 || !discount.is_normal() || !discounted.is_finite() {
        (amount.ln() + exponent + log_normal_cdf(argument)).exp()
    } else {
        discounted * normal_cdf(argument)
    }
}

#[derive(Default)]
pub(crate) struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    pub(crate) fn add(&mut self, term: f64) {
        let next = self.value + term;
        self.correction += if self.value.abs() >= term.abs() {
            (self.value - next) + term
        } else {
            (term - next) + self.value
        };
        self.value = next;
    }
    pub(crate) fn total(&self) -> f64 {
        self.value + self.correction
    }
}

pub(crate) fn log_interval_mass(lower: f64, upper: f64) -> f64 {
    if lower >= upper {
        return -INF;
    }
    if lower == -INF {
        return log_normal_cdf(upper);
    }
    if upper == INF {
        return log_normal_cdf(-lower);
    }
    let center = lower.midpoint(upper);
    let half = 0.5 * (upper - lower);
    if center.abs() * half + 0.5 * half * half < 0.25 {
        let mut integral = 0.0;
        let mut previous = 0.0;
        let mut term = 1.0;
        for order in 0..24 {
            if order % 2 == 0 {
                integral += 2.0 * term / f64::from(order + 1);
            }
            let next = (-center * half * term - half * half * previous) / f64::from(order + 1);
            previous = term;
            term = next;
        }
        return -0.5 * center * center + LOG_DENSITY + half.ln() + integral.ln();
    }
    let high = if lower >= 0.0 {
        log_normal_cdf(-lower)
    } else {
        log_normal_cdf(upper)
    };
    let low = if lower >= 0.0 {
        log_normal_cdf(-upper)
    } else {
        log_normal_cdf(lower)
    };
    high + (-(low - high).exp_m1()).ln()
}

#[cfg(test)]
mod price_validation_tests {
    use super::*;

    #[test]
    fn nonnegative_prices_only_clip_roundoff_at_the_monetary_scale() {
        for scale in [1e-100, 1.0, 1e100] {
            assert_eq!(
                checked_with_scale(-32.0 * f64::EPSILON * scale, scale).unwrap(),
                0.0
            );
            assert!(matches!(
                checked_with_scale(-128.0 * f64::EPSILON * scale, scale),
                Err(Error::ComputationFailure(_))
            ));
            assert_eq!(checked_with_scale(scale, scale).unwrap(), scale);
        }
    }

    #[test]
    fn nonnegative_price_validation_rejects_nonfinite_and_unscaled_negative_values() {
        for value in [-1.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
            assert!(matches!(checked(value), Err(Error::ComputationFailure(_))));
        }
        assert_eq!(checked(0.0).unwrap(), 0.0);
    }
}
