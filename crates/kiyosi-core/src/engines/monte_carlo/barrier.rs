use super::{path_means, random_seed, scaled_value, unstable};
use crate::engines::analytic::barrier::{
    hit_value, log_ratio, resolved_price, validate_life, validate_schedule,
};
use crate::{
    AnalyticDoubleBarrierEngine, AnalyticEuropeanEngine, BarrierOption, DoubleBarrierOption,
    EuropeanOption, Greek, GreeksRequest, MonteCarloSettings, MonteCarloVanillaEngine,
    NumericalShiftSettings, ObservationMode, OptionContract, OptionType, PricingContext,
    PricingEngine, PricingResult, RebateTiming, Result,
};

#[derive(Clone, Copy)]
pub(super) enum Contract<'a> {
    Single(&'a BarrierOption),
    Double(&'a DoubleBarrierOption),
}

/// Prices single-barrier contracts with conditional-survival CPU Monte Carlo.
///
/// Continuous monitoring uses one terminal draw and Brownian-bridge survival;
/// scheduled monitoring uses original fixing dates and an unobserved expiry leg
/// when needed. Steps are validated but unused. Sampling, exact path counts and
/// thread reproducibility follow [`MonteCarloVanillaEngine`]. Shared and distinct
/// instances support concurrent calls with request-owned workers and samples.
/// Returns likelihood-ratio delta/gamma; scalar numerical analysis is available
/// through [`crate::numerical_greeks`]. See [`Self::price_with_greeks`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloBarrierEngine {
    settings: MonteCarloSettings,
}

impl MonteCarloBarrierEngine {
    /// Creates an engine with validated shared CPU sampling settings.
    ///
    /// See [`MonteCarloVanillaEngine::new`] for configuration examples.
    ///
    /// # Errors
    ///
    /// Reuses [`MonteCarloVanillaEngine::new`] validation and error categories.
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        MonteCarloVanillaEngine::new(settings)?;
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for configuration examples.
    pub fn settings(&self) -> MonteCarloSettings {
        self.settings
    }

    /// Prices after validating flat rates, inclusive life, calendar and prior history.
    ///
    /// Current hits and resolved payments precede sampling; resolved vanilla payoffs
    /// use analytic pricing. Prior at-hit rebates are already paid. Calls finish
    /// synchronously without cancellation polling. See [`Self::price_with_greeks`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for node curves,
    /// [`crate::Error::InvalidInput`] outside life,
    /// [`crate::Error::InvalidInput`] for non-trading fixings,
    /// [`crate::Error::InvalidInput`] for missing history or Sobol dimensions
    /// above 21,201, [`crate::Error::InvalidInput`] for impossible history,
    /// or [`crate::Error::ComputationFailure`] for unrepresentable paths or results.
    /// Operational failures retain their native [`std::io::Error`] through
    /// [`crate::Error::Io`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(self
            .native(
                Contract::Single(option),
                context,
                GreeksRequest::default(),
                Default::default(),
            )?
            .price())
    }

    /// Returns requested delta/gamma using the price's normal samples.
    ///
    /// Other Greeks remain absent. Expiry and spot stencils crossing a current
    /// monitored barrier suppress delta/gamma. Shifts only affect that availability
    /// and numerical derivatives of the analytic continuous at-hit rebate.
    /// Near-expiry likelihood estimates can have substantial sampling error.
    ///
    /// # Errors
    ///
    /// Checks flat rates before requests and shifts, then propagates [`Self::price`]
    /// errors. Empty requests or invalid shifts return
    /// [`crate::Error::InvalidInput`]; non-finite requested Greek samples
    /// return [`crate::Error::ComputationFailure`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers, as in [`Self::price`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?, 80.0, BarrierType::DownAndOut))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
    ///     path_count: 32768, seed: Some(42), sampling: MonteCarloSampling::Sobol,
    ///     ..Default::default()
    /// })?;
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), Default::default())?;
    /// assert!(result.price() > 0.0);
    /// assert!(result.has(Greek::Delta));
    /// assert_eq!(engine.settings().path_count, 32768);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.validate_applicability(context)?;
        request.validate()?;
        settings.validate()?;
        self.native(Contract::Single(option), context, request, settings)
    }

    pub(super) fn native(
        &self,
        contract: Contract<'_>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let option = match contract {
            Contract::Single(option) => option,
            Contract::Double(option) => &option.lower,
        };
        let upper = match contract {
            Contract::Single(_) => None,
            Contract::Double(option) => Some(option.upper_barrier()),
        };
        self.validate_applicability(context)?;
        validate_life(option, context)?;
        validate_schedule(option, context)?;
        let time = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        let radius =
            crate::risk::numerical::symmetric_shift(context.spot_price(), shifts.spot_shift);
        let discontinuity = match contract {
            Contract::Single(option) => option.at_spot_discontinuity(context, radius),
            Contract::Double(option) => option.at_spot_discontinuity(context, radius),
        };
        let request = GreeksRequest::new(
            [Greek::Delta, Greek::Gamma]
                .into_iter()
                .filter(|&g| request.has(g) && time != expiry && !discontinuity),
        );
        let (resolved, volatility_affects_price) = match contract {
            Contract::Single(option) => (
                resolved_price(option, context)?,
                crate::engines::analytic::barrier::volatility_affects_price(option, context),
            ),
            Contract::Double(option) => (
                crate::engines::analytic::double_barrier::resolved_price(option, context)?,
                AnalyticDoubleBarrierEngine.volatility_affects_price(option, context),
            ),
        };
        if let Some(value) = resolved {
            if volatility_affects_price && (request.has(Greek::Delta) || request.has(Greek::Gamma))
            {
                let vanilla = EuropeanOption::new(
                    option.option_type(),
                    option.strike(),
                    option.effective_date(),
                    option.expiry_date(),
                )?;
                return AnalyticEuropeanEngine
                    .price_with_greeks(&vanilla, context, request, shifts);
            }
            let mut result = PricingResult::new(value)?;
            result.set(request, Greek::Delta, 0.0)?;
            result.set(request, Greek::Gamma, 0.0)?;
            return Ok(result);
        }
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        let sigma = model.volatility();
        let duration = time.years_until(expiry);
        let drift = rate - dividend - 0.5 * sigma * sigma;
        let continuous = option.observation_mode() == ObservationMode::Continuous;
        let mut dates = if continuous {
            Vec::new()
        } else {
            option
                .observation_dates()
                .iter()
                .copied()
                .filter(|date| date.start_of_day() > time)
                .collect::<Vec<_>>()
        };
        if dates.last() != Some(&option.expiry_date()) {
            dates.push(option.expiry_date());
        }
        let mut previous = time;
        let mut steps = Vec::with_capacity(dates.len());
        for date in dates {
            let next = date.start_of_day();
            let dt = previous.years_until(next);
            let width = sigma * dt.sqrt();
            let increment = drift * dt;
            if width <= 0.0 || !width.is_finite() || !increment.is_finite() {
                return Err(unstable());
            }
            steps.push((
                increment,
                width,
                -rate * time.years_until(next),
                !continuous && option.observation_dates().binary_search(&date).is_ok(),
            ));
            previous = next;
        }
        let spot = context.spot_price();
        let barrier = log_ratio(option.barrier_level(), spot);
        let up = option.barrier_type().is_up();
        let knock_in = option.barrier_type().is_knock_in();
        let at_hit = option.rebate_timing() == RebateTiming::AtHit;
        let log_discount = -rate * duration;
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let delta = request.has(Greek::Delta);
        let gamma = request.has(Greek::Gamma);
        let means = path_means(self.settings, steps.len(), |normals| {
            let mut terminal = 0.0;
            let mut touched = false;
            let mut knocked_out = 0.0;
            for (&(increment, width, discount, observe), &z) in steps.iter().zip(normals) {
                terminal += increment + width * z;
                let asset = scaled_value(spot, 1.0, terminal)?;
                if asset <= 0.0 {
                    return Err(unstable());
                }
                if observe
                    && (if let Some(upper) = upper {
                        asset <= option.barrier_level() || asset >= upper
                    } else if up {
                        asset >= option.barrier_level()
                    } else {
                        asset <= option.barrier_level()
                    })
                {
                    touched = true;
                }
                if touched && !knock_in {
                    knocked_out = scaled_value(
                        option.rebate(),
                        1.0,
                        if at_hit { discount } else { log_discount },
                    )?;
                    break;
                }
            }
            let asset = scaled_value(spot, 1.0, terminal)?;
            let intrinsic = (sign * (asset - option.strike())).max(0.0);
            let weighted = |amount: f64, log_weight: f64| {
                scaled_value(amount.abs(), amount.signum(), log_discount + log_weight)
            };
            let (value, first, second) = if let (true, Some(upper)) = (continuous, upper) {
                let [survival, first, second] = super::double_barrier::survival(
                    -barrier,
                    terminal - barrier,
                    log_ratio(upper, option.barrier_level()),
                    steps[0].1,
                    delta || gamma,
                )?;
                let rebate = if !knock_in && at_hit {
                    0.0
                } else {
                    option.rebate()
                };
                let value = weighted(
                    intrinsic,
                    if knock_in {
                        (-survival).ln_1p()
                    } else {
                        survival.ln()
                    },
                )? + weighted(
                    rebate,
                    if knock_in {
                        survival.ln()
                    } else {
                        (-survival).ln_1p()
                    },
                )?;
                let difference = (if knock_in { -1.0 } else { 1.0 }) * (intrinsic - rebate);
                let derivative = |weight: f64| {
                    scaled_value(
                        difference.abs(),
                        difference.signum() * weight.signum(),
                        log_discount + weight.abs().ln(),
                    )
                };
                (
                    value,
                    if delta || gamma {
                        derivative(first)?
                    } else {
                        0.0
                    },
                    if gamma { derivative(second)? } else { 0.0 },
                )
            } else if continuous {
                let live = if up {
                    asset < option.barrier_level()
                } else {
                    asset > option.barrier_level()
                };
                let slope = if live {
                    2.0 * ((barrier - terminal).abs() / steps[0].1) / steps[0].1
                } else {
                    0.0
                };
                let exponent = -barrier.abs() * slope;
                let survival = if live {
                    (-exponent.exp_m1()).ln()
                } else {
                    f64::NEG_INFINITY
                };
                let rebate = if !knock_in && at_hit {
                    0.0
                } else {
                    option.rebate()
                };
                let value = weighted(intrinsic, if knock_in { exponent } else { survival })?
                    + weighted(rebate, if knock_in { survival } else { exponent })?;
                let mut first = 0.0;
                let mut second = 0.0;
                if (delta || gamma) && slope > 0.0 && exponent > f64::NEG_INFINITY {
                    let orientation = if knock_in { -1.0 } else { 1.0 };
                    let difference = orientation * (intrinsic - rebate);
                    first = weighted(
                        if up { -difference } else { difference },
                        exponent + slope.ln(),
                    )?;
                    if gamma {
                        second = weighted(-difference, exponent + 2.0 * slope.ln())?;
                    }
                }
                (value, first, second)
            } else {
                (
                    if touched && !knock_in {
                        knocked_out
                    } else {
                        weighted(
                            if touched == knock_in {
                                intrinsic
                            } else {
                                option.rebate()
                            },
                            0.0,
                        )?
                    },
                    0.0,
                    0.0,
                )
            };
            let width = steps[0].1;
            let z = normals[0];
            let score = z / width;
            let d = if delta {
                (value * score + first) / spot
            } else {
                0.0
            };
            let g = if gamma {
                let second_score = ((z * z - 1.0) / width - z) / width;
                ((value * second_score + first * (2.0 * score - 1.0) + second) / spot) / spot
            } else {
                0.0
            };
            if [value, d, g].iter().any(|x| !x.is_finite()) {
                return Err(unstable());
            }
            Ok([value, d, g])
        })?;
        let mut values = means;
        if continuous && !knock_in && at_hit && option.rebate() != 0.0 {
            let hit = |context: &PricingContext| {
                if let Contract::Double(option) = contract {
                    return crate::engines::analytic::double_barrier::hit_value(option, context);
                }
                hit_value(
                    option.rebate(),
                    log_ratio(option.barrier_level(), context.spot_price()).abs(),
                    up,
                    drift,
                    sigma,
                    duration,
                    rate,
                )
            };
            values[0] += hit(context)?;
            if request.has(Greek::Delta) || request.has(Greek::Gamma) {
                let evaluate = |context: &PricingContext, _| PricingResult::new(hit(context)?);
                let derivatives = match contract {
                    Contract::Single(option) => crate::risk::numerical::complete_greeks(
                        option, context, request, shifts, false, evaluate,
                    )?,
                    Contract::Double(option) => crate::risk::numerical::complete_greeks(
                        option, context, request, shifts, false, evaluate,
                    )?,
                };
                if let Some(value) = derivatives.get(Greek::Delta) {
                    values[1] += value;
                }
                if let Some(value) = derivatives.get(Greek::Gamma) {
                    values[2] += value;
                }
            }
        }
        let mut result = PricingResult::new(values[0])?;
        result.set(request, Greek::Delta, values[1])?;
        result.set(request, Greek::Gamma, values[2])?;
        Ok(result)
    }
}

impl PricingEngine<BarrierOption> for MonteCarloBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<BarrierOption> + Send + Sync + '_>>> {
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
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request, settings)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        crate::engines::analytic::barrier::volatility_affects_price(option, context)
    }
}
