use super::{scaled_value, terminal_samples, unstable};
use crate::{
    DigitalOption, Error, Greek, GreeksRequest, MonteCarloSettings, NumericalShiftSettings,
    OptionType, PricingContext, PricingEngine, PricingResult, Result,
};

/// Prices European cash-or-nothing and asset-or-nothing contracts by terminal sampling.
///
/// Uses flat BSM rates, standardized strict exercise tests and likelihood-ratio
/// delta/gamma. Other engine Greeks remain absent; scalar numerical sensitivities
/// are available through [`crate::numerical_greeks`]. Ignores `step_count`.
/// Sampling, exact path counts, thread caps and reproducibility follow
/// [`crate::MonteCarloVanillaEngine`]. Shared and distinct instances permit concurrent
/// calls; every call owns its samples and workers. See [`Self::price_with_greeks`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloDigitalEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloDigitalEngine {
    /// Creates an engine with validated shared CPU sampling settings.
    ///
    /// See [`crate::MonteCarloVanillaEngine::new`] for sampling configuration examples.
    ///
    /// Ignores `step_count`; see [`MonteCarloSettings`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for invalid paths, threads or sampling.
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for configuration examples.
    pub fn settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns the discounted terminal mean, settling strictly in the money at expiry.
    ///
    /// See [`Self::price_with_greeks`] for an example. Calls finish synchronously
    /// without cancellation polling; operational failures preserve their native
    /// [`std::io::Error`] through [`Error::Io`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for node curves before life
    /// validation, [`Error::InvalidInput`] outside the inclusive life,
    /// and [`Error::ComputationFailure`] for non-finite parameters, samples or
    /// results. Inputs remain unchanged on success and failure.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Ok(self
            .native(option, context, GreeksRequest::default())?
            .price())
    }

    /// Returns requested likelihood-ratio delta/gamma from the price's normal draws.
    ///
    /// All other Greeks, and all expiry Greeks, remain absent. No numerical bumps
    /// are required. Near-expiry estimates can have substantial sampling error.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before requests, then propagates
    /// [`Self::price`] errors. Empty requests return
    /// [`Error::InvalidInput`]; non-finite requested Greek samples return
    /// [`Error::ComputationFailure`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers, as in [`Self::price`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloDigitalEngine, MonteCarloSettings, DigitalOption,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
    ///     seed: Some(42), ..Default::default()
    /// })?;
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::all())?;
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// assert!(!result.has(Greek::Vega));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.validate_applicability(context)?;
        request.validate()?;
        self.native(option, context, request)
    }

    fn native(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.validate_applicability(context)?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation < option.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        let spot = context.spot_price();
        let call = option.option_type() == OptionType::Call;
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            let pays = if call {
                spot > option.strike()
            } else {
                spot < option.strike()
            };
            return PricingResult::new(if pays {
                option.payout().unwrap_or(spot)
            } else {
                0.0
            });
        }
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        let sigma = model.volatility();
        let root = time.sqrt();
        let width = sigma * root;
        let carry = (rate - dividend) * time;
        let (_, d2) = crate::engines::analytic::european::black_scholes_arguments(
            spot,
            option.strike(),
            carry,
            sigma,
            root,
        );
        let asset = option.payout().is_none();
        let amount = option.payout().unwrap_or(spot);
        let drift = if asset {
            (-dividend - 0.5 * sigma * sigma) * time
        } else {
            -rate * time
        };
        if !width.is_finite() || !carry.is_finite() || !drift.is_finite() {
            return Err(unstable());
        }
        let log_spot = spot.ln();
        let log_width = sigma.ln() + root.ln();
        let (moments, _) = terminal_samples(self.settings, |normal| {
            // Standardization retains exercise when the terminal spot rounds to strike.
            let distance = d2 + normal;
            if !(if call { distance > 0.0 } else { distance < 0.0 }) {
                return Ok([0.0; 3]);
            }
            let exponent = drift + if asset { width * normal } else { 0.0 };
            let payoff = scaled_value(amount, 1.0, exponent)?;
            let log_payoff = amount.ln() + exponent;
            let delta = if request.has(Greek::Delta) && normal != 0.0 {
                (log_payoff + normal.abs().ln() - log_spot - log_width)
                    .exp()
                    .copysign(normal)
            } else {
                0.0
            };
            let score = (-width).mul_add(normal, normal * normal - 1.0);
            let gamma = if request.has(Greek::Gamma) && score != 0.0 {
                (log_payoff + score.abs().ln() - 2.0 * log_spot - 2.0 * log_width)
                    .exp()
                    .copysign(score)
            } else {
                0.0
            };
            if !delta.is_finite() || !gamma.is_finite() {
                return Err(unstable());
            }
            Ok([payoff, delta, gamma])
        })?;
        let mean = |i: usize| scaled_value(moments[i].scale, moments[i].mean, 0.0);
        let mut result = PricingResult::new(mean(0)?)?;
        if request.has(Greek::Delta) {
            result.set(request, Greek::Delta, mean(1)?)?;
        }
        if request.has(Greek::Gamma) {
            result.set(request, Greek::Gamma, mean(2)?)?;
        }
        Ok(result)
    }
}

impl PricingEngine<DigitalOption> for MonteCarloDigitalEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<DigitalOption> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: MonteCarloSettings {
                seed: Some(self.settings.resolved_seed()?),
                ..self.settings
            },
        })))
    }
    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        _settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request)
    }
}
