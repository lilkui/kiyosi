use std::f64::consts::{FRAC_1_SQRT_2, PI};

use crate::{Error, ErrorCategory, EuropeanOption, OptionType, PricingContext, Result};

/// Stateless synchronous European BSM analytic pricing.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticVanillaEngine;

fn normal_cdf(value: f64) -> f64 {
    0.5 * libm::erfc(-value * FRAC_1_SQRT_2)
}

fn log_normal_cdf(value: f64) -> f64 {
    if value >= -10.0 {
        return normal_cdf(value).ln();
    }
    // Retain the existing erfc tail expansion when exponential weights offset underflow.
    let inverse_square = (1.0 / value) / value;
    let mut term = 1.0;
    let mut sum = 1.0;
    for index in 1..=20 {
        term *= -f64::from(2 * index - 1) * inverse_square;
        sum += term;
    }
    -0.5 * value * value - (-value).ln() + (sum / (2.0 * PI).sqrt()).ln()
}

fn weighted_cdf(amount: f64, exponent: f64, argument: f64) -> f64 {
    let discount = exponent.exp();
    let discounted = amount * discount;
    if argument < -10.0 || !discount.is_normal() || !discounted.is_finite() {
        (amount.ln() + exponent + log_normal_cdf(argument)).exp()
    } else {
        discounted * normal_cdf(argument)
    }
}

impl AnalyticVanillaEngine {
    /// Prices within the inclusive contract life. At expiry returns intrinsic value.
    /// Non-finite numerical results return `InvalidResult`.
    pub fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation < option.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "valuation must be within the contract life",
            ));
        }
        let model = context.model_parameters();
        model
            .risk_free_curve()
            .validate_horizon(valuation, expiry)?;
        model.dividend_curve().validate_horizon(valuation, expiry)?;
        let spot = context.spot_price();
        let strike = option.strike();
        let sign = match option.option_type() {
            OptionType::Call => 1.0,
            OptionType::Put => -1.0,
        };
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            return Ok((sign * (spot - strike)).max(0.0));
        }
        let risk_log = model.risk_free_curve().log_factor(valuation, expiry)?;
        let dividend_log = model.dividend_curve().log_factor(valuation, expiry)?;
        let volatility = model.volatility();
        let root_time = time.sqrt();
        let width = volatility * root_time;
        if !width.is_finite() {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "unstable volatility limit",
            ));
        }
        let relative_difference = (spot - strike) / strike;
        let log_moneyness = if relative_difference.abs() < 0.5 {
            relative_difference.ln_1p()
        } else {
            spot.ln() - strike.ln()
        };
        let forward = log_moneyness + (dividend_log - risk_log);
        let standardized = if width == 0.0 {
            (forward / volatility) / root_time
        } else {
            forward / width
        };
        let d1 = standardized + 0.5 * width;
        let d2 = standardized - 0.5 * width;
        let asset = weighted_cdf(spot, dividend_log, sign * d1);
        let cash = weighted_cdf(strike, risk_log, sign * d2);
        let mut value = sign * (asset - cash);
        let standardized = d1 - 0.5 * width;
        if width < 1e-5 && standardized.is_finite() && sign * standardized <= -10.0 {
            // Expand both tail legs together to avoid cancellation at tiny positive volatility.
            let distance = standardized.abs();
            let inverse_square = (1.0 / distance) / distance;
            let mut term = 1.0;
            let mut sum = 1.0;
            let mut correction = -3.0;
            for order in 1..=20 {
                term *= -f64::from(2 * order + 1) * inverse_square;
                sum += term;
                correction -= f64::from(2 * order + 3) * term;
            }
            value = (spot.ln() + dividend_log - 0.5 * standardized * width
                + volatility.ln()
                + root_time.ln()
                - 0.5 * distance * distance
                - 0.5 * (2.0 * PI).ln()
                - 2.0 * distance.ln()
                + (sum + width * width * correction / 24.0).ln())
            .exp();
        } else if width < 1e-5 && standardized.abs() < 10.0 {
            let squared = standardized * standardized;
            let time_value = (spot.ln() + dividend_log + volatility.ln() + root_time.ln()
                - 0.5 * squared
                - 0.5 * (2.0 * PI).ln())
            .exp()
                * (1.0 + (squared - 1.0) * width * width / 24.0);
            value = sign * cash * (standardized * width).exp_m1() + time_value;
        }
        if !value.is_finite() {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "analytic pricing produced a non-finite result",
            ));
        }
        Ok(value)
    }
}
