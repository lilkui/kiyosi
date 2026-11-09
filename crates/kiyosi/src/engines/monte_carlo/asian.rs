use crate::instruments::asian::AverageTerms;
use crate::numerics::{checked, log_ratio, scaled_cash};
use crate::{
    ArithmeticAveragePriceOption, Error, GeometricAveragePriceOption, GreeksRequest,
    MonteCarloEstimate, MonteCarloSampling, PricingContext, PricingEngine, PricingResult, Result,
};
use crate::{Greek, MonteCarloSettings};
use crate::{OptionContract, Timestamp, TurnbullWakemanArithmeticAveragePriceEngine};

/// Prices geometric and arithmetic continuous averages using CPU Monte Carlo.
///
/// Geometric averages and single fixings sample their exact distribution in one
/// dimension. Arithmetic averages retain trapezoidal integration with the first
/// interval integrated conditionally; forward starts sample a separate lead interval.
/// Sampling, exact path counts, thread caps and reproducibility follow
/// [`crate::MonteCarloVanillaEngine`]. Shared and distinct engines support concurrent
/// calls with request-owned buffers and shared workers. See [`Self::price`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonteCarloAsianEngine {
    settings: MonteCarloSettings,
    step_count: i32,
}

impl Default for MonteCarloAsianEngine {
    fn default() -> Self {
        Self {
            settings: MonteCarloSettings::default(),
            step_count: super::DEFAULT_STEP_COUNT,
        }
    }
}

impl MonteCarloAsianEngine {
    fn require_geometric_flat_rates(context: &PricingContext) -> Result<()> {
        let model = context.bsm_parameters()?;
        if model.risk_free_curve().exact_flat_rate().is_none()
            || model.dividend_curve().exact_flat_rate().is_none()
        {
            return Err(Error::UnsupportedOperation(
                "geometric Asian Monte Carlo requires flat rates".into(),
            ));
        }
        Ok(())
    }

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
            ..*self
        })
    }

    /// Creates an engine from sampling controls or complete sampling and grid settings.
    ///
    /// Sampling-only inputs use 50 integration intervals. Complete inputs preserve their grid.
    /// See [`crate::MonteCarloVanillaEngine::new`] for sampling configuration examples,
    /// or [`Self::with_step_count`] to configure arithmetic-average integration.
    ///
    /// # Errors
    ///
    /// Reuses [`crate::MonteCarloVanillaEngine::new`] validation and error categories.
    pub fn new(settings: impl Into<super::MonteCarloGridSettings>) -> Result<Self> {
        let settings = settings.into();
        Self::with_step_count(settings.sampling, settings.step_count)
    }

    /// Creates an engine with a configured arithmetic-average integration interval count.
    ///
    /// Geometric averages sample their exact distribution without integration steps.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for steps outside 2..=10,000,
    /// then reuses [`Self::new`] sampling validation. Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{MonteCarloAsianEngine, MonteCarloSettings};
    /// # fn main() -> kiyosi::Result<()> {
    /// let engine = MonteCarloAsianEngine::with_step_count(MonteCarloSettings::default(), 100)?;
    /// assert_eq!(engine.step_count(), 100);
    /// let rebuilt = MonteCarloAsianEngine::with_step_count(
    ///     engine.sampling_settings(), engine.step_count())?;
    /// assert_eq!(rebuilt, engine);
    /// # Ok(()) }
    /// ```
    pub fn with_step_count(settings: MonteCarloSettings, step_count: i32) -> Result<Self> {
        super::validate_step_count(step_count, 2)?;
        settings.validate()?;
        Ok(Self {
            settings,
            step_count,
        })
    }

    /// Returns complete sampling and integration-grid settings.
    ///
    /// Reconstruct with [`Self::new`] without losing the grid. See
    /// [`crate::MonteCarloVanillaEngine::settings`] for an example.
    pub fn settings(&self) -> super::MonteCarloGridSettings {
        super::MonteCarloGridSettings {
            sampling: self.settings,
            step_count: self.step_count,
        }
    }

    /// Returns the arithmetic-average integration interval count; see [`Self::with_step_count`].
    pub fn step_count(&self) -> i32 {
        self.step_count
    }

    /// Returns owned sampling settings; the grid count is available from [`Self::step_count`].
    ///
    /// See [`Self::with_step_count`] for configuration reconstruction.
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Returns a discounted average-payoff estimate, or realized settlement at expiry.
    ///
    /// Calls finish synchronously without cancellation polling. Recoverable entropy
    /// and thread failures preserve [`std::io::Error`] via [`Error::Io`].
    ///
    /// # Errors
    ///
    /// Missing or stale history returns [`crate::Error::MissingHistory`] or
    /// [`crate::Error::StaleHistory`], respectively; see the [history contract](crate).
    ///
    /// Checks inclusive life then history consistency, returning
    /// [`Error::InvalidInput`] for either rejection.
    /// Before expiry geometric contracts require flat curves
    /// ([`Error::UnsupportedOperation`]); both kinds require curve coverage
    /// ([`Error::InvalidInput`]). Non-finite parameters, paying samples
    /// or results return [`Error::ComputationFailure`].
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
    /// use kiyosi::{MonteCarloAsianEngine, MonteCarloSettings,
    ///     GeometricAveragePriceOption, Date, OptionType,
    ///     BlackScholesMertonParameters, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = GeometricAveragePriceOption::new(OptionType::Call, 100.0,
    ///     start, start, Date::new(2026, 1, 1)?.start_of_day(), None, None)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
    ///     seed: Some(42), ..Default::default()
    /// })?;
    /// assert!((engine.price(&option, &context)? - 5.0359).abs() < 0.2);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     kiyosi::GreeksRequest::all())?;
    /// assert!(result.has(kiyosi::Greek::Delta));
    /// assert!(!result.has(kiyosi::Greek::Vega));
    /// let estimate = engine.price_with_statistics(&option, &context)?;
    /// assert!(estimate.standard_error().is_some());
    /// # Ok(()) }
    /// ```
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        context.bsm_parameters()?;
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns a price with sampling standard error and an asymptotic 95% interval.
    ///
    /// Accepts geometric and arithmetic average-price contracts. Uncertainty follows
    /// [`MonteCarloEstimate`]; terminal and analytic pseudorandom branches have zero
    /// uncertainty. Sampling error does not measure arithmetic integration bias.
    /// See [`Self::price`] for an example.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors and returns [`Error::ComputationFailure`]
    /// for non-finite uncertainty or confidence bounds.
    ///
    /// # Panics
    ///
    /// Propagates worker panics as in [`Self::price`].
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior, as in [`Self::price`].
    pub fn price_with_statistics<I>(
        &self,
        option: &I,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate>
    where
        Self: PricingEngine<I>,
    {
        context.bsm_parameters()?;
        <Self as PricingEngine<I>>::price_with_statistics(self, option, context)
    }

    /// Returns requested delta and gamma directly from the price's shared normal draws.
    ///
    /// Uses engine-default estimation controls; see [`crate::numerical_greeks`] for custom bumps.
    /// Terminal stencils use engine-default spot bumps.
    /// Only delta and gamma are supported, including terminal-payoff Greeks at expiry. Before expiry, no numerical bumps are
    /// required. For scalar differences and history-boundary stencil rules, use
    /// [`crate::numerical_greeks`], which pins a request-local seed even for unseeded
    /// engines. See [`Self::price`].
    ///
    /// # Errors
    ///
    /// Missing or stale history returns [`crate::Error::MissingHistory`] or
    /// [`crate::Error::StaleHistory`], respectively; see the [history contract](crate).
    ///
    /// Propagates [`Self::price`] errors.
    /// Non-finite requested Greeks return [`Error::ComputationFailure`].
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work, as in [`Self::price`].
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
        <Self as PricingEngine<I>>::price_with_greeks_with_settings(
            self,
            option,
            context,
            request,
            self.greek_settings(option),
        )
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "The constructor validates positive step counts before converting them to dimensions."
    )]
    fn native(
        &self,
        o: &AverageTerms,
        context: &PricingContext,
        geometric: bool,
        request: GreeksRequest,
        statistics: bool,
    ) -> Result<(PricingResult, Option<f64>, Option<u64>)> {
        let tau = o.validate(context)?;
        let known_error = (statistics
            && self.settings.sampling == MonteCarloSampling::PseudoRandom)
            .then_some(0.0);
        if tau == 0.0 {
            return Ok((
                PricingResult::new(o.settlement(context)?)?,
                known_error,
                None,
            ));
        }
        let model = context.bsm_parameters()?;
        if geometric {
            Self::require_geometric_flat_rates(context)?;
        }
        o.coverage(context)?;
        if crate::engines::deterministic::enabled(context) {
            return Ok((
                PricingResult::new(crate::engines::deterministic::average(
                    o, context, geometric,
                )?)?,
                known_error,
                None,
            ));
        }
        let valuation = context.valuation_time();
        let start = o.averaging_start;
        let expiry = o.terms.expiry_time();
        let single = o.period() == 0.0;
        let lead = if single {
            0.0
        } else {
            valuation.years_until(start).max(0.0)
        };
        let future = if single {
            tau
        } else {
            valuation.max(start).years_until(expiry)
        };
        let weight = if single { 1.0 } else { future / o.period() };
        let sigma = model.volatility();
        let variance_time = if single { tau } else { lead + future / 3.0 };
        let flat =
            model.risk_free_curve().nodes().is_empty() && model.dividend_curve().nodes().is_empty();
        let carry = if flat {
            model.flat_risk_free_rate()? - model.flat_dividend_yield()?
        } else {
            0.0
        };
        let drift_rate = carry - 0.5 * sigma * sigma;
        let log_discount = model.risk_free_curve().log_factor(valuation, expiry)?;
        let carry_between = |from, to| -> Result<f64> {
            Ok(
                -curve_log_factor(model.risk_free_curve(), valuation, from, to)?
                    + curve_log_factor(model.dividend_curve(), valuation, from, to)?,
            )
        };
        if !geometric && !single && o.adjusted_strike(tau) <= 0.0 {
            let log_mean = log_average_forward(context, lead, future)?;
            let first = if o.sign() > 0.0 {
                (log_discount + weight.ln() + log_mean).exp()
            } else {
                0.0
            };
            let elapsed_weight = start.years_until(valuation) / o.period();
            let accrued = elapsed_weight
                .mul_add(o.realized.unwrap_or(0.0), -o.terms.strike())
                .max(0.0);
            let price = if flat {
                TurnbullWakemanArithmeticAveragePriceEngine
                    .price(&ArithmeticAveragePriceOption(o.clone()), context)?
            } else if o.sign() > 0.0 {
                checked(
                    scaled_cash(context.spot_price(), log_discount + weight.ln() + log_mean)
                        + scaled_cash(accrued, log_discount),
                )?
            } else {
                0.0
            };
            return Ok((result([price, first, 0.0], request)?, known_error, None));
        }
        let drift = if single && !flat {
            carry_between(0.0, tau)? - 0.5 * sigma * sigma * tau
        } else {
            weight * drift_rate * if single { tau } else { lead + future / 2.0 }
        };
        let mut p = Parameters {
            spot: context.spot_price(),
            strike: o.terms.strike(),
            realized: o.realized.unwrap_or(0.0),
            weight,
            log_discount,
            drift,
            width: sigma * (weight * variance_time.sqrt()),
            log_width: sigma.ln() + weight.ln() + 0.5 * variance_time.ln(),
            log_base_width: sigma.ln() + 0.5 * variance_time.ln(),
            initial_moneyness: weight * log_ratio(context.spot_price(), o.terms.strike())
                + if valuation > start {
                    (1.0 - weight) * log_ratio(o.realized.unwrap_or(0.0), o.terms.strike())
                } else {
                    0.0
                },
            sign: o.sign(),
            step_count: self.step_count,
            lead_drift: drift_rate * lead,
            lead_width: sigma * lead.sqrt(),
            lead_width_ratio: 0.0,
            interval_forward: 0.0,
        };
        let (means, standard_error, seed) = if geometric || single {
            if !drift.is_finite() || !p.width.is_finite() {
                return Err(crate::engines::monte_carlo::unstable());
            }
            crate::engines::monte_carlo::path_means(self.settings, 1, statistics, |normals| {
                p.geometric(normals[0], request)
            })?
        } else {
            let dt = future / f64::from(self.step_count);
            p.drift = drift_rate * dt;
            p.width = sigma * dt.sqrt();
            p.log_width = sigma.ln() + 0.5 * dt.ln();
            p.lead_width_ratio = (lead / dt).sqrt();
            p.interval_forward = if flat {
                carry * dt
            } else {
                carry_between(lead, lead + dt)?
            };
            let mut interval_drifts = Vec::new();
            if !flat {
                p.drift = p.interval_forward - 0.5 * sigma * sigma * dt;
                p.lead_drift = carry_between(0.0, lead)? - 0.5 * sigma * sigma * lead;
                for step in 2..=self.step_count {
                    let from = lead + future * f64::from(step - 1) / f64::from(self.step_count);
                    let to = if step == self.step_count {
                        tau
                    } else {
                        lead + future * f64::from(step) / f64::from(self.step_count)
                    };
                    interval_drifts.push(carry_between(from, to)? - 0.5 * sigma * sigma * dt);
                }
            }
            if [
                p.drift,
                p.width,
                p.lead_drift,
                p.lead_width,
                p.interval_forward,
            ]
            .iter()
            .chain(&interval_drifts)
            .any(|v| !v.is_finite())
            {
                return Err(crate::engines::monte_carlo::unstable());
            }
            let dimensions = self.step_count as usize - 1 + usize::from(lead > 0.0);
            crate::engines::monte_carlo::path_means(
                self.settings,
                dimensions,
                statistics,
                |normals| p.arithmetic(normals, lead > 0.0, &interval_drifts, request),
            )?
        };
        Ok((
            result(means, request)?.with_seed(Some(seed)),
            standard_error,
            Some(seed),
        ))
    }
}

struct Parameters {
    spot: f64,
    strike: f64,
    realized: f64,
    weight: f64,
    log_discount: f64,
    drift: f64,
    width: f64,
    log_width: f64,
    log_base_width: f64,
    initial_moneyness: f64,
    sign: f64,
    step_count: i32,
    lead_drift: f64,
    lead_width: f64,
    lead_width_ratio: f64,
    interval_forward: f64,
}

impl Parameters {
    fn geometric(&self, normal: f64, request: GreeksRequest) -> Result<[f64; 3]> {
        let distance = self.initial_moneyness + self.drift + self.width * normal;
        let center = if self.width == 0.0 {
            ((self.initial_moneyness + self.drift) / (self.log_width / 2.0).exp())
                / (self.log_width / 2.0).exp()
        } else {
            (self.initial_moneyness + self.drift) / self.width
        };
        let standardized = center + normal;
        if self.sign * standardized <= 0.0 {
            return Ok([0.0; 3]);
        }
        let log_strike = self.strike.ln();
        let log_payoff = if distance.abs() < 1e-5 && standardized.is_finite() {
            log_strike
                + self.log_width
                + standardized.abs().ln()
                + if distance == 0.0 {
                    0.0
                } else {
                    (distance.exp_m1().abs() / distance.abs()).ln()
                }
        } else if self.sign > 0.0 {
            log_strike + distance + (-(-distance).exp_m1()).ln()
        } else {
            log_strike + (-distance.exp_m1()).ln()
        };
        let log_asset = log_strike + distance + self.log_discount + self.weight.ln();
        let delta = if request.has(Greek::Delta) {
            self.sign * (log_asset - self.spot.ln()).exp()
        } else {
            0.0
        };
        let numerator = normal - self.log_base_width.exp();
        let gamma = if request.has(Greek::Gamma) && numerator != 0.0 {
            self.sign
                * (log_asset - 2.0 * self.spot.ln() + numerator.abs().ln() - self.log_base_width)
                    .exp()
                    .copysign(numerator)
        } else {
            0.0
        };
        checked_sample([(log_payoff + self.log_discount).exp(), delta, gamma])
    }
}

fn checked_sample(values: [f64; 3]) -> Result<[f64; 3]> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(crate::engines::monte_carlo::unstable());
    }
    Ok(values)
}

fn result(means: [f64; 3], request: GreeksRequest) -> Result<PricingResult> {
    let mut result = PricingResult::new(means[0])?;
    result.set(request, Greek::Delta, means[1])?;
    result.set(request, Greek::Gamma, means[2])?;
    Ok(result)
}

impl PricingEngine<GeometricAveragePriceOption> for MonteCarloAsianEngine {
    fn price_with_seed(
        &self,
        option: &GeometricAveragePriceOption,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        context.bsm_parameters()?;
        let (result, _, seed) =
            self.native(&option.0, context, true, GreeksRequest::new([]), false)?;
        Ok((result.price(), seed))
    }
    fn greek_method(
        &self,
        option: &GeometricAveragePriceOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<GeometricAveragePriceOption>>::validate_applicability(
            self, context,
        )?;
        if context.valuation_time() != option.expiry_time() {
            Self::require_geometric_flat_rates(context)?;
        }
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            if crate::engines::deterministic::enabled(context) {
                crate::GreekMethod::NativeWithSpotControl
            } else {
                crate::GreekMethod::Native
            },
        ))
    }

    fn price_with_statistics(
        &self,
        option: &GeometricAveragePriceOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        context.bsm_parameters()?;
        let (result, error, seed) =
            self.native(&option.0, context, true, GreeksRequest::new([]), true)?;
        MonteCarloEstimate::from_sampling(result.price(), error, self.sampling_settings(), seed)
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.sampling_settings().validate_solver_seed()
    }

    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<GeometricAveragePriceOption> + '_>>> {
        Ok(if self.settings.seed.is_none() {
            Some(Box::new(Self {
                settings: self.settings.with_resolved_seed()?,
                ..*self
            }))
        } else {
            None
        })
    }
    fn price(&self, option: &GeometricAveragePriceOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        Ok(self
            .native(&option.0, context, true, GreeksRequest::new([]), false)?
            .0
            .price())
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &GeometricAveragePriceOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        super::greeks(self, option, context, request, settings, || {
            self.native(&option.0, context, true, request, false)
                .map(|result| result.0)
        })
    }
}

impl PricingEngine<ArithmeticAveragePriceOption> for MonteCarloAsianEngine {
    fn price_with_seed(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        context.bsm_parameters()?;
        let (result, _, seed) =
            self.native(&option.0, context, false, GreeksRequest::new([]), false)?;
        Ok((result.price(), seed))
    }
    fn greek_method(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<ArithmeticAveragePriceOption>>::validate_applicability(
            self, context,
        )?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            if crate::engines::deterministic::enabled(context) {
                crate::GreekMethod::NativeWithSpotControl
            } else {
                crate::GreekMethod::Native
            },
        ))
    }

    fn price_with_statistics(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        context.bsm_parameters()?;
        let (result, error, seed) =
            self.native(&option.0, context, false, GreeksRequest::new([]), true)?;
        MonteCarloEstimate::from_sampling(result.price(), error, self.sampling_settings(), seed)
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.sampling_settings().validate_solver_seed()
    }

    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<ArithmeticAveragePriceOption> + '_>>> {
        Ok(if self.settings.seed.is_none() {
            Some(Box::new(Self {
                settings: self.settings.with_resolved_seed()?,
                ..*self
            }))
        } else {
            None
        })
    }
    fn price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        context.bsm_parameters()?;
        Ok(self
            .native(&option.0, context, false, GreeksRequest::new([]), false)?
            .0
            .price())
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        super::greeks(self, option, context, request, settings, || {
            self.native(&option.0, context, false, request, false)
                .map(|result| result.0)
        })
    }
    fn volatility_affects_price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> bool {
        let tau = context.valuation_time().years_until(option.expiry_time());
        option.0.adjusted_strike(tau) > 0.0
    }
}

fn curve_log_factor(
    curve: &crate::RateTermStructure,
    valuation: Timestamp,
    from: f64,
    to: f64,
) -> Result<f64> {
    if let Some(rate) = curve.exact_flat_rate() {
        return Ok(-rate * (to - from));
    }
    let mut previous = valuation;
    let mut total = 0.0;
    for &node in curve.nodes().iter().filter(|&&n| n > valuation) {
        let begin = valuation.years_until(previous).max(from);
        let end = valuation.years_until(node).min(to);
        if end > begin {
            total -= curve.forward_rate(previous)? * (end - begin);
        }
        if valuation.years_until(node) >= to {
            break;
        }
        previous = node;
    }
    Ok(total)
}

fn log_average_forward(context: &PricingContext, lead: f64, future: f64) -> Result<f64> {
    let model = context.bsm_parameters()?;
    let valuation = context.valuation_time();
    let mut times = vec![lead, lead + future];
    for curve in [model.risk_free_curve(), model.dividend_curve()] {
        times.extend(
            curve
                .nodes()
                .iter()
                .map(|&n| valuation.years_until(n))
                .filter(|&t| t > lead && t < lead + future),
        );
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    let carry = |from, to| -> Result<f64> {
        Ok(
            -curve_log_factor(model.risk_free_curve(), valuation, from, to)?
                + curve_log_factor(model.dividend_curve(), valuation, from, to)?,
        )
    };
    let mut mean = f64::NEG_INFINITY;
    for pair in times.windows(2) {
        let x = carry(pair[0], pair[1])?;
        let growth = if x == 0.0 {
            0.0
        } else if x > 0.0 {
            x + (-(-x).exp_m1() / x).ln()
        } else {
            (x.exp_m1() / x).ln()
        };
        mean = log_add(
            mean,
            carry(0.0, pair[0])? + growth + ((pair[1] - pair[0]) / future).ln(),
        );
    }
    Ok(mean)
}

fn log_add(a: f64, b: f64) -> f64 {
    let high = a.max(b);
    if high == f64::NEG_INFINITY {
        high
    } else {
        high + (-(a - b).abs()).exp().ln_1p()
    }
}

fn positive_difference(a: f64, b: f64) -> f64 {
    if a == f64::INFINITY {
        return a;
    }
    if b == f64::NEG_INFINITY {
        return a.exp();
    }
    let difference = -(b - a).exp_m1();
    if difference > 0.0 {
        (a + difference.ln()).exp()
    } else {
        0.0
    }
}

impl Parameters {
    #[expect(
        clippy::cast_sign_loss,
        reason = "The constructor validates positive step counts before comparing them to path indices."
    )]
    fn arithmetic(
        &self,
        normals: &[f64],
        lead: bool,
        interval_drifts: &[f64],
        request: GreeksRequest,
    ) -> Result<[f64; 3]> {
        let lead_normal = if lead { normals[0] } else { 0.0 };
        let lead_growth = self.lead_drift + self.lead_width * lead_normal;
        let (mut growth, mut relative_sum, mut log_sum) = (0.0, 0.0, f64::NEG_INFINITY);
        let (mut normal_sum, mut weighted_normals) = (0.0, 0.0);
        for (index, &normal) in normals[usize::from(lead)..].iter().enumerate() {
            let drift = if interval_drifts.is_empty() {
                self.drift
            } else {
                interval_drifts[index]
            };
            growth += drift + self.width * normal;
            let fraction = if index + 2 == self.step_count as usize {
                0.5
            } else {
                1.0
            } / f64::from(self.step_count);
            if self.width < f64::MIN_POSITIVE {
                normal_sum += normal;
                weighted_normals += fraction * normal_sum;
            }
            if growth > 600.0 || log_sum != f64::NEG_INFINITY {
                // Above exp(600), unit offsets no longer affect floating-point sums.
                if log_sum == f64::NEG_INFINITY {
                    log_sum = (1.0 - 0.5 / f64::from(self.step_count) + relative_sum).ln();
                }
                log_sum = log_add(log_sum, fraction.ln() + growth);
            } else {
                relative_sum += fraction * growth.exp_m1();
            }
        }
        let half = 0.5 / f64::from(self.step_count);
        let rest = 1.0 - half;
        let log_spot = self.spot.ln();
        let log_rest = if log_sum == f64::NEG_INFINITY {
            rest.ln() + (relative_sum / rest).ln_1p()
        } else {
            log_sum
        };
        let log_q = log_spot + self.weight.ln() + lead_growth + log_rest + self.interval_forward;
        let log_c = log_spot + self.weight.ln() + lead_growth + half.ln();
        let log_history = if self.realized > 0.0 && self.weight < 1.0 {
            self.realized.ln() + (-self.weight).ln_1p()
        } else {
            f64::NEG_INFINITY
        };
        let log_accrued = log_add(log_history, log_c);
        let log_strike = self.strike.ln();
        if log_accrued >= log_strike {
            if self.sign < 0.0 {
                return Ok([0.0; 3]);
            }
            let cash = positive_difference(
                log_accrued + self.log_discount,
                log_strike + self.log_discount,
            );
            return checked_sample([
                (log_q + self.log_discount).exp() + cash,
                if request.has(Greek::Delta) {
                    (log_q + self.log_discount - log_spot).exp()
                        + (log_c + self.log_discount - log_spot).exp()
                } else {
                    0.0
                },
                0.0,
            ]);
        }
        let log_b = log_strike + (-(log_accrued - log_strike).exp_m1()).ln();
        let mut forward = log_q - log_b;
        if log_sum == f64::NEG_INFINITY {
            // Keep tiny growth separate from the unit value and monetary scale.
            let extra = lead_growth.exp_m1()
                + lead_growth.exp()
                    * (rest * self.interval_forward.exp_m1()
                        + relative_sum * self.interval_forward.exp());
            let excess = (1.0 - self.weight) * ((self.realized - self.strike) / self.spot)
                + self.weight * ((self.spot - self.strike) / self.spot)
                + self.weight * extra;
            let b = (log_b - log_spot).exp();
            if b > 0.0 && (excess / b).abs() < 0.5 {
                forward = (excess / b).ln_1p();
            }
        }
        let vanishing_center = (weighted_normals + self.lead_width_ratio * lead_normal) / rest;
        let mut center = if self.width > 0.0 {
            forward / self.width
        } else if forward == 0.0 {
            vanishing_center
        } else {
            (forward / (self.log_width / 2.0).exp()) / (self.log_width / 2.0).exp()
        };
        let baseline = (1.0 - self.weight) * ((self.realized - self.strike) / self.spot)
            + self.weight * ((self.spot - self.strike) / self.spot);
        if interval_drifts.is_empty()
            && self.width < f64::MIN_POSITIVE
            && self.drift == 0.0
            && self.lead_drift == 0.0
            && self.interval_forward == 0.0
            && baseline == 0.0
        {
            center = vanishing_center;
        }
        let d1 = center + 0.5 * self.width;
        let d2 = center - 0.5 * self.width;
        let log_asset = log_q + self.log_discount + crate::numerics::log_normal_cdf(self.sign * d1);
        let log_cash = log_b + self.log_discount + crate::numerics::log_normal_cdf(self.sign * d2);
        let mut price = if self.sign > 0.0 {
            positive_difference(log_asset, log_cash)
        } else {
            positive_difference(log_cash, log_asset)
        };
        if self.width < 1e-5 && center.abs() < 10.0 {
            let time_value = (log_q + self.log_discount + self.log_width
                - 0.5 * center * center
                - 0.5 * (2.0 * std::f64::consts::PI).ln())
            .exp()
                * (1.0 + (center * center - 1.0) * self.width * self.width / 24.0);
            let correction = if forward == 0.0 {
                0.0
            } else {
                (forward.exp_m1() / forward).ln()
            };
            let intrinsic = if center == 0.0 {
                0.0
            } else {
                (log_cash + self.log_width + center.abs().ln() + correction)
                    .exp()
                    .copysign(center)
            };
            price = self.sign * intrinsic + time_value;
        } else if self.width < 1e-5 {
            price = if self.sign * center > 0.0 {
                if self.width == 0.0 && forward == 0.0 {
                    (log_b + self.log_discount + self.log_width + center.abs().ln()).exp()
                } else if self.sign > 0.0 {
                    (log_q + self.log_discount + (-(-forward).exp_m1()).ln()).exp()
                } else {
                    (log_b + self.log_discount + (-forward.exp_m1()).ln()).exp()
                }
            } else {
                let inverse_square = (1.0 / center) / center;
                let (mut term, mut sum) = (1.0, 1.0);
                for order in 1..=20 {
                    term *= -f64::from(2 * order + 1) * inverse_square;
                    sum += term;
                }
                (log_q + self.log_discount - 0.5 * forward + self.log_width
                    - 0.5 * center * center
                    - 0.5 * (2.0 * std::f64::consts::PI).ln()
                    - 2.0 * center.abs().ln()
                    + sum.ln())
                .exp()
            };
        }
        let delta = if request.has(Greek::Delta) {
            self.sign
                * ((log_c + self.log_discount - log_spot
                    + crate::numerics::log_normal_cdf(self.sign * d2))
                .exp()
                    + (log_q + self.log_discount - log_spot
                        + crate::numerics::log_normal_cdf(self.sign * d1))
                    .exp())
        } else {
            0.0
        };
        let gamma = if request.has(Greek::Gamma) {
            let k = (-(1.0 - self.weight)).mul_add(self.realized, self.strike);
            (self.log_discount + 2.0 * k.ln()
                - 2.0 * log_spot
                - log_b
                - self.log_width
                - 0.5 * d2 * d2
                - 0.5 * (2.0 * std::f64::consts::PI).ln())
            .exp()
        } else {
            0.0
        };
        checked_sample([price, delta, gamma])
    }
}
