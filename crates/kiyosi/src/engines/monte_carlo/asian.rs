use crate::engines::curve::Curve;
use crate::instruments::asian::AverageTerms;
use crate::numerics::{checked, log_add, log_ratio, scaled_cash};
use crate::{
    ArithmeticAveragePriceOption, AveragePriceOptionChanges, Error, GeometricAveragePriceOption,
    Greek, GreeksRequest, PricingContext, PricingEngine, PricingResult, Result,
    engines::{MonteCarloEstimate, MonteCarloSampling, MonteCarloSettings},
};
use crate::{Timestamp, TurnbullWakemanArithmeticAveragePriceEngine};

const STRIKE_TILE: usize = 32;

/// Prices geometric and arithmetic continuous averages using CPU Monte Carlo.
///
/// Geometric averages and single fixings sample their exact distribution in one
/// dimension. Arithmetic averages use trapezoidal integration with the first
/// interval integrated conditionally; forward starts sample a separate lead interval.
/// Near the money, a geometric payoff on the same integration grid reduces
/// sampling variance for arithmetic prices and spot Greeks.
/// Sampling, exact path counts, thread caps and reproducibility follow
/// [`crate::MonteCarloVanillaEngine`]. Shared and distinct engines support concurrent
/// calls with request-owned buffers and shared workers. See [`Self::price`].
///
/// Returns a discounted average-payoff estimate, or realized settlement at expiry.
///
/// Cancellation follows [`crate::CancellationToken`]. Recoverable entropy
/// and thread failures preserve [`std::io::Error`] via [`Error::Io`].
///
/// # Errors
///
/// Missing required history returns [`crate::Error::MissingHistory`];
/// see the [caller-supplied history contract](crate).
///
/// Checks inclusive life then history consistency. Invalid bounds or impossible
/// history return [`Error::InvalidInput`].
/// Before expiry geometric contracts require flat curves
/// ([`Error::UnsupportedOperation`]); both kinds require curve coverage
/// ([`Error::InvalidInput`]). Sampling also returns `InvalidInput` when the
/// path count times the number of draws per path exceeds one billion.
/// Non-finite parameters, paying samples
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
/// use kiyosi::PricingEngine;
/// use kiyosi::{MonteCarloAsianEngine, GeometricAveragePriceOption, Date, OptionType,
///     BlackScholesMertonParameters, PricingContext, engines::MonteCarloSettings};
/// # fn main() -> kiyosi::Result<()> {
/// let start = Date::new(2025, 1, 1)?.start_of_day();
/// let option = GeometricAveragePriceOption::new(OptionType::Call, 100.0,
///     start, start, Date::new(2026, 1, 1)?.start_of_day(), None)?;
/// let context = PricingContext::new(
///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
/// let engine = MonteCarloAsianEngine::new(kiyosi::engines::MonteCarloGridSettings {
///     sampling: MonteCarloSettings { seed: Some(42), ..Default::default() },
///     ..Default::default()
/// })?;
/// assert!((engine.price(&option, &context)? - 5.0359).abs() < 0.2);
/// let result = engine.price_with_greeks(&option, &context,
///     kiyosi::GreeksRequest::all())?;
/// assert!(result.get(kiyosi::Greek::Delta).is_some());
/// assert!(result.get(kiyosi::Greek::Vega).is_none());
/// let estimate = engine.price_with_statistics(&option, &context)?;
/// assert!(estimate.standard_error().is_some());
/// # Ok(()) }
/// ```
///
/// # Greeks
///
/// Returns requested delta and gamma directly from the price's shared normal draws.
///
/// Uses engine-default estimation controls; see [`crate::numerical_greeks`] for custom bumps.
/// Terminal stencils use engine-default spot bumps.
/// Only delta and gamma are supported, including terminal-payoff Greeks at expiry. Before expiry, no numerical bumps are
/// required. For scalar differences and history-boundary stencil rules, use
/// [`crate::numerical_greeks`], which pins a request-local seed even for unseeded
/// engines. See [`Self::price`].
///
/// ## Errors
///
/// Missing required history returns [`crate::Error::MissingHistory`];
/// see the [caller-supplied history contract](crate).
///
/// Propagates [`Self::price`] errors.
/// Non-finite requested Greeks return [`Error::ComputationFailure`].
///
/// ## Panics
///
/// Propagates worker panics after the call's parallel work, as in [`Self::price`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloAsianEngine {
    settings: super::MonteCarloGridSettings,
}

impl MonteCarloAsianEngine {
    fn require_geometric_flat_rates(context: &PricingContext) -> Result<()> {
        let model = context.natural_bsm_parameters()?;
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
    /// The returned engine copies all other controls, including any integration or
    /// exercise grid. Save it before pricing to replay the calculation within the
    /// same build. See
    /// [`crate::MonteCarloVanillaEngine::with_resolved_seed`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Io`] if system entropy is unavailable.
    pub fn with_resolved_seed(&self) -> crate::Result<Self> {
        let mut engine = *self;
        engine.settings.sampling = engine.settings.sampling.with_resolved_seed()?;
        Ok(engine)
    }

    /// Creates an engine from complete sampling and grid settings.
    ///
    /// Geometric averages sample their exact distribution without integration steps.
    /// See [`crate::MonteCarloVanillaEngine::new`] for sampling configuration examples
    /// and [`Self::settings`] for configuration reconstruction.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for steps outside 2..=10,000,
    /// then propagates [`crate::engines::MonteCarloSettings::validate`] errors. Inputs are consumed on failure.
    pub fn new(settings: super::MonteCarloGridSettings) -> Result<Self> {
        super::validate_step_count(settings.step_count, 2)?;
        settings.sampling.validate()?;
        Ok(Self { settings })
    }

    /// Returns complete sampling and integration-grid settings.
    ///
    /// Reconstruct with [`Self::new`] without losing the grid. See
    /// [`crate::MonteCarloVanillaEngine::settings`] for an example.
    pub fn settings(&self) -> super::MonteCarloGridSettings {
        self.settings
    }

    /// Prices arithmetic-average options across strikes on common simulated paths.
    ///
    /// The template supplies direction, dates and realized history. Each price
    /// uses its own payoff and variance adjustment. Duplicate strikes share a
    /// quote, and prices preserve input order. A stochastic request uses one seed
    /// across the batch.
    /// Use [`Self::with_resolved_seed`] for replay with an unseeded engine.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{ArithmeticAveragePriceOption, BlackScholesMertonParameters, Date,
    ///     MonteCarloAsianEngine, OptionType, PricingContext, engines::{MonteCarloGridSettings,
    ///     MonteCarloSettings}};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = ArithmeticAveragePriceOption::new(OptionType::Call, 100.0,
    ///     start, start, start.add_days(365)?, None)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start)?;
    /// let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
    ///     sampling: MonteCarloSettings { path_count: 4096, seed: Some(7), ..Default::default() },
    ///     ..Default::default()
    /// })?;
    /// let prices = engine.price_strikes(&option, &context, &[95.0, 105.0])?;
    /// assert!(prices[0] > prices[1]);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Applies the contract and market constraints of [`Self::price`], including
    /// for empty input. Invalid strikes, excessive batch workspace or more than
    /// one billion combined path steps and payoff evaluations return
    /// [`Error::InvalidInput`]. Cancellation returns [`Error::Io`].
    ///
    /// # Panics
    ///
    /// Propagates worker panics after parallel work has finished.
    pub fn price_strikes(
        &self,
        template: &ArithmeticAveragePriceOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        context.natural_bsm_parameters()?;
        let option = &template.0;
        let tau = option.validate(context)?;
        if tau > 0.0 {
            option.coverage(context)?;
        }
        crate::cancellation::check()?;
        super::validate_monte_carlo_strike_batch_size(strikes.len())?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        let stochastic = tau > 0.0 && !crate::engines::deterministic::enabled(context);
        super::quote_distinct_strikes(strikes, |unique| {
            self.price_distinct_arithmetic_strikes(template, context, unique, stochastic)
        })
    }

    fn price_distinct_arithmetic_strikes(
        &self,
        template: &ArithmeticAveragePriceOption,
        context: &PricingContext,
        strikes: &[f64],
        stochastic: bool,
    ) -> Result<Vec<f64>> {
        let option = &template.0;
        let shared = stochastic && option.period() > 0.0;
        let valuation = context.valuation_time();
        let sampled = |strike| option.adjusted_strike_for(valuation, strike) > 0.0;
        let mut first = None;
        let mut active_tiles = 0;
        for chunk in strikes.chunks(STRIKE_TILE) {
            let mut active = false;
            for &strike in chunk {
                crate::cancellation::check()?;
                if shared && sampled(strike) {
                    first.get_or_insert(strike);
                    active = true;
                }
            }
            active_tiles += usize::from(active);
        }
        let contract = |strike| {
            template.replace(AveragePriceOptionChanges {
                strike: Some(strike),
                ..Default::default()
            })
        };
        let mut prices = Vec::new();
        prices.try_reserve_exact(strikes.len()).map_err(|_| {
            Error::ComputationFailure("cannot reserve Asian strike batch output".into())
        })?;
        let settings = self.settings.sampling;
        let Some(first) = first else {
            let engine = if stochastic && !shared && !strikes.is_empty() {
                settings.validate_path_steps(strikes.len().saturating_mul(2))?;
                self.with_resolved_seed()?
            } else {
                *self
            };
            for &strike in strikes {
                crate::cancellation::check()?;
                prices.push(engine.price(&contract(strike)?, context)?);
            }
            return Ok(prices);
        };
        let request = GreeksRequest::new([]);
        let Prepared::Arithmetic(grid) =
            self.prepare(&contract(first)?.0, context, false, request)?
        else {
            return Err(super::unstable());
        };
        settings.validate_path_steps(
            active_tiles
                .saturating_mul(grid.dimensions)
                .saturating_add(strikes.len()),
        )?;
        let engine = self.with_resolved_seed()?;
        let settings = engine.settings.sampling;
        for chunk in strikes.chunks(STRIKE_TILE) {
            crate::cancellation::check()?;
            let mut payoffs = [None; STRIKE_TILE];
            for (lane, &strike) in payoffs.iter_mut().zip(chunk) {
                if sampled(strike) {
                    *lane = Some(ArithmeticParameters {
                        strike,
                        ..grid.payoff
                    });
                }
            }
            let mut values = [None; STRIKE_TILE];
            if payoffs.iter().any(Option::is_some) {
                grid.sample_strikes(settings, &payoffs, &mut values)?;
            }
            for (value, &strike) in values.into_iter().zip(chunk) {
                prices.push(match value {
                    Some(price) if price >= 0.0 => crate::numerics::finite_price(price)?,
                    _ => engine.price(&contract(strike)?, context)?,
                });
            }
        }
        Ok(prices)
    }

    fn native(
        &self,
        option: &AverageTerms,
        context: &PricingContext,
        geometric: bool,
        request: GreeksRequest,
        statistics: bool,
    ) -> Result<(PricingResult, Option<f64>)> {
        let settings = self.settings.sampling;
        let (means, error, seed) = match self.prepare(option, context, geometric, request)? {
            Prepared::Known(result) => {
                let error = (statistics && settings.sampling == MonteCarloSampling::PseudoRandom)
                    .then_some(0.0);
                return Ok((result, error));
            }
            Prepared::Geometric(payoff) => super::path_means(settings, 1, statistics, |normals| {
                payoff.geometric(normals[0], request)
            })?,
            Prepared::Arithmetic(grid) => grid.sample(settings, request, statistics)?,
        };
        Ok((result(means, request)?.with_seed(Some(seed)), error))
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "The constructor validates positive step counts before converting them to dimensions."
    )]
    fn prepare(
        &self,
        o: &AverageTerms,
        context: &PricingContext,
        geometric: bool,
        request: GreeksRequest,
    ) -> Result<Prepared> {
        let step_count = self.settings.step_count;
        let tau = o.validate(context)?;
        if tau == 0.0 {
            return Ok(Prepared::Known(PricingResult::new(o.settlement(context)?)?));
        }
        let model = context.natural_bsm_parameters()?;
        if geometric {
            Self::require_geometric_flat_rates(context)?;
        }
        o.coverage(context)?;
        if crate::engines::deterministic::enabled(context) {
            return Ok(Prepared::Known(PricingResult::new(
                crate::engines::deterministic::average(o, context, geometric)?,
            )?));
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
        let prior_weight = if valuation > start {
            start.years_until(valuation) / o.period()
        } else {
            0.0
        };
        let sigma = model.volatility();
        let flat =
            model.risk_free_curve().nodes().is_empty() && model.dividend_curve().nodes().is_empty();
        let (rate, dividend) = if flat || geometric {
            (model.flat_risk_free_rate()?, model.flat_dividend_yield()?)
        } else {
            (0.0, 0.0)
        };
        let carry = rate - dividend;
        let drift_rate = carry - 0.5 * sigma * sigma;
        let log_discount = model.risk_free_curve().log_factor(valuation, expiry)?;
        let curves = if flat || geometric || single {
            None
        } else {
            Some([
                Curve::new(model.risk_free_curve(), valuation, expiry)?,
                Curve::new(model.dividend_curve(), valuation, expiry)?,
            ])
        };
        let carry_between = |from, to| match &curves {
            Some([risk, dividend]) => -risk.log_factor(from, to) + dividend.log_factor(from, to),
            None => rate * (to - from) - dividend * (to - from),
        };
        if !geometric && !single && o.adjusted_strike(valuation) <= 0.0 {
            let log_mean = log_average_forward(model, valuation, lead, future, carry_between);
            let first = if o.sign() > 0.0 {
                (log_discount + weight.ln() + log_mean).exp()
            } else {
                0.0
            };
            let accrued = prior_weight
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
            return Ok(Prepared::Known(result([price, first, 0.0], request)?));
        }
        if geometric || single {
            let variance_time = if single { tau } else { lead + future / 3.0 };
            let moneyness = if single {
                let carry = model.dividend_curve().log_factor(valuation, expiry)? - log_discount;
                context.log_forward_moneyness(expiry, o.terms.strike(), carry)
            } else {
                weight * log_ratio(context.spot_price(), o.terms.strike())
            };
            let p = GeometricParameters {
                spot: context.spot_price(),
                strike: o.terms.strike(),
                weight,
                log_discount,
                drift: if single {
                    -0.5 * sigma * sigma * tau
                } else {
                    weight * drift_rate * (lead + future / 2.0)
                },
                width: sigma * (weight * variance_time.sqrt()),
                log_width: sigma.ln() + weight.ln() + 0.5 * variance_time.ln(),
                log_base_width: sigma.ln() + 0.5 * variance_time.ln(),
                initial_moneyness: moneyness
                    + if valuation > start {
                        prior_weight * log_ratio(o.realized.unwrap_or(0.0), o.terms.strike())
                    } else {
                        0.0
                    },
                sign: o.sign(),
            };
            if !p.drift.is_finite() || !p.width.is_finite() {
                return Err(crate::engines::monte_carlo::unstable());
            }
            Ok(Prepared::Geometric(p))
        } else {
            let dt = future / f64::from(step_count);
            let interval_forward = if flat {
                carry * dt
            } else {
                carry_between(lead, lead + dt)
            };
            let mut interval_drifts = Vec::new();
            if !flat {
                for step in 2..=step_count {
                    let from = lead + future * f64::from(step - 1) / f64::from(step_count);
                    let to = if step == step_count {
                        tau
                    } else {
                        lead + future * f64::from(step) / f64::from(step_count)
                    };
                    interval_drifts.push(carry_between(from, to) - 0.5 * sigma * sigma * dt);
                }
            }
            let p = ArithmeticParameters {
                spot: context.spot_price(),
                strike: o.terms.strike(),
                realized: o.realized.unwrap_or(0.0),
                weight,
                prior_weight,
                log_discount,
                drift: if flat {
                    drift_rate * dt
                } else {
                    interval_forward - 0.5 * sigma * sigma * dt
                },
                width: sigma * dt.sqrt(),
                log_width: sigma.ln() + 0.5 * dt.ln(),
                sign: o.sign(),
                step_count,
                lead_drift: if flat {
                    drift_rate * lead
                } else {
                    carry_between(0.0, lead) - 0.5 * sigma * sigma * lead
                },
                lead_width: sigma * lead.sqrt(),
                lead_width_ratio: (lead / dt).sqrt(),
                interval_forward,
            };
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
            let dimensions = step_count as usize - 1 + usize::from(lead > 0.0);
            Ok(Prepared::Arithmetic(ArithmeticGrid {
                payoff: p,
                interval_drifts,
                lead: lead > 0.0,
                dimensions,
            }))
        }
    }
}

enum Prepared {
    Known(PricingResult),
    Geometric(GeometricParameters),
    Arithmetic(ArithmeticGrid),
}

struct ArithmeticGrid {
    payoff: ArithmeticParameters,
    interval_drifts: Vec<f64>,
    lead: bool,
    dimensions: usize,
}

impl ArithmeticGrid {
    fn sample_strikes(
        &self,
        settings: MonteCarloSettings,
        payoffs: &[Option<ArithmeticParameters>; STRIKE_TILE],
        prices: &mut [Option<f64>; STRIKE_TILE],
    ) -> Result<()> {
        let request = GreeksRequest::new([]);
        let mut controls = [None; STRIKE_TILE];
        for (control, payoff) in controls.iter_mut().zip(payoffs) {
            if let Some(payoff) = payoff {
                *control = payoff.geometric_control(&self.interval_drifts, request)?;
            }
        }
        let geometric = controls.iter().any(Option::is_some);
        let flat = self.interval_drifts.is_empty();
        let (means, _, _) = super::path_means(settings, self.dimensions, false, |normals| {
            let path = self.path(normals, geometric)?;
            let mut values = [0.0; STRIKE_TILE];
            for ((value, payoff), control) in values.iter_mut().zip(payoffs).zip(&controls) {
                if let Some(payoff) = payoff {
                    *value = payoff.arithmetic(path, flat, request, control.as_ref())?[0];
                }
            }
            Ok(values)
        })?;
        for (((price, payoff), mean), control) in
            prices.iter_mut().zip(payoffs).zip(means).zip(controls)
        {
            *price = payoff.map(|_| mean + control.map_or(0.0, |c| c.expected[0]));
        }
        Ok(())
    }

    fn sample(
        &self,
        settings: MonteCarloSettings,
        request: GreeksRequest,
        statistics: bool,
    ) -> Result<([f64; 3], Option<f64>, u64)> {
        let control = self
            .payoff
            .geometric_control(&self.interval_drifts, request)?;
        let sample = |settings, control: Option<&GeometricControl>| {
            super::path_means(settings, self.dimensions, statistics, |normals| {
                let path = self.path(normals, control.is_some())?;
                self.payoff
                    .arithmetic(path, self.interval_drifts.is_empty(), request, control)
            })
        };
        let (mut means, error, seed) = sample(settings, control.as_ref())?;
        if let Some(control) = control {
            for (mean, expected) in means.iter_mut().zip(control.expected) {
                *mean += expected;
            }
        }
        if means[0] < 0.0 {
            // An unusually negative put residual can overwhelm the exact control mean.
            sample(
                MonteCarloSettings {
                    seed: Some(seed),
                    ..settings
                },
                None,
            )
        } else {
            Ok((means, error, seed))
        }
    }
    #[expect(
        clippy::cast_sign_loss,
        reason = "The constructor validates positive step counts before comparing them to path indices."
    )]
    fn path(&self, normals: &[f64], geometric: bool) -> Result<ArithmeticPath> {
        let p = &self.payoff;
        let lead = self.lead;
        let interval_drifts = &self.interval_drifts;
        let lead_normal = if lead { normals[0] } else { 0.0 };
        let lead_growth = p.lead_drift + p.lead_width * lead_normal;
        let (mut growth, mut relative_sum, mut log_sum) = (0.0, 0.0, f64::NEG_INFINITY);
        let (mut normal_sum, mut weighted_normals) = (0.0, 0.0);
        let mut geometric_growth = 0.0;
        for (index, &normal) in normals[usize::from(lead)..].iter().enumerate() {
            super::check_path_step(index)?;
            let drift = if interval_drifts.is_empty() {
                p.drift
            } else {
                interval_drifts[index]
            };
            growth += drift + p.width * normal;
            let fraction = if index + 2 == p.step_count as usize {
                0.5
            } else {
                1.0
            } / f64::from(p.step_count);
            if geometric {
                geometric_growth += fraction * growth;
            }
            if p.width < f64::MIN_POSITIVE {
                normal_sum += normal;
                weighted_normals += fraction * normal_sum;
            }
            if growth > 600.0 || log_sum != f64::NEG_INFINITY {
                // Above exp(600), unit offsets fall below floating-point precision.
                if log_sum == f64::NEG_INFINITY {
                    log_sum = (1.0 - 0.5 / f64::from(p.step_count) + relative_sum).ln();
                }
                log_sum = log_add(log_sum, fraction.ln() + growth);
            } else {
                relative_sum += fraction * growth.exp_m1();
            }
        }
        let half = 0.5 / f64::from(p.step_count);
        let rest = 1.0 - half;
        let log_spot = p.spot.ln();
        let log_rest = if log_sum == f64::NEG_INFINITY {
            rest.ln() + (relative_sum / rest).ln_1p()
        } else {
            log_sum
        };
        let log_q = log_spot + p.weight.ln() + lead_growth + log_rest + p.interval_forward;
        let log_c = log_spot + p.weight.ln() + lead_growth + half.ln();
        let log_history = if p.realized > 0.0 && p.prior_weight > 0.0 {
            p.realized.ln() + p.prior_weight.ln()
        } else {
            f64::NEG_INFINITY
        };
        let log_accrued = log_add(log_history, log_c);
        // Keep tiny growth separate from the unit value and monetary scale.
        let extra = (log_sum == f64::NEG_INFINITY).then(|| {
            lead_growth.exp_m1()
                + lead_growth.exp()
                    * (rest * p.interval_forward.exp_m1() + relative_sum * p.interval_forward.exp())
        });
        let vanishing_center = (weighted_normals + p.lead_width_ratio * lead_normal) / rest;
        Ok(ArithmeticPath {
            lead_growth,
            geometric_growth,
            log_spot,
            log_q,
            log_c,
            log_accrued,
            extra,
            vanishing_center,
        })
    }
}

#[derive(Clone, Copy)]
struct ArithmeticPath {
    lead_growth: f64,
    geometric_growth: f64,
    log_spot: f64,
    log_q: f64,
    log_c: f64,
    log_accrued: f64,
    extra: Option<f64>,
    vanishing_center: f64,
}

struct GeometricParameters {
    spot: f64,
    strike: f64,
    weight: f64,
    log_discount: f64,
    drift: f64,
    width: f64,
    log_width: f64,
    log_base_width: f64,
    initial_moneyness: f64,
    sign: f64,
}

impl GeometricParameters {
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

macro_rules! asian_pricing {
    ($option:ty, $geometric:literal) => {
        impl PricingEngine<$option> for MonteCarloAsianEngine {
            fn price_with_seed(
                &self,
                option: &$option,
                context: &PricingContext,
            ) -> Result<(f64, Option<u64>)> {
                context.natural_bsm_parameters()?;
                let (result, _) = self.native(
                    &option.0,
                    context,
                    $geometric,
                    GreeksRequest::new([]),
                    false,
                )?;
                Ok((result.price(), result.seed()))
            }
            fn greek_method(
                &self,
                option: &$option,
                context: &PricingContext,
            ) -> Result<crate::GreekMethod> {
                <Self as PricingEngine<$option>>::validate_applicability(self, context)?;
                if $geometric && context.valuation_time() != option.expiry_time() {
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
            fn price_with_statistics(
                &self,
                option: &$option,
                context: &PricingContext,
            ) -> Result<MonteCarloEstimate> {
                context.natural_bsm_parameters()?;
                let (result, error) =
                    self.native(&option.0, context, $geometric, GreeksRequest::new([]), true)?;
                MonteCarloEstimate::from_sampling(
                    result.price(),
                    error,
                    self.settings.sampling,
                    result.seed(),
                )
            }
            fn validate_solver_seed(&self) -> Result<()> {
                self.settings.sampling.validate_solver_seed()
            }

            fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<$option> + '_>>> {
                prepare_seeded_request!(self, self.settings.sampling)
            }
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                self.price_with_seed(option, context)
                    .map(|(price, _)| price)
            }

            fn price_with_greeks_with_settings(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                settings: crate::NumericalShiftSettings,
            ) -> Result<PricingResult> {
                super::greeks(self, option, context, request, settings, || {
                    self.native(&option.0, context, $geometric, request, false)
                        .map(|result| result.0)
                })
            }
        }
    };
}
asian_pricing!(GeometricAveragePriceOption, true);
asian_pricing!(ArithmeticAveragePriceOption, false);

fn log_average_forward(
    model: &crate::BlackScholesMertonParameters,
    valuation: Timestamp,
    lead: f64,
    future: f64,
    carry: impl Fn(f64, f64) -> f64,
) -> f64 {
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
    let mut mean = f64::NEG_INFINITY;
    for pair in times.windows(2) {
        let growth = crate::numerics::log_mean_exponential(carry(pair[0], pair[1]));
        mean = log_add(
            mean,
            carry(0.0, pair[0]) + growth + ((pair[1] - pair[0]) / future).ln(),
        );
    }
    mean
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

#[derive(Clone, Copy)]
struct ArithmeticParameters {
    spot: f64,
    strike: f64,
    realized: f64,
    weight: f64,
    prior_weight: f64,
    log_discount: f64,
    drift: f64,
    width: f64,
    log_width: f64,
    sign: f64,
    step_count: i32,
    lead_drift: f64,
    lead_width: f64,
    lead_width_ratio: f64,
    interval_forward: f64,
}

#[derive(Clone, Copy)]
struct GeometricControl {
    log_strike: f64,
    log_asset_scale: f64,
    first_weight: f64,
    expected: [f64; 3],
}

impl ArithmeticParameters {
    fn geometric_control(
        &self,
        interval_drifts: &[f64],
        request: GreeksRequest,
    ) -> Result<Option<GeometricControl>> {
        let n = f64::from(self.step_count);
        let first_weight = 1.0 - 0.5 / n;
        let log_asset_scale = self.spot.ln() + self.weight.ln();
        let adjusted_strike = (-self.prior_weight).mul_add(self.realized, self.strike);
        let log_strike = adjusted_strike.ln();
        if self.width < 1e-5
            || self.width > 0.8
            || self.log_discount.abs() > 50.0
            || self.spot.ln().abs() > 200.0
            || log_asset_scale.abs() > 200.0
            || !log_strike.is_finite()
        {
            return Ok(None);
        }
        // Each increment contributes to all the future fixings after it.
        // The tail weights are (n - 1/2)/n, ..., (1/2)/n.
        let mut mean = self.lead_drift + first_weight * self.drift;
        for step in 1..self.step_count {
            super::check_path_step(usize::try_from(step).expect("positive step index"))?;
            let drift = interval_drifts
                .get(usize::try_from(step - 1).expect("nonnegative step index"))
                .copied()
                .unwrap_or(self.drift);
            mean += (first_weight - f64::from(step) / n) * drift;
        }
        let variance = self.lead_width.powi(2) + self.width.powi(2) * (n / 3.0 - 1.0 / (12.0 * n));
        let width = variance.sqrt();
        let log_forward = log_asset_scale + mean + 0.5 * variance;
        // Restrict the control to ordinary prices where the two average payoffs
        // are closely related and the signed residual stays well scaled.
        if !(0.02..=0.8).contains(&width) || (log_forward - log_strike).abs() > 2.0 * width {
            return Ok(None);
        }
        let expected = self.geometric_values(log_forward, log_strike, width, request);
        Ok(Some(GeometricControl {
            log_strike,
            log_asset_scale,
            first_weight,
            expected,
        }))
    }

    fn geometric_values(
        &self,
        log_forward: f64,
        log_strike: f64,
        width: f64,
        request: GreeksRequest,
    ) -> [f64; 3] {
        let d1 = (log_forward - log_strike) / width + 0.5 * width;
        let d2 = d1 - width;
        let log_asset =
            log_forward + self.log_discount + crate::numerics::log_normal_cdf(self.sign * d1);
        let log_cash =
            log_strike + self.log_discount + crate::numerics::log_normal_cdf(self.sign * d2);
        let price = if self.sign > 0.0 {
            positive_difference(log_asset, log_cash)
        } else {
            positive_difference(log_cash, log_asset)
        };
        let log_spot = self.spot.ln();
        let delta = if request.has(Greek::Delta) {
            self.sign * (log_asset - log_spot).exp()
        } else {
            0.0
        };
        let gamma = if request.has(Greek::Gamma) {
            (log_forward + self.log_discount
                - 2.0 * log_spot
                - width.ln()
                - 0.5 * d1 * d1
                - 0.5 * (2.0 * std::f64::consts::PI).ln())
            .exp()
        } else {
            0.0
        };
        [price, delta, gamma]
    }

    fn arithmetic(
        &self,
        path: ArithmeticPath,
        flat: bool,
        request: GreeksRequest,
        control: Option<&GeometricControl>,
    ) -> Result<[f64; 3]> {
        let ArithmeticPath {
            lead_growth,
            geometric_growth,
            log_spot,
            log_q,
            log_c,
            log_accrued,
            extra,
            vanishing_center,
        } = path;
        let residual = |mut values: [f64; 3]| {
            if let Some(control) = control {
                let width = control.first_weight * self.width;
                let log_forward = control.log_asset_scale
                    + lead_growth
                    + geometric_growth
                    + control.first_weight * self.drift
                    + 0.5 * width * width;
                let geometric =
                    self.geometric_values(log_forward, control.log_strike, width, request);
                for (value, term) in values.iter_mut().zip(geometric) {
                    *value -= term;
                }
            }
            checked_sample(values)
        };
        let log_strike = self.strike.ln();
        if log_accrued >= log_strike {
            if self.sign < 0.0 {
                return residual([0.0; 3]);
            }
            let cash = positive_difference(
                log_accrued + self.log_discount,
                log_strike + self.log_discount,
            );
            return residual([
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
        let baseline = self.prior_weight * ((self.realized - self.strike) / self.spot)
            + self.weight * ((self.spot - self.strike) / self.spot);
        if let Some(extra) = extra {
            let excess = baseline + self.weight * extra;
            let b = (log_b - log_spot).exp();
            if b > 0.0 && (excess / b).abs() < 0.5 {
                forward = (excess / b).ln_1p();
            }
        }
        let mut center = if self.width > 0.0 {
            forward / self.width
        } else if forward == 0.0 {
            vanishing_center
        } else {
            (forward / (self.log_width / 2.0).exp()) / (self.log_width / 2.0).exp()
        };
        if flat
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
            let k = (-self.prior_weight).mul_add(self.realized, self.strike);
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
        residual([price, delta, gamma])
    }
}
