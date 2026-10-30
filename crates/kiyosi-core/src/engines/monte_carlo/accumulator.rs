use super::{Moments, estimate, path_samples, random_seed, scaled_value, unstable};
use crate::{
    Accumulator, Error, GreeksRequest, MonteCarloEstimate, MonteCarloSettings,
    MonteCarloVanillaEngine, NumericalShiftSettings, PricingContext, PricingEngine, PricingResult,
    Result,
};

/// Prices accumulators with exact lognormal transitions between trading dates.
///
/// Applies current midnight events before simulation, with inclusive knock-out
/// before purchases and acceleration strictly below strike. Accrued units settle
/// at knock-out or expiry. History follows [`Accumulator`]; zero prior quantity
/// means none. Uses flat BSM rates and Actual/365 Fixed time; `step_count` is
/// validated but unused. Draws complete normal vectors before evaluating paths,
/// so early termination never shifts later draws. Sampling and thread
/// reproducibility follow [`MonteCarloVanillaEngine`]. Shared and distinct
/// instances permit concurrent calls with request-owned workers and buffers.
/// Greeks and implied-volatility solves pin one request-local seed; solver
/// tolerances constrain the sampled curve, not its statistical error.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloAccumulatorEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloAccumulatorEngine {
    /// Creates an engine with validated CPU sampling settings.
    ///
    /// # Errors
    ///
    /// Propagates [`MonteCarloVanillaEngine::new`] validation errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     start, Date::new(2025, 2, 3)?))?.replace(3.0)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 110.0, start.into())?;
    /// let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
    ///     seed: Some(42), path_count: 4096, ..Default::default()
    /// })?;
    /// assert_eq!(engine.settings().path_count, 4096);
    /// let estimate = engine.price_with_statistics(&option, &context)?;
    /// assert_eq!(estimate.price(), 30.0);
    /// assert_eq!(estimate.standard_error(), Some(0.0));
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Vega]), Default::default())?;
    /// assert_eq!(result.require(Greek::Vega)?, 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        MonteCarloVanillaEngine::new(settings)?;
        Ok(Self { settings })
    }

    /// Returns owned sampling settings; see [`Self::new`].
    pub fn settings(&self) -> MonteCarloSettings {
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
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        let (sample, _, _) = self.sample(option, context)?;
        scaled_value(sample.scale, sample.mean, 0.0)
    }

    /// Returns a price with optional standard error and asymptotic 95% interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; known pseudorandom settlements
    /// have zero uncertainty. Sobol uses one dimension per future trading date.
    /// Calls finish synchronously without cancellation polling. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Checks flat rates ([`Error::UnsupportedOperation`]), inclusive life
    /// ([`Error::InvalidInput`]), then trading expiry
    /// ([`Error::InvalidInput`]), including for fixed settlements.
    /// Sobol dimensions above 21,201 return [`Error::InvalidInput`].
    /// Non-finite paths, quantities, settlements or uncertainty return
    /// [`Error::ComputationFailure`]. Entropy and thread failures retain native
    /// I/O details through [`Error::Io`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_statistics(
        &self,
        option: &Accumulator,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (sample, pairs, known) = self.sample(option, context)?;
        estimate(self.settings, sample, pairs, 0.0, known)
    }

    fn sample(
        &self,
        option: &Accumulator,
        context: &PricingContext,
    ) -> Result<(Moments, Moments, bool)> {
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        if context.valuation_time() < option.effective_date().start_of_day()
            || context.valuation_time() > option.expiry_date().start_of_day()
        {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        if !context.calendar().is_trading_day(option.expiry_date()) {
            return Err(Error::InvalidInput("expiry must be a trading day".into()));
        }
        let (quantity, settlement) = option.initial(context);
        if let Some(value) = settlement {
            if !value.is_finite() {
                return Err(unstable());
            }
            let mut moments = Moments::default();
            moments.add(value);
            return Ok((moments, moments, true));
        }
        if !quantity.is_finite() {
            return Err(unstable());
        }
        let valuation = context.valuation_time();
        let sigma = model.volatility();
        let mut previous = valuation;
        let mut date = context.valuation_date();
        let mut steps = Vec::new();
        loop {
            if date.start_of_day() > valuation && context.calendar().is_trading_day(date) {
                let time = previous.years_until(date.into());
                let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
                let diffusion = sigma * time.sqrt();
                let log_discount = -rate * valuation.years_until(date.into());
                if !drift.is_finite() || !diffusion.is_finite() || !log_discount.is_finite() {
                    return Err(unstable());
                }
                steps.push((drift, diffusion, log_discount));
                previous = date.into();
            }
            if date == option.expiry_date() {
                break;
            }
            date = date.add_days(1)?;
        }
        let terminal_log_discount = steps.last().ok_or_else(unstable)?.2;
        let ([moments], [pairs]) = path_samples(self.settings, steps.len(), |normals| {
            let mut spot = context.spot_price();
            let mut quantity = quantity;
            for ((drift, diffusion, log_discount), normal) in steps.iter().zip(normals) {
                spot *= diffusion.mul_add(*normal, *drift).exp();
                if !spot.is_finite() || spot <= 0.0 {
                    return Err(unstable());
                }
                if spot >= option.knock_out_level() {
                    return Ok([scaled_value(
                        quantity,
                        spot - option.strike(),
                        *log_discount,
                    )?]);
                }
                quantity += option.purchase(spot);
                if !quantity.is_finite() {
                    return Err(unstable());
                }
            }
            Ok([scaled_value(
                quantity,
                spot - option.strike(),
                terminal_log_discount,
            )?])
        })?;
        Ok((moments, pairs, false))
    }

    /// Computes requested finite-difference Greeks with shared normal draws.
    ///
    /// Pins a request-local seed when none is configured. Expiry Greeks and
    /// stencils crossing active spot/time events are unavailable, following
    /// [`crate::numerical_greeks`]. Small bumps can be noisy for discontinuous
    /// payoffs; shifts control the sampled curve, not statistical accuracy.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Checks flat rates before requests and shifts, then propagates
    /// [`crate::numerical_greeks`] and [`Self::price_with_statistics`] errors.
    /// Inputs and engine settings remain unchanged on every outcome.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}

impl PricingEngine<Accumulator> for MonteCarloAccumulatorEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<Accumulator> + Send + Sync + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self::new(MonteCarloSettings {
            seed: Some(random_seed()?),
            ..self.settings
        })?)))
    }
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.price_with_greeks(option, context, request, shifts)
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}
