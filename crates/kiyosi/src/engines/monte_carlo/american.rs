use std::{cell::RefCell, marker::PhantomData, sync::Arc};

use super::{
    BLOCK_SIZE, BlockRunner, Error, EuropeanOption, GreeksRequest, Moments, MonteCarloSampling,
    MonteCarloSettings, MonteCarloVanillaEngine, NormalDraws, NumericalShiftSettings, OptionType,
    Pcg32, PricingContext, PricingEngine, PricingResult, Result, check_path_step, scaled_value,
    sobol, unstable,
};
use crate::risk::numerical::symmetric_shift;
use crate::{
    AmericanOption, AmericanOptionChanges, BlackScholesMertonParameterChanges, Greek,
    OptionContract, PricingContextChanges,
};

const WORKSPACE_BUDGET: usize = 1 << 30;
const WORK_BUDGET: usize = 1_000_000_000;
const PREFERRED_PATH_WORKSPACE: usize = 256 << 20;
const FULL_PATH_OVERHEAD: usize = 8 << 20;
const MAX_CACHED_PATH_VALUES: usize = 128 * 1024 * 1024 / size_of::<f64>();
const PATH_BLOCK_OVERHEAD: usize = size_of::<Vec<f64>>() + 2 * size_of::<usize>();

#[derive(PartialEq)]
struct PathKey {
    seed: u64,
    settings: super::MonteCarloSettings,
    intervals: usize,
    path_values: usize,
}

impl PathKey {
    fn storage_bytes(&self) -> usize {
        let blocks = self.settings.sample_count().div_ceil(BLOCK_SIZE);
        self.path_values * size_of::<f64>()
            + blocks * (PATH_BLOCK_OVERHEAD + size_of::<Arc<Vec<f64>>>())
    }
}

type PathBlocks = Arc<Vec<Arc<Vec<f64>>>>;
type PathCache = Vec<(PathKey, PathBlocks)>;

thread_local! {
    static PATH_CACHE: RefCell<Option<PathCache>> = const { RefCell::new(None) };
}

pub(crate) struct AmericanPathCache {
    active: bool,
    previous: Option<PathCache>,
    _thread: PhantomData<*mut ()>,
}

impl AmericanPathCache {
    pub(crate) fn for_greeks(request: GreeksRequest) -> Self {
        let spot_bumps = [
            Greek::Delta,
            Greek::Gamma,
            Greek::Speed,
            Greek::Vanna,
            Greek::Zomma,
            Greek::Charm,
            Greek::Color,
        ];
        Self::for_path_sharing(spot_bumps.into_iter().any(|greek| request.has(greek)))
    }

    pub(crate) fn for_path_sharing(active: bool) -> Self {
        let previous = if active {
            PATH_CACHE.with(|cache| cache.replace(Some(Vec::new())))
        } else {
            None
        };
        Self {
            active,
            previous,
            _thread: PhantomData,
        }
    }
}

impl Drop for AmericanPathCache {
    fn drop(&mut self) {
        if self.active {
            PATH_CACHE.with(|cache| cache.replace(self.previous.take()));
        }
    }
}

fn american_workspace<T>(capacity: usize) -> Result<Vec<T>> {
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(capacity).map_err(|_| {
        Error::ComputationFailure(
            "cannot reserve American Monte Carlo workspace; reduce paths, steps or concurrent calls"
                .into(),
        )
    })?;
    Ok(buffer)
}

struct ExerciseBlock<const N: usize, const S: usize = N> {
    coordinates: Arc<Vec<f64>>,
    checkpoints: Vec<f64>,
    reverse_draws: Option<NormalDraws<'static>>,
    spots: Vec<[f64; S]>,
    cash: Vec<[f64; N]>,
    regression: [[[f64; 4]; 3]; N],
}

fn reverse_workspace<const N: usize, const S: usize>(
    settings: MonteCarloSettings,
    reserved_paths: usize,
    samples: usize,
    intervals: usize,
    extra_bytes: usize,
) -> Result<(usize, usize)> {
    let blocks = samples.div_ceil(BLOCK_SIZE);
    let sampler = if settings.sampling == MonteCarloSampling::Sobol {
        sobol::validate_dimensions(intervals)?;
        intervals * sobol::SEQUENCE_BYTES_PER_DIMENSION
    } else {
        0
    };
    let bytes = samples * size_of::<f64>()
        + reserved_paths * (size_of::<[f64; N]>() + size_of::<[f64; S]>())
        + blocks * (size_of::<ExerciseBlock<N, S>>() + PATH_BLOCK_OVERHEAD)
        + sampler
        + extra_bytes;
    if bytes > WORKSPACE_BUDGET {
        return Err(Error::InvalidInput(
            "American Monte Carlo workspace exceeds 1 GiB; reduce path_count or step_count".into(),
        ));
    }
    if samples
        .saturating_mul(intervals)
        .saturating_add(reserved_paths.saturating_mul(N * (intervals - 1)))
        > WORK_BUDGET
    {
        return Err(Error::InvalidInput(
            "American Monte Carlo exceeds one billion simulated path steps and exercise updates; reduce path_count or step_count".into(),
        ));
    }
    Ok((1, bytes))
}

fn exercise_window<const N: usize, const S: usize>(
    settings: MonteCarloSettings,
    reserved_paths: usize,
    stored_paths: usize,
    intervals: usize,
    extra_bytes: usize,
) -> Result<(usize, usize)> {
    let blocks = settings.sample_count().div_ceil(BLOCK_SIZE);
    let sobol = settings.sampling == MonteCarloSampling::Sobol;
    let workers = if sobol {
        sobol::validate_dimensions(intervals)?;
        let available = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let limit = settings.max_active_workers.unsigned_abs() as usize;
        blocks.min(if limit == 0 {
            available
        } else {
            available.min(limit)
        })
    } else {
        0
    };
    let workspace = |window| {
        let checkpoints = (intervals - 1) / window;
        let cursor_bytes = intervals * size_of::<u64>()
            + if checkpoints == 0 {
                0
            } else {
                intervals.div_ceil(sobol::LAZY_GROUP) * size_of::<usize>()
            };
        let sampling = if sobol {
            intervals * sobol::SEQUENCE_BYTES_PER_DIMENSION + workers * cursor_bytes
        } else {
            0
        };
        // Include the cache's shared block handles when a full path matrix is retained.
        let overhead =
            size_of::<ExerciseBlock<N, S>>() + PATH_BLOCK_OVERHEAD + size_of::<Arc<Vec<f64>>>();
        stored_paths
            .saturating_mul((window + checkpoints) * size_of::<f64>())
            .saturating_add(reserved_paths * (size_of::<[f64; N]>() + size_of::<[f64; S]>()))
            .saturating_add(blocks * overhead)
            .saturating_add(sampling)
            .saturating_add(extra_bytes)
    };
    let minimum_window = intervals.isqrt();
    let minimum_bytes = workspace(minimum_window);
    if minimum_bytes > WORKSPACE_BUDGET {
        return Err(Error::InvalidInput(
            "American Monte Carlo workspace exceeds 1 GiB; reduce path_count or step_count".into(),
        ));
    }
    // Keep a complete matrix when only its cash, block or sampler bookkeeping
    // would force another pass through the paths.
    let full = workspace(intervals);
    let matrix = stored_paths.saturating_mul(intervals * size_of::<f64>());
    let preferred = if matrix <= PREFERRED_PATH_WORKSPACE
        && full <= PREFERRED_PATH_WORKSPACE + FULL_PATH_OVERHEAD
    {
        full
    } else {
        PREFERRED_PATH_WORKSPACE
    };
    let target = preferred.max(minimum_bytes);
    let mut low = minimum_window;
    let mut high = intervals;
    while low < high {
        let candidate = low + (high - low).div_ceil(2);
        if workspace(candidate) <= target {
            low = candidate;
        } else {
            high = candidate - 1;
        }
    }
    // Balance windows so the last one, which stays resident from the original
    // simulation, is about as long as the earlier windows that need replay.
    let balanced = intervals.div_ceil(intervals.div_ceil(low));
    let window = if workspace(balanced) <= target {
        balanced
    } else {
        low
    };
    let bytes = workspace(window);
    let replay = (intervals - 1) / window * window;
    // Count original and replayed path steps, then one exercise update per scenario and step.
    let work = stored_paths
        .saturating_mul(intervals + replay)
        .saturating_add(reserved_paths.saturating_mul(N * (intervals - 1)));
    if work > WORK_BUDGET {
        return Err(Error::InvalidInput(
            "American Monte Carlo exceeds one billion simulated path steps and exercise updates; reduce path_count or step_count".into(),
        ));
    }
    Ok((window, bytes))
}

impl<const N: usize, const S: usize> Default for ExerciseBlock<N, S> {
    fn default() -> Self {
        Self {
            coordinates: Arc::default(),
            checkpoints: Vec::new(),
            reverse_draws: None,
            spots: Vec::new(),
            cash: Vec::new(),
            regression: [[[0.0; 4]; 3]; N],
        }
    }
}

struct ExercisePaths<'a> {
    settings: MonteCarloSettings,
    paths: usize,
    samples: usize,
    intervals: usize,
    window: usize,
    seed: u64,
    anchor: Option<f64>,
    growth: [f64; 2],
    sobol: Option<&'a sobol::SobolSequence>,
}

impl ExercisePaths<'_> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "Exercise steps are bounded by 10,000."
    )]
    fn reverse_row<const N: usize, const S: usize>(
        &self,
        block_index: usize,
        block: &mut ExerciseBlock<N, S>,
        step: usize,
    ) {
        let elapsed = step + 1;
        let dimension = self.intervals - elapsed;
        // In units of one time step, W_m | W_(m+1) has mean m/(m+1)
        // times W_(m+1) and variance m/(m+1).
        let [weight, deviation] = if dimension == 0 {
            [0.0, (elapsed as f64).sqrt()]
        } else {
            let fraction = elapsed as f64 / (elapsed + 1) as f64;
            [fraction, fraction.sqrt()]
        };
        let coordinates =
            Arc::get_mut(&mut block.coordinates).expect("reverse paths are request-owned");
        let begin = block_index * BLOCK_SIZE;
        if let Some(sequence) = self.sobol {
            let normals = sequence.dimension_normals(dimension, begin, coordinates.len());
            for (coordinate, normal) in coordinates.iter_mut().zip(normals) {
                *coordinate = weight.mul_add(*coordinate, deviation * normal);
            }
        } else {
            let draws = block.reverse_draws.get_or_insert_with(|| NormalDraws {
                generator: Pcg32::new(self.seed, block_index as u64),
                spare: None,
                cursor: None,
            });
            for coordinate in coordinates {
                *coordinate = weight.mul_add(*coordinate, deviation * draws.normal());
            }
        }
    }

    fn first_in_last_window(&self) -> usize {
        (self.intervals - 1) / self.window * self.window
    }

    fn block_path_count(&self, begin: usize, end: usize) -> usize {
        end - begin + self.paired_paths(begin, end)
    }

    fn paired_paths(&self, begin: usize, end: usize) -> usize {
        if self.settings.antithetic {
            end.min(self.paths / 2).saturating_sub(begin)
        } else {
            0
        }
    }

    fn block_coordinate_count(&self, begin: usize, end: usize) -> usize {
        // With no spot anchor, antithetic draws accumulate opposite Brownian
        // coordinates. Only anchored spot evolution needs both histories.
        if self.anchor.is_none() {
            end - begin
        } else {
            self.block_path_count(begin, end)
        }
    }

    fn visit_row(
        &self,
        block: usize,
        values: &[f64],
        mut visit: impl FnMut(f64) -> Result<()>,
    ) -> Result<()> {
        let begin = block * BLOCK_SIZE;
        let paired = if self.anchor.is_none() {
            self.paired_paths(begin, (begin + BLOCK_SIZE).min(self.samples))
        } else {
            0
        };
        let (paired, unpaired) = values.split_at(paired);
        for &coordinate in paired {
            visit(coordinate)?;
            visit(-coordinate)?;
        }
        for &coordinate in unpaired {
            visit(coordinate)?;
        }
        Ok(())
    }

    fn advance(&self, value: &mut f64, normal: f64) -> Result<f64> {
        if self.anchor.is_some() {
            let [drift, diffusion] = self.growth;
            return evolve_spot(value, diffusion.mul_add(normal, drift).exp());
        }
        *value += normal;
        Ok(*value)
    }

    fn fill(
        &self,
        block: usize,
        steps: std::ops::Range<usize>,
        mut initial: impl FnMut(usize) -> f64,
        mut save: impl FnMut(usize, usize, f64),
    ) -> Result<()> {
        if self.sobol.is_none() {
            return self.fill_pseudorandom(block, steps, initial, save);
        }
        let begin = block * BLOCK_SIZE;
        let end = (begin + BLOCK_SIZE).min(self.samples);
        let cursor = self.sobol.map(|sequence| {
            let mut cursor = sequence.cursor(begin);
            if steps.len() != self.intervals {
                cursor.enable_skips();
                cursor.seek(steps.start);
            }
            cursor
        });
        let mut draws = NormalDraws {
            generator: Pcg32::new(self.seed, block as u64),
            spare: None,
            cursor,
        };
        draws.skip_normals(steps.start);
        let mut lane = 0;
        for sample in begin..end {
            crate::cancellation::check()?;
            let paired =
                self.anchor.is_some() && self.settings.antithetic && sample < self.paths / 2;
            let mut positive = initial(lane);
            let mut negative = if paired { initial(lane + 1) } else { 0.0 };
            for step in steps.clone() {
                check_path_step(step)?;
                let normal = draws.normal();
                save(step, lane, self.advance(&mut positive, normal)?);
                if paired {
                    save(step, lane + 1, self.advance(&mut negative, -normal)?);
                }
            }
            lane += 1 + usize::from(paired);
            if let Some(cursor) = &mut draws.cursor {
                cursor.next_path();
                cursor.seek(steps.start);
            } else {
                draws.skip_normals(self.intervals - steps.len());
            }
        }
        Ok(())
    }

    fn fill_pseudorandom(
        &self,
        block: usize,
        steps: std::ops::Range<usize>,
        mut initial: impl FnMut(usize) -> f64,
        mut save: impl FnMut(usize, usize, f64),
    ) -> Result<()> {
        const TILE: usize = 16;
        let begin = block * BLOCK_SIZE;
        let end = (begin + BLOCK_SIZE).min(self.samples);
        let mut lane = 0;
        let generator = Pcg32::new(self.seed, block as u64);
        for first in (begin..end).step_by(TILE) {
            crate::cancellation::check()?;
            let count = TILE.min(end - first);
            let mut draws: [NormalDraws<'_>; TILE] = std::array::from_fn(|i| {
                let mut draws = NormalDraws {
                    generator,
                    spare: None,
                    cursor: None,
                };
                // Assign each lane the original path's position in the block's stream.
                draws.skip_normals((first + i - begin) * self.intervals + steps.start);
                draws
            });
            let mut paths = [(0, false); TILE];
            let mut positive = [0.0; TILE];
            let mut negative = [0.0; TILE];
            for i in 0..count {
                let paired =
                    self.anchor.is_some() && self.settings.antithetic && first + i < self.paths / 2;
                paths[i] = (lane, paired);
                positive[i] = initial(lane);
                if paired {
                    negative[i] = initial(lane + 1);
                }
                lane += 1 + usize::from(paired);
            }
            for step in steps.clone() {
                check_path_step(step)?;
                for (i, &(lane, paired)) in paths[..count].iter().enumerate() {
                    let normal = draws[i].normal();
                    save(step, lane, self.advance(&mut positive[i], normal)?);
                    if paired {
                        save(step, lane + 1, self.advance(&mut negative[i], -normal)?);
                    }
                }
            }
        }
        Ok(())
    }

    fn generate(&self, block: usize, count: usize) -> Result<(Vec<f64>, Vec<f64>)> {
        let last = self.first_in_last_window();
        let mut coordinates = american_workspace(count * self.window)?;
        coordinates.resize(count * (self.intervals - last), 0.0);
        let mut checkpoints = american_workspace(count * (last / self.window))?;
        checkpoints.resize(count * (last / self.window), 0.0);
        self.fill(
            block,
            0..self.intervals,
            |_| self.anchor.unwrap_or(0.0),
            |step, lane, value| {
                if step >= last {
                    coordinates[(step - last) * count + lane] = value;
                } else if (step + 1) % self.window == 0 {
                    checkpoints[((step + 1) / self.window - 1) * count + lane] = value;
                }
            },
        )?;
        Ok((coordinates, checkpoints))
    }

    fn replay<const N: usize, const S: usize>(
        &self,
        index: usize,
        block: &mut ExerciseBlock<N, S>,
        start: usize,
    ) -> Result<()> {
        let begin = index * BLOCK_SIZE;
        let width = self.block_coordinate_count(begin, (begin + BLOCK_SIZE).min(self.samples));
        let coordinates =
            Arc::get_mut(&mut block.coordinates).expect("checkpointed paths are request-owned");
        coordinates.resize(width * self.window, 0.0);
        let initial = |lane| {
            if start == 0 {
                self.anchor.unwrap_or(0.0)
            } else {
                block.checkpoints[(start / self.window - 1) * width + lane]
            }
        };
        self.fill(
            index,
            start..start + self.window,
            initial,
            |step, lane, value| {
                coordinates[(step - start) * width + lane] = value;
            },
        )
    }
}
#[test]
fn antithetic_coordinates_fit_without_window_replay() -> Result<()> {
    let paths = 262_144_usize;
    let intervals = 160;
    let settings = MonteCarloSettings {
        path_count: 262_144,
        antithetic: true,
        ..Default::default()
    };
    assert!(exercise_window::<1, 1>(settings, paths, paths, intervals, 0)?.0 < intervals);
    assert_eq!(
        exercise_window::<1, 1>(settings, paths, paths / 2, intervals, 0)?.0,
        intervals
    );
    Ok(())
}

fn evolve_spot(spot: &mut f64, growth: f64) -> Result<f64> {
    *spot *= growth;
    if !spot.is_finite() || *spot <= 0.0 {
        return Err(unstable());
    }
    Ok(*spot)
}

impl MonteCarloVanillaEngine {
    /// Returns the American default absolute spot bump, 8% of spot.
    ///
    /// Exercise decisions amplify small-bump noise; sampling error and bump bias
    /// require convergence checks. Other shifts use [`NumericalShiftSettings`] defaults.
    /// Rust and Python use this value before expiry when `spot_shift` is automatic
    /// (`None`). Terminal payoffs use the smaller of 0.01 spot units and 1% of spot.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use kiyosi::*;
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloVanillaEngine::new(kiyosi::MonteCarloGridSettings {
    ///     sampling: MonteCarloSettings { seed: Some(42), ..Default::default() },
    ///     ..Default::default()
    /// })?;
    /// let result = engine.price_with_greeks(&put, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]))?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn default_american_spot_shift(context: &PricingContext) -> f64 {
        0.08 * context.spot_price()
    }

    /// Fits an American exercise rule and prices it on a separate set of paths.
    ///
    /// The configured path count applies to valuation; training uses the supplied
    /// count and a separate seed derived from the engine seed. Valuation streams each
    /// path until exercise without retaining the training matrix. Fitting error is
    /// part of the estimator; check convergence in both training and valuation counts.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for an invalid training count or work limit,
    /// and propagates applicability, validation, sampling and cancellation errors.
    ///
    /// # Panics
    ///
    /// Propagates panics after parallel sampling finishes.
    pub fn price_american_out_of_sample(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        training_path_count: i32,
    ) -> Result<f64> {
        let mut training = *self;
        training.settings.sampling.path_count = training_path_count;
        training.settings.sampling.validate()?;
        let intervals = self.settings.step_count.unsigned_abs();
        let work =
            u64::from(self.settings.sampling.path_count.unsigned_abs()) * u64::from(intervals);
        if work > WORK_BUDGET as u64 {
            return Err(Error::InvalidInput(
                "American Monte Carlo valuation exceeds one billion path steps".into(),
            ));
        }
        let valuation = self.with_resolved_seed()?;
        let seed = valuation.settings.sampling.resolved_seed()?;
        training.settings.sampling.seed = Some(seed ^ 0x9e37_79b9_7f4a_7c15);
        let mut policy = Vec::new();
        let (_, used_seed) = training.american_spots(option, [context], true, Some(&mut policy))?;
        if used_seed.is_none() || policy.is_empty() {
            return valuation.price(option, context);
        }
        let (rate, dividend) = crate::engines::flat_rates(context)?;
        let sigma = context.natural_bsm_parameters()?.volatility();
        let time = context.valuation_time().years_until(option.expiry_time());
        let dt = time / f64::from(intervals);
        let drift = (rate - dividend - 0.5 * sigma * sigma) * dt;
        let diffusion = sigma * dt.sqrt();
        let strike = option.strike();
        let spot = context.spot_price();
        let payoff = crate::engines::Payoff::vanilla(option.option_type(), strike, true);
        let ([moments], _, _) = super::path_samples_skipping(
            valuation.settings.sampling,
            intervals as usize,
            false,
            |draws| {
                let mut current = spot;
                for (step, &[coefficients]) in (0..intervals).zip(&policy) {
                    check_path_step(step as usize)?;
                    evolve_spot(&mut current, diffusion.mul_add(draws.normal(), drift).exp())?;
                    let intrinsic = payoff.value(current);
                    if intrinsic > 0.0 {
                        if let Some([constant, linear, quadratic]) = coefficients {
                            let x = current / strike;
                            let continuation = constant + x * (linear + x * quadratic);
                            if !continuation.is_finite() {
                                return Err(unstable());
                            }
                            if intrinsic / strike > continuation {
                                let elapsed = dt * f64::from(step + 1);
                                return Ok([scaled_value(1.0, intrinsic, -rate * elapsed)?]);
                            }
                        }
                    }
                }
                evolve_spot(&mut current, diffusion.mul_add(draws.normal(), drift).exp())?;
                Ok([scaled_value(1.0, payoff.value(current), -rate * time)?])
            },
        )?;
        Ok(scaled_value(moments.scale, moments.mean, 0.0)?.max(payoff.value(spot)))
    }

    /// Prices American strikes on the same simulated spot paths.
    ///
    /// Compatible quotes share each spot row and fit separate continuation
    /// regressions. Prices preserve input order and duplicates. The request uses
    /// one seed when sampling is required. Call [`Self::with_resolved_seed`] first
    /// to replay an unseeded engine's draws.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = start.add_days(365)?;
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    /// let market = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start,
    /// )?;
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloGridSettings {
    ///     sampling: MonteCarloSettings { path_count: 4096, seed: Some(42), ..Default::default() },
    ///     ..Default::default()
    /// })?;
    /// let prices = engine.price_american_strikes(&put, &market, &[90.0, 110.0])?;
    /// assert!(prices[0] < prices[1]);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Propagates input, market, sampling and cancellation errors.
    /// A failed request returns no partial prices.
    ///
    /// # Panics
    ///
    /// Propagates panics after parallel sampling finishes.
    pub fn price_american_strikes(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        crate::engines::flat_rates(context)?;
        crate::instruments::monitoring::validate_life(option, context)?;
        context.validate_curve_horizon(option.expiry_time())?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        let mut prices = american_workspace(strikes.len())?;
        if strikes.is_empty() {
            return Ok(prices);
        }
        let engine = if crate::engines::deterministic::enabled(context)
            || context.valuation_time() == option.expiry_time()
        {
            *self
        } else {
            if self.settings.step_count < 2 {
                return Err(Error::InvalidInput(
                    "American Monte Carlo requires at least two exercise intervals".into(),
                ));
            }
            self.with_resolved_seed()?
        };
        let _paths = AmericanPathCache::for_path_sharing(strikes.len() > 4);
        let _sobol = sobol::RequestCache::new();
        let mut remaining = strikes;
        while let Some(&strike) = remaining.first() {
            crate::cancellation::check()?;
            let mut quoted = false;
            macro_rules! quote {
                ($count:literal) => {
                    if let Some(strikes) = remaining.first_chunk::<$count>() {
                        match engine.american_scenarios::<$count, 1>(
                            option,
                            [context; $count],
                            *strikes,
                            true,
                            None,
                        ) {
                            Ok((batch, _)) => {
                                prices.extend(batch);
                                remaining = &remaining[$count..];
                                quoted = true;
                            }
                            Err(Error::InvalidInput(_) | Error::ComputationFailure(_)) => {}
                            Err(error) => return Err(error),
                        }
                    }
                };
            }
            quote!(4);
            if !quoted {
                quote!(2);
            }
            if !quoted {
                let contract = option.replace(AmericanOptionChanges {
                    strike: Some(strike),
                    ..Default::default()
                })?;
                prices.push(engine.american_price(&contract, context, true)?.0);
                remaining = &remaining[1..];
            }
        }
        Ok(prices)
    }

    fn american_price(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        terminal_only: bool,
    ) -> Result<(f64, Option<u64>)> {
        let ([price], seed) = self.american_spots(option, [context], terminal_only, None)?;
        Ok((price, seed))
    }

    fn same_time_greek_prices(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<Vec<(PricingContext, PricingResult)>> {
        let request = GreeksRequest::new(Greek::ALL.into_iter().filter(|greek| {
            request.has(*greek) && !matches!(greek, Greek::Theta | Greek::Charm | Greek::Color)
        }));
        if Greek::ALL
            .into_iter()
            .filter(|&greek| request.has(greek))
            .count()
            < 2
        {
            return Ok(Vec::new());
        }
        let scenarios =
            match crate::risk::numerical::greek_scenarios(option, context, request, shifts) {
                Ok(scenarios) => scenarios,
                Err(error @ Error::Io(_)) => return Err(error),
                Err(_) => return Ok(Vec::new()),
            };
        if scenarios.len() <= 3 {
            return Ok(Vec::new());
        }
        // Call stencils crossing the no-early-exercise shortcut keep the
        // dedicated rate handling in the scalar Greek evaluator.
        for scenario in &scenarios {
            let model = scenario.natural_bsm_parameters()?;
            if model.volatility() == 0.0
                || (option.option_type() == OptionType::Call
                    && model.flat_risk_free_rate()? >= 0.0
                    && model.flat_dividend_yield()? <= 0.0)
            {
                return Ok(Vec::new());
            }
        }
        let mut results = Vec::with_capacity(scenarios.len());
        let mut start = 0;
        while start < scenarios.len() {
            let end = (start + 9).min(scenarios.len());
            let pending = &scenarios[start..end];
            macro_rules! sample {
                ($n:literal) => {
                    self.greek_scenario_group::<$n>(option, pending)?
                };
            }
            let batch = if let Some(batch) = sample!(9) {
                Some(batch)
            } else if let Some(batch) = sample!(7) {
                Some(batch)
            } else if let Some(batch) = sample!(5) {
                Some(batch)
            } else if let Some(batch) = sample!(3) {
                Some(batch)
            } else if let Some(batch) = sample!(2) {
                Some(batch)
            } else {
                sample!(1)
            };
            let Some(batch) = batch else {
                return Ok(Vec::new());
            };
            start += batch.len();
            results.extend(batch);
        }
        Ok(scenarios.into_iter().zip(results).collect())
    }

    fn greek_scenario_group<const N: usize>(
        &self,
        option: &AmericanOption,
        pending: &[PricingContext],
    ) -> Result<Option<Vec<PricingResult>>> {
        let Some(contexts) = pending.first_chunk::<N>() else {
            return Ok(None);
        };
        let settings = self.settings.sampling;
        let samples = settings.sample_count();
        let paths = samples * if settings.antithetic { 2 } else { 1 };
        let intervals = self.settings.step_count.unsigned_abs() as usize;
        if intervals < 2 {
            return Ok(None);
        }
        let workspace =
            if samples.saturating_mul(intervals) > PREFERRED_PATH_WORKSPACE / size_of::<f64>() {
                reverse_workspace::<N, N>(settings, paths, samples, intervals, 0)
            } else {
                exercise_window::<N, N>(settings, paths, samples, intervals, 0)
            };
        if workspace.is_err() {
            return Ok(None);
        }
        let (prices, seed) = match self.american_spots::<N>(
            option,
            std::array::from_fn(|i| &contexts[i]),
            true,
            None,
        ) {
            Ok(sample) => sample,
            Err(error @ Error::Io(_)) => return Err(error),
            Err(_) => return Ok(None),
        };
        prices
            .into_iter()
            .map(|price| Ok(PricingResult::new(price)?.with_seed(seed)))
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }

    fn american_spots<const N: usize>(
        &self,
        option: &AmericanOption,
        contexts: [&PricingContext; N],
        terminal_only: bool,
        policy: Option<&mut Vec<[Option<[f64; 3]>; N]>>,
    ) -> Result<([f64; N], Option<u64>)> {
        self.american_scenarios::<N, N>(
            option,
            contexts,
            [option.strike(); N],
            terminal_only,
            policy,
        )
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "Validated positive i32 path and step counts bound the buffers and fit exactly in f64."
    )]
    fn american_scenarios<const N: usize, const S: usize>(
        &self,
        option: &AmericanOption,
        contexts: [&PricingContext; N],
        strikes: [f64; N],
        terminal_only: bool,
        mut policy: Option<&mut Vec<[Option<[f64; 3]>; N]>>,
    ) -> Result<([f64; N], Option<u64>)> {
        let context = contexts[0];
        let initial = contexts.map(PricingContext::spot_price);
        let settings = self.settings.sampling;
        let step_count = self.settings.step_count;
        let contract = |strike| {
            option.replace(AmericanOptionChanges {
                strike: Some(strike),
                ..Default::default()
            })
        };
        let scalar = || {
            let mut prices = [0.0; N];
            let mut seed = None;
            for ((price, context), strike) in prices.iter_mut().zip(contexts).zip(strikes) {
                (*price, seed) = self.american_price(&contract(strike)?, context, terminal_only)?;
            }
            Ok((prices, seed))
        };
        let mut parameters = [[0.0; 3]; N];
        for (values, scenario) in parameters.iter_mut().zip(contexts) {
            if scenario.valuation_time() != context.valuation_time() {
                return scalar();
            }
            let model = scenario.natural_bsm_parameters()?;
            scenario.validate_curve_horizon(option.expiry_time())?;
            *values = [
                model.flat_risk_free_rate()?,
                model.flat_dividend_yield()?,
                model.volatility(),
            ];
        }
        let same_growth = parameters
            .iter()
            .all(|values| values.map(f64::to_bits) == parameters[0].map(f64::to_bits));
        if N > 1
            && !same_growth
            && parameters.iter().any(|&[rate, dividend, sigma]| {
                sigma == 0.0
                    || (terminal_only
                        && option.option_type() == OptionType::Call
                        && dividend <= 0.0
                        && rate >= 0.0)
            })
        {
            return scalar();
        }
        let [rate, dividend, _] = parameters[0];
        if crate::engines::deterministic::enabled(context) {
            let mut prices = [0.0; N];
            for ((price, context), strike) in prices.iter_mut().zip(contexts).zip(strikes) {
                let payoff = crate::engines::Payoff::vanilla(option.option_type(), strike, true);
                *price = crate::engines::deterministic::vanilla(option, payoff, context)?;
            }
            return Ok((prices, None));
        }
        crate::instruments::monitoring::validate_life(option, context)?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_time();
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let payoff = |spot: f64, strike: f64| (sign * (spot - strike)).max(0.0);
        let immediate = std::array::from_fn(|i| payoff(initial[i], strikes[i]));
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            return Ok((immediate, None));
        }
        if step_count < 2 {
            return Err(Error::InvalidInput(
                "American Monte Carlo requires at least two exercise intervals".into(),
            ));
        }
        if terminal_only
            && option.option_type() == OptionType::Call
            && dividend <= 0.0
            && rate >= 0.0
        {
            let mut prices = immediate;
            let mut seed = None;
            for ((price, context), strike) in prices.iter_mut().zip(contexts).zip(strikes) {
                let contract = contract(strike)?;
                let (value, used_seed) = <Self as PricingEngine<EuropeanOption>>::price_with_seed(
                    self,
                    contract.vanilla(),
                    context,
                )?;
                *price = price.max(value);
                seed = used_seed;
            }
            return Ok((prices, seed));
        }
        let intervals = step_count as usize;
        let paths = settings.path_count as usize;
        let samples = if settings.antithetic {
            paths.div_ceil(2)
        } else {
            paths
        };
        let reserved_paths = samples * if settings.antithetic { 2 } else { 1 };
        let dt = time / intervals as f64;
        let growth = parameters.map(|[rate, dividend, sigma]| {
            [
                (rate - dividend - 0.5 * sigma * sigma) * dt,
                sigma * dt.sqrt(),
            ]
        });
        let shared_diffusion: [usize; N] = std::array::from_fn(|i| {
            growth[..i]
                .iter()
                .position(|previous| previous[1].to_bits() == growth[i][1].to_bits())
                .unwrap_or(i)
        });
        let log_discount = parameters.map(|[rate, _, _]| -rate * dt);
        let discount = log_discount.map(f64::exp);
        if growth
            .iter()
            .flatten()
            .chain(&discount)
            .any(|v| !v.is_finite())
        {
            return Err(unstable());
        }
        // Both normal generators are bounded by ten standard deviations.
        let normalized = initial
            .iter()
            .zip(growth)
            .all(|(spot, [drift, diffusion])| {
                spot.ln().abs() + (drift.abs() + 10.0 * diffusion.abs()) * (intervals as f64)
                    < 600.0
            });
        if N > 1 && !normalized {
            return scalar();
        }
        let anchor = (!normalized).then_some(initial[0]);
        debug_assert!(
            S == N
                || (S == 1
                    && same_growth
                    && initial
                        .iter()
                        .all(|spot| spot.to_bits() == initial[0].to_bits()))
        );
        let strike_spot = |row: [f64; S], lane: usize| row[if S == 1 { 0 } else { lane }];
        let cutoff: [f64; N] = std::array::from_fn(|i| {
            let log = crate::numerics::log_ratio(strikes[i], initial[i]);
            // Leave a roundoff margin around the exercise boundary so the
            // original spot-space calculation decides the near-strike cases.
            sign * log - 32.0 * f64::EPSILON * (1.0 + log.abs())
        });
        // A rejected shared spot must be out of the money for every strike.
        let outside = strikes
            .into_iter()
            .fold(strikes[0], if sign > 0.0 { f64::min } else { f64::max });
        let spots_at = |step: usize| {
            let elapsed = (step + 1) as f64;
            let drifts = growth.map(|[drift, _]| drift * elapsed);
            let root = elapsed.sqrt();
            let mut group_cutoff = [f64::INFINITY; N];
            for (lane, &source) in shared_diffusion.iter().enumerate() {
                group_cutoff[source] = group_cutoff[source].min(cutoff[lane] - sign * drifts[lane]);
            }
            let screen: [bool; N] = std::array::from_fn(|i| {
                // Predictable screening is most useful on sparse exercise rows.
                group_cutoff[i] > 0.5 * growth[i][1] * root
            });
            // Use the same reconstruction in scalar and batched scenarios.
            let scales: [f64; N] = std::array::from_fn(|i| {
                if normalized {
                    initial[i] * drifts[i].exp()
                } else {
                    initial[i]
                }
            });
            move |coordinate: f64| {
                if !normalized {
                    [coordinate; S]
                } else if same_growth {
                    let log = growth[0][1] * coordinate;
                    if screen[0] && sign * log < group_cutoff[0] {
                        return std::array::from_fn(|i| if S == 1 { outside } else { strikes[i] });
                    }
                    let factor = log.exp();
                    std::array::from_fn(|i| scales[i] * factor)
                } else {
                    let mut factors = [None; N];
                    std::array::from_fn(|i| {
                        let source = shared_diffusion[i];
                        if source == i {
                            let log = growth[i][1] * coordinate;
                            if !screen[i] || sign * log >= group_cutoff[i] {
                                factors[i] = Some(log.exp());
                            }
                        }
                        factors[source].map_or(strikes[i], |factor| scales[i] * factor)
                    })
                }
            }
        };
        let policy_bytes = if policy.is_some() {
            intervals * size_of::<[Option<[f64; 3]>; N]>()
        } else {
            0
        };
        let stored_paths = if normalized { samples } else { reserved_paths };
        let path_values = stored_paths.saturating_mul(intervals);
        let reverse = normalized && path_values > PREFERRED_PATH_WORKSPACE / size_of::<f64>();
        let workspace = if reverse {
            reverse_workspace::<N, S>(settings, reserved_paths, samples, intervals, policy_bytes)
        } else {
            exercise_window::<N, S>(
                settings,
                reserved_paths,
                stored_paths,
                intervals,
                policy_bytes,
            )
        };
        let (window, workspace_bytes) = match workspace {
            Err(Error::InvalidInput(_)) if !same_growth => return scalar(),
            result => result?,
        };
        let seed = settings.resolved_seed()?;
        let key = PathKey {
            seed,
            settings,
            intervals,
            path_values,
        };
        let (retain_paths, cached_paths) = PATH_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let Some(cache) = cache.as_mut() else {
                return (false, None);
            };
            if let Some(index) = cache
                .iter()
                .position(|(previous, _)| normalized && window == intervals && *previous == key)
            {
                let entry = cache.remove(index);
                let found = Arc::clone(&entry.1);
                cache.push(entry);
                return (false, Some(found));
            }
            let retain = normalized && window == intervals && path_values <= MAX_CACHED_PATH_VALUES;
            let mut stored: usize = cache.iter().map(|(key, _)| key.storage_bytes()).sum();
            let retained_limit =
                MAX_CACHED_PATH_VALUES.saturating_sub(path_values) * size_of::<f64>();
            while stored > WORKSPACE_BUDGET - workspace_bytes
                || retain && (stored > retained_limit || cache.len() == 2)
            {
                stored -= cache.remove(0).0.storage_bytes();
            }
            (retain, None)
        });
        let sobol = if cached_paths.is_none() && settings.sampling == MonteCarloSampling::Sobol {
            Some(sobol::sequence(intervals, seed)?)
        } else {
            None
        };
        let exercise_paths = ExercisePaths {
            settings,
            paths,
            samples,
            intervals,
            window,
            seed,
            anchor,
            growth: growth[0],
            sobol: sobol.as_deref(),
        };
        let mut window_start = if reverse {
            0
        } else {
            exercise_paths.first_in_last_window()
        };
        let block_count = samples.div_ceil(BLOCK_SIZE);
        let mut blocks = american_workspace(block_count)?;
        blocks.resize_with(block_count, ExerciseBlock::default);
        let runner = BlockRunner::new(block_count, settings.max_active_workers)?;
        let terminal = spots_at(intervals - 1);
        runner.run(&mut blocks, |block_index, block| {
            let begin = block_index * BLOCK_SIZE;
            let end = ((block_index + 1) * BLOCK_SIZE).min(samples);
            let path_count = exercise_paths.block_path_count(begin, end);
            let coordinate_count = exercise_paths.block_coordinate_count(begin, end);
            block.coordinates = if reverse {
                let mut values = american_workspace(coordinate_count)?;
                values.resize(coordinate_count, 0.0);
                Arc::new(values)
            } else if let Some(cached) = &cached_paths {
                Arc::clone(&cached[block_index])
            } else {
                let (values, checkpoints) =
                    exercise_paths.generate(block_index, coordinate_count)?;
                block.checkpoints = checkpoints;
                Arc::new(values)
            };
            if reverse {
                exercise_paths.reverse_row(block_index, block, intervals - 1);
            }
            let mut spots = american_workspace(path_count)?;
            let mut cash = american_workspace(path_count)?;
            exercise_paths.visit_row(
                block_index,
                &block.coordinates[block.coordinates.len() - coordinate_count..],
                |coordinate| {
                    let row = terminal(coordinate);
                    spots.push(row);
                    cash.push(std::array::from_fn(|i| {
                        payoff(strike_spot(row, i), strikes[i])
                    }));
                    Ok(())
                },
            )?;
            block.spots = spots;
            block.cash = cash;
            Ok(())
        })?;
        if retain_paths {
            let paths = Arc::new(
                blocks
                    .iter()
                    .map(|block| Arc::clone(&block.coordinates))
                    .collect(),
            );
            PATH_CACHE.with(|cache| {
                if let Some(cache) = cache.borrow_mut().as_mut() {
                    cache.push((key, paths));
                }
            });
        }
        let apply_exercise =
            |spots: [f64; S], cash: &mut [f64; N], coefficients: &[Option<[f64; 3]>; N]| {
                for (i, (cash, coefficient)) in cash.iter_mut().zip(coefficients).enumerate() {
                    let spot = strike_spot(spots, i);
                    let strike = strikes[i];
                    let Some([constant, linear, quadratic]) = *coefficient else {
                        continue;
                    };
                    let intrinsic = payoff(spot, strike);
                    if intrinsic <= 0.0 {
                        continue;
                    }
                    let x = spot / strike;
                    let continuation = constant + x * (linear + x * quadratic);
                    if !continuation.is_finite() {
                        return Err(unstable());
                    }
                    if intrinsic / strike > continuation {
                        *cash = intrinsic;
                    }
                }
                Ok(())
            };
        let mut coefficients = [None; N];
        // Regression synchronizes all paths at every exercise date. Small
        // per-date rows are cheaper to scan serially even on long time grids.
        let regression = if paths < 8_192 {
            BlockRunner::new(block_count, 1)?
        } else {
            runner
        };
        if let Some(policy) = policy.as_mut() {
            policy.resize(intervals - 1, [None; N]);
        }
        regression.install(|| {
            for step in (0..intervals - 1).rev() {
                if step < window_start {
                    window_start -= window;
                    regression.run(&mut blocks, |index, block| {
                        exercise_paths.replay(index, block, window_start)
                    })?;
                }
                let row = if reverse { 0 } else { step - window_start };
                let spots_at_step = spots_at(step);
                let decide = coefficients.iter().any(Option::is_some);
                // Keep the previous date's reconstructed spots beside its cash so
                // each volatility scenario needs one exponential per path and date.
                regression.run(&mut blocks, |index, block| {
                    if reverse {
                        exercise_paths.reverse_row(index, block, step);
                    }
                    let mut matrices = [[[0.0; 4]; 3]; N];
                    let begin = index * BLOCK_SIZE;
                    let width = exercise_paths
                        .block_coordinate_count(begin, (begin + BLOCK_SIZE).min(samples));
                    let values = &block.coordinates[row * width..(row + 1) * width];
                    let mut states = block.spots.iter_mut().zip(&mut block.cash);
                    exercise_paths.visit_row(index, values, |value| {
                        let (spots, cash) = states
                            .next()
                            .expect("coordinates match their exercise states");
                        if decide {
                            apply_exercise(*spots, cash, &coefficients)?;
                        }
                        *spots = spots_at_step(value);
                        for (index, (cash, matrix)) in
                            cash.iter_mut().zip(&mut matrices).enumerate()
                        {
                            let spot = strike_spot(*spots, index);
                            let strike = strikes[index];
                            *cash *= discount[index];
                            if !cash.is_finite() {
                                return Err(unstable());
                            }
                            if payoff(spot, strike) > 0.0 {
                                let x = spot / strike;
                                let square = x * x;
                                let target = *cash / strike;
                                matrix[0][0] += 1.0;
                                matrix[0][1] += x;
                                matrix[0][2] += square;
                                matrix[1][1] += square;
                                matrix[1][2] += x * square;
                                matrix[2][2] += square * square;
                                matrix[0][3] += target;
                                matrix[1][3] += x * target;
                                matrix[2][3] += square * target;
                            }
                        }
                        Ok(())
                    })?;
                    block.regression = matrices;
                    Ok(())
                })?;
                // Merge fixed blocks in path order, independent of the worker count.
                let mut matrices = [[[0.0; 4]; 3]; N];
                for block in &blocks {
                    for (sum, partial) in matrices
                        .iter_mut()
                        .flatten()
                        .flatten()
                        .zip(block.regression.iter().flatten().flatten())
                    {
                        *sum += partial;
                    }
                }
                coefficients.fill(None);
                for (mut matrix, coefficient) in matrices.into_iter().zip(&mut coefficients) {
                    if matrix[0][0] == 0.0 {
                        continue;
                    }
                    if matrix.iter().flatten().any(|value| !value.is_finite()) {
                        return Err(unstable());
                    }
                    matrix[1][0] = matrix[0][1];
                    matrix[2][0] = matrix[0][2];
                    matrix[2][1] = matrix[1][2];
                    *coefficient = Some(solve_quadratic(matrix).ok_or_else(unstable)?);
                }
                if let Some(policy) = policy.as_mut() {
                    policy[step] = coefficients;
                }
            }
            Ok(())
        })?;
        let mut means = [Moments::default(); N];
        let decide = coefficients.iter().any(Option::is_some);
        for block in blocks {
            crate::cancellation::check()?;
            for (spots, mut cash) in block.spots.into_iter().zip(block.cash) {
                if decide {
                    apply_exercise(spots, &mut cash, &coefficients)?;
                }
                for (mean, value) in means.iter_mut().zip(cash) {
                    mean.add(value, false);
                }
            }
        }
        let mut prices = immediate;
        for ((price, mean), discount) in prices.iter_mut().zip(means).zip(log_discount) {
            *price = scaled_value(mean.scale, mean.mean, discount)?.max(*price);
        }
        Ok((prices, Some(seed)))
    }
}

impl PricingEngine<AmericanOption> for MonteCarloVanillaEngine {
    fn price_with_seed(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        self.american_price(option, context, true)
    }
    fn default_spot_shift(&self, _option: &AmericanOption, context: &PricingContext) -> f64 {
        Self::default_american_spot_shift(context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        <Self as PricingEngine<EuropeanOption>>::validate_applicability(self, context)
    }

    fn validate_solver_seed(&self) -> Result<()> {
        self.settings.sampling.validate_solver_seed()
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AmericanOption> + '_>>> {
        prepare_seeded_request!(self, self.settings.sampling)
    }

    fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        self.price_with_seed(option, context)
            .map(|(price, _)| price)
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let _sampling = super::RequestCache::new();
        let _growth = AmericanPathCache::for_greeks(request);
        context.natural_bsm_parameters()?;
        <Self as PricingEngine<AmericanOption>>::validate_applicability(self, context)?;
        settings.validate()?;
        if context.valuation_time() == option.expiry_time() {
            let result = PricingResult::new(self.price(option, context)?)?
                .with_request(request, GreeksRequest::all());
            return crate::risk::numerical::expiry_greeks(
                option,
                context,
                settings,
                result,
                |context| self.price(option, context),
            );
        }
        let settings = NumericalShiftSettings {
            spot_shift: Some(
                settings
                    .spot_shift
                    .unwrap_or_else(|| Self::default_american_spot_shift(context)),
            ),
            ..settings
        };
        let engine = self.with_resolved_seed()?;
        let shared = engine.same_time_greek_prices(option, context, request, settings)?;
        let lookup = |scenario: &PricingContext| {
            shared
                .iter()
                .find(|(key, _)| key == scenario)
                .map(|(_, result)| result.clone())
        };
        let spot = context.spot_price();
        let shift = symmetric_shift(spot, settings.spot_shift_at(spot));
        let speed_shift = 2.0 * shift;
        let speed = request.has(Greek::Speed)
            && speed_shift.is_finite()
            && spot > speed_shift
            && symmetric_shift(spot, speed_shift).to_bits() == speed_shift.to_bits()
            && !option.at_spot_discontinuity(context, speed_shift);
        let batch = if shared.is_empty()
            && (request.has(Greek::Delta) || request.has(Greek::Gamma) || speed)
            && shift > 0.0
            && spot > shift
            && !option.all_greeks_unavailable(context)
            && !option.at_spot_discontinuity(context, shift)
        {
            let shifted = |spot| {
                context.replace(PricingContextChanges {
                    spot_price: Some(spot),
                    ..Default::default()
                })
            };
            let up = shifted(spot + shift)?;
            let down = shifted(spot - shift)?;
            let (prices, seed) =
                engine.american_spots(option, [context, &up, &down], true, None)?;
            Some((up, down, prices, seed))
        } else {
            None
        };
        let mut volatility_batch = None;
        if shared.is_empty()
            && batch.is_none()
            && (request.has(Greek::Vega) || request.has(Greek::Volga))
            && !option.all_greeks_unavailable(context)
        {
            let model = context.natural_bsm_parameters()?;
            let volatility = model.volatility();
            let shift = symmetric_shift(volatility, settings.volatility_shift_at(volatility));
            if shift > 0.0 && volatility > shift && (2.0 * shift).is_finite() {
                let shifted = |volatility| {
                    context.replace(PricingContextChanges {
                        model_parameters: Some(
                            model
                                .replace(BlackScholesMertonParameterChanges {
                                    volatility: Some(volatility),
                                    ..Default::default()
                                })?
                                .into(),
                        ),
                        ..Default::default()
                    })
                };
                if let (Ok(up), Ok(down)) =
                    (shifted(volatility + shift), shifted(volatility - shift))
                {
                    match engine.american_spots(option, [context, &up, &down], true, None) {
                        Ok((prices, seed)) => volatility_batch = Some((up, down, prices, seed)),
                        Err(error @ Error::Io(_)) => return Err(error),
                        Err(_) => {} // Preserve scalar validation and error ordering.
                    }
                }
            }
        }
        let native = |scenario: &PricingContext, requested: GreeksRequest| {
            if let Some(result) = lookup(scenario) {
                return Ok(result);
            }
            let (price, seed) = match batch
                .as_ref()
                .or(volatility_batch.as_ref())
                .filter(|_| std::ptr::eq(scenario, context))
            {
                Some((_, _, [base, _, _], seed)) => (*base, *seed),
                None => engine.american_price(option, scenario, true)?,
            };
            let context = scenario;
            let mut result = PricingResult::new(price)?.with_seed(seed);
            if option.option_type() != OptionType::Call
                || super::super::deterministic::enabled(context)
            {
                return Ok(result);
            }
            let model = context.natural_bsm_parameters()?;
            let rate = model.flat_risk_free_rate()?;
            let dividend = model.flat_dividend_yield()?;
            for (greek, curve, can_switch) in [
                (Greek::Rho, model.risk_free_curve(), dividend <= 0.0),
                (Greek::DividendRho, model.dividend_curve(), rate >= 0.0),
            ] {
                if !requested.has(greek) || !can_switch {
                    continue;
                }
                let shift = crate::risk::numerical::symmetric_shift(
                    curve.shift_origin(),
                    settings.rate_shift,
                );
                if shift <= 0.0 || !(2.0 * shift).is_finite() {
                    continue;
                }
                let down = curve.shifted(-shift)?;
                let up = curve.shifted(shift)?;
                let Some((lo, hi)) = down.exact_flat_rate().zip(up.exact_flat_rate()) else {
                    continue;
                };
                let crosses_shortcut = if greek == Greek::Rho {
                    lo < 0.0 && hi >= 0.0
                } else {
                    lo <= 0.0 && hi > 0.0
                };
                if !crosses_shortcut {
                    continue;
                }
                // Keep the exercise-grid estimator on both sides of the terminal shortcut boundary.
                let evaluate = |curve: crate::RateTermStructure| {
                    let mut changes = BlackScholesMertonParameterChanges::default();
                    if greek == Greek::Rho {
                        changes.risk_free_rate = Some(curve.into());
                    } else {
                        changes.dividend_yield = Some(curve.into());
                    }
                    let shifted = context.replace(PricingContextChanges {
                        model_parameters: Some(model.replace(changes)?.into()),
                        ..Default::default()
                    })?;
                    Ok::<_, Error>(engine.american_price(option, &shifted, false)?.0)
                };
                result.set(
                    requested,
                    greek,
                    (evaluate(up)? - evaluate(down)?) / (2.0 * shift),
                )?;
            }
            Ok(result)
        };
        crate::risk::numerical::complete_greeks_with_pair(
            option,
            context,
            request,
            settings,
            false,
            native(context, request)?,
            native,
            |high, low, _| {
                if let Some(prices) = lookup(high).zip(lookup(low)) {
                    return Ok(Some(prices));
                }
                if let Some((_, _, [_, up, down], seed)) = batch
                    .iter()
                    .chain(volatility_batch.iter())
                    .find(|(up, down, _, _)| up == high && down == low)
                {
                    return Ok(Some((
                        PricingResult::new(*up)?.with_seed(*seed),
                        PricingResult::new(*down)?.with_seed(*seed),
                    )));
                }
                let ([up, down], seed) = engine.american_spots(option, [high, low], true, None)?;
                Ok(Some((
                    PricingResult::new(up)?.with_seed(seed),
                    PricingResult::new(down)?.with_seed(seed),
                )))
            },
        )
    }
}

// Reduce the basis when paths cannot identify quadratic continuation.
fn solve_quadratic(samples: [[f64; 4]; 3]) -> Option<[f64; 3]> {
    for degree in (0..=2).rev() {
        let mut matrix = samples;
        let scale = matrix[..=degree]
            .iter()
            .flat_map(|row| &row[..=degree])
            .fold(0.0_f64, |scale, value| scale.max(value.abs()));
        let mut singular = false;
        for column in 0..=degree {
            let pivot = (column..=degree).max_by(|&left, &right| {
                matrix[left][column]
                    .abs()
                    .total_cmp(&matrix[right][column].abs())
            })?;
            if !matrix[pivot][column].is_finite() {
                return None;
            }
            if matrix[pivot][column].abs() <= 1e-14 * scale {
                singular = true;
                break;
            }
            matrix.swap(column, pivot);
            let pivot_row = matrix[column];
            for row in column + 1..=degree {
                let factor = matrix[row][column] / matrix[column][column];
                for (value, pivot) in matrix[row][column..=degree]
                    .iter_mut()
                    .zip(&pivot_row[column..=degree])
                {
                    *value -= factor * pivot;
                }
                matrix[row][3] -= factor * matrix[column][3];
            }
        }
        if singular {
            continue;
        }
        let mut coefficients = [0.0; 3];
        for row in (0..=degree).rev() {
            let mut value = matrix[row][3];
            for (column, coefficient) in coefficients
                .iter()
                .enumerate()
                .take(degree + 1)
                .skip(row + 1)
            {
                value -= matrix[row][column] * coefficient;
            }
            coefficients[row] = value / matrix[row][row];
        }
        return coefficients
            .iter()
            .all(|value| value.is_finite())
            .then_some(coefficients);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlackScholesMertonParameters, Date, MonteCarloSettings};

    #[test]
    fn cache_retention_limit_does_not_trigger_path_replay() -> Result<()> {
        let settings = MonteCarloSettings {
            path_count: 340_000,
            antithetic: false,
            ..Default::default()
        };
        let (window, bytes) = exercise_window::<1, 1>(settings, 340_000, 340_000, 50, 0)?;
        assert!(bytes > MAX_CACHED_PATH_VALUES * size_of::<f64>());
        assert_eq!(window, 50);

        let settings = MonteCarloSettings {
            path_count: 680_000,
            ..settings
        };
        let (window, bytes) = exercise_window::<1, 1>(settings, 680_000, 680_000, 50, 0)?;
        assert_eq!(window, 25);
        assert!(bytes <= PREFERRED_PATH_WORKSPACE);
        Ok(())
    }

    #[test]
    fn volatility_stencil_keeps_one_full_coordinate_grid() -> Result<()> {
        let paths = 65_536;
        let intervals = 200;
        let settings = MonteCarloSettings {
            path_count: paths,
            ..Default::default()
        };
        let (window, bytes) = exercise_window::<3, 3>(settings, 65_536, 65_536, intervals, 0)?;
        assert_eq!(window, intervals);
        assert!((100 * 1024 * 1024..110 * 1024 * 1024).contains(&bytes));
        Ok(())
    }

    #[test]
    fn weekend_rate_greeks_keep_calendar_accrual() -> Result<()> {
        let start = Date::new(2025, 1, 4)?.start_of_day();
        let end = start.add_days(2)?;
        let option = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
        let curve = crate::RateTermStructure::bounded_flat_with_calendar(
            0.05,
            start,
            end,
            crate::TradingCalendar::weekdays(),
        )?;
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(curve.clone(), curve, 0.25)?,
            100.0,
            start,
        )?;
        let engine = MonteCarloVanillaEngine::new(crate::MonteCarloGridSettings {
            sampling: MonteCarloSettings {
                path_count: 2048,
                seed: Some(42),
                ..Default::default()
            },
            step_count: 25,
        })?;
        let result = engine.price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Rho, Greek::DividendRho]),
        )?;
        assert_eq!(result.require(Greek::Rho)?, 0.0);
        assert_eq!(result.require(Greek::DividendRho)?, 0.0);
        Ok(())
    }

    #[test]
    fn american_workspace_reports_capacity_failure() {
        assert!(matches!(
            american_workspace::<f64>(usize::MAX),
            Err(Error::ComputationFailure(_))
        ));
    }

    #[test]
    fn dividend_stencils_crossing_zero_keep_one_exercise_estimator() {
        let start = Date::new(2025, 1, 1).unwrap().start_of_day();
        let option = AmericanOption::new(
            OptionType::Call,
            100.0,
            start,
            Date::new(2026, 1, 1).unwrap().start_of_day(),
        )
        .unwrap();
        let engine = MonteCarloVanillaEngine::new(crate::MonteCarloGridSettings {
            sampling: MonteCarloSettings {
                path_count: 2048,
                seed: Some(42),
                ..Default::default()
            },
            step_count: 7,
        })
        .unwrap();
        let shift = NumericalShiftSettings::default().rate_shift;
        for rate in [0.0, 0.05] {
            for dividend in [-shift / 2.0, 0.0, shift / 2.0, shift] {
                let context = |dividend| {
                    PricingContext::new(
                        BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
                        100.0,
                        start,
                    )
                    .unwrap()
                };
                let result = engine
                    .price_with_greeks(
                        &option,
                        &context(dividend),
                        GreeksRequest::new([Greek::DividendRho]),
                    )
                    .unwrap();
                let shift = crate::risk::numerical::symmetric_shift(dividend, shift);
                if shift == 0.0 {
                    assert!(!result.has(Greek::DividendRho));
                    continue;
                }
                let up = engine
                    .american_price(&option, &context(dividend + shift), false)
                    .unwrap()
                    .0;
                let down = engine
                    .american_price(&option, &context(dividend - shift), false)
                    .unwrap()
                    .0;
                assert!(
                    (result.require(Greek::DividendRho).unwrap() - (up - down) / (2.0 * shift))
                        .abs()
                        < 1e-10
                );
                assert_eq!(
                    result.price(),
                    engine.price(&option, &context(dividend)).unwrap()
                );
                assert!(!result.has(Greek::Rho));
                let unavailable = crate::numerical_greeks(
                    &engine,
                    &option,
                    &context(dividend),
                    GreeksRequest::new([Greek::DividendRho]),
                    NumericalShiftSettings {
                        rate_shift: f64::MAX,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert!(!unavailable.has(Greek::DividendRho));
            }
        }
    }
}
#[test]
fn checkpoint_replay_preserves_pseudorandom_and_sobol_paths() -> Result<()> {
    let intervals = 35;
    let sequence = sobol::sequence(intervals, 1729)?;
    for sequence in [None, Some(sequence.as_ref())] {
        for antithetic in [false, true] {
            let paths = 513;
            let samples = if antithetic { 257 } else { paths };
            let full = ExercisePaths {
                settings: MonteCarloSettings {
                    antithetic,
                    ..Default::default()
                },
                paths,
                samples,
                intervals,
                window: intervals,
                seed: 1729,
                anchor: None,
                growth: [0.0001, 0.02],
                sobol: sequence,
            };
            let windowed = ExercisePaths { window: 6, ..full };
            for block_index in 0..samples.div_ceil(BLOCK_SIZE) {
                let begin = block_index * BLOCK_SIZE;
                let end = (begin + BLOCK_SIZE).min(samples);
                let count = full.block_coordinate_count(begin, end);
                let (expected, _) = full.generate(block_index, count)?;
                if sequence.is_none() {
                    let mut draws = NormalDraws {
                        generator: Pcg32::new(1729, block_index as u64),
                        spare: None,
                        cursor: None,
                    };
                    let mut scalar = vec![Vec::new(); intervals];
                    for sample in begin..end {
                        let mut accumulated = 0.0;
                        for row in &mut scalar {
                            let normal = draws.normal();
                            accumulated += normal;
                            row.push(accumulated);
                            if antithetic && sample < paths / 2 {
                                row.push(-accumulated);
                            }
                        }
                    }
                    for (actual, scalar) in expected.chunks_exact(count).zip(scalar) {
                        let mut expanded = Vec::new();
                        full.visit_row(block_index, actual, |coordinate| {
                            expanded.push(coordinate);
                            Ok(())
                        })?;
                        assert_eq!(expanded, scalar);
                    }
                }
                let (spots, checkpoints) = windowed.generate(block_index, count)?;
                let mut block = ExerciseBlock {
                    coordinates: Arc::new(spots),
                    checkpoints,
                    ..ExerciseBlock::<1>::default()
                };
                let mut start = windowed.first_in_last_window();
                assert_eq!(&block.coordinates[..], &expected[start * count..]);
                while start > 0 {
                    start -= windowed.window;
                    windowed.replay(block_index, &mut block, start)?;
                    assert_eq!(
                        &block.coordinates[..],
                        &expected[start * count..(start + windowed.window) * count]
                    );
                }
            }
        }
    }
    Ok(())
}
