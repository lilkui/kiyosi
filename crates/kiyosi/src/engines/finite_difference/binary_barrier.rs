use super::{
    Curve, Diffusion, FiniteDifferenceBinaryBarrierEngine, FiniteDifferenceDoubleTouchEngine,
    FiniteDifferenceSettings, Greek, GreeksRequest, LocalDiffusion, NumericalShiftSettings,
    OptionContract, OptionType, Payoff, PricingContext, PricingEngine, PricingResult, Result,
    SpatialGrid, TimeGrid, black_scholes_arguments, diffusion, diffusion_upper, local_volatility,
    scaled, step, unstable, weighted_cdf,
};
use crate::instruments::binary_barrier::Contract;
use crate::instruments::monitoring::validate_life;
use crate::numerics::checked;
use crate::{
    AnalyticDigitalEngine, BinaryBarrierOption, DigitalOption, DoubleTouchOption, ObservationMode,
    SettlementTiming, TouchOption,
};

impl FiniteDifferenceBinaryBarrierEngine {
    /// Prices a binary barrier or single touch with BSM or local volatility.
    ///
    /// Continuous barriers are exact grid edges; scheduled observations project event cells.
    /// Prior at-hit payments have no remaining value; resolved cash flows use exact discounts.
    /// Accepts flat or nonconstant rate curves. Local volatility requires surface coverage only
    /// through the remaining stochastic horizon, including the last at-hit observation.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve coverage,
    /// for nontrading observations, missing or impossible history, invalid grids,
    /// or unstable explicit steps. Returns [`Error::ComputationFailure`][crate::Error::ComputationFailure] for
    /// numerical failures. Inputs remain unchanged.
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing observations or barriers leave affected measures absent.
    /// Expiry Greeks follow terminal payoffs. Before expiry, a current barrier equality without a prior touch suppresses Greeks.
    /// Local volatility supports only delta and gamma with the local-volatility surface held fixed;
    /// other requested Greeks remain absent.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Inputs and engine settings remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = TouchOption::cash(TouchOptionTerms::new(start, Date::new(2026, 1, 1)?.start_of_day(),
    ///     120.0, BarrierType::UpAndIn), 10.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceBinaryBarrierEngine::default();
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.price() > 0.0);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price_with_greeks(self, option, context, request, shifts)
    }
}

macro_rules! binary_pricing {
    ($engine:ty, $option:ty) => {
        impl PricingEngine<$option> for $engine {
            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                local_volatility::validate_model(context)
            }
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                Ok(native(
                    &option.contract(),
                    context,
                    self.settings,
                    GreeksRequest::new([]),
                    Default::default(),
                )?
                .price())
            }
            fn price_with_greeks(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                shifts: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                request.validate()?;
                shifts.validate()?;
                local_volatility::validate_request(context, request)?;
                let mut settings = self.settings;
                if settings.asset_upper_boundary.is_none()
                    && local_volatility::local_model(context).is_none()
                {
                    let upper = default_upper(&option.contract(), context);
                    if upper.is_finite() {
                        settings.asset_upper_boundary = Some(upper);
                    }
                }
                super::complete_greeks(
                    option,
                    context,
                    request,
                    shifts,
                    true,
                    native(&option.contract(), context, settings, request, shifts)?,
                    |context, request| {
                        native(&option.contract(), context, settings, request, shifts)
                    },
                )
            }
            fn volatility_affects_price(&self, option: &$option, context: &PricingContext) -> bool {
                option.contract().volatility_affects_price(context)
            }
        }
    };
}
binary_pricing!(FiniteDifferenceBinaryBarrierEngine, BinaryBarrierOption);
binary_pricing!(FiniteDifferenceBinaryBarrierEngine, TouchOption);
binary_pricing!(FiniteDifferenceDoubleTouchEngine, DoubleTouchOption);

impl FiniteDifferenceDoubleTouchEngine {
    /// Prices cash double-touch contracts using both inclusive boundaries and shared history.
    ///
    /// Fits continuous boundaries exactly and projects scheduled events simultaneously.
    /// Prior at-hit payments have zero remaining value; resolved cash uses exact discounts.
    /// Accepts BSM or local volatility with flat or nonconstant rate curves. Surface coverage
    /// follows the remaining stochastic horizon, not a previously settled payment.
    /// See [`DoubleTouchOption::cash`] for construction and pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve coverage,
    /// for nontrading observations, unknown or impossible history, invalid grids,
    /// or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure]. Inputs remain unchanged.
    pub fn price(&self, option: &DoubleTouchOption, context: &PricingContext) -> Result<f64> {
        <Self as PricingEngine<DoubleTouchOption>>::price(self, option, context)
    }

    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing either boundary or an observation omit affected measures.
    /// Expiry Greeks follow terminal payoffs. Before expiry, current boundary equality without a prior touch suppresses Greeks.
    /// Local volatility supports only delta and gamma with the local-volatility surface held fixed;
    /// other requested Greeks remain absent.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let mut terms = DoubleTouchOptionTerms::new(start, Date::new(2026, 1, 1)?.start_of_day(),
    ///     80.0, 120.0, DoubleBarrierType::KnockOut);
    /// terms.touch_state = Some(BarrierTouchState::Untouched);
    /// let option = DoubleTouchOption::cash(terms, 1.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let result = FiniteDifferenceDoubleTouchEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!((result.price() - 0.36726156351907174).abs() < 0.00001);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DoubleTouchOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<DoubleTouchOption>>::price_with_greeks(
            self, option, context, request, shifts,
        )
    }
}

fn default_upper(contract: &Contract<'_>, context: &PricingContext) -> f64 {
    let relevant = context.spot_price().max(
        contract
            .digital
            .map_or(contract.monitoring.barrier_level(), DigitalOption::strike),
    );
    (4.0 * relevant).max(
        contract
            .upper_barrier
            .unwrap_or(contract.monitoring.barrier_level())
            + relevant,
    )
}

fn native(
    contract: &Contract<'_>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    let request = local_volatility::validate_request(context, request)?;
    match context.model_parameters() {
        crate::ModelParameters::Heston(_) => Err(crate::Error::UnsupportedOperation(
            "finite differences do not support Heston".into(),
        )),
        crate::ModelParameters::BlackScholesMerton(model) => native_with_diffusion(
            contract,
            context,
            settings,
            request,
            shifts,
            model.volatility(),
        ),
        crate::ModelParameters::LocalVolatility(model) => native_with_diffusion(
            contract,
            context,
            settings,
            request,
            shifts,
            LocalDiffusion::new(context, model.surface()),
        ),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Barrier positions intentionally round and saturate to nonnegative indices before capping at the grid size."
)]
fn native_with_diffusion<D: Diffusion>(
    contract: &Contract<'_>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    diffusion: D,
) -> Result<PricingResult> {
    let option = contract.monitoring;
    validate_life(option, context)?;
    let (prior, touched, finished) = contract.state(context)?;
    let upper_barrier = contract.upper_barrier;
    let knock_in = option.barrier_type().is_knock_in();
    let at_hit = contract.timing == SettlementTiming::AtHit;
    let spot = context.spot_price();
    let terminal = |asset| {
        if let Some(digital) = contract.digital {
            if (digital.option_type() == OptionType::Call && asset > digital.strike())
                || (digital.option_type() == OptionType::Put && asset < digital.strike())
            {
                contract.payout.unwrap_or(asset)
            } else {
                0.0
            }
        } else {
            contract.payout.unwrap_or(asset)
        }
    };
    if prior && at_hit {
        return PricingResult::new(0.0);
    }
    let expiry = option.expiry_time();
    if context.valuation_time() == expiry {
        return PricingResult::new(if touched == knock_in {
            terminal(spot)
        } else {
            0.0
        });
    }
    if touched || finished {
        if touched != knock_in {
            return PricingResult::new(0.0);
        }
        if at_hit {
            return PricingResult::new(contract.payout.unwrap_or(spot));
        }
        let value = if let Some(digital) = contract.digital {
            if D::LOCAL {
                let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift());
                let request = if option.spot_discontinuity(context, h, contract.upper_barrier) {
                    GreeksRequest::new([])
                } else {
                    request
                };
                return Ok(super::native(
                    digital,
                    Payoff::digital(digital),
                    context,
                    settings,
                    request,
                    shifts,
                    false,
                )?
                .0);
            }
            AnalyticDigitalEngine.price(digital, context)?
        } else {
            let model = context.model_parameters();
            let curve = if contract.payout.is_some() {
                model.risk_free_curve()
            } else {
                model.dividend_curve()
            };
            checked(scaled(
                contract.payout.unwrap_or(spot),
                curve.log_factor(context.valuation_time(), expiry)?,
            ))?
        };
        return PricingResult::new(value);
    }
    let scheduled = option.observation_mode() == ObservationMode::Scheduled;
    let horizon = if scheduled && at_hit {
        option
            .observation_times()
            .last()
            .copied()
            .unwrap_or(option.expiry_time())
    } else {
        expiry
    };
    let time = context.valuation_time().years_until(horizon);
    let model = context.model_parameters();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), horizon)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), horizon)?;
    let upper = if let Some(upper) = settings.asset_upper_boundary {
        upper
    } else {
        let upper = default_upper(contract, context);
        if D::LOCAL {
            let relevant = spot.max(
                contract
                    .digital
                    .map_or(option.barrier_level(), DigitalOption::strike),
            );
            upper.max(diffusion_upper(relevant, context, time, &risk, &dividend)?)
        } else {
            upper
        }
    };
    let mut space = SpatialGrid::new(
        settings,
        upper,
        0.0,
        spot.max(
            contract
                .digital
                .map_or(option.barrier_level(), DigitalOption::strike),
        )
        .max(upper_barrier.unwrap_or(option.barrier_level())),
    )?;
    let up = option.barrier_type().is_up();
    if !scheduled {
        space.uniform_domain(
            if up { 0.0 } else { option.barrier_level() },
            upper_barrier.unwrap_or(if up { option.barrier_level() } else { upper }),
        )?;
    }
    let observations: Vec<_> = option
        .observation_times()
        .iter()
        .filter(|&&date| date > context.valuation_time())
        .map(|&date| context.valuation_time().years_until(date))
        .collect();
    diffusion::validate_event_resolution(
        context.valuation_time(),
        expiry,
        option.observation_times(),
    )?;
    let grid = diffusion::time_grid(
        context,
        horizon,
        settings.time_step_count,
        &risk,
        &dividend,
        &observations,
    )?;
    let theta = settings.scheme.theta();
    if !D::LOCAL {
        grid.validate_stability(
            &space,
            diffusion.sigma(0.0, 0.0),
            theta,
            scheduled,
            &risk,
            &dividend,
        )?;
    }
    let knocked = |asset| {
        upper_barrier.is_some_and(|upper| asset >= upper)
            || if up {
                asset >= option.barrier_level()
            } else {
                asset <= option.barrier_level()
            }
    };
    let discount = |current, end| {
        if contract.payout.is_some() {
            risk.log_factor(current, end)
        } else {
            dividend.log_factor(current, end)
        }
    };
    let base = |asset, current| {
        let tau = time - current;
        if tau == 0.0 {
            return terminal(asset);
        }
        let log = discount(current, time);
        if let Some(digital) = contract.digital {
            let sign = if digital.option_type() == OptionType::Call {
                1.0
            } else {
                -1.0
            };
            if asset == 0.0 {
                return if sign < 0.0 {
                    scaled(contract.payout.unwrap_or(0.0), log)
                } else {
                    0.0
                };
            }
            if D::LOCAL {
                return if sign > 0.0 {
                    scaled(contract.payout.unwrap_or(asset), log)
                } else {
                    0.0
                };
            }
            let (d1, d2) = black_scholes_arguments(
                asset,
                digital.strike(),
                dividend.log_factor(current, time) - risk.log_factor(current, time),
                diffusion.sigma(0.0, 0.0),
                tau.sqrt(),
            );
            weighted_cdf(
                contract.payout.unwrap_or(asset),
                log,
                sign * if contract.payout.is_some() { d2 } else { d1 },
            )
        } else {
            scaled(contract.payout.unwrap_or(asset), log)
        }
    };
    let hit = |asset, current, event| {
        if contract.digital.is_some() || !knock_in {
            0.0
        } else {
            scaled(
                contract.payout.unwrap_or(asset),
                discount(current, if at_hit { event } else { time }),
            )
        }
    };
    let surviving = |asset| {
        if contract.digital.is_none() && knock_in {
            0.0
        } else {
            terminal(asset)
        }
    };
    let levels: Vec<_> = std::iter::once(option.barrier_level())
        .chain(upper_barrier)
        .collect();
    let breaks: Vec<_> = levels
        .iter()
        .copied()
        .chain(contract.digital.map(DigitalOption::strike))
        .collect();
    let edges = |current, events: bool| {
        let value = |asset| {
            if !events {
                return base(asset, current);
            }
            let next = observations.iter().find(|&&event| event >= current);
            if knocked(asset) && (!scheduled || next.is_some()) {
                hit(asset, current, next.copied().unwrap_or(current))
            } else if contract.digital.is_none() && knock_in {
                0.0
            } else {
                base(asset, current)
            }
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let observe = |values: &mut [f64], current| {
        let projected: Vec<_> = levels
            .iter()
            .map(|level| {
                let cut = ((level / space.spacing).round() as usize).min(space.count);
                // Project both boundaries from the same continuation, including shared cells.
                let value = super::barrier::project(&space, &breaks, cut, |asset| {
                    if upper_barrier.is_some() {
                        if knocked(asset) {
                            hit(asset, current, current)
                        } else {
                            space.linear(values, asset)
                        }
                    } else if knocked(asset) {
                        hit(asset, current, current) - space.linear(values, asset)
                    } else {
                        0.0
                    }
                });
                (
                    cut,
                    if upper_barrier.is_some() {
                        value
                    } else {
                        values[cut] + value
                    },
                )
            })
            .collect();
        for (i, value) in values.iter_mut().enumerate() {
            if knocked(space.asset(i)) {
                *value = hit(space.asset(i), current, current);
            }
        }
        for (cut, value) in projected {
            values[cut] = value;
        }
    };
    let march = |events: bool| -> Result<Vec<f64>> {
        let values: Vec<_> = (0..=space.count)
            .map(|i| {
                super::barrier::project(&space, &breaks, i, |asset| {
                    if !events {
                        terminal(asset)
                    } else if (!scheduled || observations.contains(&time)) && knocked(asset) {
                        hit(asset, time, time)
                    } else {
                        surviving(asset)
                    }
                })
            })
            .collect();
        step::march(
            values,
            &space,
            &grid,
            diffusion,
            settings.scheme,
            [&risk, &dividend],
            if events && scheduled {
                &observations
            } else {
                &[]
            },
            |current| edges(current, events),
            observe,
        )
    };
    let mut values = if D::LOCAL && knock_in && contract.digital.is_some() {
        local_digital_knock_in(
            contract,
            settings,
            &space,
            &grid,
            diffusion,
            [&risk, &dividend],
            &observations,
            upper,
        )?
    } else {
        march(true)?
    };
    if contract.digital.is_some() && knock_in && !D::LOCAL {
        for (value, vanilla) in values.iter_mut().zip(march(false)?) {
            *value = vanilla - *value;
        }
    }
    let curvature = space.curvature(&values);
    let sample = space.sample(&values, &curvature, spot);
    let scale = scaled(
        contract.payout.unwrap_or(spot),
        if contract.payout.is_some() {
            risk.log_factor(0.0, time)
        } else {
            dividend.log_factor(0.0, time)
        },
    );
    let mut result = PricingResult::new(crate::numerics::checked_with_scale(sample[0], scale)?)?;
    let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift());
    if !option.spot_discontinuity(context, h, contract.upper_barrier) {
        result.set(request, Greek::Delta, sample[1])?;
        result.set(request, Greek::Gamma, sample[2])?;
        if request.has(Greek::Speed) {
            if let Some(speed) = space.speed(&values, &curvature, spot, h) {
                result.set(request, Greek::Speed, speed)?;
            }
        }
    }
    Ok(result)
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared grids, diffusion and event inputs"
)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Barrier positions intentionally round and saturate to nonnegative indices before capping at the grid size."
)]
fn local_digital_knock_in<D: Diffusion>(
    contract: &Contract<'_>,
    settings: FiniteDifferenceSettings,
    space: &SpatialGrid,
    grid: &TimeGrid,
    diffusion: D,
    [risk, dividend]: [&Curve; 2],
    observations: &[f64],
    upper: f64,
) -> Result<Vec<f64>> {
    let digital = contract.digital.ok_or_else(unstable)?;
    let payoff = Payoff::digital(digital);
    let full = SpatialGrid::new(settings, upper, 0.0, digital.strike())?;
    let time = *grid.times.last().ok_or_else(unstable)?;
    let scheduled = contract.monitoring.observation_mode() == ObservationMode::Scheduled;
    let level = contract.monitoring.barrier_level();
    let knocked = |asset| {
        if contract.monitoring.barrier_type().is_up() {
            asset >= level
        } else {
            asset <= level
        }
    };
    let breaks = [level, digital.strike()];
    let continuation = (0..=full.count)
        .map(|i| {
            super::barrier::project(&full, &[digital.strike()], i, |asset| payoff.value(asset))
        })
        .collect();
    let values = (0..=space.count)
        .map(|i| {
            super::barrier::project(space, &breaks, i, |asset| {
                if (!scheduled || observations.contains(&time)) && knocked(asset) {
                    payoff.value(asset)
                } else {
                    0.0
                }
            })
        })
        .collect();
    let full_edges = |current| {
        let discount = if contract.payout.is_some() {
            risk.log_factor(current, time)
        } else {
            dividend.log_factor(current, time)
        };
        if digital.option_type() == OptionType::Call {
            [0.0, scaled(contract.payout.unwrap_or(upper), discount)]
        } else {
            [scaled(contract.payout.unwrap_or(0.0), discount), 0.0]
        }
    };
    let edges = |current, continuation: &[f64]| {
        let value = |asset| {
            if knocked(asset) && (!scheduled || observations.iter().any(|&t| t >= current)) {
                full.linear(continuation, asset)
            } else {
                0.0
            }
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let observe = |values: &mut [f64], continuation: &[f64], _| {
        let cut = ((level / space.spacing).round() as usize).min(space.count);
        let projected = values[cut]
            + super::barrier::project(space, &breaks, cut, |asset| {
                if knocked(asset) {
                    full.linear(continuation, asset) - space.linear(values, asset)
                } else {
                    0.0
                }
            });
        for (i, value) in values.iter_mut().enumerate() {
            if knocked(space.asset(i)) {
                *value = full.linear(continuation, space.asset(i));
            }
        }
        values[cut] = projected;
    };
    step::march_with_continuation(
        values,
        space,
        continuation,
        &full,
        grid,
        diffusion,
        settings.scheme,
        [risk, dividend],
        if scheduled { observations } else { &[] },
        full_edges,
        edges,
        observe,
    )
}
