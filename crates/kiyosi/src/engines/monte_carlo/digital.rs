use super::{path_means, scaled_value, unstable};
use crate::{
    DigitalOption, Greek, GreeksRequest, MonteCarloEstimate, MonteCarloSampling,
    MonteCarloSettings, OptionType, PricingContext, PricingEngine, PricingResult, Result,
};

/// Prices European cash-or-nothing and asset-or-nothing contracts by terminal sampling.
///
/// Uses flat BSM rates, strict in-the-money tests in standardized normal coordinates and likelihood-ratio
/// delta/gamma. Other engine Greeks are absent; scalar numerical sensitivities
/// are available through [`crate::numerical_greeks`].
/// Sampling, exact path counts, thread caps and reproducibility follow
/// [`crate::MonteCarloVanillaEngine`]. Shared and distinct instances permit concurrent
/// calls with request-owned samples and shared workers. See [`Self::price_with_greeks`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloDigitalEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloDigitalEngine {
    /// Returns a replayable engine, drawing a seed only when none is configured.
    ///
    /// All other controls, including any integration or exercise grid, are preserved.
    /// The original engine remains unchanged. Save this engine before pricing to
    /// replay the calculation within the same build; see
    /// [`crate::MonteCarloVanillaEngine::with_resolved_seed`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Io`] if system entropy is unavailable.
    pub fn with_resolved_seed(&self) -> crate::Result<Self> {
        Ok(Self {
            settings: self.settings.with_resolved_seed()?,
        })
    }

    /// Creates an engine with validated shared CPU sampling settings.
    ///
    /// See [`crate::MonteCarloVanillaEngine::new`] for sampling configuration examples.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for invalid paths, threads or sampling.
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for configuration examples.
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns the discounted terminal mean, settling strictly in the money at expiry.
    ///
    /// See [`Self::price_with_greeks`] for an example. Calls finish synchronously
    /// without cancellation polling; operational failures preserve their native
    /// [`std::io::Error`] through [`crate::Error::Io`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for nonconstant rate curves before life
    /// validation, [`crate::Error::InvalidInput`] after expiry or outside curve coverage,
    /// and [`crate::Error::ComputationFailure`] for non-finite parameters, samples or
    /// results. Inputs remain unchanged on success and failure.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        Ok(self
            .native(option, context, GreeksRequest::new([]), false)?
            .0
            .price())
    }

    /// Returns a price with sampling standard error and an asymptotic 95% interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; terminal pseudorandom settlements
    /// have zero uncertainty. See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors and returns [`crate::Error::ComputationFailure`]
    /// for non-finite uncertainty or confidence bounds. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Propagates worker panics as in [`Self::price`].
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior, as in [`Self::price`].
    pub fn price_with_statistics(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (result, standard_error, seed) =
            self.native(option, context, GreeksRequest::new([]), true)?;
        MonteCarloEstimate::from_sampling(
            result.price(),
            standard_error,
            self.sampling_settings(),
            seed,
        )
    }

    /// Returns requested likelihood-ratio delta/gamma from the price's normal draws.
    ///
    /// Uses engine-default estimation controls; see [`crate::numerical_greeks`] for custom bumps.
    /// Terminal stencils use engine-default spot bumps.
    /// Other Greeks are absent. Expiry delta and gamma use terminal payoff stencils; before expiry no numerical bumps
    /// are required. Near-expiry estimates can have substantial sampling error.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before requests, then propagates
    /// [`Self::price`] errors. Empty requests return
    /// [`crate::Error::InvalidInput`]; non-finite requested Greek samples return
    /// [`crate::Error::ComputationFailure`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work, as in [`Self::price`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloDigitalEngine, MonteCarloSettings, DigitalOption,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
    ///     seed: Some(42), ..Default::default()
    /// })?;
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::all())?;
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// assert!(!result.has(Greek::Vega));
    /// let estimate = engine.price_with_statistics(&option, &context)?;
    /// assert!(estimate.standard_error().is_some());
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let settings = crate::NumericalShiftSettings::default();
        if crate::engines::deterministic::enabled(context) {
            return crate::numerical_greeks(self, option, context, request, settings).map(
                |result| {
                    result.with_request(request, GreeksRequest::new([Greek::Delta, Greek::Gamma]))
                },
            );
        }
        context.bsm_parameters()?;
        self.validate_applicability(context)?;
        request.validate()?;
        settings.validate()?;
        let result = self
            .native(option, context, request, false)?
            .0
            .with_request(request, GreeksRequest::new([Greek::Delta, Greek::Gamma]));
        crate::risk::numerical::expiry_greeks(option, context, settings, result, |context| {
            self.price(option, context)
        })
    }

    fn native(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        statistics: bool,
    ) -> Result<(PricingResult, Option<f64>, Option<u64>)> {
        self.validate_applicability(context)?;
        if crate::engines::deterministic::enabled(context) {
            let error = (statistics && self.settings.sampling == MonteCarloSampling::PseudoRandom)
                .then_some(0.0);
            return Ok((
                PricingResult::new(crate::engines::deterministic::vanilla(
                    option,
                    crate::engines::Payoff::digital(option),
                    context,
                )?)?,
                error,
                None,
            ));
        }
        let valuation = context.valuation_time();
        let expiry = option.expiry_time();
        crate::instruments::monitoring::validate_life(option, context)?;
        let spot = context.spot_price();
        let call = option.option_type() == OptionType::Call;
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            let pays = if call {
                spot > option.strike()
            } else {
                spot < option.strike()
            };
            let result = PricingResult::new(if pays {
                option.payout().unwrap_or(spot)
            } else {
                0.0
            })?;
            let error = (statistics && self.settings.sampling == MonteCarloSampling::PseudoRandom)
                .then_some(0.0);
            return Ok((result, error, None));
        }
        let model = context.bsm_parameters()?;
        context.validate_curve_horizon(option.expiry_time())?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
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
        let (means, standard_error, seed) = path_means(self.settings, 1, statistics, |normals| {
            let normal = normals[0];
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
        let mut result = PricingResult::new(means[0])?;
        if request.has(Greek::Delta) {
            result.set(request, Greek::Delta, means[1])?;
        }
        if request.has(Greek::Gamma) {
            result.set(request, Greek::Gamma, means[2])?;
        }
        Ok((result.with_seed(Some(seed)), standard_error, Some(seed)))
    }
}

impl PricingEngine<DigitalOption> for MonteCarloDigitalEngine {
    fn price_with_seed(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        context.bsm_parameters()?;
        let (result, _, seed) = self.native(option, context, GreeksRequest::new([]), false)?;
        Ok((result.price(), seed))
    }
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
                crate::GreekMethod::NativeWithSpotControl
            } else {
                crate::GreekMethod::Native
            },
        ))
    }

    fn price_with_statistics(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        self.price_with_statistics(option, context)
    }
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.bsm_parameters()?;
        context.bsm_parameters()?.flat_risk_free_rate()?;
        context.bsm_parameters()?.flat_dividend_yield()?;
        Ok(())
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.sampling_settings().validate_solver_seed()
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<DigitalOption> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: self.settings.with_resolved_seed()?,
        })))
    }
    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        context.bsm_parameters()?;
        Self::price_with_greeks(self, option, context, request)
    }
}
