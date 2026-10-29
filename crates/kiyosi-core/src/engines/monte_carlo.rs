use std::{f64::consts::TAU, thread};

use crate::{
    Error, ErrorCategory, EuropeanOption, GreeksRequest, NumericalShiftSettings, OptionContract,
    OptionType, PricingContext, PricingEngine, PricingResult, Result,
};

const BLOCK_SIZE: usize = 256;

mod accumulator;
mod american;
mod asian;
pub use asian::MonteCarloAsianEngine;
mod autocallable;
mod barrier;
mod digital;
mod double_barrier;
mod sobol;

pub use accumulator::MonteCarloAccumulatorEngine;
pub use autocallable::MonteCarloAutocallableEngine;
pub use barrier::MonteCarloBarrierEngine;
pub use digital::MonteCarloDigitalEngine;
pub use double_barrier::MonteCarloDoubleBarrierEngine;

/// Selects terminal sampling independently of antithetic pairing.
/// See [`MonteCarloVanillaEngine::new`] for a Sobol example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MonteCarloSampling {
    /// Uses pseudorandom PCG32 streams (the default).
    #[default]
    PseudoRandom,
    /// Uses seeded LMS-scrambled Sobol points with a digital shift.
    Sobol,
}

/// Configures terminal, Asian, barrier, autocallable, accumulator or American CPU sampling.
/// See [`MonteCarloVanillaEngine::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloSettings {
    /// Sets the exact path count, from 1 through 10,000,000; defaults to 100,000.
    pub path_count: i32,
    /// Sets 2..=10,000 grid points (default 50); American pricing before expiry needs 3.
    /// Asian arithmetic integration uses this many intervals; exact distributions, barriers, autocallables and accumulators ignore it.
    pub step_count: i32,
    /// Selects a reproducible stream, or system entropy when absent (the default).
    pub seed: Option<u64>,
    /// Caps CPU workers per call: 0 automatic (default), 1 serial, or a positive cap.
    pub thread_count: i32,
    /// Selects pseudorandom (default) or Sobol sampling; Sobol requires power-of-two paths.
    pub sampling: MonteCarloSampling,
    /// Pairs Z and -Z (default `false`), keeping exact paths and one unpaired path if odd.
    pub antithetic: bool,
}

impl Default for MonteCarloSettings {
    fn default() -> Self {
        Self {
            path_count: 100_000,
            step_count: 50,
            seed: None,
            thread_count: 0,
            sampling: MonteCarloSampling::PseudoRandom,
            antithetic: false,
        }
    }
}

/// Prices European and American vanilla contracts with flat-rate CPU sampling.
/// European contracts sample the exact terminal distribution without stepping.
/// American contracts use strike-scaled quadratic continuation regression on a
/// uniform grid, reducing to linear or constant fits for rank-deficient samples.
/// Calls with nonpositive dividends and nonnegative rates use terminal sampling,
/// retaining the immediate-exercise lower bound. See [`Self::default_american_spot_shift`].
/// Sobol uses one dimension for terminal sampling or `step_count - 1` for exercise
/// grids, and `ceil(path_count / 2)` points with antithetic pairing.
/// Fixed seeds reproduce results across thread counts within the same build.
/// Shared and distinct instances permit concurrent calls without mutation;
/// every call owns its workers and buffers and changes no global thread settings.
/// See [`Self::price`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloVanillaEngine {
    settings: MonteCarloSettings,
}

/// Owns a Monte Carlo price and its optional sampling uncertainty.
/// Uses unbiased sample variance and a two-sided asymptotic 95% normal interval
/// for pseudorandom draws, or independent pair means with even antithetic counts;
/// sparse samples and heavy tails can make the interval unreliable.
/// Sobol always leaves uncertainty absent. Simulated payoffs with fewer than two
/// independent observations or odd antithetic counts also leave uncertainty absent.
/// Known pseudorandom settlements and fixed cashflows have zero uncertainty.
/// See [`MonteCarloVanillaEngine::price_with_statistics`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonteCarloEstimate {
    price: f64,
    standard_error: Option<f64>,
    confidence_interval: Option<(f64, f64)>,
}

impl MonteCarloEstimate {
    /// Returns the finite sample mean. See [`MonteCarloVanillaEngine::price_with_statistics`].
    pub fn price(&self) -> f64 {
        self.price
    }
    /// Returns uncertainty in price units, if estimable. See [`Self`] for applicability.
    pub fn standard_error(&self) -> Option<f64> {
        self.standard_error
    }
    /// Returns the untruncated asymptotic 95% interval, if estimable. See [`Self`].
    pub fn confidence_interval(&self) -> Option<(f64, f64)> {
        self.confidence_interval
    }
}

impl MonteCarloVanillaEngine {
    /// Creates an engine after validating paths, steps, threads and sampling, in that order.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for paths outside 1..=10,000,000,
    /// steps outside 2..=10,000, a negative thread cap, or a non-power-of-two Sobol count.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{MonteCarloSettings, MonteCarloVanillaEngine};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
    ///     seed: Some(42), thread_count: 4, ..Default::default()
    /// })?;
    /// assert_eq!(engine.settings().path_count, 100_000);
    /// let sobol = MonteCarloVanillaEngine::new(MonteCarloSettings {
    ///     path_count: 32768, sampling: kiyosi_core::MonteCarloSampling::Sobol,
    ///     antithetic: true, seed: Some(42), ..Default::default()
    /// })?;
    /// assert!(sobol.settings().antithetic);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        if !(1..=10_000_000).contains(&settings.path_count)
            || !(2..=10_000).contains(&settings.step_count)
            || settings.thread_count < 0
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "invalid Monte Carlo settings",
            ));
        }
        if settings.sampling == MonteCarloSampling::Sobol
            && !(settings.path_count as u32).is_power_of_two()
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "Sobol path count must be a power of two",
            ));
        }
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns finite-difference Greeks using shared normal samples.
    /// Pins one request-local seed, including when the engine has no configured seed.
    /// Replays identical normal draws for every feasible bump; no samples or state
    /// survive the request. At expiry all Greeks are absent. Uses
    /// [`NumericalShiftSettings`] and [`crate::numerical_greeks`] availability rules.
    /// American calls whose rate stencil crosses zero use the exercise grid on both
    /// rate legs. For American spot bumps, use [`Self::default_american_spot_shift`].
    ///
    /// # Errors
    /// Rejects node curves before validating requests and shifts, then propagates
    /// [`crate::numerical_greeks`] and [`Self::price`] errors. Inputs remain unchanged.
    ///
    /// # Examples
    /// See [`Self::price`] for a shared-sample delta example.
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price_with_greeks(self, option, context, request, settings)
    }

    /// Returns a discounted sample mean, or intrinsic value at expiry.
    /// Calls complete synchronously without cancellation or signal polling.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::UnsupportedOperation`] for node curves before life
    /// validation, [`ErrorCategory::InvalidTimeRange`] outside the inclusive contract
    /// life, or [`ErrorCategory::InvalidResult`] for unstable parameters, nonpositive
    /// or non-finite terminal prices, or non-finite results. Inputs remain unchanged.
    /// Entropy and thread-creation failures retain their native [`std::io::Error`]
    /// in [`Error::operational_error`].
    /// American pricing before expiry returns [`ErrorCategory::InvalidParameter`]
    /// for fewer than three grid points, after life validation.
    ///
    /// # Panics
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{MonteCarloSettings, MonteCarloVanillaEngine, EuropeanOption,
    ///     OptionType, Date, BlackScholesMertonParameters, PricingContext};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings { seed: Some(42), ..Default::default() })?;
    /// assert!((engine.price(&option, &context)? - 9.227).abs() < 0.2);
    /// let analysis = engine.price_with_greeks(&option, &context,
    ///     kiyosi_core::GreeksRequest::new([kiyosi_core::Greek::Delta]), Default::default())?;
    /// assert!(analysis.require(kiyosi_core::Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns the price, standard error and asymptotic 95% confidence interval.
    /// Uses the same samples and reduction order as [`Self::price`].
    /// Leaves uncertainty absent for Sobol and insufficient independent samples;
    /// see [`MonteCarloEstimate`] for antithetic and expiry rules.
    ///
    /// # Errors
    /// Propagates [`Self::price`] errors and returns [`ErrorCategory::InvalidResult`]
    /// when uncertainty or interval endpoints are non-finite. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// # use kiyosi_core::*;
    /// # fn main() -> kiyosi_core::Result<()> {
    /// # let start = Date::new(2025, 1, 1)?;
    /// # let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// # let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings { seed: Some(42), ..Default::default() })?;
    /// let estimate = engine.price_with_statistics(&option, &context)?;
    /// assert!(estimate.standard_error().is_some_and(|error| error > 0.0));
    /// assert!(estimate.confidence_interval().is_some());
    /// # Ok(()) }
    /// ```
    pub fn price_with_statistics(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (sample, pairs, log_discount) = self.sample(option, context)?;
        estimate(
            self.settings,
            sample,
            pairs,
            log_discount,
            context.valuation_time() == option.expiry_date().start_of_day(),
        )
    }

    fn sample(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
    ) -> Result<(Moments, Moments, f64)> {
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation < option.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "valuation must be within contract life",
            ));
        }
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            let mut sample = Moments::default();
            sample.add((sign * (context.spot_price() - option.strike())).max(0.0));
            return Ok((sample, Moments::default(), 0.0));
        }
        let sigma = model.volatility();
        let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
        let diffusion = sigma * time.sqrt();
        if !drift.is_finite() || !diffusion.is_finite() {
            return Err(unstable());
        }
        let (total, pairs) = terminal_samples(self.settings, |normal| {
            let terminal = context.spot_price() * diffusion.mul_add(normal, drift).exp();
            if !terminal.is_finite() || terminal <= 0.0 {
                return Err(unstable());
            }
            Ok([(sign * (terminal - option.strike())).max(0.0)])
        })?;
        let [total] = total;
        let [pairs] = pairs;
        Ok((total, pairs, -rate * time))
    }
}

impl PricingEngine for MonteCarloVanillaEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine + Send + Sync + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: MonteCarloSettings {
                seed: Some(random_seed()?),
                ..self.settings
            },
        })))
    }
    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let (sample, _, log_discount) = self.sample(option, context)?;
        scaled_value(sample.scale, sample.mean, log_discount)
    }
    fn price_with_greeks(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}

fn estimate(
    settings: MonteCarloSettings,
    sample: Moments,
    pairs: Moments,
    log_discount: f64,
    known: bool,
) -> Result<MonteCarloEstimate> {
    let price = scaled_value(sample.scale, sample.mean, log_discount)?;
    let uncertainty = if settings.antithetic { pairs } else { sample };
    let standard_error = if settings.sampling == MonteCarloSampling::Sobol {
        None
    } else if known {
        Some(0.0)
    } else if uncertainty.count > 1 && (!settings.antithetic || settings.path_count % 2 == 0) {
        Some(scaled_value(
            uncertainty.scale,
            (uncertainty.m2.max(0.0) / (uncertainty.count - 1) as f64 / uncertainty.count as f64)
                .sqrt(),
            log_discount,
        )?)
    } else {
        None
    };
    let confidence_interval = standard_error.map(|error| {
        let half_width = 1.959963984540054 * error;
        (price - half_width, price + half_width)
    });
    if standard_error.is_some_and(|value| !value.is_finite())
        || confidence_interval
            .is_some_and(|(lower, upper)| !lower.is_finite() || !upper.is_finite())
    {
        return Err(unstable());
    }
    Ok(MonteCarloEstimate {
        price,
        standard_error,
        confidence_interval,
    })
}

fn terminal_samples<const N: usize>(
    settings: MonteCarloSettings,
    payoff: impl Fn(f64) -> Result<[f64; N]> + Sync,
) -> Result<([Moments; N], [Moments; N])> {
    path_samples(settings, 1, |normals| payoff(normals[0]))
}

pub(crate) fn path_means<const N: usize>(
    settings: MonteCarloSettings,
    dimensions: usize,
    payoff: impl Fn(&[f64]) -> Result<[f64; N]> + Sync,
) -> Result<[f64; N]> {
    let (moments, _) = path_samples(settings, dimensions, payoff)?;
    let mut means = [0.0; N];
    for (mean, moment) in means.iter_mut().zip(moments) {
        *mean = scaled_value(moment.scale, moment.mean, 0.0)?;
    }
    Ok(means)
}

fn path_samples<const N: usize>(
    settings: MonteCarloSettings,
    dimensions: usize,
    payoff: impl Fn(&[f64]) -> Result<[f64; N]> + Sync,
) -> Result<([Moments; N], [Moments; N])> {
    let seed = match settings.seed {
        Some(seed) => seed,
        None => random_seed()?,
    };
    let paths = settings.path_count as usize;
    let samples = if settings.antithetic {
        paths.div_ceil(2)
    } else {
        paths
    };
    let sobol = if settings.sampling == MonteCarloSampling::Sobol {
        Some(sobol::SobolSequence::new(dimensions, seed)?)
    } else {
        None
    };
    let empty = [Moments::default(); N];
    let mut partials = vec![Ok((empty, empty)); samples.div_ceil(BLOCK_SIZE)];
    let simulate = |block: usize| -> Result<([Moments; N], [Moments; N])> {
        let mut generator = Pcg32::new(seed, block as u64);
        let mut cursor = sobol
            .as_ref()
            .map(|sequence| sequence.cursor(block * BLOCK_SIZE));
        let mut moments = empty;
        let mut pairs = empty;
        let mut spare = None;
        let mut normals = vec![0.0; dimensions];
        for path in block * BLOCK_SIZE..((block + 1) * BLOCK_SIZE).min(samples) {
            for normal in &mut normals {
                *normal = if let Some(cursor) = &mut cursor {
                    cursor.normal()
                } else {
                    match spare.take() {
                        Some(value) => value,
                        None => {
                            let radius = (-2.0 * generator.uniform().ln()).sqrt();
                            let (sin, cos) = (TAU * generator.uniform()).sin_cos();
                            spare = Some(radius * sin);
                            radius * cos
                        }
                    }
                };
            }
            if let Some(cursor) = &mut cursor {
                cursor.next_path();
            }
            let positive = payoff(&normals)?;
            for i in 0..N {
                moments[i].add(positive[i]);
            }
            if settings.antithetic && path < paths / 2 {
                for normal in &mut normals {
                    *normal = -*normal;
                }
                let negative = payoff(&normals)?;
                for i in 0..N {
                    moments[i].add(negative[i]);
                    pairs[i].add(positive[i].midpoint(negative[i]));
                }
            }
        }
        Ok((moments, pairs))
    };
    run_blocks(&mut partials, settings.thread_count, simulate)?;
    let mut total = empty;
    let mut pairs = empty;
    for partial in partials {
        let (payoffs, pair_means) = partial?;
        for i in 0..N {
            total[i].merge(payoffs[i]);
            pairs[i].merge(pair_means[i]);
        }
    }
    Ok((total, pairs))
}

fn run_blocks<T: Send>(
    partials: &mut [T],
    thread_count: i32,
    simulate: impl Fn(usize) -> T + Sync,
) -> Result<()> {
    let available = thread::available_parallelism().map_or(1, |count| count.get());
    let workers = if thread_count == 0 {
        available
    } else {
        (thread_count as usize).min(available)
    }
    .min(partials.len());
    if workers == 1 {
        for (block, partial) in partials.iter_mut().enumerate() {
            *partial = simulate(block);
        }
        return Ok(());
    }
    thread::scope(|scope| -> Result<()> {
        let chunk_size = partials.len().div_ceil(workers);
        let mut handles = Vec::new();
        for (chunk, values) in partials.chunks_mut(chunk_size).enumerate() {
            let simulate = &simulate;
            handles.push(
                thread::Builder::new()
                    .spawn_scoped(scope, move || {
                        for (index, value) in values.iter_mut().enumerate() {
                            *value = simulate(chunk * chunk_size + index);
                        }
                    })
                    .map_err(Error::operational)?,
            );
        }
        for handle in handles {
            if let Err(panic) = handle.join() {
                std::panic::resume_unwind(panic);
            }
        }
        Ok(())
    })
}

pub(crate) fn unstable() -> Error {
    Error::new(
        ErrorCategory::InvalidResult,
        "Monte Carlo produced a non-finite parameter, path or result",
    )
}

fn scaled_value(scale: f64, weight: f64, log_discount: f64) -> Result<f64> {
    let value = scale * weight;
    let discount = log_discount.exp();
    let result = if scale == 0.0 || weight == 0.0 {
        0.0
    } else if discount.is_normal() && value.is_normal() {
        value * discount
    } else {
        (scale.ln() + weight.abs().ln() + log_discount)
            .exp()
            .copysign(weight)
    };
    if !result.is_finite() {
        return Err(unstable());
    }
    Ok(result)
}

pub(crate) fn random_seed() -> Result<u64> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|error| Error::operational(error.into()))?;
    Ok(u64::from_ne_bytes(bytes))
}

#[derive(Clone, Copy, Default)]
struct Moments {
    count: usize,
    scale: f64,
    mean: f64,
    m2: f64,
}

impl Moments {
    fn add(&mut self, value: f64) {
        // Normalize before accumulating so finite prices survive overflowing sums and squares.
        if value.abs() > self.scale {
            let ratio = self.scale / value.abs();
            self.mean *= ratio;
            self.m2 *= ratio * ratio;
            self.scale = value.abs();
        }
        self.count += 1;
        let value = if self.scale == 0.0 {
            0.0
        } else {
            value / self.scale
        };
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (value - self.mean);
    }

    fn merge(&mut self, other: Self) {
        if self.count == 0 {
            *self = other;
            return;
        }
        let scale = self.scale.max(other.scale);
        let left = if scale == 0.0 {
            0.0
        } else {
            self.scale / scale
        };
        let right = if scale == 0.0 {
            0.0
        } else {
            other.scale / scale
        };
        let delta = other.mean * right - self.mean * left;
        let count = self.count + other.count;
        self.mean = self.mean * left + delta * (other.count as f64 / count as f64);
        self.m2 = self.m2 * left * left
            + other.m2 * right * right
            + delta * delta * (self.count as f64 * (other.count as f64 / count as f64));
        self.count = count;
        self.scale = scale;
    }
}

// PCG-XSH-RR, ported from the existing CPU generator; see THIRD_PARTY_NOTICES.txt.
struct Pcg32 {
    state: u64,
    increment: u64,
}
impl Pcg32 {
    fn new(seed: u64, stream: u64) -> Self {
        let mut result = Self {
            state: 0,
            increment: (stream << 1) | 1,
        };
        result.next();
        result.state = result.state.wrapping_add(seed);
        result.next();
        result
    }
    fn next(&mut self) -> u32 {
        let previous = self.state;
        self.state = previous
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.increment);
        (((previous >> 18 ^ previous) >> 27) as u32).rotate_right((previous >> 59) as u32)
    }
    fn uniform(&mut self) -> f64 {
        let bits = (u64::from(self.next()) << 20) | u64::from(self.next() >> 12);
        // The half-unit offset keeps Box-Muller inputs strictly inside (0, 1).
        (bits as f64 + 0.5) / 4503599627370496.0
    }
}
