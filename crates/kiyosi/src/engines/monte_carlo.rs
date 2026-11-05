use std::{
    f64::consts::TAU,
    io,
    sync::{Arc, Mutex},
    thread,
};

use rayon::prelude::*;

use crate::numerics::log_ratio;
use crate::{
    Error, EuropeanOption, GreeksRequest, NumericalShiftSettings, OptionContract, OptionType,
    PricingContext, PricingEngine, PricingResult, Result,
};

const BLOCK_SIZE: usize = 256;
const DEFAULT_STEP_COUNT: i32 = 50;

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
///
/// See [`MonteCarloVanillaEngine::new`] for a Sobol example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MonteCarloSampling {
    /// Uses pseudorandom PCG32 streams (the default).
    #[default]
    PseudoRandom,
    /// Uses seeded Sobol points with linear matrix scrambling (LMS) and a digital shift.
    Sobol,
}

/// Configures terminal, Asian, barrier, autocallable, accumulator or American CPU sampling.
///
/// Grid sizes belong to [`MonteCarloVanillaEngine::with_step_count`] and
/// [`MonteCarloAsianEngine::with_step_count`].
///
/// See [`MonteCarloVanillaEngine::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloSettings {
    /// Sets the exact path count, from 1 through 10,000,000; defaults to 131,072.
    pub path_count: i32,
    /// Selects a reproducible stream, or system entropy when absent (the default).
    pub seed: Option<u64>,
    /// Caps CPU workers per call; defaults to 0 (automatic).
    ///
    /// One runs on the calling thread. Positive caps are also limited by
    /// available CPUs and simulation blocks.
    /// Parallel calls share a library-owned pool; see [`MonteCarloVanillaEngine`].
    pub thread_count: i32,
    /// Selects pseudorandom (default) or Sobol sampling; Sobol requires power-of-two paths.
    pub sampling: MonteCarloSampling,
    /// Pairs Z and -Z (default `false`), keeping exact paths and one unpaired path if odd.
    pub antithetic: bool,
}

impl Default for MonteCarloSettings {
    fn default() -> Self {
        Self {
            path_count: 131_072,
            seed: None,
            thread_count: 0,
            sampling: MonteCarloSampling::PseudoRandom,
            antithetic: false,
        }
    }
}

/// Configures sampling and the time grid for vanilla or Asian Monte Carlo engines.
///
/// Engine construction validates sampling and product-specific grid limits.
/// See [`MonteCarloVanillaEngine::settings`] for a complete configuration round-trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloGridSettings {
    /// Sets the shared path, seed, thread and sampling controls.
    pub sampling: MonteCarloSettings,
    /// Sets integration or exercise intervals; defaults to 50.
    ///
    /// Vanilla construction accepts 1 through 10,000; Asian construction requires
    /// at least 2. American exercise pricing also requires at least 2.
    pub step_count: i32,
}

impl Default for MonteCarloGridSettings {
    fn default() -> Self {
        Self {
            sampling: MonteCarloSettings::default(),
            step_count: DEFAULT_STEP_COUNT,
        }
    }
}

impl From<MonteCarloSettings> for MonteCarloGridSettings {
    fn from(sampling: MonteCarloSettings) -> Self {
        Self {
            sampling,
            ..Default::default()
        }
    }
}

impl MonteCarloSettings {
    fn with_resolved_seed(self) -> Result<Self> {
        Ok(Self {
            seed: Some(self.resolved_seed()?),
            ..self
        })
    }

    fn resolved_seed(self) -> Result<u64> {
        if let Some(seed) = self.seed {
            return Ok(seed);
        }
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).map_err(|error| Error::Io(error.into()))?;
        Ok(u64::from_ne_bytes(bytes))
    }

    /// Validates sampling controls without constructing an engine or drawing randomness.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for paths outside 1 through 10,000,000,
    /// a negative worker cap, or a non-power-of-two Sobol path count.
    /// Leaves the settings unchanged; engine-specific step counts are validated separately.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloSampling, MonteCarloSettings};
    /// # fn main() -> kiyosi::Result<()> {
    /// let settings = MonteCarloSettings {
    ///     path_count: 4096, sampling: MonteCarloSampling::Sobol, ..Default::default()
    /// };
    /// settings.validate()?;
    /// # Ok(()) }
    /// ```
    pub fn validate(&self) -> Result<()> {
        if !(1..=10_000_000).contains(&self.path_count) {
            return Err(Error::InvalidInput(
                "path_count must be between 1 and 10000000".into(),
            ));
        }
        if self.thread_count < 0 {
            return Err(Error::InvalidInput(
                "thread_count must be nonnegative".into(),
            ));
        }
        if self.sampling == MonteCarloSampling::Sobol
            && !self.path_count.unsigned_abs().is_power_of_two()
        {
            return Err(Error::InvalidInput(
                "path_count must be a power of two for Sobol sampling".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_seeds_are_preserved() {
        for seed in [0, 42, u64::MAX] {
            let settings = MonteCarloSettings {
                path_count: 256,
                seed: Some(seed),
                thread_count: 1,
                sampling: MonteCarloSampling::Sobol,
                antithetic: true,
            };
            assert_eq!(settings.resolved_seed().unwrap(), seed);
            assert_eq!(settings.with_resolved_seed().unwrap(), settings);
        }
    }

    #[test]
    fn unseeded_settings_pin_one_seed_and_preserve_other_controls() {
        let settings = MonteCarloSettings {
            path_count: 256,
            thread_count: 1,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            ..Default::default()
        };
        let seeded = settings.with_resolved_seed().unwrap();
        assert!(seeded.seed.is_some());
        assert_eq!(seeded.path_count, 256);
        assert_eq!(seeded.thread_count, 1);
        assert_eq!(seeded.sampling, MonteCarloSampling::Sobol);
        assert!(seeded.antithetic);
        assert_eq!(seeded.with_resolved_seed().unwrap(), seeded);
        assert_eq!(settings.seed, None);
    }

    #[test]
    fn moments_collect_variance_only_when_requested() {
        for variance in [false, true] {
            let mut sample = Moments::default();
            for value in [-2.0, 4.0, 8.0] {
                sample.add(value, variance);
            }
            let mut merged = Moments::default();
            for value in [-2.0, 4.0, 8.0] {
                let mut part = Moments::default();
                part.add(value, variance);
                merged.merge(part, variance);
            }
            for result in [sample, merged] {
                assert_eq!(result.count, 3);
                assert_eq!(result.scale, 8.0);
                assert!((result.mean - 5.0 / 12.0).abs() < 1e-15);
                let expected = if variance { 19.0 / 24.0 } else { 0.0 };
                assert!((result.m2 - expected).abs() < 1e-15);
            }
        }
    }

    #[test]
    #[expect(clippy::cast_sign_loss, reason = "test counts are nonnegative.")]
    fn statistics_mode_preserves_samples_and_mean_reductions() {
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            for path_count in [1, 2, 256, 257, 512] {
                if sampling == MonteCarloSampling::Sobol && path_count == 257 {
                    continue;
                }
                for antithetic in [false, true] {
                    for thread_count in [1, 2] {
                        let settings = MonteCarloSettings {
                            path_count,
                            seed: Some(42),
                            thread_count,
                            sampling,
                            antithetic,
                        };
                        let payoff = |normals: &[f64]| Ok([normals[0], normals[1] * 1e150]);
                        let (means, unused_pairs) =
                            path_samples(settings, 2, false, payoff).unwrap();
                        let (samples, pairs) = path_samples(settings, 2, true, payoff).unwrap();
                        for i in 0..2 {
                            assert_eq!(means[i].count, path_count as usize);
                            assert_eq!(means[i].scale.to_bits(), samples[i].scale.to_bits());
                            assert_eq!(means[i].mean.to_bits(), samples[i].mean.to_bits());
                            assert_eq!(means[i].m2, 0.0);
                            assert_eq!(unused_pairs[i].count, 0);
                            let statistics = sampling == MonteCarloSampling::PseudoRandom
                                && (!antithetic || path_count % 2 == 0);
                            if statistics && antithetic {
                                assert_eq!(pairs[i].count, path_count as usize / 2);
                            } else {
                                assert_eq!(pairs[i].count, 0);
                            }
                            if !statistics || antithetic {
                                assert_eq!(samples[i].m2, 0.0);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn serial_blocks_stay_on_the_calling_thread() {
        let caller = thread::current().id();
        for count in [0, 1, 513] {
            let mut blocks = vec![(usize::MAX, caller); count];
            run_blocks(&mut blocks, 1, |block| (block, thread::current().id())).unwrap();
            assert_eq!(
                blocks,
                (0..count).map(|block| (block, caller)).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    #[expect(clippy::cast_sign_loss, reason = "test counts are nonnegative.")]
    fn parallel_blocks_reuse_a_private_pool_and_respect_each_calls_cap() {
        let available = thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let pool = worker_pool(available).unwrap();
        assert!(Arc::ptr_eq(&pool, &worker_pool(available).unwrap()));
        let external = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        external.install(|| {
            let caller = thread::current().id();
            for thread_count in [0, 2, 4, i32::MAX] {
                let mut blocks = vec![(usize::MAX, caller, false); 513];
                run_blocks(&mut blocks, thread_count, |block| {
                    (
                        block,
                        thread::current().id(),
                        pool.current_thread_index().is_some(),
                    )
                })
                .unwrap();
                let mut workers = std::collections::HashSet::new();
                for (expected, (block, worker, in_pool)) in blocks.into_iter().enumerate() {
                    assert_eq!(block, expected);
                    assert_eq!(in_pool, available > 1);
                    assert_eq!(worker == caller, available == 1);
                    workers.insert(worker);
                }
                let cap = if thread_count == 0 {
                    available
                } else {
                    thread_count as usize
                };
                assert!(workers.len() <= cap.min(available));
            }
            assert_eq!(rayon::current_num_threads(), 1);
        });
    }

    #[test]
    fn block_panics_propagate_and_leave_the_pool_usable() {
        let panic = std::panic::catch_unwind(|| {
            run_blocks(&mut [0; 513], 2, |block| {
                if block == 3 {
                    std::panic::panic_any("Monte Carlo worker panic");
                }
                block
            })
            .unwrap();
        })
        .unwrap_err();
        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"Monte Carlo worker panic")
        );
        let mut blocks = [0; 513];
        run_blocks(&mut blocks, 2, |block| block).unwrap();
        assert_eq!(blocks, std::array::from_fn(|block| block));
    }

    #[test]
    fn pool_creation_preserves_native_os_errors_after_partial_startup() {
        let error = build_pool(2, |worker| {
            if worker.index() == 1 {
                Err(io::Error::from_raw_os_error(13))
            } else {
                thread::Builder::new()
                    .spawn(move || worker.run())
                    .map(|_| ())
            }
        })
        .unwrap_err();
        let Error::Io(native) = error else {
            panic!("native I/O cause was lost")
        };
        assert_eq!(native.raw_os_error(), Some(13));
        assert_eq!(native.kind(), io::Error::from_raw_os_error(13).kind());
    }

    #[test]
    fn pool_creation_preserves_custom_io_payloads() {
        let error = build_pool(1, |_| {
            Err(io::Error::new(io::ErrorKind::Interrupted, std::fmt::Error))
        })
        .unwrap_err();
        let Error::Io(native) = error else {
            panic!("native I/O cause was lost")
        };
        assert_eq!(native.kind(), io::ErrorKind::Interrupted);
        assert!(native.get_ref().unwrap().is::<std::fmt::Error>());
    }
}

/// Prices European and American vanilla contracts with flat-rate CPU sampling.
///
/// European contracts sample the exact terminal distribution without stepping.
/// American contracts use least-squares Monte Carlo (Longstaff-Schwartz) with
/// strike-scaled quadratic regression of continuation values on a
/// uniform grid, reducing to linear or constant fits for rank-deficient samples.
/// Calls with nonpositive dividends and nonnegative rates use terminal sampling,
/// retaining the immediate-exercise lower bound. See [`Self::default_american_spot_shift`].
/// Sobol uses one dimension for terminal sampling or `step_count` for exercise
/// grids, and `ceil(path_count / 2)` points with antithetic pairing.
/// Fixed seeds reproduce results across thread counts within the same build.
/// Shared and distinct instances permit concurrent calls without mutation;
/// every call owns its buffers. Parallel calls reuse a library-owned Rayon pool,
/// sized from available CPU parallelism on first use and retained until process exit.
/// Concurrent calls share these workers; a per-call cap does not reserve workers.
/// Neither the application's global Rayon pool nor `RAYON_NUM_THREADS` configures
/// this pool. Serial calls run on the calling thread without initializing the pool.
/// Forking after pool initialization does not preserve usable workers in the child;
/// use a fresh process, such as Python's `spawn` multiprocessing start method.
/// See [`Self::price`] for an example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloVanillaEngine {
    settings: MonteCarloSettings,
    step_count: i32,
}

impl Default for MonteCarloVanillaEngine {
    fn default() -> Self {
        Self {
            settings: MonteCarloSettings::default(),
            step_count: DEFAULT_STEP_COUNT,
        }
    }
}

fn validate_step_count(step_count: i32, minimum: i32) -> Result<()> {
    if !(minimum..=10_000).contains(&step_count) {
        return Err(Error::InvalidInput(
            "step_count is outside the supported Monte Carlo range".into(),
        ));
    }
    Ok(())
}

/// Owns a Monte Carlo price and its optional sampling uncertainty.
///
/// Uses unbiased sample variance and a two-sided asymptotic 95% normal confidence interval
/// for pseudorandom draws, or independent pair means with even antithetic counts;
/// sparse samples and heavy tails can make the interval unreliable.
/// Sobol always leaves uncertainty absent. Simulated payoffs with fewer than two
/// independent observations or odd antithetic counts also leave uncertainty absent.
/// Known pseudorandom settlements, fixed cash flows and resolved analytic branches
/// have zero uncertainty. Sampling uncertainty does not measure numerical integration
/// or model approximation error.
/// See [`MonteCarloVanillaEngine::price_with_statistics`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonteCarloEstimate {
    price: f64,
    standard_error: Option<f64>,
    confidence_interval: Option<(f64, f64)>,
    uncertainty_status: MonteCarloUncertaintyStatus,
}

/// Explains uncertainty availability; see [`MonteCarloEstimate::uncertainty_status`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonteCarloUncertaintyStatus {
    /// Reports a finite standard error, including known zero uncertainty.
    Available,
    /// Reports Sobol sampling, which has no independent-sample error estimator.
    UnsupportedSampling,
    /// Reports fewer than two independent samples or antithetic pair means.
    InsufficientObservations,
    /// Reports an odd antithetic path count with an unmatched observation.
    IncompleteAntitheticPair,
}

impl MonteCarloUncertaintyStatus {
    /// Returns the Python status string; see [`MonteCarloEstimate::uncertainty_status`].
    pub const fn name(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::UnsupportedSampling => "unsupported_sampling",
            Self::InsufficientObservations => "insufficient_observations",
            Self::IncompleteAntitheticPair => "incomplete_antithetic_pair",
        }
    }
}

impl MonteCarloEstimate {
    /// Creates an estimate from a finite mean, optional standard error and its status.
    ///
    /// Computes the asymptotic 95% normal interval when uncertainty is available.
    /// See [`MonteCarloVanillaEngine::price_with_statistics`] for reporting units.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ComputationFailure`] for non-finite values, a negative
    /// standard error, inconsistent status or confidence bounds that overflow.
    pub fn from_statistics(
        price: f64,
        standard_error: Option<f64>,
        uncertainty_status: MonteCarloUncertaintyStatus,
    ) -> Result<Self> {
        if !price.is_finite()
            || standard_error.is_some_and(|error| !error.is_finite() || error < 0.0)
            || standard_error.is_some()
                != (uncertainty_status == MonteCarloUncertaintyStatus::Available)
        {
            return Err(unstable());
        }
        let confidence_interval = standard_error.map(|error| {
            let half_width = 1.959_963_984_540_054 * error;
            (price - half_width, price + half_width)
        });
        if confidence_interval
            .is_some_and(|(lower, upper)| !lower.is_finite() || !upper.is_finite())
        {
            return Err(unstable());
        }
        Ok(Self {
            price,
            standard_error,
            confidence_interval,
            uncertainty_status,
        })
    }

    fn from_sampling(
        price: f64,
        standard_error: Option<f64>,
        settings: MonteCarloSettings,
    ) -> Result<Self> {
        let status = if standard_error.is_some() {
            MonteCarloUncertaintyStatus::Available
        } else if settings.sampling == MonteCarloSampling::Sobol {
            MonteCarloUncertaintyStatus::UnsupportedSampling
        } else if settings.antithetic && settings.path_count % 2 != 0 {
            MonteCarloUncertaintyStatus::IncompleteAntitheticPair
        } else {
            MonteCarloUncertaintyStatus::InsufficientObservations
        };
        Self::from_statistics(price, standard_error, status)
    }

    /// Returns availability or the reason sampling uncertainty is absent.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloEstimate, MonteCarloUncertaintyStatus};
    /// # fn main() -> kiyosi::Result<()> {
    /// let result = MonteCarloEstimate::from_statistics(10.0, None,
    ///     MonteCarloUncertaintyStatus::InsufficientObservations)?;
    /// assert_eq!(result.uncertainty_status().name(), "insufficient_observations");
    /// # Ok(()) }
    /// ```
    pub fn uncertainty_status(&self) -> MonteCarloUncertaintyStatus {
        self.uncertainty_status
    }

    /// Returns the finite sample mean. See [`MonteCarloVanillaEngine::price_with_statistics`].
    pub fn price(&self) -> f64 {
        self.price
    }
    /// Returns the standard error of the price estimate, if estimable. See [`Self`] for applicability.
    pub fn standard_error(&self) -> Option<f64> {
        self.standard_error
    }
    /// Returns the untruncated asymptotic 95% confidence interval, if estimable. See [`Self`].
    pub fn confidence_interval(&self) -> Option<(f64, f64)> {
        self.confidence_interval
    }
}

impl MonteCarloVanillaEngine {
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
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::MonteCarloVanillaEngine;
    /// # fn main() -> kiyosi::Result<()> {
    /// let engine = MonteCarloVanillaEngine::default().with_resolved_seed()?;
    /// assert!(engine.sampling_settings().seed.is_some());
    /// assert_eq!(engine.with_resolved_seed()?, engine);
    /// # Ok(()) }
    /// ```
    pub fn with_resolved_seed(&self) -> crate::Result<Self> {
        Ok(Self {
            settings: self.settings.with_resolved_seed()?,
            ..*self
        })
    }

    /// Creates an engine from sampling controls or complete sampling and grid settings.
    ///
    /// Sampling-only inputs use 50 exercise intervals. Complete inputs preserve their grid.
    /// See [`Self::settings`] for reconstruction or [`Self::with_step_count`] for a grid override.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for paths outside 1..=10,000,000,
    /// a negative thread cap, or a non-power-of-two Sobol count.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloSettings, MonteCarloVanillaEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
    ///     seed: Some(42), thread_count: 4, ..Default::default()
    /// })?;
    /// assert_eq!(engine.sampling_settings().path_count, 131_072);
    /// let sobol = MonteCarloVanillaEngine::new(MonteCarloSettings {
    ///     path_count: 32768, sampling: kiyosi::MonteCarloSampling::Sobol,
    ///     antithetic: true, seed: Some(42), ..Default::default()
    /// })?;
    /// assert!(sobol.sampling_settings().antithetic);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: impl Into<MonteCarloGridSettings>) -> Result<Self> {
        let settings = settings.into();
        Self::with_step_count(settings.sampling, settings.step_count)
    }

    /// Creates an engine with a configured American exercise interval count.
    ///
    /// European pricing samples the terminal distribution and does not use this grid.
    /// American pricing before expiry requires at least two intervals.
    /// American exercise-grid pricing limits retained path storage to 1 GiB per call;
    /// see [`Self::price`] for the size calculation and validation timing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for steps outside 1..=10,000,
    /// then reuses [`Self::new`] sampling validation. Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloSettings, MonteCarloVanillaEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let engine = MonteCarloVanillaEngine::with_step_count(MonteCarloSettings::default(), 100)?;
    /// assert_eq!(engine.step_count(), 100);
    /// let rebuilt = MonteCarloVanillaEngine::with_step_count(
    ///     engine.sampling_settings(), engine.step_count())?;
    /// assert_eq!(rebuilt, engine);
    /// # Ok(()) }
    /// ```
    pub fn with_step_count(settings: MonteCarloSettings, step_count: i32) -> Result<Self> {
        validate_step_count(step_count, 1)?;
        settings.validate()?;
        Ok(Self {
            settings,
            step_count,
        })
    }

    /// Returns complete sampling and exercise-grid settings.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloSettings, MonteCarloVanillaEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let engine = MonteCarloVanillaEngine::with_step_count(MonteCarloSettings::default(), 100)?;
    /// assert_eq!(MonteCarloVanillaEngine::new(engine.settings())?, engine);
    /// let mut settings = engine.settings();
    /// settings.sampling.path_count = 4096;
    /// let changed = MonteCarloVanillaEngine::new(settings)?;
    /// assert_eq!(changed.step_count(), 100);
    /// # Ok(()) }
    /// ```
    pub fn settings(&self) -> MonteCarloGridSettings {
        MonteCarloGridSettings {
            sampling: self.settings,
            step_count: self.step_count,
        }
    }

    /// Returns the American exercise interval count; see [`Self::with_step_count`].
    pub fn step_count(&self) -> i32 {
        self.step_count
    }

    /// Returns owned sampling settings; the grid count is available from [`Self::step_count`].
    ///
    /// See [`Self::with_step_count`] for configuration reconstruction.
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns finite-difference Greeks using shared normal samples.
    ///
    /// Pins one request-local seed, including when the engine has no configured seed.
    /// Replays identical normal draws for every feasible bump; no samples or state
    /// survive the request. At expiry, time Greeks remain absent and other Greeks follow terminal payoffs. Uses
    /// [`NumericalShiftSettings`] and [`crate::numerical_greeks`] availability rules.
    /// American calls whose rate stencil crosses zero use the exercise grid on both
    /// rate legs. For American spot bumps, use [`Self::default_american_spot_shift`].
    ///
    /// # Errors
    ///
    /// Rejects nonconstant rate curves before validating requests and shifts, then propagates
    /// [`crate::numerical_greeks`] and [`Self::price`] errors. Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// See [`Self::price`] for a shared-sample delta example.
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        context.bsm_parameters()?;
        <Self as PricingEngine<I>>::price_with_greeks(self, option, context, request)
    }

    /// Returns a discounted sample mean, or intrinsic value at expiry.
    ///
    /// Near-strike payoffs retain small movements even when terminal spot rounds to strike.
    /// Calls complete synchronously without cancellation or signal polling.
    /// American exercise grids retain `path_count * step_count` values of
    /// eight bytes each, rounding odd antithetic path counts up to the next even
    /// count for reserved capacity. Retained path storage is limited to 1 GiB per
    /// call, excluding other buffers. Expiry and terminal-only American calls
    /// (nonpositive dividends and nonnegative rates) do not allocate this grid.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves before life
    /// validation, [`Error::InvalidInput`] outside the inclusive contract
    /// life, or [`Error::ComputationFailure`] for unstable parameters, nonpositive
    /// or non-finite terminal prices, or non-finite results. Inputs remain unchanged.
    /// Entropy and thread-creation failures retain their native [`std::io::Error`]
    /// in [`Error::Io`].
    /// American pricing before expiry returns [`Error::InvalidInput`]
    /// for fewer than two exercise intervals, after life validation.
    /// Exercise-grid pricing also returns [`Error::InvalidInput`] when retained
    /// path storage would exceed 1 GiB, before entropy, worker or path allocation.
    /// Non-finite American regression matrices, coefficients or continuation values
    /// return [`Error::ComputationFailure`]; finite rank-deficient fits reduce their degree.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloSettings, MonteCarloVanillaEngine, EuropeanOption,
    ///     OptionType, Date, BlackScholesMertonParameters, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings { seed: Some(42), ..Default::default() })?;
    /// assert!((engine.price(&option, &context)? - 9.227).abs() < 0.2);
    /// let analysis = engine.price_with_greeks(&option, &context,
    ///     kiyosi::GreeksRequest::new([kiyosi::Greek::Delta]))?;
    /// assert!(analysis.require(kiyosi::Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        context.bsm_parameters()?;
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns the price, standard error and asymptotic 95% confidence interval.
    ///
    /// Uses the same samples and reduction order as [`Self::price`].
    /// Leaves uncertainty absent for Sobol and insufficient independent samples;
    /// see [`MonteCarloEstimate`] for antithetic and expiry rules.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors and returns [`Error::ComputationFailure`]
    /// when uncertainty or interval endpoints are non-finite. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work, as in [`Self::price`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use kiyosi::*;
    /// # fn main() -> kiyosi::Result<()> {
    /// # let start = Date::new(2025, 1, 1)?.start_of_day();
    /// # let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
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
        context.bsm_parameters()?;
        let (sample, pairs, log_discount) = self.sample(option, context, true)?;
        estimate(
            self.settings,
            sample,
            pairs,
            log_discount,
            context.valuation_time() == option.expiry_time()
                || crate::engines::deterministic::enabled(context),
        )
    }

    fn sample(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        statistics: bool,
    ) -> Result<(Moments, Moments, f64)> {
        let model = context.bsm_parameters()?;
        context.validate_curve_horizon(option.expiry_time())?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_time();
        crate::instruments::monitoring::validate_life(option, context)?;
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            let mut sample = Moments::default();
            sample.add(
                (sign * (context.spot_price() - option.strike())).max(0.0),
                false,
            );
            return Ok((sample, Moments::default(), 0.0));
        }
        let sigma = model.volatility();
        if sigma == 0.0 {
            let mut sample = Moments::default();
            sample.add(
                crate::engines::deterministic::vanilla(
                    option,
                    crate::engines::Payoff::vanilla(option.option_type(), option.strike(), false),
                    context,
                )?,
                false,
            );
            return Ok((sample, Moments::default(), 0.0));
        }
        let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
        let diffusion = sigma * time.sqrt();
        if !drift.is_finite() || !diffusion.is_finite() {
            return Err(unstable());
        }
        let moneyness = log_ratio(context.spot_price(), option.strike());
        let (total, pairs) = terminal_samples(self.settings, statistics, |normal| {
            let increment = diffusion.mul_add(normal, drift);
            let terminal = context.spot_price() * increment.exp();
            if !terminal.is_finite() || terminal <= 0.0 {
                return Err(unstable());
            }
            let distance = moneyness + increment;
            let payoff = if distance.abs() < 0.5 {
                option.strike() * distance.exp_m1()
            } else {
                terminal - option.strike()
            };
            Ok([(sign * payoff).max(0.0)])
        })?;
        let [total] = total;
        let [pairs] = pairs;
        Ok((total, pairs, -rate * time))
    }
}

impl PricingEngine for MonteCarloVanillaEngine {
    fn price_with_statistics(
        &self,
        option: &EuropeanOption,
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
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: self.settings.with_resolved_seed()?,
            ..*self
        })))
    }
    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        let (sample, _, log_discount) = self.sample(option, context, false)?;
        scaled_value(sample.scale, sample.mean, log_discount)
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Sample counts are bounded by the validated path limit of ten million and fit exactly in f64."
)]
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
    MonteCarloEstimate::from_sampling(price, standard_error, settings)
}

fn terminal_samples<const N: usize>(
    settings: MonteCarloSettings,
    statistics: bool,
    payoff: impl Fn(f64) -> Result<[f64; N]> + Sync,
) -> Result<([Moments; N], [Moments; N])> {
    path_samples(settings, 1, statistics, |normals| payoff(normals[0]))
}

pub(crate) fn path_means<const N: usize>(
    settings: MonteCarloSettings,
    dimensions: usize,
    statistics: bool,
    payoff: impl Fn(&[f64]) -> Result<[f64; N]> + Sync,
) -> Result<([f64; N], Option<f64>)> {
    let (moments, pairs) = path_samples(settings, dimensions, statistics, payoff)?;
    let mut means = [0.0; N];
    for (mean, moment) in means.iter_mut().zip(&moments) {
        *mean = scaled_value(moment.scale, moment.mean, 0.0)?;
    }
    let standard_error = if statistics {
        estimate(settings, moments[0], pairs[0], 0.0, false)?.standard_error()
    } else {
        None
    };
    Ok((means, standard_error))
}

#[expect(
    clippy::cast_sign_loss,
    reason = "Settings validate positive path counts before allocating sample buffers."
)]
fn path_samples<const N: usize>(
    settings: MonteCarloSettings,
    dimensions: usize,
    statistics: bool,
    payoff: impl Fn(&[f64]) -> Result<[f64; N]> + Sync,
) -> Result<([Moments; N], [Moments; N])> {
    let seed = settings.resolved_seed()?;
    let paths = settings.path_count as usize;
    let samples = if settings.antithetic {
        paths.div_ceil(2)
    } else {
        paths
    };
    let statistics = statistics
        && settings.sampling == MonteCarloSampling::PseudoRandom
        && (!settings.antithetic || paths % 2 == 0);
    let variance = statistics && !settings.antithetic;
    let sobol = if settings.sampling == MonteCarloSampling::Sobol {
        Some(sobol::SobolSequence::new(dimensions, seed)?)
    } else {
        None
    };
    let empty = [Moments::default(); N];
    // Seed and merge fixed blocks in index order so worker counts cannot change the result.
    let mut partials: Vec<_> = (0..samples.div_ceil(BLOCK_SIZE))
        .map(|_| Ok((empty, empty)))
        .collect();
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
                *normal = sample_normal(&mut generator, &mut spare, &mut cursor);
            }
            if let Some(cursor) = &mut cursor {
                cursor.next_path();
            }
            let positive = payoff(&normals)?;
            for i in 0..N {
                moments[i].add(positive[i], variance);
            }
            if settings.antithetic && path < paths / 2 {
                for normal in &mut normals {
                    *normal = -*normal;
                }
                let negative = payoff(&normals)?;
                for i in 0..N {
                    moments[i].add(negative[i], variance);
                    if statistics {
                        pairs[i].add(positive[i].midpoint(negative[i]), true);
                    }
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
            total[i].merge(payoffs[i], variance);
            if statistics && settings.antithetic {
                pairs[i].merge(pair_means[i], true);
            }
        }
    }
    Ok((total, pairs))
}

fn sample_normal(
    generator: &mut Pcg32,
    spare: &mut Option<f64>,
    cursor: &mut Option<sobol::SobolCursor<'_>>,
) -> f64 {
    if let Some(cursor) = cursor {
        cursor.normal()
    } else if let Some(value) = spare.take() {
        value
    } else {
        let radius = (-2.0 * generator.uniform().ln()).sqrt();
        let (sin, cos) = (TAU * generator.uniform()).sin_cos();
        *spare = Some(radius * sin);
        radius * cos
    }
}

#[expect(
    clippy::cast_sign_loss,
    reason = "Settings validate nonnegative thread counts before selecting workers."
)]
fn run_blocks<T: Send>(
    partials: &mut [T],
    thread_count: i32,
    simulate: impl Fn(usize) -> T + Sync,
) -> Result<()> {
    let available = thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let workers = if thread_count == 0 {
        available
    } else {
        (thread_count as usize).min(available)
    }
    .min(partials.len());
    if workers <= 1 {
        for (block, partial) in partials.iter_mut().enumerate() {
            *partial = simulate(block);
        }
        return Ok(());
    }
    let pool = worker_pool(available)?;
    let chunk_size = partials
        .len()
        .div_ceil(workers.min(pool.current_num_threads()));
    pool.install(|| {
        // Each chunk runs sequentially, keeping the number of active workers within the cap.
        partials
            .par_chunks_mut(chunk_size)
            .enumerate()
            .for_each(|(chunk, values)| {
                for (index, value) in values.iter_mut().enumerate() {
                    *value = simulate(chunk * chunk_size + index);
                }
            });
    });
    Ok(())
}

fn worker_pool(available: usize) -> Result<Arc<rayon::ThreadPool>> {
    static POOL: Mutex<Option<Arc<rayon::ThreadPool>>> = Mutex::new(None);
    let mut shared = POOL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(pool) = &*shared {
        return Ok(Arc::clone(pool));
    }
    let pool = Arc::new(build_pool(available, |worker| {
        thread::Builder::new()
            .name("kiyosi-monte-carlo".into())
            .spawn(move || worker.run())
            .map(|_| ())
    })?);
    *shared = Some(Arc::clone(&pool));
    Ok(pool)
}

fn build_pool(
    workers: usize,
    mut spawn: impl FnMut(rayon::ThreadBuilder) -> io::Result<()>,
) -> Result<rayon::ThreadPool> {
    let mut spawn_error = None;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .spawn_handler(|worker| {
            spawn(worker).map_err(|error| {
                // Rayon exposes only a borrowed I/O cause; retain ownership for Error::Io.
                let kind = error.kind();
                spawn_error = Some(error);
                io::Error::from(kind)
            })
        })
        .build();
    pool.map_err(|error| Error::Io(spawn_error.unwrap_or_else(|| io::Error::other(error))))
}

pub(crate) fn unstable() -> Error {
    Error::ComputationFailure("Monte Carlo produced a non-finite parameter, path or result".into())
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

#[derive(Clone, Copy, Default)]
struct Moments {
    count: usize,
    scale: f64,
    mean: f64,
    m2: f64,
}

impl Moments {
    #[expect(
        clippy::cast_precision_loss,
        reason = "Sample counts are bounded by the validated path limit of ten million and fit exactly in f64."
    )]
    fn add(&mut self, value: f64, variance: bool) {
        // Normalize before accumulating so finite prices survive overflowing sums and squares.
        if value.abs() > self.scale {
            let ratio = self.scale / value.abs();
            self.mean *= ratio;
            if variance {
                self.m2 *= ratio * ratio;
            }
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
        if variance {
            self.m2 += delta * (value - self.mean);
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "Sample counts are bounded by the validated path limit of ten million and fit exactly in f64."
    )]
    fn merge(&mut self, other: Self, variance: bool) {
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
        if variance {
            self.m2 = self.m2 * left * left
                + other.m2 * right * right
                + delta * delta * (self.count as f64 * (other.count as f64 / count as f64));
        }
        self.count = count;
        self.scale = scale;
    }
}

// PCG-XSH-RR pseudorandom number generator.
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
    #[expect(
        clippy::cast_possible_truncation,
        reason = "PCG XSH-RR intentionally retains the low 32 bits before rotation."
    )]
    fn next(&mut self) -> u32 {
        let previous = self.state;
        self.state = previous
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.increment);
        (((previous >> 18 ^ previous) >> 27) as u32).rotate_right((previous >> 59) as u32)
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "The random word contains 52 bits and converts exactly to f64."
    )]
    fn uniform(&mut self) -> f64 {
        let bits = (u64::from(self.next()) << 20) | u64::from(self.next() >> 12);
        // The half-unit offset keeps Box-Muller inputs strictly inside (0, 1).
        (bits as f64 + 0.5) / 4_503_599_627_370_496.0
    }
}
