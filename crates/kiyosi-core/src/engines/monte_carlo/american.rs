use super::*;
use crate::{AmericanOption, BlackScholesMertonParameterChanges, Greek, PricingContextChanges};

impl MonteCarloVanillaEngine {
    /// Returns the American default absolute spot bump, 8% of spot.
    /// Exercise decisions amplify small-bump noise; sampling error and bump bias
    /// still require convergence checks. Other shifts use [`NumericalShiftSettings`] defaults.
    /// Python uses this value when `spot_shift` is omitted or `None`.
    ///
    /// # Examples
    /// ```
    /// # use kiyosi_core::*;
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings { seed: Some(42), ..Default::default() })?;
    /// let result = engine.price_with_greeks(&put, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), NumericalShiftSettings {
    ///         spot_shift: MonteCarloVanillaEngine::default_american_spot_shift(&context),
    ///         ..Default::default()
    ///     })?;
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
    ) -> Result<f64> {
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
        let payoff = |spot: f64| (sign * (spot - option.strike())).max(0.0);
        let immediate = payoff(context.spot_price());
        let time = valuation.years_until(expiry);
        if time == 0.0 {
            return Ok(immediate);
        }
        if self.settings.step_count < 3 {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "American Monte Carlo requires at least three grid points",
            ));
        }
        if terminal_only
            && option.option_type() == OptionType::Call
            && dividend <= 0.0
            && rate >= 0.0
        {
            let european = EuropeanOption::new(
                option.option_type(),
                option.strike(),
                option.effective_date(),
                option.expiry_date(),
            )?;
            return Ok(self.price(&european, context)?.max(immediate));
        }
        let intervals = self.settings.step_count as usize - 1;
        let dt = time / intervals as f64;
        let sigma = model.volatility();
        let drift = (rate - dividend - 0.5 * sigma * sigma) * dt;
        let diffusion = sigma * dt.sqrt();
        let discount = (-rate * dt).exp();
        if !drift.is_finite() || !diffusion.is_finite() || !discount.is_finite() {
            return Err(unstable());
        }
        let seed = match self.settings.seed {
            Some(seed) => seed,
            None => random_seed()?,
        };
        let paths = self.settings.path_count as usize;
        let samples = if self.settings.antithetic {
            paths.div_ceil(2)
        } else {
            paths
        };
        let sobol = if self.settings.sampling == MonteCarloSampling::Sobol {
            Some(sobol::SobolSequence::new(intervals, seed)?)
        } else {
            None
        };
        let mut partials = vec![Ok(Vec::new()); samples.div_ceil(BLOCK_SIZE)];
        run_blocks(&mut partials, self.settings.thread_count, |block| {
            let mut generator = Pcg32::new(seed, block as u64);
            let mut cursor = sobol
                .as_ref()
                .map(|sequence| sequence.cursor(block * BLOCK_SIZE));
            let mut spare = None;
            let begin = block * BLOCK_SIZE;
            let end = ((block + 1) * BLOCK_SIZE).min(samples);
            let mut values = Vec::with_capacity(
                (end - begin) * intervals * if self.settings.antithetic { 2 } else { 1 },
            );
            for path in begin..end {
                let paired = self.settings.antithetic && path < paths / 2;
                let offset = values.len();
                values.resize(offset + intervals * if paired { 2 } else { 1 }, 0.0);
                let mut positive = context.spot_price();
                let mut negative = positive;
                for step in 0..intervals {
                    let normal = if let Some(cursor) = &mut cursor {
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
                    positive *= diffusion.mul_add(normal, drift).exp();
                    if !positive.is_finite() || positive <= 0.0 {
                        return Err(unstable());
                    }
                    values[offset + step] = positive;
                    if paired {
                        negative *= (-diffusion).mul_add(normal, drift).exp();
                        if !negative.is_finite() || negative <= 0.0 {
                            return Err(unstable());
                        }
                        values[offset + intervals + step] = negative;
                    }
                }
                if let Some(cursor) = &mut cursor {
                    cursor.next_path();
                }
            }
            Ok(values)
        })?;
        let blocks = partials.into_iter().collect::<Result<Vec<_>>>()?;
        let rows = || {
            blocks
                .iter()
                .flat_map(|block| block.chunks_exact(intervals))
        };
        let mut cash_flows: Vec<_> = rows().map(|path| payoff(path[intervals - 1])).collect();
        for step in (0..intervals - 1).rev() {
            let mut matrix = [[0.0; 4]; 3];
            for (path, cash) in rows().zip(&mut cash_flows) {
                *cash *= discount;
                if !cash.is_finite() {
                    return Err(unstable());
                }
                if payoff(path[step]) > 0.0 {
                    let x = path[step] / option.strike();
                    let square = x * x;
                    let target = *cash / option.strike();
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
            if matrix[0][0] == 0.0 {
                continue;
            }
            matrix[1][0] = matrix[0][1];
            matrix[2][0] = matrix[0][2];
            matrix[2][1] = matrix[1][2];
            let Some(coefficients) = solve_quadratic(matrix) else {
                continue;
            };
            for (path, cash) in rows().zip(&mut cash_flows) {
                let intrinsic = payoff(path[step]);
                if intrinsic <= 0.0 {
                    continue;
                }
                let x = path[step] / option.strike();
                let continuation = coefficients[0] + x * (coefficients[1] + x * coefficients[2]);
                if continuation.is_finite() && intrinsic / option.strike() > continuation {
                    *cash = intrinsic;
                }
            }
        }
        let mut mean = Moments::default();
        for cash in cash_flows {
            mean.add(cash);
        }
        Ok(scaled_value(mean.scale, mean.mean, -rate * dt)?.max(immediate))
    }
}

impl PricingEngine<AmericanOption> for MonteCarloVanillaEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        <Self as PricingEngine<EuropeanOption>>::validate_applicability(self, context)
    }

    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<AmericanOption> + Send + Sync + '_>>> {
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

    fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        self.american_price(option, context, true)
    }

    fn price_with_greeks(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<AmericanOption>>::validate_applicability(self, context)?;
        request.validate()?;
        settings.validate()?;
        let engine = Self {
            settings: MonteCarloSettings {
                seed: Some(match self.settings.seed {
                    Some(seed) => seed,
                    None => random_seed()?,
                }),
                ..self.settings
            },
        };
        crate::risk::numerical::complete_greeks(
            option,
            context,
            request,
            settings,
            false,
            |context, requested| {
                let mut result = PricingResult::new(engine.price(option, context)?)?;
                let rate = context.model_parameters().risk_free_rate()?;
                let shift = crate::risk::numerical::symmetric_shift(rate, settings.rate_shift);
                if requested.has(Greek::Rho)
                    && shift > 0.0
                    && context.valuation_time() != option.expiry_date().start_of_day()
                    && option.option_type() == OptionType::Call
                    && context.model_parameters().dividend_yield()? <= 0.0
                    && rate - shift < 0.0
                    && rate + shift >= 0.0
                {
                    // Keep the exercise-grid estimator on both sides of the terminal shortcut boundary.
                    let evaluate = |rate: f64| {
                        let model = context.model_parameters().replace(
                            BlackScholesMertonParameterChanges {
                                risk_free_rate: Some(rate.into()),
                                ..Default::default()
                            },
                        )?;
                        let shifted = context.replace(PricingContextChanges {
                            model_parameters: Some(model),
                            ..Default::default()
                        })?;
                        engine.american_price(option, &shifted, false)
                    };
                    result.set(
                        requested,
                        Greek::Rho,
                        (evaluate(rate + shift)? - evaluate(rate - shift)?) / (200.0 * shift),
                    )?;
                }
                Ok(result)
            },
        )
    }
}

// Reduce the basis when paths cannot identify quadratic continuation, as in the legacy engine.
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
