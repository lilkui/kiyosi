use super::{Moments, MonteCarloEstimate, MonteCarloSettings};
use super::{estimate, path_samples, scaled_value, unstable};
use crate::instruments::autocallable::pricing::{
    fixed_value, future_event_times, initial, validate, volatility_affects_price,
};
use crate::numerics::log_ratio;
use crate::{
    AutocallableNote, AutocallableTerms, GreeksRequest, NumericalShiftSettings, PricingContext,
    PricingEngine, PricingResult, Result,
};

/// Prices snowball variants and Phoenix notes with trading-day CPU Monte Carlo.
///
/// Uses exact lognormal transitions on trading days when knock-in matters, or
/// remaining observation dates and expiry otherwise. Time is Actual/365 Fixed.
/// Current events precede simulation. Future knock-in, knock-out and coupon tests
/// preserve log-price movements even when the simulated spot rounds to a threshold.
/// Complete normal vectors are drawn before evaluating each path, so early
/// knock-out cannot shift later paths. Sampling, antithetic counts and thread
/// reproducibility follow [`crate::MonteCarloVanillaEngine`]. Shared and distinct
/// instances permit concurrent calls with request-owned buffers and shared workers.
/// Greeks and implied-parameter solvers pin one request-local seed. Solvers
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
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2025, 4, 1)?.start_of_day();
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
    /// assert_eq!(engine.sampling_settings().path_count, 4096);
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
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns the mean discounted remaining cash flows. See [`Self::new`].
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
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        context.bsm_parameters()?;
        let (sample, _, _) = self.sample(option, context, false)?;
        scaled_value(sample.scale, sample.mean, 0.0)
    }

    /// Returns a price with optional standard error and asymptotic 95% confidence interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; known pseudorandom cash flows
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
    /// impossible history [`crate::Error::InvalidInput`], and nontrading
    /// dates [`crate::Error::InvalidInput`]. Sobol dimensions above 21,201
    /// return [`crate::Error::InvalidInput`]. Non-finite paths,
    /// payments or uncertainty return [`crate::Error::ComputationFailure`].
    /// Entropy and thread failures preserve native I/O details through
    /// [`crate::Error::Io`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price_with_statistics<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        context.bsm_parameters()?;
        let (sample, pairs, known) = self.sample(note, context, true)?;
        estimate(self.settings, sample, pairs, 0.0, known)
    }

    fn sample<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        statistics: bool,
    ) -> Result<(Moments, Moments, bool)> {
        let model = context.bsm_parameters()?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        if crate::engines::deterministic::enabled(context) {
            let mut sample = Moments::default();
            sample.add(
                crate::engines::deterministic::autocallable(note, context)?,
                false,
            );
            return Ok((sample, Moments::default(), true));
        }
        validate(note, context)?;
        let state = initial(note, context);
        if let Some(value) = state.settlement.or_else(|| {
            fixed_value(note, context, state.knocked_in).map(|value| value + state.coupon)
        }) {
            if !value.is_finite() {
                return Err(unstable());
            }
            let mut moments = Moments::default();
            moments.add(value, false);
            return Ok((moments, moments, true));
        }
        context.validate_curve_horizon(note.expiry_time())?;
        let terms = note.view();
        let daily = terms.daily_knock_in() && !state.knocked_in && note.knock_in_affects_payoff();
        let valuation = context.valuation_time();
        let dates = future_event_times(note, context, daily)?;
        let mut previous = valuation;
        let sigma = model.volatility();
        let initial_spot = context.spot_price();
        let knock_in = terms.knock_in.map(|ki| log_ratio(ki.level, initial_spot));
        let below_knock_in = |log_return| knock_in.is_some_and(|level| log_return < level);
        let mut steps = Vec::with_capacity(dates.len());
        let mut next = note.observations().partition_point(|r| r.time <= valuation);
        for date in dates {
            let time = previous.years_until(date);
            let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
            let diffusion = sigma * time.sqrt();
            let log_discount = -rate * valuation.years_until(date);
            if !drift.is_finite() || !diffusion.is_finite() || !log_discount.is_finite() {
                return Err(unstable());
            }
            let record = note.observations().get(next).filter(|r| r.time == date);
            if record.is_some() {
                next += 1;
            }
            let record = record.map(|record| {
                (
                    record,
                    log_ratio(record.knock_out_level, initial_spot),
                    log_ratio(record.coupon_barrier_level, initial_spot),
                    note.period_coupon(record),
                )
            });
            steps.push((
                drift,
                diffusion,
                log_discount,
                record,
                daily && note.daily_time(date, note.calendar()),
            ));
            previous = date;
        }
        let terminal_log_discount = steps.last().ok_or_else(unstable)?.2;
        let ([moments], [pairs]) =
            path_samples(self.settings, steps.len(), statistics, |normals| {
                let mut spot = initial_spot;
                let mut log_return = 0.0;
                let mut knocked_in = state.knocked_in;
                let mut coupons = state.coupon;
                for ((drift, diffusion, log_discount, record, observe_knock_in), normal) in
                    steps.iter().zip(normals)
                {
                    // Keep threshold decisions before rounding the simulated spot.
                    log_return += diffusion.mul_add(*normal, *drift);
                    spot = scaled_value(initial_spot, 1.0, log_return)?;
                    if !spot.is_finite() || spot <= 0.0 {
                        return Err(unstable());
                    }
                    if *observe_knock_in {
                        knocked_in |= below_knock_in(log_return);
                    }
                    if let Some((record, knock_out, coupon_barrier, period_coupon)) = *record {
                        let coupon = if log_return >= coupon_barrier {
                            period_coupon
                        } else {
                            0.0
                        };
                        if log_return >= knock_out {
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
                knocked_in |= below_knock_in(log_return);
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
    /// Propagates worker panics after the call's parallel work has finished.
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
        context.bsm_parameters()?;
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}

impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>> for MonteCarloAutocallableEngine {
    fn price_with_statistics(
        &self,
        option: &AutocallableNote<T>,
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
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AutocallableNote<T>> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: self.settings.with_resolved_seed()?,
        })))
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
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
