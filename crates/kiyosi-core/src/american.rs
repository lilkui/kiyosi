use std::f64::consts::{PI, SQRT_2};
use std::sync::LazyLock;

use crate::pricing::log_normal_cdf;
use crate::{
    AmericanOption, AnalyticVanillaEngine, Error, ErrorCategory, EuropeanOption, GreeksRequest,
    NumericalShiftSettings, OptionType, PricingContext, PricingEngine, PricingResult, Result,
    numerical_greeks,
};

fn exponential_cdf(weight: f64, argument: f64) -> f64 {
    (weight + log_normal_cdf(argument)).exp()
}

#[expect(
    clippy::too_many_arguments,
    reason = "Preserves the existing 2002 formula parameters"
)]
fn phi(
    spot: f64,
    time: f64,
    gamma: f64,
    boundary: f64,
    strike_boundary: f64,
    rate: f64,
    dividend: f64,
    volatility: f64,
    power_boundary: f64,
) -> f64 {
    let variance = volatility * volatility;
    let root = volatility * time.sqrt();
    let lambda = -rate + gamma * (rate - dividend) + 0.5 * gamma * (gamma - 1.0) * variance;
    let kappa = 2.0 * (rate - dividend) / variance + 2.0 * gamma - 1.0;
    let log_spot = spot.ln();
    let log_boundary = boundary.ln();
    let log_strike_boundary = strike_boundary.ln();
    let first =
        -(log_spot - log_boundary + (rate - dividend + (gamma - 0.5) * variance) * time) / root;
    let second = -(2.0 * log_strike_boundary - log_spot - log_boundary
        + (rate - dividend + (gamma - 0.5) * variance) * time)
        / root;
    let log_weight = lambda * time + gamma * (log_spot - power_boundary.ln());
    exponential_cdf(log_weight, first)
        - exponential_cdf(
            log_weight + kappa * (log_strike_boundary - log_spot),
            second,
        )
}

struct BivariateQuadrature {
    angle: f64,
    points: [(f64, f64); 20],
}

fn bivariate_quadrature(correlation: f64) -> BivariateQuadrature {
    let abscissas = [
        0.07652652113349733,
        0.22778585114164508,
        0.37370608871541956,
        0.5108670019508271,
        0.636053680726515,
        0.7463319064601508,
        0.8391169718222188,
        0.912_234_428_251_326,
        0.9639719272779138,
        0.9931285991850949,
    ];
    let angle = correlation.asin();
    let points = std::array::from_fn(|index| {
        let sign = if index % 2 == 0 { -1.0 } else { 1.0 };
        let sine = (angle * 0.5 * (1.0 + sign * abscissas[index / 2])).sin();
        (sine, 1.0 - sine * sine)
    });
    BivariateQuadrature { angle, points }
}

fn exponential_bivariate_cdf(
    log_weight: f64,
    first: f64,
    second: f64,
    quadrature: &BivariateQuadrature,
    sign: f64,
) -> f64 {
    let weights = [
        0.15275338713072585,
        0.14917298647260375,
        0.14209610931838205,
        0.13168863844917663,
        0.11819453196151842,
        0.10193011981724044,
        0.08327674157670475,
        0.06267204833410906,
        0.04060142980038694,
        0.01761400713915212,
    ];
    let product = first * second;
    let half_sum = 0.5 * (first * first + second * second);
    let mut integral = 0.0;
    for (index, &(sine, denominator)) in quadrature.points.iter().enumerate() {
        integral += weights[index / 2]
            * (log_weight + (sign * sine * product - half_sum) / denominator).exp();
    }
    (log_weight + log_normal_cdf(first) + log_normal_cdf(second)).exp()
        + (sign * quadrature.angle) * integral / (4.0 * PI)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Preserves the existing 2002 formula parameters"
)]
fn ksi(
    spot: f64,
    time: f64,
    gamma: f64,
    boundary: f64,
    outer_boundary: f64,
    inner_boundary: f64,
    split_time: f64,
    rate: f64,
    carry: f64,
    volatility: f64,
    power_boundary: f64,
    quadrature: &BivariateQuadrature,
) -> f64 {
    let variance = volatility * volatility;
    let split_root = volatility * split_time.sqrt();
    let root = volatility * time.sqrt();
    let drift = carry + (gamma - 0.5) * variance;
    let log_spot = spot.ln();
    let log_boundary = boundary.ln();
    let log_outer = outer_boundary.ln();
    let log_inner = inner_boundary.ln();
    let e1 = (log_spot - log_inner + drift * split_time) / split_root;
    let e2 = (2.0 * log_outer - log_spot - log_inner + drift * split_time) / split_root;
    let e3 = (log_spot - log_inner - drift * split_time) / split_root;
    let e4 = (2.0 * log_outer - log_spot - log_inner - drift * split_time) / split_root;
    let f1 = (log_spot - log_boundary + drift * time) / root;
    let f2 = (2.0 * log_outer - log_spot - log_boundary + drift * time) / root;
    let f3 = (2.0 * log_inner - log_spot - log_boundary + drift * time) / root;
    let f4 = (log_spot - log_boundary + 2.0 * (log_inner - log_outer) + drift * time) / root;
    let lambda = -rate + gamma * carry + 0.5 * gamma * (gamma - 1.0) * variance;
    let kappa = 2.0 * carry / variance + 2.0 * gamma - 1.0;
    let log_weight = lambda * time + gamma * (log_spot - power_boundary.ln());
    exponential_bivariate_cdf(log_weight, -e1, -f1, quadrature, 1.0)
        - exponential_bivariate_cdf(
            log_weight + kappa * (log_outer - log_spot),
            -e2,
            -f2,
            quadrature,
            1.0,
        )
        - exponential_bivariate_cdf(
            log_weight + kappa * (log_inner - log_spot),
            -e3,
            -f3,
            quadrature,
            -1.0,
        )
        + exponential_bivariate_cdf(
            log_weight + kappa * (log_inner - log_outer),
            -e4,
            -f4,
            quadrature,
            -1.0,
        )
}

fn unsupported_boundary() -> Error {
    Error::new(
        ErrorCategory::UnsupportedOperation,
        "Bjerksund-Stensland exercise boundaries must exceed the strike",
    )
}

fn bjerksund_call(
    spot: f64,
    strike: f64,
    time: f64,
    rate: f64,
    dividend: f64,
    volatility: f64,
    european_value: f64,
) -> Result<f64> {
    if time == 0.0 {
        return Ok((spot - strike).max(0.0));
    }
    if dividend <= 0.0 {
        return Ok(european_value);
    }
    let carry = rate - dividend;
    if carry * time + 2.0 * volatility * time.sqrt() <= 0.0 {
        return Err(unsupported_boundary());
    }
    let variance = volatility * volatility;
    let offset = carry - 0.5 * variance;
    let root = offset.hypot(SQRT_2 * rate.sqrt() * volatility);
    // Rationalize the positive-carry root to avoid cancellation.
    let beta = if offset > 0.0 {
        rate / (0.5 * root + 0.5 * offset)
    } else {
        (root - offset) / variance
    };
    if variance == 0.0 || !variance.is_finite() || !beta.is_finite() || beta <= 1.0 {
        return Err(Error::new(
            ErrorCategory::InvalidResult,
            "Bjerksund-Stensland exercise exponent is numerically unstable",
        ));
    }
    let b_inf = beta / (beta - 1.0) * strike;
    let b_zero = strike.max(rate / dividend * strike);
    let split_ratio = 0.5 * (5.0_f64.sqrt() - 1.0);
    let split_time = split_ratio * time;
    let scale = (strike / (b_inf - b_zero)) * (strike / b_zero);
    let h1 = -(carry * split_time + 2.0 * volatility * split_time.sqrt()) * scale;
    let h2 = -(carry * time + 2.0 * volatility * time.sqrt()) * scale;
    let inner = b_zero + (b_inf - b_zero) * (1.0 - h1.exp());
    let outer = b_zero + (b_inf - b_zero) * (1.0 - h2.exp());
    if inner <= strike || outer <= strike {
        return Err(unsupported_boundary());
    }
    if spot >= outer {
        return Ok(spot - strike);
    }
    static QUADRATURE: LazyLock<BivariateQuadrature> =
        LazyLock::new(|| bivariate_quadrature((0.5 * (5.0_f64.sqrt() - 1.0)).sqrt()));
    let quadrature = &QUADRATURE;
    // Normalize powers so alpha and spot^beta never overflow separately.
    Ok((outer - strike) * (beta * (spot.ln() - outer.ln())).exp()
        - (outer - strike)
            * phi(
                spot, split_time, beta, outer, outer, rate, dividend, volatility, outer,
            )
        + phi(
            spot, split_time, 1.0, outer, outer, rate, dividend, volatility, 1.0,
        )
        - phi(
            spot, split_time, 1.0, inner, outer, rate, dividend, volatility, 1.0,
        )
        - strike
            * phi(
                spot, split_time, 0.0, outer, outer, rate, dividend, volatility, 1.0,
            )
        + strike
            * phi(
                spot, split_time, 0.0, inner, outer, rate, dividend, volatility, 1.0,
            )
        + (inner - strike)
            * phi(
                spot, split_time, beta, inner, outer, rate, dividend, volatility, inner,
            )
        - (inner - strike)
            * ksi(
                spot, time, beta, inner, outer, inner, split_time, rate, carry, volatility, inner,
                quadrature,
            )
        + ksi(
            spot, time, 1.0, inner, outer, inner, split_time, rate, carry, volatility, 1.0,
            quadrature,
        )
        - ksi(
            spot, time, 1.0, strike, outer, inner, split_time, rate, carry, volatility, 1.0,
            quadrature,
        )
        - strike
            * ksi(
                spot, time, 0.0, inner, outer, inner, split_time, rate, carry, volatility, 1.0,
                quadrature,
            )
        + strike
            * ksi(
                spot, time, 0.0, strike, outer, inner, split_time, rate, carry, volatility, 1.0,
                quadrature,
            ))
}

/// Prices American vanilla options using the existing Bjerksund–Stensland 2002 approximation.
/// This synchronous, stateless engine supports shared concurrent use. It accepts
/// scalar or explicitly flat rates only; it never substitutes another American algorithm.
/// See [`Self::price`] for an example and applicability limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct BjerksundStenslandVanillaEngine;

impl BjerksundStenslandVanillaEngine {
    fn require_flat(context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }

    /// Returns the approximation, bounded below by intrinsic and European values.
    /// At expiry returns intrinsic value, including under negative rates.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::UnsupportedOperation`] for node curves (before life
    /// validation), negative transformed interest rates before expiry (call: rate;
    /// put: dividend), or exercise boundaries at/below strike. Returns
    /// [`ErrorCategory::InvalidTimeRange`] outside the inclusive exercise window and
    /// [`ErrorCategory::InvalidResult`] for unstable exponents or non-finite results.
    /// European continuation pricing errors propagate. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{AmericanOption, BjerksundStenslandVanillaEngine,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.03, 0.2)?, 100.0, start.into())?;
    /// let price = BjerksundStenslandVanillaEngine.price(&option, &context)?;
    /// assert!((price - 6.932464292493037).abs() < 1e-11);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        Self::require_flat(context)?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation < option.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "valuation must be within the contract life",
            ));
        }
        let time = valuation.years_until(expiry);
        let model = context.model_parameters();
        let (rate, dividend) = (model.risk_free_rate()?, model.dividend_yield()?);
        let call = option.option_type() == OptionType::Call;
        if time > 0.0 && (if call { rate } else { dividend }) < 0.0 {
            return Err(Error::new(
                ErrorCategory::UnsupportedOperation,
                "Bjerksund-Stensland requires a non-negative transformed interest rate",
            ));
        }
        let european = EuropeanOption::new(
            option.option_type(),
            option.strike(),
            option.effective_date(),
            option.expiry_date(),
        )?;
        let continuation = AnalyticVanillaEngine.price(&european, context)?;
        let (spot, strike) = (context.spot_price(), option.strike());
        let value = if call {
            bjerksund_call(
                spot,
                strike,
                time,
                rate,
                dividend,
                model.volatility(),
                continuation,
            )?
        } else {
            bjerksund_call(
                strike,
                spot,
                time,
                dividend,
                rate,
                model.volatility(),
                continuation,
            )?
        };
        if !value.is_finite() {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "Bjerksund-Stensland pricing produced a non-finite result",
            ));
        }
        let intrinsic = (if call { spot - strike } else { strike - spot }).max(0.0);
        Ok(value.max(intrinsic).max(continuation))
    }

    /// Returns the price and requested finite-difference Greeks with core bump defaults.
    /// At expiry all Greeks are absent. Feasible bumped domain failures propagate.
    ///
    /// # Errors
    /// Checks flat-rate applicability before request/shift validation, then reuses
    /// [`Self::price`] and [`numerical_greeks`] errors. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{AmericanOption, BjerksundStenslandVanillaEngine,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.03, 0.2)?, 100.0, start.into())?;
    /// let result = BjerksundStenslandVanillaEngine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::require_flat(context)?;
        numerical_greeks(self, option, context, request, settings)
    }
}

impl PricingEngine<AmericanOption> for BjerksundStenslandVanillaEngine {
    fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request, settings)
    }
}
