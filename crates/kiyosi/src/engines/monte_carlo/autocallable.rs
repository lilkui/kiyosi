use super::{Moments, MonteCarloEstimate, MonteCarloSettings};
use super::{estimate, path_samples, scaled_value, unstable};
use crate::instruments::autocallable::pricing::{
    fixed_value, future_event_dates, initial, validate, volatility_affects_price,
};
use crate::{
    AutocallableNote, AutocallableTerms, GreeksRequest, NumericalShiftSettings, PricingContext,
    PricingEngine, PricingResult, Result,
};

/// Prices snowball variants and Phoenix notes with trading-day CPU Monte Carlo.
///
/// Uses exact lognormal transitions on trading days when knock-in matters, or
/// remaining observation dates and expiry otherwise. Time is Actual/365 Fixed.
/// Current events precede simulation.
/// Complete normal vectors are drawn before evaluating each path, so early
/// knock-out cannot shift later paths. Sampling, antithetic counts and thread
/// reproducibility follow [`crate::MonteCarloVanillaEngine`]. Shared and distinct
/// instances permit concurrent calls with request-owned workers and buffers.
/// Greeks and implied-parameter solves pin one request-local seed. Solvers
/// constrain the sampled price curve, not its statistical error. Coupon trials
/// retain their own event-grid shortcuts when knock-in exposure changes.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloAutocallableEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloAutocallableEngine {
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
    /// let start = Date::new(2025, 1, 1)?;
    /// let end = Date::new(2025, 4, 1)?;
    /// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.02, start, end,
    ///     autocallable_observations(&[end], &[110.0], Some(&[0.08]), None)?))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
    /// let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
    ///     seed: Some(42), path_count: 4096, ..Default::default()
    /// })?;
    /// let estimate = engine.price_with_statistics(&note, &context)?;
    /// assert!((estimate.price() - 1.0).abs() < 0.02);
    /// assert!(estimate.standard_error().is_some());
    /// assert_eq!(engine.settings().path_count, 4096);
    /// let result = engine.price_with_greeks(&note, &context,
    ///     GreeksRequest::new([Greek::Rho]), Default::default())?;
    /// assert!(result.has(Greek::Rho));
    /// # Ok(()) }
    /// ```
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned sampling settings. See [`Self::new`].
    pub fn settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns the mean discounted remaining cashflows. See [`Self::new`].
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
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        let (sample, _, _) = self.sample(option, context)?;
        scaled_value(sample.scale, sample.mean, 0.0)
    }

    /// Returns a price with optional standard error and asymptotic 95% interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; known pseudorandom cashflows
    /// have zero uncertainty even before expiry. Coupons are discounted at their
    /// contractual dates, including a current Phoenix coupon exactly once.
    /// Calls finish synchronously without cancellation polling. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Checks flat rates ([`crate::Error::UnsupportedOperation`]), inclusive
    /// life ([`crate::Error::InvalidInput`]), trading observations,
    /// prior history and trading expiry using the shared autocallable validation.
    /// Missing history returns [`crate::Error::InvalidInput`],
    /// impossible history [`crate::Error::InvalidInput`], and non-trading
    /// dates [`crate::Error::InvalidInput`]. Sobol dimensions above 21,201
    /// return [`crate::Error::InvalidInput`]. Non-finite paths,
    /// payments or uncertainty return [`crate::Error::ComputationFailure`].
    /// Entropy and thread failures preserve native I/O details through
    /// [`crate::Error::Io`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_statistics<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (sample, pairs, known) = self.sample(note, context)?;
        estimate(self.settings, sample, pairs, 0.0, known)
    }

    fn sample<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<(Moments, Moments, bool)> {
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        validate(note, context)?;
        let state = initial(note, context);
        if let Some(value) = state.settlement.or_else(|| {
            fixed_value(note, context, state.knocked_in).map(|value| value + state.coupon)
        }) {
            if !value.is_finite() {
                return Err(unstable());
            }
            let mut moments = Moments::default();
            moments.add(value);
            return Ok((moments, moments, true));
        }
        let terms = note.view();
        let daily = terms.daily_knock_in() && !state.knocked_in && note.knock_in_affects_payoff();
        let valuation = context.valuation_time();
        let dates = future_event_dates(note, context, daily)?;
        let mut previous = valuation;
        let sigma = model.volatility();
        let mut steps = Vec::with_capacity(dates.len());
        let mut next = note
            .observations()
            .partition_point(|r| r.date.start_of_day() <= valuation);
        for date in dates {
            let time = previous.years_until(date.into());
            let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
            let diffusion = sigma * time.sqrt();
            let log_discount = -rate * valuation.years_until(date.into());
            if !drift.is_finite() || !diffusion.is_finite() || !log_discount.is_finite() {
                return Err(unstable());
            }
            let record = note.observations().get(next).filter(|r| r.date == date);
            if record.is_some() {
                next += 1;
            }
            steps.push((drift, diffusion, log_discount, record));
            previous = date.into();
        }
        let terminal_log_discount = steps.last().ok_or_else(unstable)?.2;
        let ([moments], [pairs]) = path_samples(self.settings, steps.len(), |normals| {
            let mut spot = context.spot_price();
            let mut knocked_in = state.knocked_in;
            let mut coupons = state.coupon;
            for ((drift, diffusion, log_discount, record), normal) in steps.iter().zip(normals) {
                spot *= diffusion.mul_add(*normal, *drift).exp();
                if !spot.is_finite() || spot <= 0.0 {
                    return Err(unstable());
                }
                if daily {
                    knocked_in |= terms.below_knock_in(spot);
                }
                if let Some(record) = record {
                    let coupon = note.observation_coupon(record, spot);
                    if spot >= record.knock_out_level {
                        let value = coupons
                            + scaled_value(
                                1.0,
                                note.knock_out_payment(record) + coupon,
                                *log_discount,
                            )?;
                        return if value.is_finite() {
                            Ok([value])
                        } else {
                            Err(unstable())
                        };
                    }
                    coupons += scaled_value(1.0, coupon, *log_discount)?;
                }
            }
            knocked_in |= terms.below_knock_in(spot);
            let value = coupons
                + scaled_value(1.0, note.terminal(spot, knocked_in), terminal_log_discount)?;
            if value.is_finite() {
                Ok([value])
            } else {
                Err(unstable())
            }
        })?;
        Ok((moments, pairs, false))
    }

    /// Computes requested finite-difference Greeks with shared normal draws.
    ///
    /// Pins a request-local seed when none is configured. Expiry Greeks and
    /// stencils crossing active spot/time events are unavailable, following
    /// [`crate::numerical_greeks`]. Discontinuous payoffs can make small bumps
    /// noisy; shift sizes control the sampled curve, not statistical accuracy.
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
    pub fn price_with_greeks<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}

impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>> for MonteCarloAutocallableEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AutocallableNote<T>> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: self.settings.with_resolved_seed()?,
        })))
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn volatility_affects_price(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> bool {
        volatility_affects_price(option, context)
    }
}
