use super::{
    BLOCK_SIZE, Error, EuropeanOption, GreeksRequest, Moments, MonteCarloSampling,
    MonteCarloVanillaEngine, NumericalShiftSettings, OptionType, Pcg32, PricingContext,
    PricingEngine, PricingResult, Result, run_blocks, sample_normal, scaled_value, sobol, unstable,
};
use crate::{AmericanOption, BlackScholesMertonParameterChanges, Greek, PricingContextChanges};

const MAX_PATH_VALUES: usize = 1024 * 1024 * 1024 / size_of::<f64>();

impl MonteCarloVanillaEngine {
    /// Returns the American default absolute spot bump, 8% of spot.
    ///
    /// Exercise decisions amplify small-bump noise; sampling error and bump bias
    /// still require convergence checks. Other shifts use [`NumericalShiftSettings`] defaults.
    /// Rust and Python use this value when `spot_shift` is automatic (`None`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use kiyosi::*;
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloVanillaEngine::new(MonteCarloSettings { seed: Some(42), ..Default::default() })?;
    /// let result = engine.price_with_greeks(&put, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), NumericalShiftSettings {
    ///         spot_shift: Some(MonteCarloVanillaEngine::default_american_spot_shift(&context)),
    ///         ..Default::default()
    ///     })?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn default_american_spot_shift(context: &PricingContext) -> f64 {
        0.08 * context.spot_price()
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "Validated positive i32 path and step counts bound the buffers and fit exactly in f64."
    )]
    fn american_price(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        terminal_only: bool,
    ) -> Result<f64> {
        let model = context.bsm_parameters()?;
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation < option.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
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
        if self.step_count < 2 {
            return Err(Error::InvalidInput(
                "American Monte Carlo requires at least two exercise intervals".into(),
            ));
        }
        if terminal_only
            && option.option_type() == OptionType::Call
            && dividend <= 0.0
            && rate >= 0.0
        {
            return Ok(self.price(option.vanilla(), context)?.max(immediate));
        }
        let intervals = self.step_count as usize;
        let paths = self.settings.path_count as usize;
        let samples = if self.settings.antithetic {
            paths.div_ceil(2)
        } else {
            paths
        };
        let reserved_paths = samples * if self.settings.antithetic { 2 } else { 1 };
        if reserved_paths
            .checked_mul(intervals)
            .is_none_or(|count| count > MAX_PATH_VALUES)
        {
            return Err(Error::InvalidInput(
                "American Monte Carlo path storage exceeds 1 GiB; reduce path_count or step_count"
                    .into(),
            ));
        }
        let dt = time / intervals as f64;
        let sigma = model.volatility();
        let drift = (rate - dividend - 0.5 * sigma * sigma) * dt;
        let diffusion = sigma * dt.sqrt();
        let discount = (-rate * dt).exp();
        if !drift.is_finite() || !diffusion.is_finite() || !discount.is_finite() {
            return Err(unstable());
        }
        let seed = self.settings.resolved_seed()?;
        let sobol = if self.settings.sampling == MonteCarloSampling::Sobol {
            Some(sobol::SobolSequence::new(intervals, seed)?)
        } else {
            None
        };
        let mut partials: Vec<_> = (0..samples.div_ceil(BLOCK_SIZE))
            .map(|_| Ok(Vec::new()))
            .collect();
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
                    let normal = sample_normal(&mut generator, &mut spare, &mut cursor);
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
            if matrix.iter().flatten().any(|value| !value.is_finite()) {
                return Err(unstable());
            }
            matrix[1][0] = matrix[0][1];
            matrix[2][0] = matrix[0][2];
            matrix[2][1] = matrix[1][2];
            let coefficients = solve_quadratic(matrix).ok_or_else(unstable)?;
            for (path, cash) in rows().zip(&mut cash_flows) {
                let intrinsic = payoff(path[step]);
                if intrinsic <= 0.0 {
                    continue;
                }
                let x = path[step] / option.strike();
                let continuation = coefficients[0] + x * (coefficients[1] + x * coefficients[2]);
                if !continuation.is_finite() {
                    return Err(unstable());
                }
                if intrinsic / option.strike() > continuation {
                    *cash = intrinsic;
                }
            }
        }
        let mut mean = Moments::default();
        for cash in cash_flows {
            mean.add(cash, false);
        }
        Ok(scaled_value(mean.scale, mean.mean, -rate * dt)?.max(immediate))
    }
}

impl PricingEngine<AmericanOption> for MonteCarloVanillaEngine {
    fn default_spot_shift(&self, context: &PricingContext) -> f64 {
        Self::default_american_spot_shift(context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.bsm_parameters()?;
        <Self as PricingEngine<EuropeanOption>>::validate_applicability(self, context)
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AmericanOption> + '_>>> {
        if self.settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            settings: self.settings.with_resolved_seed()?,
            ..*self
        })))
    }

    fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        self.american_price(option, context, true)
    }

    fn price_with_greeks(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        context.bsm_parameters()?;
        <Self as PricingEngine<AmericanOption>>::validate_applicability(self, context)?;
        request.validate()?;
        settings.validate()?;
        let settings = NumericalShiftSettings {
            spot_shift: Some(
                settings
                    .spot_shift
                    .unwrap_or_else(|| Self::default_american_spot_shift(context)),
            ),
            ..settings
        };
        let engine = Self {
            settings: self.settings.with_resolved_seed()?,
            ..*self
        };
        let native = |context: &PricingContext, requested: GreeksRequest| {
            let mut result = PricingResult::new(engine.price(option, context)?)?;
            let rate = context.bsm_parameters()?.risk_free_rate()?;
            let dividend = context.bsm_parameters()?.dividend_yield()?;
            for (greek, value) in [(Greek::Rho, rate), (Greek::DividendRho, dividend)] {
                if !requested.has(greek)
                    || context.valuation_time() == option.expiry_date().start_of_day()
                    || option.option_type() != OptionType::Call
                {
                    continue;
                }
                let shift = crate::risk::numerical::symmetric_shift(value, settings.rate_shift);
                let crosses_shortcut = if greek == Greek::Rho {
                    dividend <= 0.0 && rate - shift < 0.0 && rate + shift >= 0.0
                } else {
                    rate >= 0.0 && dividend - shift <= 0.0 && dividend + shift > 0.0
                };
                if shift <= 0.0 || !(200.0 * shift).is_finite() || !crosses_shortcut {
                    continue;
                }
                // Keep the exercise-grid estimator on both sides of the terminal shortcut boundary.
                let evaluate = |value: f64| {
                    let model =
                        context
                            .bsm_parameters()?
                            .replace(BlackScholesMertonParameterChanges {
                                risk_free_rate: (greek == Greek::Rho).then_some(value.into()),
                                dividend_yield: (greek == Greek::DividendRho)
                                    .then_some(value.into()),
                                ..Default::default()
                            })?;
                    let shifted = context.replace(PricingContextChanges {
                        model_parameters: Some(model.into()),
                        ..Default::default()
                    })?;
                    engine.american_price(option, &shifted, false)
                };
                result.set(
                    requested,
                    greek,
                    (evaluate(value + shift)? - evaluate(value - shift)?) / (200.0 * shift),
                )?;
            }
            Ok(result)
        };
        crate::risk::numerical::complete_greeks(
            option,
            context,
            request,
            settings,
            false,
            native(context, request)?,
            native,
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
    fn dividend_stencils_crossing_zero_keep_one_exercise_estimator() {
        let start = Date::new(2025, 1, 1).unwrap();
        let option = AmericanOption::new(
            OptionType::Call,
            100.0,
            start,
            Date::new(2026, 1, 1).unwrap(),
        )
        .unwrap();
        let engine = MonteCarloVanillaEngine::with_step_count(
            MonteCarloSettings {
                path_count: 2048,
                seed: Some(42),
                ..Default::default()
            },
            7,
        )
        .unwrap();
        let shift = 1.0 / 8192.0;
        for rate in [0.0, 0.05] {
            for dividend in [-shift / 2.0, 0.0, shift / 2.0, shift] {
                let context = |dividend| {
                    PricingContext::new(
                        BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
                        100.0,
                        start.into(),
                    )
                    .unwrap()
                };
                let result = engine
                    .price_with_greeks(
                        &option,
                        &context(dividend),
                        GreeksRequest::new([Greek::DividendRho]),
                        NumericalShiftSettings {
                            rate_shift: shift,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let up = engine
                    .american_price(&option, &context(dividend + shift), false)
                    .unwrap();
                let down = engine
                    .american_price(&option, &context(dividend - shift), false)
                    .unwrap();
                assert!(
                    (result.require(Greek::DividendRho).unwrap() - (up - down) / (200.0 * shift))
                        .abs()
                        < 1e-10
                );
                assert_eq!(
                    result.price(),
                    engine.price(&option, &context(dividend)).unwrap()
                );
                assert!(!result.has(Greek::Rho));
                let unavailable = engine
                    .price_with_greeks(
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
