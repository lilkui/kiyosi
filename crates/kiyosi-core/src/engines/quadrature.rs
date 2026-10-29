mod accumulator;
pub use accumulator::QuadratureAccumulatorEngine;
mod autocallable;
pub use autocallable::QuadratureAutocallableEngine;
mod barrier;
pub(crate) mod gaussian;
pub use barrier::{QuadratureBarrierEngine, QuadratureSettings};

use std::f64::consts::PI;

use crate::engines::analytic::vanilla::{black_scholes_arguments, weighted_cdf};
use crate::{
    AnalyticVanillaEngine, DigitalOption, Error, ErrorCategory, EuropeanOption, GreeksRequest,
    NumericalShiftSettings, OptionContract, OptionType, PricingContext, PricingEngine,
    PricingResult, Result,
};

// Keep the terminal engines' fixed Simpson budget and exp(-72) endpoint cutoff.
fn normal_tail_integral(threshold: f64, amount: f64, log_discount: f64) -> f64 {
    if threshold.is_nan() {
        return threshold;
    }
    if threshold.is_infinite() {
        return weighted_cdf(amount, log_discount, -threshold);
    }
    let lower = threshold.abs();
    let scaled_tail = threshold > 10.0;
    let discount = if scaled_tail { 1.0 } else { log_discount.exp() };
    let scale = if scaled_tail { 1.0 } else { amount * discount };
    let direct_scale = discount.is_normal() && scale.is_finite();
    let density_at = |value: f64| {
        if scaled_tail {
            let distance = value - lower;
            (-distance * (lower + 0.5 * distance)).exp()
        } else {
            (-0.5 * value * value).exp() / (2.0 * PI).sqrt()
        }
    };
    let density = density_at(lower);
    if density == 0.0 {
        return if threshold >= 0.0 {
            0.0
        } else if direct_scale {
            scale
        } else {
            weighted_cdf(amount, log_discount, -threshold)
        };
    }
    let upper = lower.hypot(12.0);
    let panels = 2048;
    let step = (upper - lower) / f64::from(panels);
    let mut sum = density + density_at(upper);
    let mut correction = 0.0;
    for index in 1..panels {
        let weight = if index % 2 == 0 { 2.0 } else { 4.0 };
        let term = weight * density_at(lower + f64::from(index) * step) - correction;
        let next = sum + term;
        correction = (next - sum) - term;
        sum = next;
    }
    let tail = sum * step / 3.0;
    if scaled_tail {
        return (amount.ln() + log_discount - 0.5 * lower * lower - 0.5 * (2.0 * PI).ln()
            + tail.ln())
        .exp();
    }
    let probability = if threshold < 0.0 { 1.0 - tail } else { tail };
    if direct_scale {
        scale * probability
    } else {
        (amount.ln() + log_discount + probability.ln()).exp()
    }
}

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.model_parameters();
    Ok((model.risk_free_rate()?, model.dividend_yield()?))
}

fn time_to_expiry(option: &impl OptionContract, context: &PricingContext) -> Result<f64> {
    let valuation = context.valuation_time();
    let expiry = option.expiry_date().start_of_day();
    if valuation < option.effective_date().start_of_day() || valuation > expiry {
        return Err(Error::new(
            ErrorCategory::InvalidTimeRange,
            "valuation must be within contract life",
        ));
    }
    Ok(valuation.years_until(expiry))
}

fn arguments(
    strike: f64,
    context: &PricingContext,
    rate: f64,
    dividend: f64,
    time: f64,
) -> Result<(f64, f64)> {
    let sigma = context.model_parameters().volatility();
    let root = time.sqrt();
    let width = sigma * root;
    let (d1, d2) = black_scholes_arguments(
        context.spot_price(),
        strike,
        (rate - dividend) * time,
        sigma,
        root,
    );
    if !width.is_finite() || d2.is_nan() {
        return Err(Error::new(
            ErrorCategory::InvalidResult,
            "integral pricing parameters are non-finite",
        ));
    }
    Ok((d1, d2))
}

macro_rules! terminal_engine {
    ($engine:ident, $instrument:ty, $description:literal) => {
        #[doc = $description]
        /// Uses fixed 2,048-panel Simpson integration with a density cutoff of `exp(-72)`.
        /// Owns no mutable state; shared instances support concurrent pricing.
        /// See [`Self::price`] for an example.
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $engine;

        impl $engine {
            /// Returns requested Greeks through scalar finite differences.
            /// All Greeks are absent at expiry. Uses [`NumericalShiftSettings`] defaults.
            ///
            /// # Errors
            /// Rejects node curves with [`ErrorCategory::UnsupportedOperation`] before
            /// request validation, then propagates [`crate::numerical_greeks`] errors.
            /// Inputs remain unchanged on success and failure.
            ///
            /// # Examples
            /// See [`Self::price`] for a pricing example.
            pub fn price_with_greeks(
                &self,
                option: &$instrument,
                context: &PricingContext,
                request: GreeksRequest,
                settings: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                flat_rates(context)?;
                crate::numerical_greeks(self, option, context, request, settings)
            }
        }

        impl PricingEngine<$instrument> for $engine {
            fn price(&self, option: &$instrument, context: &PricingContext) -> Result<f64> {
                Self::price(self, option, context)
            }
            fn price_with_greeks(
                &self,
                option: &$instrument,
                context: &PricingContext,
                request: GreeksRequest,
                settings: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                Self::price_with_greeks(self, option, context, request, settings)
            }
        }
    };
}

terminal_engine!(
    QuadratureVanillaEngine,
    EuropeanOption,
    "Prices European vanilla contracts by terminal lognormal quadrature with flat rates."
);
terminal_engine!(
    QuadratureDigitalEngine,
    DigitalOption,
    "Prices European cash-or-nothing and asset-or-nothing contracts by quadrature with flat rates."
);

impl QuadratureVanillaEngine {
    /// Prices a European vanilla contract, returning intrinsic value at expiry.
    /// Retains analytic stabilization below `sigma * sqrt(time) < 1e-5` when log discounts are finite.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::UnsupportedOperation`] for node curves before life
    /// validation, [`ErrorCategory::InvalidTimeRange`] outside contract life, or
    /// [`ErrorCategory::InvalidResult`] for non-finite parameters or prices.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{QuadratureVanillaEngine, EuropeanOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = QuadratureVanillaEngine;
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!((result.price() - 9.227005508154036).abs() < 1e-9);
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let (rate, dividend) = flat_rates(context)?;
        let time = time_to_expiry(option, context)?;
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let spot = context.spot_price();
        if time == 0.0 {
            return Ok((sign * (spot - option.strike())).max(0.0));
        }
        // Infinite log discounts need their quadrature limits, which the analytic helper rejects.
        if context.model_parameters().volatility() * time.sqrt() < 1e-5
            && (rate * time).is_finite()
            && (dividend * time).is_finite()
        {
            // Separate tail legs cancel the time value at tiny positive volatility.
            return AnalyticVanillaEngine.price(option, context);
        }
        let (d1, d2) = arguments(option.strike(), context, rate, dividend, time)?;
        let asset = normal_tail_integral(-sign * d1, spot, -dividend * time);
        let cash = normal_tail_integral(-sign * d2, option.strike(), -rate * time);
        Ok(PricingResult::new(sign * (asset - cash))?.price())
    }
}

impl QuadratureDigitalEngine {
    /// Prices a digital contract, settling strictly in the money at expiry.
    /// See [`QuadratureVanillaEngine::price`] for context construction.
    ///
    /// # Errors
    /// Uses the same flat-rate, life and numerical error categories as
    /// [`QuadratureVanillaEngine::price`]. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{QuadratureDigitalEngine, DigitalOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// assert!((QuadratureDigitalEngine.price(&option, &context)? - 4.945810910532236).abs() < 1e-9);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        let (rate, dividend) = flat_rates(context)?;
        let time = time_to_expiry(option, context)?;
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let spot = context.spot_price();
        if time == 0.0 {
            return Ok(if sign * (spot - option.strike()) > 0.0 {
                option.payout().unwrap_or(spot)
            } else {
                0.0
            });
        }
        let (d1, d2) = arguments(option.strike(), context, rate, dividend, time)?;
        let value = if let Some(payout) = option.payout() {
            normal_tail_integral(-sign * d2, payout, -rate * time)
        } else {
            normal_tail_integral(-sign * d1, spot, -dividend * time)
        };
        Ok(PricingResult::new(value)?.price())
    }
}
