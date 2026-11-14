use std::f64::consts::{FRAC_1_SQRT_2, PI};

use crate::{Error, Result};

const INF: f64 = f64::INFINITY;
const LOG_DENSITY: f64 = -0.918_938_533_204_672_7;

#[expect(
    clippy::float_cmp,
    reason = "Exact equality detects when subdivision cannot produce a distinct floating-point midpoint."
)]
pub(crate) fn adaptive_simpson(
    evaluate: &impl Fn(f64) -> Result<f64>,
    [left, right]: [f64; 2],
    values: [f64; 3],
    estimate: f64,
    tolerance: f64,
    depth: u32,
) -> Result<f64> {
    let center = left + (right - left) / 2.0;
    let low = evaluate(left + (center - left) / 2.0)?;
    let high = evaluate(center + (right - center) / 2.0)?;
    let first = (center - left) * (values[0] + 4.0 * low + values[1]) / 6.0;
    let second = (right - center) * (values[1] + 4.0 * high + values[2]) / 6.0;
    let refined = first + second;
    if !refined.is_finite() {
        return Err(Error::ComputationFailure(
            "barrier first-exit integral is not representable".into(),
        ));
    }
    if (refined - estimate).abs() <= 15.0 * tolerance || center == left || center == right {
        return Ok(refined + (refined - estimate) / 15.0);
    }
    if depth == 0 {
        return Err(Error::NonConvergence(
            "barrier first-exit integral did not converge".into(),
        ));
    }
    Ok(adaptive_simpson(
        evaluate,
        [left, center],
        [values[0], low, values[1]],
        first,
        tolerance / 2.0,
        depth - 1,
    )? + adaptive_simpson(
        evaluate,
        [center, right],
        [values[1], high, values[2]],
        second,
        tolerance / 2.0,
        depth - 1,
    )?)
}

pub(crate) fn finite_price(price: f64) -> Result<f64> {
    if price.is_finite() {
        Ok(price)
    } else {
        Err(Error::ComputationFailure("non-finite price".into()))
    }
}

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
    if discount.is_normal() && value.is_normal() {
        value
    } else {
        (amount.ln() + exponent).exp()
    }
}

pub(crate) fn scaled_signed_cash(amount: f64, exponent: f64) -> f64 {
    scaled_cash(amount.abs(), exponent).copysign(amount)
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
mod integration_tests {
    use super::*;

    #[test]
    fn first_exit_integral_meets_accuracy_with_sufficient_depth() {
        let value = adaptive_simpson(
            &|x: f64| Ok(x.powi(6)),
            [0.0, 1.0],
            [0.0, 0.015_625, 1.0],
            1.0625 / 6.0,
            1e-12,
            20,
        )
        .unwrap();
        assert!((value - 1.0 / 7.0).abs() < 1e-12);
    }

    #[test]
    fn first_exit_integral_exhaustion_reports_non_convergence() {
        let error = adaptive_simpson(
            &|x: f64| Ok(x.powi(6)),
            [0.0, 1.0],
            [0.0, 0.015_625, 1.0],
            1.0625 / 6.0,
            1e-12,
            0,
        )
        .unwrap_err();
        assert!(matches!(error, Error::NonConvergence(_)));
    }

    #[test]
    fn first_exit_integral_preserves_evaluation_failure() {
        let evaluate = |_: f64| checked(f64::INFINITY);
        let error = adaptive_simpson(&evaluate, [0.0, 1.0], [0.0; 3], 0.0, 1e-12, 0).unwrap_err();
        assert!(matches!(error, Error::ComputationFailure(_)));
    }

    #[test]
    fn non_finite_first_exit_integral_is_computation_failure() {
        let evaluate = |_: f64| Ok(f64::MAX);
        let error =
            adaptive_simpson(&evaluate, [0.0, 1.0], [f64::MAX; 3], 0.0, 1e-12, 0).unwrap_err();
        assert!(matches!(error, Error::ComputationFailure(_)));
    }
}

#[cfg(test)]
mod price_validation_tests {
    use super::*;

    #[test]
    fn signed_cash_scaling_preserves_sign_and_extreme_finite_values() {
        for sign in [-1.0, 1.0] {
            for (amount, exponent, expected) in [
                (3.0, 0.0, 3.0),
                ((-700.0_f64).exp(), 1000.0, 300.0_f64.exp()),
                (700.0_f64.exp(), -1000.0, (-300.0_f64).exp()),
                (f64::MAX, 0.0, f64::MAX),
                (
                    f64::MIN_POSITIVE,
                    -std::f64::consts::LN_2,
                    f64::MIN_POSITIVE / 2.0,
                ),
                (f64::from_bits(1), std::f64::consts::LN_2, f64::from_bits(2)),
                // References rounded from high-precision exponential arithmetic.
                (f64::from_bits(1), 1000.0, 9.733_444_573_000_164e110),
                (f64::MAX, -1454.0, f64::from_bits(1)),
            ] {
                let actual = scaled_signed_cash(sign * amount, exponent);
                assert!((actual / (sign * expected) - 1.0).abs() < 1e-13);
            }
            for exponent in [-1000.0, 0.0, 1000.0] {
                let zero = sign * 0.0_f64;
                assert_eq!(scaled_signed_cash(zero, exponent).to_bits(), zero.to_bits());
            }
            assert_eq!(
                scaled_signed_cash(sign * f64::from_bits(1), -1.0).to_bits(),
                (sign * 0.0_f64).to_bits()
            );
        }
    }

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
