use std::f64::consts::PI;

use crate::engines::analytic::european::black_scholes_arguments;
use crate::numerics::weighted_cdf;
use crate::{
    DigitalOption, Greek, GreeksRequest, OptionType, PricingContext, PricingEngine, PricingResult,
    Result,
};

/// Prices European cash-or-nothing and asset-or-nothing contracts under BSM.
///
/// Values are owned; shared instances support concurrent reads and pricing.
/// See [`Self::price`] for an example.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticDigitalEngine;

impl AnalyticDigitalEngine {
    /// Returns analytic delta/gamma and completes other requested Greeks numerically.
    ///
    /// At expiry spot Greeks follow terminal-payoff stencils; volatility and rate
    /// Greeks are zero, while time Greeks are absent. Stencils crossing payoff
    /// discontinuities are unavailable. See [`crate::numerical_greeks`] for terminal
    /// rules. Bumps use [`crate::NumericalShiftSettings`].
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors and returns
    /// [`crate::Error::ComputationFailure`] for non-finite requested derivatives.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
    ///     DigitalOption, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::asset_or_nothing(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let result = AnalyticDigitalEngine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]))?;
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// assert_eq!(result.get(Greek::Gamma), None);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.price_with_greeks_with_settings(option, context, request, Default::default())
    }

    /// Returns analytic delta/gamma with configurable numerical fallback Greeks.
    ///
    /// Before expiry, positive-volatility delta and gamma retain their analytic
    /// estimator. Settings control the remaining numerical derivatives, zero-volatility
    /// differences and terminal stencils. See [`Self::price_with_greeks`] for conventions.
    ///
    /// # Errors
    ///
    /// Validates model applicability, the request and every shift before pricing.
    /// Propagates [`crate::NumericalShiftSettings::validate`] and [`Self::price`] errors.
    /// The inputs remain unchanged on success and failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// # let start = Date::new(2025, 1, 1)?.start_of_day();
    /// # let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    /// #     start, start.add_days(365)?)?;
    /// # let context = PricingContext::new(
    /// #     BlackScholesMertonParameters::new(0.0, 0.0, 0.00005)?, 100.0, start)?;
    /// let shifts = NumericalShiftSettings { volatility_shift: Some(0.00001), ..Default::default() };
    /// let result = AnalyticDigitalEngine.price_with_greeks_with_settings(
    ///     &option, &context, GreeksRequest::all(), shifts)?;
    /// assert!(result.require(Greek::Vega)?.is_finite());
    /// assert!(result.require(Greek::Gamma)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks_with_settings(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        context.bsm_parameters()?;
        settings.validate()?;
        crate::risk::numerical::complete_greeks(
            option,
            context,
            request,
            settings,
            true,
            Self::native(option, context, request)?,
            |context, request| Self::native(option, context, request),
        )
    }

    /// Prices a digital through expiry, permitting pre-effective valuation.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for non-BSM models,
    /// [`crate::Error::InvalidInput`] after expiry or outside curve coverage,
    /// and [`crate::Error::ComputationFailure`] for non-finite numerical results.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
    ///     DigitalOption, OptionType, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// assert!((AnalyticDigitalEngine.price(&option, &context)? - 4.945810910532236).abs() < 1e-12);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        Ok(Self::native(option, context, GreeksRequest::new([]))?.price())
    }

    pub(crate) fn native(
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let valuation = context.valuation_time();
        let expiry = option.expiry_time();
        crate::instruments::monitoring::validate_life(option, context)?;
        let spot = context.spot_price();
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            return PricingResult::new(crate::engines::Payoff::digital(option).value(spot));
        }
        let model = context.bsm_parameters()?;
        model
            .risk_free_curve()
            .validate_horizon(valuation, expiry)?;
        model.dividend_curve().validate_horizon(valuation, expiry)?;
        let risk_log = model.risk_free_curve().log_factor(valuation, expiry)?;
        let dividend_log = model.dividend_curve().log_factor(valuation, expiry)?;
        let sigma = model.volatility();
        if sigma == 0.0 {
            return PricingResult::new(crate::engines::deterministic::vanilla(
                option,
                crate::engines::Payoff::digital(option),
                context,
            )?);
        }
        let root = time.sqrt();
        let width = sigma * root;
        let (d1, d2) =
            black_scholes_arguments(spot, option.strike(), dividend_log - risk_log, sigma, root);
        let asset = option.payout().is_none();
        let (amount, discount, d) = if asset {
            (spot, dividend_log, d1)
        } else {
            (option.payout().unwrap_or(spot), risk_log, d2)
        };
        let mut result = PricingResult::new(weighted_cdf(amount, discount, sign * d))?;
        let log_width = sigma.ln() + root.ln();
        let log_spot = spot.ln();
        let density = |log_weight: f64| {
            if d.is_finite() {
                (log_weight - 0.5 * d * d - 0.5 * (2.0 * PI).ln()).exp()
            } else {
                0.0
            }
        };
        if request.has(Greek::Delta) {
            let delta = if asset {
                weighted_cdf(1.0, discount, sign * d) + sign * density(discount - log_width)
            } else {
                sign * density(amount.ln() + discount - log_spot - log_width)
            };
            result.set(request, Greek::Delta, delta)?;
        }
        if request.has(Greek::Gamma) {
            let forward = d1 - 0.5 * width;
            let weight = if asset {
                discount - log_spot
            } else {
                amount.ln() + discount - 2.0 * log_spot
            };
            let gamma_sign = if asset { sign } else { -sign };
            let gamma = if forward == 0.0 {
                gamma_sign * density(weight - 2.0_f64.ln() - log_width)
            } else {
                let numerator = if asset { width - d1 } else { width + d2 };
                if numerator == 0.0 {
                    0.0
                } else {
                    gamma_sign
                        * density(weight + numerator.abs().ln() - 2.0 * log_width)
                            .copysign(numerator)
                }
            };
            result.set(request, Greek::Gamma, gamma)?;
        }
        Ok(result)
    }
}

impl PricingEngine<DigitalOption> for AnalyticDigitalEngine {
    fn greek_method(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<DigitalOption>>::validate_applicability(self, context)?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            if crate::engines::deterministic::enabled(context) {
                crate::GreekMethod::Numerical
            } else {
                crate::GreekMethod::Mixed
            },
        ))
    }

    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        AnalyticDigitalEngine::price(self, option, context)
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<DigitalOption>>::validate_applicability(self, context)?;
        settings.validate()?;
        Self::price_with_greeks_with_settings(self, option, context, request, settings)
    }
}
