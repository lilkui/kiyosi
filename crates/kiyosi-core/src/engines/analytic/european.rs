use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::barrier::{log_ratio, standardize};
use crate::{
    Error, EuropeanOption, Greek, GreeksRequest, NumericalShiftSettings, OptionType,
    PricingContext, PricingEngine, PricingResult, Result,
};

/// Prices European vanilla options analytically under the BSM model.
///
/// Supports deterministic rate curves; see the [crate example](crate).
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticEuropeanEngine;

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

pub(crate) fn black_scholes_arguments(
    spot: f64,
    strike: f64,
    log_carry: f64,
    volatility: f64,
    root_time: f64,
) -> (f64, f64) {
    let forward = log_ratio(spot, strike) + log_carry;
    let width = volatility * root_time;
    let standardized = standardize(forward, volatility, root_time);
    (standardized + 0.5 * width, standardized - 0.5 * width)
}

impl AnalyticEuropeanEngine {
    /// Returns the price and selected analytic Greeks.
    ///
    /// At expiry every Greek is unavailable. Time derivatives hold curve nodes
    /// fixed and use right-hand rates at knots. Shift settings are validated
    /// but do not change analytic derivatives. See the [crate example](crate).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for empty requests or invalid shifts,
    /// then propagates [`Self::price`] errors. Non-finite requested derivatives
    /// return [`Error::ComputationFailure`]. Inputs remain unchanged.
    pub fn price_with_greeks(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        request.validate()?;
        settings.validate()?;
        let mut result = PricingResult::new(self.price(option, context)?)?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            return Ok(result);
        }
        let model = context.model_parameters();
        let spot = context.spot_price();
        let strike = option.strike();
        let sigma = model.volatility();
        let root = time.sqrt();
        let width = sigma * root;
        let dividend_log = model.dividend_curve().log_factor(valuation, expiry)?;
        let risk_log = model.risk_free_curve().log_factor(valuation, expiry)?;
        let (d1, d2) = black_scholes_arguments(spot, strike, dividend_log - risk_log, sigma, root);
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let want = |g| request.has(g);
        let probability =
            |factor: f64| weighted_cdf(factor.abs(), dividend_log, sign * d1).copysign(factor);
        if want(Greek::Delta) {
            result.set(request, Greek::Delta, sign * probability(1.0))?;
        }
        if want(Greek::Rho) {
            result.set(
                request,
                Greek::Rho,
                sign * (time / 100.0) * weighted_cdf(strike, risk_log, sign * d2),
            )?;
        }
        if ![
            Greek::Gamma,
            Greek::Speed,
            Greek::Theta,
            Greek::Charm,
            Greek::Color,
            Greek::Vega,
            Greek::Vanna,
            Greek::Zomma,
        ]
        .iter()
        .any(|g| want(*g))
        {
            return Ok(result);
        }
        let rate = model.risk_free_curve().forward_rate(valuation)?;
        let dividend = model.dividend_curve().forward_rate(valuation)?;
        let log_density = -0.5 * d1 * d1 - 0.5 * (2.0 * PI).ln();
        let regular = log_density.is_finite() && d2.is_finite() && (d1 * d2).is_finite();
        // Scale before exponentiation so representable derivatives survive tail underflow.
        let density = |weight: f64, factor: f64| {
            if !regular || factor == 0.0 {
                0.0
            } else {
                (log_density + weight + factor.abs().ln())
                    .exp()
                    .copysign(factor)
            }
        };
        let log_gamma = dividend_log - spot.ln() - sigma.ln() - root.ln();
        let carry = || {
            sign * dividend * weighted_cdf(spot, dividend_log, sign * d1)
                - sign * rate * weighted_cdf(strike, risk_log, sign * d2)
        };
        for greek in Greek::ALL {
            if !want(greek) {
                continue;
            }
            let value = match greek {
                Greek::Delta | Greek::Rho => continue,
                Greek::Gamma => density(log_gamma, 1.0),
                Greek::Speed => {
                    // Keep the half-width contribution exact when the width is subnormal or zero.
                    -density(
                        log_gamma - spot.ln(),
                        1.5 + super::barrier::standardize(d1.midpoint(d2), sigma, root),
                    )
                }
                Greek::Theta => {
                    (-density(
                        spot.ln() + dividend_log + sigma.ln() - 2.0_f64.ln() - root.ln(),
                        1.0,
                    ) + carry())
                        / 365.0
                }
                Greek::Charm => {
                    (-density(dividend_log, (rate - dividend) / width - 0.5 * d2 / time)
                        + sign * probability(dividend))
                        / 365.0
                }
                Greek::Color => {
                    (density(log_gamma, dividend)
                        + d1.signum()
                            * density(
                                log_gamma - sigma.ln() - root.ln() + d1.abs().ln(),
                                rate - dividend,
                            )
                        + density(log_gamma - 2.0_f64.ln() - time.ln(), 1.0 - d1 * d2))
                        / 365.0
                }
                Greek::Vega => density(spot.ln() + dividend_log + root.ln() - 100.0_f64.ln(), 1.0),
                Greek::Vanna => -density(dividend_log - sigma.ln() - 100.0_f64.ln(), d2),
                Greek::Zomma => density(log_gamma - sigma.ln() - 100.0_f64.ln(), d1 * d2 - 1.0),
            };
            result.set(request, greek, value)?;
        }
        Ok(result)
    }
    /// Prices within the inclusive contract life, returning intrinsic value at expiry.
    ///
    /// See the [crate example](crate) for construction and pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside contract life or required curve
    /// coverage, and [`Error::ComputationFailure`] for non-finite results.
    /// Inputs remain unchanged.
    pub fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation < option.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::InvalidInput(
                "valuation must be within the contract life".into(),
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
        lognormal_price(
            spot,
            strike,
            dividend_log - risk_log,
            (dividend_log, risk_log),
            model.volatility(),
            time.sqrt(),
            sign,
        )
    }
}

impl PricingEngine for AnalyticEuropeanEngine {
    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        AnalyticEuropeanEngine::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request, settings)
    }
}

pub(crate) fn lognormal_price(
    spot: f64,
    strike: f64,
    log_forward: f64,
    log_discounts: (f64, f64),
    volatility: f64,
    root_time: f64,
    sign: f64,
) -> Result<f64> {
    let (asset_log, risk_log) = log_discounts;
    let width = volatility * root_time;
    if !width.is_finite() {
        return Err(Error::ComputationFailure(
            "unstable volatility limit".into(),
        ));
    }
    let (d1, d2) = black_scholes_arguments(spot, strike, log_forward, volatility, root_time);
    let asset = weighted_cdf(spot, asset_log, sign * d1);
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
        value = (spot.ln() + (asset_log) - 0.5 * standardized * width
            + volatility.ln()
            + root_time.ln()
            - 0.5 * distance * distance
            - 0.5 * (2.0 * PI).ln()
            - 2.0 * distance.ln()
            + (sum + width * width * correction / 24.0).ln())
        .exp();
    } else if width < 1e-5 && standardized.abs() < 10.0 {
        let squared = standardized * standardized;
        let time_value = (spot.ln() + (asset_log) + volatility.ln() + root_time.ln()
            - 0.5 * squared
            - 0.5 * (2.0 * PI).ln())
        .exp()
            * (1.0 + (squared - 1.0) * width * width / 24.0);
        value = sign * cash * (standardized * width).exp_m1() + time_value;
    }
    if !value.is_finite() {
        return Err(Error::ComputationFailure(
            "analytic pricing produced a non-finite result".into(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::black_scholes_arguments;

    #[test]
    fn arguments_preserve_scaled_moneyness_and_underflowing_width() {
        for (spot, strike, expected) in [
            (1e300, 5e299, std::f64::consts::LN_2),
            (1e-300, 5e-301, std::f64::consts::LN_2),
            (1.0 + f64::EPSILON, 1.0, f64::EPSILON.ln_1p()),
            (
                f64::MAX,
                f64::MIN_POSITIVE,
                f64::MAX.ln() - f64::MIN_POSITIVE.ln(),
            ),
            (
                f64::MIN_POSITIVE,
                f64::MAX,
                f64::MIN_POSITIVE.ln() - f64::MAX.ln(),
            ),
        ] {
            let (d1, d2) = black_scholes_arguments(spot, strike, 0.0, 1.0, 1.0);
            assert!((d1 - (expected + 0.5)).abs() <= f64::EPSILON);
            assert!((d2 - (expected - 0.5)).abs() <= f64::EPSILON);
        }
        let tiny = f64::from_bits(1);
        assert_eq!(
            black_scholes_arguments(1.0, 1.0, tiny, tiny, 0.5),
            (2.0, 2.0)
        );
    }
}
