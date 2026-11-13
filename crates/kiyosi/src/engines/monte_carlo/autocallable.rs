use super::{Moments, MonteCarloEstimate, MonteCarloSettings};
use super::{estimate, path_samples, scaled_value, unstable};
use crate::instruments::autocallable::pricing::{
    fixed_value, future_event_times, initial, validate,
};
use crate::numerics::log_ratio;
use crate::{
    AutocallableNote, AutocallableTerms, GreeksRequest, PricingContext, PricingEngine,
    PricingResult, Result,
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
/// Greeks pin one request-local seed. Implied-parameter solvers require an explicit seed and
/// constrain the sampled price curve, not its statistical error. Coupon trials
/// retain their own event-grid shortcuts when knock-in exposure changes.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloAutocallableEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloAutocallableEngine {
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
    /// let end = Date::new(2025, 4, 1)?.start_of_day();
    /// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.02, start, end,
    ///     autocallable_observations(&[end], &[110.0], Some(&[0.08]), None)?, kiyosi::TradingCalendar::weekdays()))?;
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
    ///     GreeksRequest::new([Greek::Rho]))?;
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
    /// without computing or validating uncertainty.
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
        let (sample, _, _, _) = self.sample(option, context, false)?;
        scaled_value(sample.scale, sample.mean, 0.0)
    }

    /// Returns a price with optional standard error and asymptotic 95% confidence interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]; known pseudorandom cash flows
    /// have zero uncertainty even before expiry. Coupons are discounted at their
    /// contractual dates, including a current Phoenix coupon exactly once.
    /// Cancellation follows [`crate::CancellationToken`]. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Checks flat rates ([`crate::Error::UnsupportedOperation`]), then inclusive
    /// life and prior history. Invalid life or impossible history returns
    /// [`crate::Error::InvalidInput`]. Calendar compatibility follows
    /// [`AutocallableNote::new`]. Sobol dimensions above 21,201
    /// return [`crate::Error::InvalidInput`]. Non-finite paths,
    /// payments or uncertainty return [`crate::Error::ComputationFailure`].
    /// Entropy and thread failures preserve native I/O details through
    /// [`crate::Error::Io`].
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
        let (sample, pairs, known, seed) = self.sample(note, context, true)?;
        estimate(self.settings, sample, pairs, 0.0, known, seed)
    }

    fn sample<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        statistics: bool,
    ) -> Result<(Moments, Moments, bool, Option<u64>)> {
        let model = context.bsm_parameters()?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        if crate::engines::deterministic::enabled(context) {
            let mut sample = Moments::default();
            sample.add(
                crate::engines::deterministic::autocallable(note, context)?,
                false,
            );
            return Ok((sample, Moments::default(), true, None));
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
            return Ok((moments, moments, true, None));
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
        let mut next = note
            .autocall_observations()
            .partition_point(|r| r.time() <= valuation);
        for date in dates {
            let time = previous.years_until(date);
            let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
            let diffusion = sigma * time.sqrt();
            let log_discount = -rate * valuation.years_until(date);
            if !drift.is_finite() || !diffusion.is_finite() || !log_discount.is_finite() {
                return Err(unstable());
            }
            let record = note
                .autocall_observations()
                .get(next)
                .filter(|r| r.time() == date);
            if record.is_some() {
                next += 1;
            }
            let record = record.map(|record| {
                (
                    record,
                    note.knock_out_enabled(record)
                        .then(|| log_ratio(record.knock_out_level(), initial_spot)),
                    log_ratio(record.coupon_barrier_level().unwrap_or(0.0), initial_spot),
                    note.period_coupon(),
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
        let ([moments], [pairs], seed) =
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
                        if knock_out.is_some_and(|level| log_return >= level) {
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
                knocked_in |=
                    note.observes_knock_in(note.expiry_time()) && below_knock_in(log_return);
                let value = coupons
                    + scaled_value(1.0, note.terminal(spot, knocked_in), terminal_log_discount)?;
                if value.is_finite() {
                    Ok([value])
                } else {
                    Err(unstable())
                }
            })?;
        Ok((moments, pairs, false, Some(seed)))
    }

    /// Computes requested finite-difference Greeks with shared normal draws.
    ///
    /// Pins a request-local seed when none is configured. At expiry Greeks follow
    /// terminal payoffs, with time Greeks absent. Stencils crossing active
    /// spot/time events are unavailable, following
    /// [`crate::numerical_greeks`]. Discontinuous payoffs can make small bumps
    /// noisy; shift sizes control the sampled curve, not statistical accuracy.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Checks flat rates before shift validation, then propagates
    /// [`crate::numerical_greeks`] and [`Self::price`] errors.
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
    ) -> Result<PricingResult> {
        PricingEngine::price_with_greeks(self, option, context, request)
    }
}

impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>> for MonteCarloAutocallableEngine {
    fn price_with_seed(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        let (sample, _, _, seed) = self.sample(option, context, false)?;
        Ok((scaled_value(sample.scale, sample.mean, 0.0)?, seed))
    }
    fn price_with_statistics(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        self.price_with_statistics(option, context)
    }
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        crate::engines::flat_rates(context).map(|_| ())
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.sampling_settings().validate_solver_seed()
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AutocallableNote<T>> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(self.with_resolved_seed()?)))
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
}
