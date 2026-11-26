use super::barrier::{evaluate_after_prefix, uses_local_diffusion};
use super::{
    Curve, Diffusion, FiniteDifferenceBinaryBarrierEngine, FiniteDifferenceDoubleTouchEngine,
    FiniteDifferenceSettings, Greek, GreeksRequest, NumericalShiftSettings, Payoff, PricingContext,
    PricingEngine, PricingResult, Result, SpatialGrid, TimeGrid, diffusion, diffusion_upper,
    local_volatility, payoff::boundary_value, scaled, step, unstable, validate_work,
};
use crate::Timestamp;
use crate::engines::deterministic::{self, continuation_context};
use crate::instruments::binary_barrier::{Contract, Settlement};
use crate::instruments::monitoring::validate_life;
use crate::numerics::checked;
use crate::{
    AnalyticDigitalEngine, BinaryBarrierOption, DigitalOption, DoubleTouchOption, ObservationMode,
    SettlementTiming, TouchOption,
};

macro_rules! binary_pricing {
    ($engine:ty, $option:ty) => {
        impl PricingEngine<$option> for $engine {
            fn greek_method(
                &self,
                option: &$option,
                context: &PricingContext,
            ) -> Result<crate::GreekMethod> {
                local_volatility::greek_method(option, context)
            }

            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                local_volatility::validate_model(context)
            }
            fn volatility_affects_price(&self, option: &$option, context: &PricingContext) -> bool {
                let contract = option.contract();
                contract.volatility_affects_price(context)
                    && settlement_through_prefix(&contract, context).is_ok_and(|(settlement, _)| {
                        match settlement {
                            Settlement::Pending => true,
                            Settlement::AtExpiry => contract.digital.is_some(),
                            Settlement::Value(_) => false,
                        }
                    })
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
            fn price_with_greeks_with_settings(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                shifts: crate::NumericalShiftSettings,
            ) -> Result<PricingResult> {
                <Self as PricingEngine<$option>>::validate_applicability(self, context)?;
                let contract = option.contract();
                super::barrier_analysis(
                    option,
                    context,
                    self.settings,
                    request,
                    shifts,
                    || default_upper(&contract, context),
                    |context, settings, request| {
                        native(&contract, context, settings, request, shifts)
                    },
                )
            }
        }
    };
}
binary_pricing!(FiniteDifferenceBinaryBarrierEngine, BinaryBarrierOption);
binary_pricing!(FiniteDifferenceBinaryBarrierEngine, TouchOption);
binary_pricing!(FiniteDifferenceDoubleTouchEngine, DoubleTouchOption);

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
    let option = contract.monitoring;
    validate_life(option, context)?;
    if crate::engines::deterministic::enabled_until(context, option.expiry_time())? {
        return PricingResult::new(crate::engines::deterministic::binary(contract, context)?);
    }
    let spot = context.spot_price();
    let terminal = |asset| {
        if let Some(digital) = contract.digital {
            Payoff::digital(digital).value(asset)
        } else {
            contract.payout.unwrap_or(asset)
        }
    };
    let expiry = option.expiry_time();
    let (settlement, end) = settlement_through_prefix(contract, context)?;
    match settlement {
        Settlement::Value(value) => return PricingResult::new(value),
        Settlement::AtExpiry => {
            if context.valuation_time() == expiry {
                return PricingResult::new(terminal(spot));
            }
            let value = if let Some(digital) = contract.digital {
                if uses_local_diffusion(context) {
                    let h =
                        crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
                    let request = if contract.spot_discontinuity(context, h) {
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
                AnalyticDigitalEngine
                    .price(digital, diffusion::analytic_context(context)?.as_ref())?
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
        Settlement::Pending => {}
    }
    evaluate_after_prefix(
        context,
        end,
        request,
        shifts,
        |radius| contract.spot_discontinuity(context, radius),
        |context, request, shifts| {
            let grid = diffusion::with_diffusion!(context, |diffusion| solve_grid(
                contract,
                context,
                settings,
                diffusion,
                context.spot_price()
            ))?;
            grid.evaluate(contract, context, request, shifts)
        },
    )
}

fn settlement_through_prefix(
    contract: &Contract<'_>,
    context: &PricingContext,
) -> Result<(Settlement, Timestamp)> {
    let prefix =
        deterministic::barrier_prefix(contract.monitoring, contract.upper_barrier, context)?;
    if let Some(hit) = prefix
        .hit
        .filter(|_| contract.timing == SettlementTiming::AtHit)
    {
        return Ok((
            Settlement::Value(hit.discounted_payout(contract.payout)),
            prefix.end,
        ));
    }
    let settlement = contract.settlement_with_state(
        (prefix.prior, prefix.touched, prefix.finished),
        context.spot_price(),
    );
    Ok((settlement, prefix.end))
}

fn solve_grid<D: Diffusion>(
    contract: &Contract<'_>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    diffusion: D,
    minimum_spot: f64,
) -> Result<BinaryGrid> {
    let option = contract.monitoring;
    let expiry = option.expiry_time();
    let upper_barrier = contract.upper_barrier;
    let knock_in = option.barrier_type().is_knock_in();
    let at_hit = contract.timing == SettlementTiming::AtHit;
    let spot = context.spot_price();
    let terminal = |asset| {
        contract.digital.map_or_else(
            || contract.payout.unwrap_or(asset),
            |digital| Payoff::digital(digital).value(asset),
        )
    };
    let scheduled = option.observation_mode() == ObservationMode::Scheduled;
    let up = option.barrier_type().is_up();
    let digital_knock_in = knock_in && contract.digital.is_some();
    let local_in = D::LOCAL && digital_knock_in;
    let fixed_upper = if scheduled {
        None
    } else {
        upper_barrier.or(up.then_some(option.barrier_level()))
    };
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
    let (risk, dividend) = diffusion::rate_curves(context, horizon, settings)?;
    let width = super::diffusion_log_width(context, horizon, &risk, &dividend)?;
    let strike = contract
        .digital
        .map_or(option.barrier_level(), DigitalOption::strike);
    let upper = fixed_upper
        .filter(|_| !local_in)
        .or(settings.asset_upper_boundary)
        .unwrap_or_else(|| {
            let upper = default_upper(contract, context);
            if D::LOCAL {
                upper.max(diffusion_upper(spot.max(strike), width))
            } else {
                upper
            }
        });
    if (fixed_upper.is_none() || local_in)
        && upper
            <= spot
                .max(strike)
                .max(upper_barrier.unwrap_or(option.barrier_level()))
    {
        return Err(super::invalid_grid(
            "finite-difference upper bound must exceed spot and contract levels",
        ));
    }
    let full_lower = super::grid::spot_floor(
        minimum_spot,
        strike.min(option.barrier_level()),
        (1.5 * width).max(std::f64::consts::LN_2),
    );
    let lower = if !scheduled && (!up || upper_barrier.is_some()) {
        option.barrier_level()
    } else {
        full_lower
    };
    let grid_upper = fixed_upper.unwrap_or(upper);
    let mut space = SpatialGrid::new(
        settings.asset_step_count,
        [lower, grid_upper],
        option.barrier_level(),
    )?;
    if !scheduled && settings.scheme != super::FiniteDifferenceScheme::ExplicitEuler {
        space.resolve_barriers(
            [Some(option.barrier_level()), upper_barrier],
            contract.digital.map(DigitalOption::strike),
            time,
            [&risk, &dividend],
            |level| diffusion.sigma(0.0, level),
        )?;
    }
    let observations =
        diffusion::observation_times(context.valuation_time(), expiry, option.observation_times())?;
    let grid = diffusion::time_grid(context, horizon, settings, &risk, &dividend, &observations)?;
    let implicit_fallback = !scheduled
        && upper_barrier.is_some()
        && settings.scheme == super::FiniteDifferenceScheme::Rannacher;
    if !local_in {
        let mut steps =
            grid.rollback_steps(settings.scheme, &observations)? + u64::from(implicit_fallback);
        if digital_knock_in {
            steps += grid.rollback_steps(settings.scheme, &[])?;
        }
        validate_work(space.count as u64, steps)?;
    }
    let theta = settings.scheme.theta();
    if !D::LOCAL {
        grid.validate_stability(&space, diffusion.sigma(0.0, 0.0), theta, &risk, &dividend)?;
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
            boundary_value(
                Payoff::digital(digital),
                asset,
                [
                    risk.log_factor(current, time),
                    dividend.log_factor(current, time),
                ],
                diffusion,
                tau,
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
        let next = observations.get(observations.partition_point(|&event| event < current));
        let value = |asset| {
            if !events {
                return base(asset, current);
            }
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
        super::barrier::project_event(
            &space,
            &breaks,
            &levels,
            values,
            upper_barrier.is_none(),
            |asset| knocked(asset).then(|| hit(asset, current, current)),
            |values, asset| space.linear(values, asset),
        );
    };
    let march = |events: bool| -> Result<Vec<f64>> {
        let values: Vec<_> = (0..=space.count)
            .map(|i| {
                super::grid::project_payoff(&space, &breaks, i, |asset| {
                    if !events {
                        terminal(asset)
                    } else if (!scheduled || observations.last() == Some(&time)) && knocked(asset) {
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
            implicit_fallback,
            |current| edges(current, events),
            observe,
        )
    };
    let mut values = if local_in {
        local_digital_knock_in(
            contract,
            settings,
            &space,
            &grid,
            diffusion,
            [&risk, &dividend],
            &observations,
            [full_lower, upper],
        )?
    } else {
        march(true)?
    };
    if digital_knock_in && !D::LOCAL {
        for (value, vanilla) in values.iter_mut().zip(march(false)?) {
            *value = vanilla - *value;
        }
    }
    Ok(BinaryGrid {
        grid: super::solution::Grid::new(space, values),
        discount: [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)],
    })
}

#[derive(PartialEq)]
struct BinaryGrid {
    grid: super::solution::Grid,
    discount: [f64; 2],
}
impl BinaryGrid {
    fn evaluate(
        &self,
        contract: &Contract<'_>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let spot = context.spot_price();
        let sample = self.grid.sample(spot);
        let scale = scaled(
            contract.payout.unwrap_or(spot),
            if contract.payout.is_some() {
                self.discount[0]
            } else {
                self.discount[1]
            },
        );
        let mut result =
            PricingResult::new(crate::numerics::checked_with_scale(sample[0], scale)?)?;
        let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
        if !contract.spot_discontinuity(context, h) {
            result.set(request, Greek::Delta, sample[1])?;
            result.set(request, Greek::Gamma, sample[2])?;
            if request.has(Greek::Speed) {
                if let Some(speed) = self.grid.speed(spot, h) {
                    result.set(request, Greek::Speed, speed)?;
                }
            }
        }
        Ok(result)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared grids, diffusion and event inputs"
)]
fn local_digital_knock_in<D: Diffusion>(
    contract: &Contract<'_>,
    settings: FiniteDifferenceSettings,
    space: &SpatialGrid,
    grid: &TimeGrid,
    diffusion: D,
    [risk, dividend]: [&Curve; 2],
    observations: &[f64],
    bounds: [f64; 2],
) -> Result<Vec<f64>> {
    let digital = contract.digital.ok_or_else(unstable)?;
    let payoff = Payoff::digital(digital);
    let time = *grid.times.last().ok_or_else(unstable)?;
    let scheduled = contract.monitoring.observation_mode() == ObservationMode::Scheduled;
    let level = contract.monitoring.barrier_level();
    let full = SpatialGrid::knock_in_continuation(
        settings,
        bounds,
        digital.strike(),
        diffusion.sigma(0.0, digital.strike()) * time.sqrt(),
    )?;
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
            super::grid::project_payoff(&full, &[digital.strike()], i, |asset| payoff.value(asset))
        })
        .collect();
    let values = (0..=space.count)
        .map(|i| {
            super::grid::project_payoff(space, &breaks, i, |asset| {
                if (!scheduled || observations.last() == Some(&time)) && knocked(asset) {
                    payoff.value(asset)
                } else {
                    0.0
                }
            })
        })
        .collect();
    let full_edges = |current| {
        bounds.map(|asset| {
            boundary_value(
                payoff,
                asset,
                [
                    risk.log_factor(current, time),
                    dividend.log_factor(current, time),
                ],
                diffusion,
                time - current,
            )
        })
    };
    let edges = |current, continuation: &[f64]| {
        let remaining = !scheduled || observations.last().is_some_and(|&event| event >= current);
        let value = |asset| {
            if knocked(asset) && remaining {
                full.linear(continuation, asset)
            } else {
                0.0
            }
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let observe = |values: &mut [f64], continuation: &[f64], _| {
        super::barrier::project_event(
            space,
            &breaks,
            &[level],
            values,
            true,
            |asset| knocked(asset).then(|| full.linear(continuation, asset)),
            |values, asset| space.linear(values, asset),
        );
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
        false,
        full_edges,
        edges,
        observe,
    )
}

trait BinaryInstrument: crate::OptionContract {
    fn saved_contract(&self) -> Contract<'_>;
}
macro_rules! saved_binary {
    ($option:ty) => {
        impl BinaryInstrument for $option {
            fn saved_contract(&self) -> Contract<'_> {
                self.contract()
            }
        }
    };
}
saved_binary!(BinaryBarrierOption);
saved_binary!(TouchOption);
saved_binary!(DoubleTouchOption);

#[derive(PartialEq)]
struct BinaryState {
    grid: Option<BinaryGrid>,
    digital: Option<super::VanillaState>,
}
impl<I: BinaryInstrument> super::solution::Backend<I> for BinaryState {
    fn query_work(&self, option: &I, context: &PricingContext) -> usize {
        super::solution::monitoring_query_work(option.saved_contract().monitoring, context)
    }

    fn evaluate(
        &self,
        instrument: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let contract = instrument.saved_contract();
        if crate::engines::deterministic::enabled_until(context, contract.monitoring.expiry_time())?
        {
            return PricingResult::new(crate::engines::deterministic::binary(&contract, context)?);
        }
        let (settlement, end) = settlement_through_prefix(&contract, context)?;
        match settlement {
            Settlement::Value(value) => PricingResult::new(value),
            Settlement::AtExpiry => {
                if context.valuation_time() == contract.monitoring.expiry_time() {
                    return PricingResult::new(contract.digital.map_or_else(
                        || contract.payout.unwrap_or(context.spot_price()),
                        |d| Payoff::digital(d).value(context.spot_price()),
                    ));
                }
                if let Some(digital) = contract.digital {
                    if let Some(saved) = &self.digital {
                        let h = crate::risk::numerical::symmetric_shift(
                            context.spot_price(),
                            shifts.spot_shift_at(context.spot_price()),
                        );
                        let request = if contract.spot_discontinuity(context, h) {
                            GreeksRequest::new([])
                        } else {
                            request
                        };
                        return super::solution::Backend::evaluate(
                            saved, digital, context, request, shifts,
                        );
                    }
                    return PricingResult::new(
                        AnalyticDigitalEngine
                            .price(digital, diffusion::analytic_context(context)?.as_ref())?,
                    );
                }
                let curve = if contract.payout.is_some() {
                    context.model_parameters().risk_free_curve()
                } else {
                    context.model_parameters().dividend_curve()
                };
                PricingResult::new(checked(scaled(
                    contract.payout.unwrap_or(context.spot_price()),
                    curve
                        .log_factor(context.valuation_time(), contract.monitoring.expiry_time())?,
                ))?)
            }
            Settlement::Pending => evaluate_after_prefix(
                context,
                end,
                request,
                shifts,
                |radius| contract.spot_discontinuity(context, radius),
                |context, request, shifts| {
                    self.grid
                        .as_ref()
                        .ok_or_else(unstable)?
                        .evaluate(&contract, context, request, shifts)
                },
            ),
        }
    }
}

fn prepare_solution<I: BinaryInstrument + Clone + PartialEq + Send + Sync + 'static>(
    option: &I,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    range: std::ops::RangeInclusive<f64>,
) -> Result<super::FiniteDifferenceSolution> {
    local_volatility::validate_model(context)?;
    super::solution::validate_range(&range)?;
    crate::cancellation::check()?;
    let contract = option.saved_contract();
    let anchor = super::solution::at_spot(context, *range.end())?;
    validate_life(contract.monitoring, &anchor)?;
    let mut pending = None;
    let mut digital = false;
    let local = uses_local_diffusion(context);
    let mut levels = deterministic::barrier_probe_levels(
        contract.monitoring,
        contract.upper_barrier,
        context,
        &range,
    )?;
    levels.extend(contract.digital.map(DigitalOption::strike));
    for spot in super::solution::probes(&range, levels) {
        let probe = super::solution::at_spot(context, spot)?;
        let (settlement, end) = settlement_through_prefix(&contract, &probe)?;
        match settlement {
            Settlement::Pending => pending = Some(end),
            Settlement::AtExpiry => {
                digital |= local
                    && contract.digital.is_some()
                    && context.valuation_time() < contract.monitoring.expiry_time();
            }
            Settlement::Value(_) => {}
        }
    }
    let digital = if digital {
        let digital = contract.digital.ok_or_else(unstable)?;
        Some(super::prepare_vanilla_state(
            digital,
            Payoff::digital(digital),
            &anchor,
            settings,
            false,
            *range.start(),
        )?)
    } else {
        None
    };
    let grid = if let Some(end) = pending {
        if crate::engines::deterministic::enabled_until(context, contract.monitoring.expiry_time())?
        {
            None
        } else {
            let (rolled, carry, _) = continuation_context(&anchor, end)?;
            let rolled = rolled.as_ref();
            Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
                &contract,
                rolled,
                settings,
                diffusion,
                scaled(*range.start(), carry)
            ))?)
        }
    } else {
        None
    };
    let state = BinaryState { grid, digital };
    super::solution::Backend::evaluate(
        &state,
        option,
        &anchor,
        GreeksRequest::new([]),
        Default::default(),
    )?;
    crate::cancellation::check()?;
    Ok(super::FiniteDifferenceSolution::new(
        option.clone(),
        context,
        range,
        state,
    ))
}
macro_rules! binary_solution {
    ($engine:ty, $option:ty) => {
        impl super::FiniteDifferenceEngine<$option> for $engine {
            fn solve(
                &self,
                option: &$option,
                context: &PricingContext,
                range: std::ops::RangeInclusive<f64>,
            ) -> Result<super::FiniteDifferenceSolution> {
                prepare_solution(option, context, self.settings, range)
            }
        }
    };
}
binary_solution!(FiniteDifferenceBinaryBarrierEngine, BinaryBarrierOption);
binary_solution!(FiniteDifferenceBinaryBarrierEngine, TouchOption);
binary_solution!(FiniteDifferenceDoubleTouchEngine, DoubleTouchOption);
