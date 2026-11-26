use std::{cell::RefCell, marker::PhantomData, sync::Arc};

use super::{
    BLOCK_SIZE, BlockRunner, Error, EuropeanOption, GreeksRequest, Moments, MonteCarloSampling,
    MonteCarloVanillaEngine, NumericalShiftSettings, OptionType, Pcg32, PricingContext,
    PricingEngine, PricingResult, Result, check_path_step, sample_normal, scaled_value, sobol,
    unstable,
};
use crate::{AmericanOption, BlackScholesMertonParameterChanges, Greek, PricingContextChanges};

const MAX_PATH_VALUES: usize = 1024 * 1024 * 1024 / size_of::<f64>();
const MAX_CACHED_PATH_VALUES: usize = 128 * 1024 * 1024 / size_of::<f64>();

#[derive(PartialEq)]
struct PathKey {
    seed: u64,
    settings: super::MonteCarloSettings,
    intervals: usize,
    path_values: usize,
    drift: u64,
    diffusion: u64,
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
    cash: Vec<[f64; N]>,
    regression: [[[f64; 4]; 3]; N],
}

impl<const N: usize> Default for ExerciseBlock<N> {
    fn default() -> Self {
        Self {
            spots: Arc::default(),
            cash: Vec::new(),
            regression: [[[0.0; 4]; 3]; N],
        }
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
        let model = context.natural_bsm_parameters()?;
        context.validate_curve_horizon(option.expiry_time())?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
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
        let Some(path_values) = reserved_paths
            .checked_mul(intervals)
            .filter(|&count| count <= MAX_PATH_VALUES)
        else {
            return Err(Error::InvalidInput(
                "American Monte Carlo path storage exceeds 1 GiB; reduce path_count or step_count"
                    .into(),
            ));
        };
        let dt = time / intervals as f64;
        let sigma = model.volatility();
        let drift = (rate - dividend - 0.5 * sigma * sigma) * dt;
        let diffusion = sigma * dt.sqrt();
        let discount = (-rate * dt).exp();
        if !drift.is_finite() || !diffusion.is_finite() || !discount.is_finite() {
            return Err(unstable());
        }
        // Both normal generators are bounded by ten standard deviations. Keep
        // relative prices away from underflow and overflow over the entire path.
        let excursion = (drift.abs() + 10.0 * diffusion.abs()) * intervals as f64;
        let normalized = initial
            .iter()
            .all(|spot| spot.ln().abs() + excursion < 600.0);
        if N > 1 && !normalized {
            let mut prices = [0.0; N];
            let mut seed = None;
            for (price, context) in prices.iter_mut().zip(contexts) {
                (*price, seed) = self.american_price(option, context, terminal_only)?;
            }
            return Ok((prices, seed));
        }
        let (anchor, scale) = if normalized {
            (1.0, initial)
        } else {
            (initial[0], [1.0; N])
        };
        let seed = settings.resolved_seed()?;
        let key = PathKey {
            seed,
            settings,
            intervals,
            path_values,
            drift: drift.to_bits(),
            diffusion: diffusion.to_bits(),
        };
        let (retain_paths, cached_paths) = PATH_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let Some(cache) = cache.as_mut() else {
                return (false, None);
            };
            if let Some(index) = cache
                .iter()
                .position(|(previous, _)| normalized && *previous == key)
            {
                let entry = cache.remove(index);
                let found = Arc::clone(&entry.1);
                cache.push(entry);
                return (false, Some(found));
            }
            let retain = normalized && path_values <= MAX_CACHED_PATH_VALUES;
            let limit = if retain {
                MAX_CACHED_PATH_VALUES
            } else {
                MAX_PATH_VALUES
            };
            let mut stored: usize = cache.iter().map(|(key, _)| key.path_values).sum();
            while stored > limit - path_values || retain && cache.len() == 2 {
                stored -= cache.remove(0).0.path_values;
            }
            (retain, None)
        });
        let sobol = if cached_paths.is_none() && settings.sampling == MonteCarloSampling::Sobol {
            Some(sobol::sequence(intervals, seed)?)
        } else {
            None
        };
        let block_count = samples.div_ceil(BLOCK_SIZE);
        let mut blocks = american_workspace(block_count)?;
        blocks.resize_with(block_count, ExerciseBlock::default);
        let runner = BlockRunner::new(block_count, settings.max_active_workers)?;
        runner.run(&mut blocks, |block_index, block| {
            let begin = block_index * BLOCK_SIZE;
            let end = ((block_index + 1) * BLOCK_SIZE).min(samples);
            let paired = if settings.antithetic {
                end.min(paths / 2).saturating_sub(begin)
            } else {
                0
            };
            let path_count = end - begin + paired;
            let spots = if let Some(cached) = &cached_paths {
                Arc::clone(&cached[block_index])
            } else {
                let mut values = american_workspace(path_count * intervals)?;
                values.resize(path_count * intervals, 0.0);
                let evolve = |spot: &mut f64, growth: f64| -> Result<f64> {
                    *spot *= growth;
                    if !spot.is_finite() || *spot <= 0.0 {
                        return Err(unstable());
                    }
                    Ok(*spot)
                };
                let mut generator = Pcg32::new(seed, block_index as u64);
                let mut cursor = sobol.as_ref().map(|sequence| sequence.cursor(begin));
                let mut spare = None;
                let mut path = 0;
                for sample in begin..end {
                    crate::cancellation::check()?;
                    let paired = settings.antithetic && sample < paths / 2;
                    let mut positive = anchor;
                    let mut negative = anchor;
                    for (step, row) in values.chunks_exact_mut(path_count).enumerate() {
                        check_path_step(step)?;
                        let normal = sample_normal(&mut generator, &mut spare, &mut cursor);
                        row[path] = evolve(&mut positive, diffusion.mul_add(normal, drift).exp())?;
                        if paired {
                            row[path + 1] =
                                evolve(&mut negative, (-diffusion).mul_add(normal, drift).exp())?;
                        }
                    }
                    path += if paired { 2 } else { 1 };
                    if let Some(cursor) = &mut cursor {
                        cursor.next_path();
                    }
                }
                Arc::new(values)
            };
            let mut cash = american_workspace(path_count)?;
            cash.extend(
                spots[(intervals - 1) * path_count..]
                    .iter()
                    .map(|&spot| scale.map(|initial| payoff(spot * initial))),
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
        for step in (0..intervals - 1).rev() {
            runner.run(&mut blocks, |_, block| {
                let mut matrices = [[[0.0; 4]; 3]; N];
                let count = block.cash.len();
                let spots = &block.spots[step * count..(step + 1) * count];
                for (&base, cash) in spots.iter().zip(&mut block.cash) {
                    for ((scale, cash), matrix) in scale.into_iter().zip(cash).zip(&mut matrices) {
                        let spot = base * scale;
                        *cash *= discount;
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
            let mut coefficients = [None; N];
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
            if coefficients.iter().all(Option::is_none) {
                continue;
            }
            runner.run(&mut blocks, |_, block| {
                let count = block.cash.len();
                let spots = &block.spots[step * count..(step + 1) * count];
                for (&base, cash) in spots.iter().zip(&mut block.cash) {
                    for ((scale, cash), coefficient) in
                        scale.into_iter().zip(cash).zip(coefficients)
                    {
                        let spot = base * scale;
                        let Some([constant, linear, quadratic]) = coefficient else {
                            continue;
                        };
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
                }
                Ok(())
            })?;
        }
        let mut means = [Moments::default(); N];
        for block in blocks {
            crate::cancellation::check()?;
            for cash in block.cash {
                for (mean, value) in means.iter_mut().zip(cash) {
                    mean.add(value, false);
                }
            }
        }
        let mut prices = immediate;
        for (price, mean) in prices.iter_mut().zip(means) {
            *price = scaled_value(mean.scale, mean.mean, -rate * dt)?.max(*price);
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
        let native = |context: &PricingContext, requested: GreeksRequest| {
            let (price, seed) = engine.american_price(option, context, true)?;
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
