use std::{cell::RefCell, marker::PhantomData, sync::Arc};

use super::{
    BLOCK_SIZE, BlockRunner, Error, EuropeanOption, GreeksRequest, Moments, MonteCarloSampling,
    MonteCarloSettings, MonteCarloVanillaEngine, NormalDraws, NumericalShiftSettings, OptionType,
    Pcg32, PricingContext, PricingEngine, PricingResult, Result, check_path_step, scaled_value,
    sobol, unstable,
};
use crate::risk::numerical::symmetric_shift;
use crate::{
    AmericanOption, BlackScholesMertonParameterChanges, Greek, OptionContract,
    PricingContextChanges,
};

const WORKSPACE_BUDGET: usize = 1 << 30;
const WORK_BUDGET: usize = 1_000_000_000;
const MAX_CACHED_PATH_VALUES: usize = 128 * 1024 * 1024 / size_of::<f64>();
const PATH_BLOCK_OVERHEAD: usize = size_of::<Vec<f64>>() + 2 * size_of::<usize>();

#[derive(PartialEq)]
struct PathKey {
    seed: u64,
    settings: super::MonteCarloSettings,
    intervals: usize,
    path_values: usize,
    drift: u64,
    diffusion: u64,
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

pub(crate) struct AmericanGreekCache {
    active: bool,
    previous: Option<PathCache>,
    _thread: PhantomData<*mut ()>,
}

impl AmericanGreekCache {
    pub(crate) fn new(request: GreeksRequest) -> Self {
        let spot_bumps = [
            Greek::Delta,
            Greek::Gamma,
            Greek::Speed,
            Greek::Vanna,
            Greek::Zomma,
            Greek::Charm,
            Greek::Color,
        ];
        let active = spot_bumps.into_iter().any(|greek| request.has(greek));
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

impl Drop for AmericanGreekCache {
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

struct ExerciseBlock<const N: usize> {
    spots: Arc<Vec<f64>>,
    checkpoints: Vec<f64>,
    cash: Vec<[f64; N]>,
    regression: [[[f64; 4]; 3]; N],
}

fn exercise_window<const N: usize>(
    settings: MonteCarloSettings,
    reserved_paths: usize,
    intervals: usize,
    growth_count: usize,
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
            size_of::<ExerciseBlock<N>>() + PATH_BLOCK_OVERHEAD + size_of::<Arc<Vec<f64>>>();
        reserved_paths
            .saturating_mul(
                (window + checkpoints) * growth_count * size_of::<f64>() + size_of::<[f64; N]>(),
            )
            .saturating_add(blocks * overhead)
            .saturating_add(sampling)
    };
    let window = if (intervals >= 16
        && reserved_paths
            .saturating_mul(intervals)
            .saturating_mul(growth_count)
            > MAX_CACHED_PATH_VALUES)
        || workspace(intervals) > WORKSPACE_BUDGET
    {
        intervals.isqrt()
    } else {
        intervals
    };
    let bytes = workspace(window);
    if bytes > WORKSPACE_BUDGET {
        return Err(Error::InvalidInput(
            "American Monte Carlo workspace exceeds 1 GiB; reduce path_count or step_count".into(),
        ));
    }
    let replay = (intervals - 1) / window * window;
    // Count original and replayed path steps, then one exercise update per scenario and step.
    let work = growth_count * (intervals + replay) + N * (intervals - 1);
    if reserved_paths.saturating_mul(work) > WORK_BUDGET {
        return Err(Error::InvalidInput(
            "American Monte Carlo exceeds one billion simulated path steps and exercise updates; reduce path_count or step_count".into(),
        ));
    }
    Ok((window, bytes))
}

impl<const N: usize> Default for ExerciseBlock<N> {
    fn default() -> Self {
        Self {
            spots: Arc::default(),
            checkpoints: Vec::new(),
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
    anchor: f64,
    growth: &'a [[f64; 2]],
    sobol: Option<&'a sobol::SobolSequence>,
}

impl ExercisePaths<'_> {
    fn first_in_last_window(&self) -> usize {
        (self.intervals - 1) / self.window * self.window
    }

    fn block_path_count(&self, begin: usize, end: usize) -> usize {
        let paired = if self.settings.antithetic {
            end.min(self.paths / 2).saturating_sub(begin)
        } else {
            0
        };
        end - begin + paired
    }

    fn fill<const N: usize>(
        &self,
        block: usize,
        steps: std::ops::Range<usize>,
        mut initial: impl FnMut(usize, usize) -> f64,
        mut save: impl FnMut(usize, usize, usize, f64),
    ) -> Result<()> {
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
        let evolve = |spot: &mut f64, growth: f64| -> Result<f64> {
            *spot *= growth;
            if !spot.is_finite() || *spot <= 0.0 {
                return Err(unstable());
            }
            Ok(*spot)
        };
        for sample in begin..end {
            crate::cancellation::check()?;
            let paired = self.settings.antithetic && sample < self.paths / 2;
            let mut positive: [f64; N] = std::array::from_fn(|index| {
                if index < self.growth.len() {
                    initial(lane, index)
                } else {
                    self.anchor
                }
            });
            let mut negative: [f64; N] = std::array::from_fn(|index| {
                if paired && index < self.growth.len() {
                    initial(lane + 1, index)
                } else {
                    self.anchor
                }
            });
            for step in steps.clone() {
                check_path_step(step)?;
                let normal = draws.normal();
                for (index, &[drift, diffusion]) in self.growth.iter().enumerate() {
                    let spot =
                        evolve(&mut positive[index], diffusion.mul_add(normal, drift).exp())?;
                    save(step, lane, index, spot);
                    if paired {
                        let spot = evolve(
                            &mut negative[index],
                            (-diffusion).mul_add(normal, drift).exp(),
                        )?;
                        save(step, lane + 1, index, spot);
                    }
                }
            }
            lane += if paired { 2 } else { 1 };
            if let Some(cursor) = &mut draws.cursor {
                cursor.next_path();
                cursor.seek(steps.start);
            } else {
                draws.skip_normals(self.intervals - steps.len());
            }
        }
        Ok(())
    }

    fn generate<const N: usize>(&self, block: usize, count: usize) -> Result<(Vec<f64>, Vec<f64>)> {
        let last = self.first_in_last_window();
        let growth_count = self.growth.len();
        let width = count * growth_count;
        let mut spots = american_workspace(width * self.window)?;
        spots.resize(width * (self.intervals - last), 0.0);
        let mut checkpoints = american_workspace(width * (last / self.window))?;
        checkpoints.resize(width * (last / self.window), 0.0);
        self.fill::<N>(
            block,
            0..self.intervals,
            |_, _| self.anchor,
            |step, lane, index, spot| {
                let column = lane * growth_count + index;
                if step >= last {
                    spots[(step - last) * width + column] = spot;
                } else if (step + 1) % self.window == 0 {
                    checkpoints[((step + 1) / self.window - 1) * width + column] = spot;
                }
            },
        )?;
        Ok((spots, checkpoints))
    }

    fn replay<const N: usize>(
        &self,
        index: usize,
        block: &mut ExerciseBlock<N>,
        start: usize,
    ) -> Result<()> {
        let growth_count = self.growth.len();
        let width = block.cash.len() * growth_count;
        let spots = Arc::get_mut(&mut block.spots).expect("checkpointed paths are request-owned");
        spots.resize(width * self.window, 0.0);
        let initial = |lane, model| {
            if start == 0 {
                self.anchor
            } else {
                block.checkpoints[(start / self.window - 1) * width + lane * growth_count + model]
            }
        };
        self.fill::<N>(
            index,
            start..start + self.window,
            initial,
            |step, lane, model, spot| {
                spots[(step - start) * width + lane * growth_count + model] = spot;
            },
        )
    }
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

    fn american_price(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        terminal_only: bool,
    ) -> Result<(f64, Option<u64>)> {
        let ([price], seed) = self.american_spots(option, [context], terminal_only)?;
        Ok((price, seed))
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "Validated positive i32 path and step counts bound the buffers and fit exactly in f64."
    )]
    fn american_spots<const N: usize>(
        &self,
        option: &AmericanOption,
        contexts: [&PricingContext; N],
        terminal_only: bool,
    ) -> Result<([f64; N], Option<u64>)> {
        let context = contexts[0];
        let initial = contexts.map(PricingContext::spot_price);
        let settings = self.settings.sampling;
        let step_count = self.settings.step_count;
        let scalar = || {
            let mut prices = [0.0; N];
            let mut seed = None;
            for (price, context) in prices.iter_mut().zip(contexts) {
                (*price, seed) = self.american_price(option, context, terminal_only)?;
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
            let payoff =
                crate::engines::Payoff::vanilla(option.option_type(), option.strike(), true);
            let mut prices = [0.0; N];
            for (price, context) in prices.iter_mut().zip(contexts) {
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
        let payoff = |spot: f64| (sign * (spot - option.strike())).max(0.0);
        let immediate = initial.map(payoff);
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
            for (price, context) in prices.iter_mut().zip(contexts) {
                let (value, used_seed) = <Self as PricingEngine<EuropeanOption>>::price_with_seed(
                    self,
                    option.vanilla(),
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
        let growth_count = if same_growth { 1 } else { N };
        let growth_index: [usize; N] =
            std::array::from_fn(|index| if same_growth { 0 } else { index });
        let (anchor, scale) = if normalized {
            (1.0, initial)
        } else {
            (initial[0], [1.0; N])
        };
        let (window, workspace_bytes) =
            match exercise_window::<N>(settings, reserved_paths, intervals, growth_count) {
                Err(Error::InvalidInput(_)) if !same_growth => return scalar(),
                result => result?,
            };
        let path_values = reserved_paths * intervals;
        let seed = settings.resolved_seed()?;
        let key = PathKey {
            seed,
            settings,
            intervals,
            path_values,
            drift: growth[0][0].to_bits(),
            diffusion: growth[0][1].to_bits(),
        };
        let (retain_paths, cached_paths) = PATH_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let Some(cache) = cache.as_mut() else {
                return (false, None);
            };
            if let Some(index) = cache
                .iter()
                .position(|(previous, _)| same_growth && normalized && *previous == key)
            {
                let entry = cache.remove(index);
                let found = Arc::clone(&entry.1);
                cache.push(entry);
                return (false, Some(found));
            }
            let retain = same_growth
                && normalized
                && window == intervals
                && path_values <= MAX_CACHED_PATH_VALUES;
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
            growth: &growth[..growth_count],
            sobol: sobol.as_deref(),
        };
        let mut window_start = exercise_paths.first_in_last_window();
        let block_count = samples.div_ceil(BLOCK_SIZE);
        let mut blocks = american_workspace(block_count)?;
        blocks.resize_with(block_count, ExerciseBlock::default);
        let runner = BlockRunner::new(block_count, settings.max_active_workers)?;
        runner.run(&mut blocks, |block_index, block| {
            let begin = block_index * BLOCK_SIZE;
            let end = ((block_index + 1) * BLOCK_SIZE).min(samples);
            let path_count = exercise_paths.block_path_count(begin, end);
            let spots = if let Some(cached) = &cached_paths {
                Arc::clone(&cached[block_index])
            } else {
                let (values, checkpoints) =
                    exercise_paths.generate::<N>(block_index, path_count)?;
                block.checkpoints = checkpoints;
                Arc::new(values)
            };
            let mut cash = american_workspace(path_count)?;
            cash.extend(
                spots[spots.len() - path_count * growth_count..]
                    .chunks_exact(growth_count)
                    .map(|spots| {
                        std::array::from_fn(|i| payoff(spots[growth_index[i]] * scale[i]))
                    }),
            );
            block.spots = spots;
            block.cash = cash;
            Ok(())
        })?;
        if retain_paths {
            let paths = Arc::new(
                blocks
                    .iter()
                    .map(|block| Arc::clone(&block.spots))
                    .collect(),
            );
            PATH_CACHE.with(|cache| {
                if let Some(cache) = cache.borrow_mut().as_mut() {
                    cache.push((key, paths));
                }
            });
        }
        let strike = option.strike();
        let apply_exercise =
            |cash: &mut [f64; N], spots: &[f64], coefficients: &[Option<[f64; 3]>; N]| {
                for (index, (cash, coefficient)) in cash.iter_mut().zip(coefficients).enumerate() {
                    let Some([constant, linear, quadratic]) = *coefficient else {
                        continue;
                    };
                    let spot = spots[growth_index[index]] * scale[index];
                    let intrinsic = payoff(spot);
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
        for step in (0..intervals - 1).rev() {
            if step < window_start {
                window_start -= window;
                runner.run(&mut blocks, |index, block| {
                    if coefficients.iter().any(Option::is_some) {
                        let width = block.cash.len() * growth_count;
                        for (spots, cash) in block.spots[..width]
                            .chunks_exact(growth_count)
                            .zip(&mut block.cash)
                        {
                            apply_exercise(cash, spots, &coefficients)?;
                        }
                    }
                    exercise_paths.replay(index, block, window_start)
                })?;
                coefficients.fill(None);
            }
            let row = step - window_start;
            let decide = coefficients.iter().any(Option::is_some);
            // Apply the next date's exercise decision while its cash is in cache,
            // then use that cash immediately in the current date's regression.
            runner.run(&mut blocks, |_, block| {
                let mut matrices = [[[0.0; 4]; 3]; N];
                let width = block.cash.len() * growth_count;
                let spots = &block.spots[row * width..(row + 1) * width];
                let next_spots = if decide {
                    &block.spots[(row + 1) * width..(row + 2) * width]
                } else {
                    spots
                };
                for ((base, following), cash) in spots
                    .chunks_exact(growth_count)
                    .zip(next_spots.chunks_exact(growth_count))
                    .zip(&mut block.cash)
                {
                    if decide {
                        apply_exercise(cash, following, &coefficients)?;
                    }
                    for (index, (cash, matrix)) in cash.iter_mut().zip(&mut matrices).enumerate() {
                        let spot = base[growth_index[index]] * scale[index];
                        *cash *= discount[index];
                        if !cash.is_finite() {
                            return Err(unstable());
                        }
                        if payoff(spot) > 0.0 {
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
                }
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
        }
        let mut means = [Moments::default(); N];
        let decide = coefficients.iter().any(Option::is_some);
        for block in blocks {
            crate::cancellation::check()?;
            for (spots, mut cash) in block.spots.chunks_exact(growth_count).zip(block.cash) {
                if decide {
                    apply_exercise(&mut cash, spots, &coefficients)?;
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
        let _growth = AmericanGreekCache::new(request);
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
        let spot = context.spot_price();
        let shift = symmetric_shift(spot, settings.spot_shift_at(spot));
        let speed_shift = 2.0 * shift;
        let speed = request.has(Greek::Speed)
            && speed_shift.is_finite()
            && spot > speed_shift
            && symmetric_shift(spot, speed_shift).to_bits() == speed_shift.to_bits()
            && !option.at_spot_discontinuity(context, speed_shift);
        let batch = if (request.has(Greek::Delta) || request.has(Greek::Gamma) || speed)
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
            let (prices, seed) = engine.american_spots(option, [context, &up, &down], true)?;
            Some((up, down, prices, seed))
        } else {
            None
        };
        let mut volatility_batch = None;
        if batch.is_none()
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
                    match engine.american_spots(option, [context, &up, &down], true) {
                        Ok((prices, seed)) => volatility_batch = Some((up, down, prices, seed)),
                        Err(error @ Error::Io(_)) => return Err(error),
                        Err(_) => {} // Preserve scalar validation and error ordering.
                    }
                }
            }
        }
        let native = |scenario: &PricingContext, requested: GreeksRequest| {
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
                let ([up, down], seed) = engine.american_spots(option, [high, low], true)?;
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
                anchor: 1.0,
                growth: &[[0.0001, 0.02]],
                sobol: sequence,
            };
            let windowed = ExercisePaths { window: 6, ..full };
            for block_index in 0..samples.div_ceil(BLOCK_SIZE) {
                let begin = block_index * BLOCK_SIZE;
                let count = full.block_path_count(begin, (begin + BLOCK_SIZE).min(samples));
                let (expected, _) = full.generate::<1>(block_index, count)?;
                let (spots, checkpoints) = windowed.generate::<1>(block_index, count)?;
                let mut block = ExerciseBlock {
                    spots: Arc::new(spots),
                    checkpoints,
                    cash: vec![[0.0]; count],
                    ..Default::default()
                };
                let mut start = windowed.first_in_last_window();
                assert_eq!(&block.spots[..], &expected[start * count..]);
                while start > 0 {
                    start -= windowed.window;
                    windowed.replay(block_index, &mut block, start)?;
                    assert_eq!(
                        &block.spots[..],
                        &expected[start * count..(start + windowed.window) * count]
                    );
                }
            }
        }
    }
    Ok(())
}
