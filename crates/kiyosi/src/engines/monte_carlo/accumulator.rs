use super::{Moments, estimate, path_samples, scaled_value, unstable};
use crate::numerics::log_ratio;
use crate::{
    Accumulator, GreeksRequest, MonteCarloEstimate, MonteCarloSettings, OptionType, PricingContext,
    PricingEngine, PricingResult, Result,
};

/// Prices accumulators with exact lognormal transitions between trading dates.
///
/// Applies current contractual events before simulation, with inclusive knock-out
/// before accumulation and direction-dependent acceleration. Accrued units settle
/// at knock-out or expiry. History follows [`Accumulator`]; zero prior quantity
/// means none. Uses flat BSM rates and Actual/365 Fixed time.
/// Future knock-out, acceleration and near-strike settlements retain log-price
/// movements even when the simulated spot rounds to a threshold.
/// Draws complete normal vectors before evaluating paths,
/// so early termination never shifts later draws. Sampling and thread
/// reproducibility follow [`crate::MonteCarloVanillaEngine`]. Shared and distinct
/// instances permit concurrent calls with request-owned buffers and shared workers.
/// Greeks and implied-volatility solvers pin one request-local seed; solver
/// tolerances constrain the sampled curve, not its statistical error.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloAccumulatorEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloAccumulatorEngine {
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

    /// Creates an engine with validated CPU sampling settings.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for invalid paths, threads or sampling.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     start, Date::new(2025, 2, 3)?.start_of_day()))?.replace_history(3.0)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 110.0, start.into())?;
    /// let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
    ///     seed: Some(42), path_count: 4096, ..Default::default()
    /// })?;
    /// assert_eq!(engine.sampling_settings().path_count, 4096);
    /// let estimate = engine.price_with_statistics(&option, &context)?;
    /// assert_eq!(estimate.price(), 30.0);
    /// assert_eq!(estimate.standard_error(), Some(0.0));
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Vega]))?;
    /// assert_eq!(result.require(Greek::Vega)?, 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned sampling settings; see [`Self::new`].
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns the mean discounted outstanding settlement; see [`Self::new`].
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price_with_statistics`] sampling and pricing errors,
    /// without computing or validating uncertainty. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        let (sample, _, _) = self.sample(option, context, false)?;
        scaled_value(sample.scale, sample.mean, 0.0)
    }

    /// Returns a price with optional standard error and asymptotic 95% confidence interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; known pseudorandom settlements
    /// have zero uncertainty. Sobol uses one dimension per future trading date.
    /// Calls finish synchronously without cancellation polling. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Checks flat rates ([`crate::Error::UnsupportedOperation`]), inclusive life
    /// ([`crate::Error::InvalidInput`]), then trading expiry and knock-out dates
    /// ([`crate::Error::InvalidInput`]), including for fixed settlements.
    /// Sobol dimensions above 21,201 return [`crate::Error::InvalidInput`].
    /// Non-finite paths, quantities, settlements or uncertainty return
    /// [`crate::Error::ComputationFailure`]. Entropy and thread failures retain native
    /// I/O details through [`crate::Error::Io`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_statistics(
        &self,
        option: &Accumulator,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        context.bsm_parameters()?;
        let (sample, pairs, known) = self.sample(option, context, true)?;
        estimate(self.settings, sample, pairs, 0.0, known)
    }

    fn sample(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        statistics: bool,
    ) -> Result<(Moments, Moments, bool)> {
        let model = context.bsm_parameters()?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        if crate::engines::deterministic::enabled(context) {
            let mut sample = Moments::default();
            sample.add(
                crate::engines::deterministic::accumulator(option, context)?,
                false,
            );
            return Ok((sample, Moments::default(), true));
        }
        option.validate_pricing(context)?;
        let (quantity, settlement) = option.initial(context);
        if let Some(value) = settlement {
            if !value.is_finite() {
                return Err(unstable());
            }
            let mut moments = Moments::default();
            moments.add(value, false);
            return Ok((moments, moments, true));
        }
        if !quantity.is_finite() {
            return Err(unstable());
        }
        context.validate_curve_horizon(option.expiry_time())?;
        let valuation = context.valuation_time();
        let sigma = model.volatility();
        let initial_spot = context.spot_price();
        let strike = log_ratio(option.strike(), initial_spot);
        let knock_out = log_ratio(option.knock_out_level(), initial_spot);
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let unit_payoff = |spot, log_return: f64| {
            let distance = log_return - strike;
            if distance.abs() < 0.5 {
                sign * option.strike() * distance.exp_m1()
            } else {
                option.unit_payoff(spot)
            }
        };
        let mut previous = valuation;
        let mut steps = Vec::new();
        for date in option.future_observation_times(context)? {
            let time = previous.years_until(date);
            let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
            let diffusion = sigma * time.sqrt();
            let log_discount = -rate * valuation.years_until(date);
            if !drift.is_finite() || !diffusion.is_finite() || !log_discount.is_finite() {
                return Err(unstable());
            }
            steps.push((
                drift,
                diffusion,
                log_discount,
                option.observes_knock_out(date),
                option.accumulates_at(date, option.calendar()),
            ));
            previous = date;
        }
        let terminal_log_discount = steps.last().ok_or_else(unstable)?.2;
        let ([moments], [pairs]) =
            path_samples(self.settings, steps.len(), statistics, |normals| {
                let mut spot = initial_spot;
                let mut log_return = 0.0;
                let mut quantity = quantity;
                for ((drift, diffusion, log_discount, observe, accumulate), normal) in
                    steps.iter().zip(normals)
                {
                    log_return += diffusion.mul_add(*normal, *drift);
                    spot = scaled_value(initial_spot, 1.0, log_return)?;
                    if !spot.is_finite() || spot <= 0.0 {
                        return Err(unstable());
                    }
                    if *observe && sign * log_return >= sign * knock_out {
                        return Ok([scaled_value(
                            quantity,
                            unit_payoff(spot, log_return),
                            *log_discount,
                        )?]);
                    }
                    if *accumulate {
                        quantity += option.daily_quantity()
                            * if sign * log_return < sign * strike {
                                option.acceleration_factor()
                            } else {
                                1.0
                            };
                    }
                    if !quantity.is_finite() {
                        return Err(unstable());
                    }
                }
                Ok([scaled_value(
                    quantity,
                    unit_payoff(spot, log_return),
                    terminal_log_discount,
                )?])
            })?;
        Ok((moments, pairs, false))
    }

    /// Computes requested finite-difference Greeks with shared normal draws.
    ///
    /// Pins a request-local seed when none is configured. At expiry Greeks follow
    /// terminal payoffs, with time Greeks absent. Stencils crossing active
    /// spot/time events are unavailable, following
    /// [`crate::numerical_greeks`]. Small bumps can be noisy for discontinuous
    /// payoffs; shifts control the sampled curve, not statistical accuracy.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Checks flat rates before requests and shifts, then propagates
    /// [`crate::numerical_greeks`] and [`Self::price`] errors.
    /// Inputs and engine settings remain unchanged on every outcome.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let shifts = crate::NumericalShiftSettings::default();
        context.bsm_parameters()?;
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}

impl PricingEngine<Accumulator> for MonteCarloAccumulatorEngine {
    fn price_with_statistics(
        &self,
        option: &Accumulator,
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
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<Accumulator> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: self.settings.with_resolved_seed()?,
        })))
    }
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        self.price(option, context)
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}
