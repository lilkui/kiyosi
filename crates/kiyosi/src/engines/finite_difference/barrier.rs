use super::autocallable::{MARKET_GREEKS, market_bumps, tangent_coefficients};
use super::{
    Diffusion, FiniteDifferenceBarrierEngine, FiniteDifferenceDoubleBarrierEngine,
    FiniteDifferenceSettings, Greek, GreeksRequest, NumericalShiftSettings, OptionContract,
    OptionType, Payoff, PricingContext, PricingEngine, PricingResult, Result, SpatialGrid,
    diffusion, diffusion_upper, grid::project_payoff, local_volatility, payoff::boundary_value,
    scaled, step, validate_work, validate_workspace,
};
use crate::engines::analytic::barrier::resolved_price;
use crate::engines::deterministic::{self, BarrierPrefix, continuation_context};
use crate::instruments::monitoring::validate_life;
use crate::numerics::scaled_signed_cash;
use crate::{BarrierOption, DoubleBarrierOption, ObservationMode, RebateTiming, Timestamp};

impl PricingEngine<BarrierOption> for FiniteDifferenceBarrierEngine {
    fn greek_method(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        local_volatility::greek_method(option, context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        volatility_affects_price(option, context)
    }
    /// Prices a single barrier with continuous or scheduled monitoring and flat or nonconstant rate curves.
    ///
    /// Resolves current observations against actual spot before building a grid.
    /// Prior at-hit rebates are already paid; resolved vanilla payoffs use the selected model.
    /// Local volatility supports price, delta and gamma, holding the surface fixed.
    /// Continuous barriers define exact domain edges, and time grids include exact scheduled dates.
    /// The configured upper bound is used only when pricing needs a full upper domain.
    /// Continuous single-barrier payoffs and local-volatility knock-in continuations
    /// use cell averaging at the strike.
    /// See [`Self::price`] for an example.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve/surface coverage, for
    /// impossible history, clipped or
    /// degenerate grids, or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure].
    /// Scheduled constant-volatility grids also fail when one-sided drift on the mesh
    /// would introduce excessive variance. Increase spatial resolution in that case.
    ///
    /// # Greeks
    ///
    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    ///
    /// Scheduled flat-rate BSM grids reuse the pricing factorization for eligible
    /// default first-order market sensitivities. Custom bumps reprice the contract
    /// using [`NumericalShiftSettings`] and the central BSM upper boundary.
    /// Local volatility accepts only delta/gamma, holding the absolute surface fixed.
    /// Stencils crossing observations or barriers leave affected Greeks absent.
    /// Expiry Greeks follow terminal payoffs. Before expiry at positive BSM volatility,
    /// barrier equality without a prior touch leaves spot derivatives absent; rate
    /// and volatility Greeks follow the resolved payoff.
    ///
    /// ## Errors
    ///
    /// Validates shifts before pricing. Propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors; feasible bumped failures fail the operation.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceBarrierEngine::default();
    /// assert_eq!(engine.settings().asset_step_count, 800);
    /// let result = engine.price_with_greeks_with_settings(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]),
    ///     NumericalShiftSettings { spot_shift: Some(0.001), ..Default::default() })?;
    /// assert_eq!(result.price(), engine.price(&option, &context)?);
    /// assert!(result.get(Greek::Delta).is_some());
    /// # Ok(()) }
    /// ```
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(native_barrier(
            option,
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
        )?
        .price())
    }
    fn price_with_greeks_with_settings(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<BarrierOption>>::validate_applicability(self, context)?;
        analysis(option, context, self.settings, request, shifts)
    }
}

trait Contract: OptionContract {
    fn monitoring(&self) -> &BarrierOption;
    fn upper_barrier(&self) -> Option<f64> {
        None
    }
    fn resolved_price(&self, context: &PricingContext) -> Result<Option<f64>>;
    fn payoff_requires_touch(&self) -> bool;
}
impl Contract for BarrierOption {
    fn monitoring(&self) -> &BarrierOption {
        self
    }
    fn resolved_price(&self, context: &PricingContext) -> Result<Option<f64>> {
        resolved_price(self, diffusion::analytic_context(context)?.as_ref())
    }
    fn payoff_requires_touch(&self) -> bool {
        self.payoff_requires_touch()
    }
}
impl Contract for DoubleBarrierOption {
    fn monitoring(&self) -> &BarrierOption {
        &self.lower
    }
    fn upper_barrier(&self) -> Option<f64> {
        Some(self.upper_barrier())
    }
    fn resolved_price(&self, context: &PricingContext) -> Result<Option<f64>> {
        crate::engines::analytic::double_barrier::resolved_price(
            self,
            diffusion::analytic_context(context)?.as_ref(),
        )
    }
    fn payoff_requires_touch(&self) -> bool {
        self.payoff_requires_touch()
    }
}

fn analysis(
    option: &(impl Contract + Sync),
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    super::barrier_analysis(
        option,
        context,
        settings,
        request,
        shifts,
        || default_upper(option, context),
        |context, settings, request| native_barrier(option, context, settings, request, shifts),
    )
}

impl PricingEngine<DoubleBarrierOption> for FiniteDifferenceDoubleBarrierEngine {
    fn greek_method(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        local_volatility::greek_method(option, context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn volatility_affects_price(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> bool {
        volatility_affects_price(option, context)
    }
    /// Prices double barriers with continuous or scheduled monitoring and flat or nonconstant rate curves.
    ///
    /// Fits both continuous edges exactly, using all configured asset steps between them.
    /// Projects scheduled boundary events simultaneously on the full spatial grid.
    /// Resolves current touches and known history before grid validation.
    /// Local volatility supports price, delta and gamma, holding the surface fixed.
    /// See [`Self::price`] for an example.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Reuses [`FiniteDifferenceBarrierEngine::price`] categories and validation timing,
    /// including explicit upper-domain coverage checks when a full-domain grid is needed.
    /// Continuous barriers fix the barrier grid's edges independently of that upper bound.
    ///
    /// # Greeks
    ///
    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    ///
    /// Fixes the central BSM spatial domain for bumps. Local volatility accepts only
    /// delta and gamma with the local-volatility surface held fixed. Stencils crossing
    /// either boundary or an observation omit affected measures. Expiry Greeks
    /// follow terminal payoffs. Before expiry at positive BSM volatility, boundary
    /// equality without a prior touch leaves spot derivatives absent; rate and
    /// volatility Greeks follow the resolved payoff.
    ///
    /// ## Errors
    ///
    /// Validates shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, start, Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0, DoubleBarrierType::KnockOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceDoubleBarrierEngine::default();
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]))?;
    /// assert!((result.price() - 1.0838814592583166).abs() < 0.001);
    /// assert!(result.get(Greek::Delta).is_some());
    /// # Ok(()) }
    /// ```
    fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(native_barrier(
            option,
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
        )?
        .price())
    }
    fn price_with_greeks_with_settings(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<DoubleBarrierOption>>::validate_applicability(self, context)?;
        analysis(option, context, self.settings, request, shifts)
    }
}

fn default_upper(option: &impl Contract, context: &PricingContext) -> f64 {
    let terms = option.monitoring();
    let relevant = context.spot_price().max(terms.strike());
    (4.0 * relevant).max(option.upper_barrier().unwrap_or(terms.barrier_level()) + relevant)
}

fn volatility_affects_price(option: &impl Contract, context: &PricingContext) -> bool {
    let terms = option.monitoring();
    option.volatility_affects_price(context)
        && deterministic::barrier_prefix(&terms.monitoring, option.upper_barrier(), context)
            .is_ok_and(|prefix| {
                let knock_in = terms.barrier_type().is_knock_in();
                !(prefix.touched && !knock_in || !prefix.touched && prefix.finished && knock_in)
            })
}

fn native_barrier(
    option: &impl Contract,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    let request = local_volatility::validate_request(context, request)?;
    let terms = option.monitoring();
    validate_life(terms, context)?;
    if crate::engines::deterministic::enabled_until(context, terms.expiry_time())? {
        if let Some(value) = option.resolved_price(context)? {
            return PricingResult::new(value);
        }
        return PricingResult::new(crate::engines::deterministic::barrier_value(
            terms,
            option.upper_barrier(),
            context,
        )?);
    }
    let mut end = context.valuation_time();
    if uses_local_diffusion(context) {
        let prefix =
            deterministic::barrier_prefix(&terms.monitoring, option.upper_barrier(), context)?;
        if let Some(result) =
            local_resolved_with(option, context, &prefix, request, shifts, |request| {
                Ok(super::native(
                    terms.vanilla(),
                    Payoff::vanilla(terms.option_type(), terms.strike(), false),
                    context,
                    settings,
                    request,
                    shifts,
                    false,
                )?
                .0)
            })?
        {
            return Ok(result);
        }
        end = prefix.end;
    } else if let Some(value) = option.resolved_price(context)? {
        return PricingResult::new(value);
    }
    evaluate_after_prefix(
        context,
        end,
        request,
        shifts,
        |radius| option.at_spot_discontinuity(context, radius),
        |context, request, shifts| {
            let bumps = if terms.observation_mode() == ObservationMode::Scheduled
                && !uses_local_diffusion(context)
                && settings.scheme != super::FiniteDifferenceScheme::ExplicitEuler
            {
                market_bumps(context, request, shifts)
            } else {
                [None; 3]
            };
            let solve = |bumps| {
                diffusion::with_diffusion!(context, |diffusion| solve_grid(
                    option,
                    context,
                    settings,
                    diffusion,
                    context.spot_price(),
                    bumps
                ))
            };
            let grid = match solve(bumps) {
                Err(crate::Error::ComputationFailure(_)) if bumps.iter().any(Option::is_some) => {
                    solve([None; 3])?
                }
                result => result?,
            };
            grid.evaluate(option, context, request, shifts)
        },
    )
}

pub(super) fn uses_local_diffusion(context: &PricingContext) -> bool {
    local_volatility::local_model(context).is_some() || diffusion::uses_business_time(context)
}

pub(super) fn evaluate_after_prefix(
    context: &PricingContext,
    end: Timestamp,
    request: GreeksRequest,
    mut shifts: NumericalShiftSettings,
    discontinuity: impl FnOnce(f64) -> bool,
    evaluate: impl FnOnce(
        &PricingContext,
        GreeksRequest,
        NumericalShiftSettings,
    ) -> Result<PricingResult>,
) -> Result<PricingResult> {
    if end == context.valuation_time() {
        return evaluate(context, request, shifts);
    }
    let (rolled, carry, discount) = continuation_context(context, end)?;
    let spot = context.spot_price();
    let request = if request != GreeksRequest::new([])
        && discontinuity(crate::risk::numerical::symmetric_shift(
            spot,
            shifts.spot_shift_at(spot),
        )) {
        GreeksRequest::new([])
    } else {
        request
    };
    shifts.spot_shift = Some(scaled(shifts.spot_shift_at(spot), carry));
    let rolled = evaluate(&rolled, request, shifts)?;
    let mut result = PricingResult::new(scaled(rolled.price(), discount))?;
    for (greek, order) in [
        (Greek::Delta, 1.0),
        (Greek::Gamma, 2.0),
        (Greek::Speed, 3.0),
    ] {
        if let Some(value) = rolled.get(greek) {
            result.set(
                request,
                greek,
                scaled_signed_cash(value, discount + order * carry),
            )?;
        }
    }
    Ok(result)
}

fn local_resolved_with(
    option: &impl Contract,
    context: &PricingContext,
    prefix: &BarrierPrefix,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    vanilla: impl FnOnce(GreeksRequest) -> Result<PricingResult>,
) -> Result<Option<PricingResult>> {
    let terms = option.monitoring();
    let BarrierPrefix {
        prior,
        touched,
        finished,
        hit,
        ..
    } = prefix;
    let knock_in = terms.barrier_type().is_knock_in();
    let cash = || {
        if terms.rebate() == 0.0 {
            return Ok(0.0);
        }
        crate::numerics::checked(scaled(
            terms.rebate(),
            context
                .model_parameters()
                .risk_free_curve()
                .log_factor(context.valuation_time(), terms.expiry_time())?,
        ))
    };
    if *touched && !knock_in {
        return if terms.rebate_timing() == RebateTiming::AtHit {
            PricingResult::new(if *prior {
                0.0
            } else {
                scaled(
                    terms.rebate(),
                    hit.as_ref().map_or(0.0, |hit| hit.log_discount),
                )
            })
            .map(Some)
        } else {
            PricingResult::new(cash()?).map(Some)
        };
    }
    if !touched && *finished && knock_in {
        return PricingResult::new(cash()?).map(Some);
    }
    let fixed = terms.rebate() == 0.0 && option.payoff_requires_touch();
    if fixed && !knock_in {
        return PricingResult::new(0.0).map(Some);
    }
    if *touched || *finished || fixed {
        let h = crate::risk::numerical::symmetric_shift(
            context.spot_price(),
            shifts.spot_shift_at(context.spot_price()),
        );
        let request =
            if option.all_greeks_unavailable(context) || option.at_spot_discontinuity(context, h) {
                GreeksRequest::new([])
            } else {
                request
            };
        return vanilla(request).map(Some);
    }
    Ok(None)
}

fn solve_grid<D: Diffusion>(
    option: &impl Contract,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    volatility: D,
    minimum_spot: f64,
    mut bumps: [Option<f64>; 3],
) -> Result<BarrierGrid> {
    let terms = option.monitoring();
    let upper_barrier = option.upper_barrier();
    let time = context.valuation_time().years_until(option.expiry_time());
    let expiry = option.expiry_time();
    let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
    let width = super::diffusion_log_width(context, expiry, &risk, &dividend)?;
    let scheduled = terms.observation_mode() == ObservationMode::Scheduled;
    let up = terms.barrier_type().is_up();
    let knock_in = terms.barrier_type().is_knock_in();
    let local_in = D::LOCAL && knock_in;
    let fixed_upper = if scheduled {
        None
    } else {
        upper_barrier.or(up.then_some(terms.barrier_level()))
    };
    let upper = fixed_upper
        .filter(|_| !local_in)
        .or(settings.asset_upper_boundary)
        .unwrap_or_else(|| {
            let default = default_upper(option, context);
            if D::LOCAL {
                default.max(diffusion_upper(
                    context.spot_price().max(terms.strike()),
                    width,
                ))
            } else {
                default
            }
        });
    if (fixed_upper.is_none() || local_in)
        && upper
            <= context
                .spot_price()
                .max(terms.strike())
                .max(upper_barrier.unwrap_or(terms.barrier_level()))
    {
        return Err(super::invalid_grid(
            "finite-difference upper bound must exceed spot and contract levels",
        ));
    }
    let full_lower = super::grid::spot_floor(
        minimum_spot,
        terms.strike().min(terms.barrier_level()),
        (1.5 * width).max(std::f64::consts::LN_2),
    );
    let lower = if !scheduled && (!up || upper_barrier.is_some()) {
        terms.barrier_level()
    } else {
        full_lower
    };
    let grid_upper = fixed_upper.unwrap_or(upper);
    let mut space = SpatialGrid::new(
        settings.asset_step_count,
        [lower, grid_upper],
        terms.barrier_level(),
    )?;
    if scheduled && !D::LOCAL {
        let sigma = volatility.sigma(0.0, 0.0);
        let variance = sigma * sigma;
        let h = space.spacing;
        for current in std::iter::once(0.0)
            .chain(risk.ends())
            .chain(dividend.ends())
        {
            crate::cancellation::check()?;
            let [down, _, up] =
                space.coefficients(1, risk.rate(current), dividend.rate(current), sigma);
            // A one-sided jump can preserve carry only by adding variance on a coarse grid.
            if (down + up) * h * h > 2.0 * variance {
                return Err(crate::Error::ComputationFailure(
                    "scheduled barrier grid drift overwhelms diffusion; increase asset_step_count"
                        .into(),
                ));
            }
        }
    }
    if !scheduled && settings.scheme != super::FiniteDifferenceScheme::ExplicitEuler {
        space.resolve_barriers(
            [Some(terms.barrier_level()), upper_barrier],
            Some(terms.strike()),
            time,
            [&risk, &dividend],
            |level| volatility.sigma(0.0, level),
        )?;
    }
    if !scheduled
        && upper_barrier.is_none()
        && space.is_uniform()
        && settings.scheme != super::FiniteDifferenceScheme::ExplicitEuler
    {
        super::grid::concentrate(&mut space, vec![terms.strike()], 0.15)?;
    }
    let observations = diffusion::observation_times(
        context.valuation_time(),
        expiry,
        terms.observation_times().unwrap_or(&[]),
    )?;
    let grid = diffusion::time_grid(context, expiry, settings, &risk, &dividend, &observations)?;
    let implicit_fallback = !scheduled
        && upper_barrier.is_some()
        && settings.scheme == super::FiniteDifferenceScheme::Rannacher;
    if !local_in {
        let retry = u64::from(implicit_fallback);
        let mut steps = grid.rollback_steps(settings.scheme, &observations)? + retry;
        if knock_in {
            steps += grid.rollback_steps(settings.scheme, &[])? + retry;
        }
        validate_work(space.count as u64, steps)?;
        let lanes = 1 + bumps.iter().filter(|bump| bump.is_some()).count() as u64;
        if validate_work(space.count as u64 * lanes, steps).is_err()
            || validate_workspace(
                settings.scheme,
                (space.count as u64 + 1) * lanes,
                grid.widths.len() as u64,
                risk.workspace_bytes() + dividend.workspace_bytes(),
            )
            .is_err()
        {
            bumps = [None; 3];
        }
    }
    let theta = settings.scheme.theta();
    if !D::LOCAL {
        grid.validate_stability(&space, volatility.sigma(0.0, 0.0), theta, &risk, &dividend)?;
    }
    let sign = if terms.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let payoff = |s: f64| (sign * (s - terms.strike())).max(0.0);
    let knocked = |s: f64| {
        upper_barrier.is_some_and(|upper| s >= upper)
            || if up {
                s >= terms.barrier_level()
            } else {
                s <= terms.barrier_level()
            }
    };
    let rebate_at = |current| {
        scaled(
            terms.rebate(),
            if terms.rebate_timing() == RebateTiming::AtHit {
                0.0
            } else {
                risk.log_factor(current, time)
            },
        )
    };
    let vanilla_edges = |current: f64, [rate_shift, dividend_shift, _]: [f64; 3], sigma| {
        let tau = time - current;
        let r = risk.log_factor(current, time) - rate_shift * tau;
        let q = dividend.log_factor(current, time) - dividend_shift * tau;
        let value = |asset| {
            if tau == 0.0 {
                return payoff(asset);
            }
            let contract = Payoff::vanilla(terms.option_type(), terms.strike(), false);
            if let Some(sigma) = sigma {
                boundary_value(contract, asset, [r, q], sigma, tau)
            } else {
                boundary_value(contract, asset, [r, q], volatility, tau)
            }
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let edges = |current: f64, events: bool, shifts: [f64; 3], sigma| {
        let mut edges = vanilla_edges(current, shifts, sigma);
        if !events {
            return edges;
        }
        let rate_shift = shifts[0];
        let next = observations.get(observations.partition_point(|&event| event < current));
        if !scheduled || next.is_some() {
            let rebate = if scheduled && terms.rebate_timing() == RebateTiming::AtHit {
                scaled(
                    terms.rebate(),
                    risk.log_factor(current, *next.unwrap_or(&time))
                        - rate_shift * (*next.unwrap_or(&time) - current),
                )
            } else if terms.rebate_timing() == RebateTiming::AtExpiry {
                scaled(
                    terms.rebate(),
                    risk.log_factor(current, time) - rate_shift * (time - current),
                )
            } else {
                terms.rebate()
            };
            if knocked(space.asset(0)) {
                edges[0] = rebate;
            }
            if knocked(space.upper) {
                edges[1] = rebate;
            }
        }
        edges
    };
    let average_payoff = !scheduled && upper_barrier.is_none();
    let initial: Vec<_> = (0..=space.count)
        .map(|i| {
            if average_payoff {
                project_payoff(&space, &[terms.strike()], i, payoff)
            } else {
                payoff(space.asset(i))
            }
        })
        .collect();
    let levels: Vec<_> = std::iter::once(terms.barrier_level())
        .chain(upper_barrier)
        .collect();
    let breaks: Vec<_> = levels.iter().copied().chain([terms.strike()]).collect();
    let project = |values: &mut [f64], event: f64, terminal: bool| {
        project_event(
            &space,
            &breaks,
            &levels,
            values,
            !terminal && upper_barrier.is_none(),
            |asset| knocked(asset).then_some(event),
            |values, asset| {
                if terminal {
                    payoff(asset)
                } else {
                    space.linear(values, asset)
                }
            },
        );
    };
    let observe = |values: &mut [f64], current, terminal| {
        project(values, rebate_at(current), terminal);
    };
    let parameters = [
        risk.rate(0.0),
        dividend.rate(0.0),
        volatility.sigma(0.0, 0.0),
    ];
    let derivatives = tangent_coefficients(&space, parameters, bumps)?;
    let march = |mut values: Vec<f64>, events: bool| -> Result<_> {
        if events && (!scheduled || observations.last() == Some(&time)) {
            if scheduled {
                observe(&mut values, time, true);
            } else {
                for (i, value) in values.iter_mut().enumerate() {
                    if knocked(space.asset(i)) {
                        *value = terms.rebate();
                    }
                }
            }
        }
        let dates = if events && scheduled {
            &observations[..]
        } else {
            &[]
        };
        let unshifted_edges = |current| edges(current, events, [0.0; 3], None);
        if bumps.iter().all(Option::is_none) {
            return step::march(
                values,
                &space,
                &grid,
                volatility,
                settings.scheme,
                [&risk, &dividend],
                dates,
                implicit_fallback,
                unshifted_edges,
                |values, current| observe(values, current, false),
            )
            .map(|values| (values, std::array::from_fn(|_| Vec::new())));
        }
        step::march_tangents(
            values,
            &space,
            &grid,
            parameters,
            settings.scheme,
            dates,
            &derivatives,
            unshifted_edges,
            |current, lane| {
                let bump = bumps[lane].expect("active derivative has a bump");
                let shifted = |sign| {
                    let mut shifts = [0.0; 3];
                    shifts[lane] = sign * bump;
                    edges(current, events, shifts, Some(parameters[2] + shifts[2]))
                };
                let [low, high] = [shifted(-1.0), shifted(1.0)];
                std::array::from_fn(|i| (high[i] - low[i]) / (2.0 * bump))
            },
            |values, tangents, current| {
                observe(values, current, false);
                for (lane, tangent) in tangents
                    .iter_mut()
                    .enumerate()
                    .filter(|(_, v)| !v.is_empty())
                {
                    let rebate = if lane == 0 && terms.rebate_timing() == RebateTiming::AtExpiry {
                        -(time - current) * rebate_at(current)
                    } else {
                        0.0
                    };
                    project(tangent, rebate, false);
                }
            },
        )
    };
    let (mut values, mut tangents) = if local_in {
        // Evolve activation directly to avoid cancellation between independently meshed prices.
        let full_space = SpatialGrid::knock_in_continuation(
            settings,
            [full_lower, upper],
            terms.strike(),
            volatility.sigma(0.0, terms.strike()) * time.sqrt(),
        )?;
        let continuation: Vec<_> = (0..=full_space.count)
            .map(|i| {
                if average_payoff {
                    project_payoff(&full_space, &[terms.strike()], i, payoff)
                } else {
                    payoff(full_space.asset(i))
                }
            })
            .collect();
        let mut unactivated = vec![terms.rebate(); space.count + 1];
        let observe_in = |values: &mut [f64], vanilla: &[f64], _: f64| {
            project_event(
                &space,
                &breaks,
                &levels,
                values,
                upper_barrier.is_none(),
                |asset| knocked(asset).then(|| full_space.linear(vanilla, asset)),
                |values, asset| space.linear(values, asset),
            );
        };
        if scheduled && observations.last() == Some(&time) {
            observe_in(&mut unactivated, &continuation, time);
        } else if !scheduled {
            for i in [0, space.count] {
                if knocked(space.asset(i)) {
                    unactivated[i] = payoff(space.asset(i));
                }
            }
        }
        let values = step::march_with_continuation(
            unactivated,
            &space,
            continuation,
            &full_space,
            &grid,
            volatility,
            settings.scheme,
            [&risk, &dividend],
            if scheduled { &observations } else { &[] },
            implicit_fallback,
            |current| {
                [full_lower, upper].map(|asset| {
                    boundary_value(
                        Payoff::vanilla(terms.option_type(), terms.strike(), false),
                        asset,
                        [
                            risk.log_factor(current, time),
                            dividend.log_factor(current, time),
                        ],
                        volatility,
                        time - current,
                    )
                })
            },
            |current, vanilla| {
                let remaining =
                    !scheduled || observations.last().is_some_and(|&event| event >= current);
                let edge = |asset| {
                    if remaining && knocked(asset) {
                        full_space.linear(vanilla, asset)
                    } else {
                        rebate_at(current)
                    }
                };
                [edge(space.asset(0)), edge(space.upper)]
            },
            observe_in,
        )?;
        (values, std::array::from_fn(|_| Vec::new()))
    } else {
        march(initial.clone(), true)?
    };
    if knock_in && !local_in {
        let (vanilla, vanilla_tangents) = march(initial, false)?;
        let cash = scaled(terms.rebate(), risk.log_factor(0.0, time));
        for (value, vanilla) in values.iter_mut().zip(vanilla) {
            *value = vanilla - *value + cash;
        }
        for (lane, (knock_out, vanilla)) in tangents.iter_mut().zip(vanilla_tangents).enumerate() {
            let cash_derivative = if lane == 0 { -time * cash } else { 0.0 };
            for (value, vanilla) in knock_out.iter_mut().zip(vanilla) {
                *value = vanilla - *value + cash_derivative;
            }
        }
    }
    let tangents = tangents.map(|values| (!values.is_empty()).then(|| space.spline(values)));
    Ok(BarrierGrid {
        grid: super::solution::Grid::new(space, values),
        tangents,
        discount: [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)],
    })
}

#[derive(PartialEq)]
struct BarrierGrid {
    grid: super::solution::Grid,
    tangents: [Option<super::grid::Spline>; 3],
    discount: [f64; 2],
}
impl BarrierGrid {
    fn evaluate(
        &self,
        option: &impl Contract,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let terms = option.monitoring();
        let spot = context.spot_price();
        let sample = self.grid.sample(spot);
        let scale = scaled(spot, self.discount[1])
            .max(scaled(terms.strike().max(terms.rebate()), self.discount[0]));
        let mut result =
            PricingResult::new(crate::numerics::checked_with_scale(sample[0], scale)?)?;
        let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
        if !option.at_spot_discontinuity(context, h) {
            result.set(request, Greek::Delta, sample[1])?;
            result.set(request, Greek::Gamma, sample[2])?;
            if request.has(Greek::Speed) {
                if let Some(speed) = self.grid.speed(spot, h) {
                    result.set(request, Greek::Speed, speed)?;
                }
            }
        }
        for (greek, tangent) in MARKET_GREEKS.into_iter().zip(&self.tangents) {
            if let Some(tangent) = tangent {
                let [value, ..] = self.grid.space.sample(tangent, spot);
                if value.is_finite() {
                    result.set(request, greek, value)?;
                }
            }
        }
        Ok(result)
    }
}

pub(super) fn project_event(
    space: &SpatialGrid,
    breaks: &[f64],
    levels: &[f64],
    values: &mut [f64],
    additive: bool,
    event: impl Fn(f64) -> Option<f64>,
    continuation: impl Fn(&[f64], f64) -> f64,
) {
    // Project all boundaries from the same continuation, including shared cells.
    let projected: Vec<_> = levels
        .iter()
        .map(|level| {
            let cut = space.nearest(*level);
            let value =
                project_payoff(space, breaks, cut, |asset| match (event(asset), additive) {
                    (Some(value), true) => value - continuation(values, asset),
                    (Some(value), false) => value,
                    (None, true) => 0.0,
                    (None, false) => continuation(values, asset),
                });
            (cut, if additive { values[cut] + value } else { value })
        })
        .collect();
    for (i, value) in values.iter_mut().enumerate() {
        if let Some(replacement) = event(space.asset(i)) {
            *value = replacement;
        }
    }
    for (cut, value) in projected {
        values[cut] = value;
    }
}

#[derive(PartialEq)]
struct BarrierState {
    grid: Option<BarrierGrid>,
    vanilla: Option<super::VanillaState>,
}
impl<I: Contract> super::solution::Backend<I> for BarrierState {
    fn query_work(&self, option: &I, context: &PricingContext) -> usize {
        super::solution::monitoring_query_work(&option.monitoring().monitoring, context)
    }

    fn evaluate(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        if crate::engines::deterministic::enabled_until(context, option.monitoring().expiry_time())?
        {
            if let Some(value) = option.resolved_price(context)? {
                return PricingResult::new(value);
            }
            return PricingResult::new(crate::engines::deterministic::barrier_value(
                option.monitoring(),
                option.upper_barrier(),
                context,
            )?);
        }
        let mut end = context.valuation_time();
        if uses_local_diffusion(context) {
            let prefix = deterministic::barrier_prefix(
                &option.monitoring().monitoring,
                option.upper_barrier(),
                context,
            )?;
            if let Some(result) =
                local_resolved_with(option, context, &prefix, request, shifts, |request| {
                    let state = self.vanilla.as_ref().ok_or_else(super::unstable)?;
                    super::solution::Backend::evaluate(
                        state,
                        option.monitoring().vanilla(),
                        context,
                        request,
                        shifts,
                    )
                })?
            {
                return Ok(result);
            }
            end = prefix.end;
        } else if let Some(value) = option.resolved_price(context)? {
            return PricingResult::new(value);
        }
        evaluate_after_prefix(
            context,
            end,
            request,
            shifts,
            |radius| option.at_spot_discontinuity(context, radius),
            |context, request, shifts| {
                self.grid
                    .as_ref()
                    .ok_or_else(super::unstable)?
                    .evaluate(option, context, request, shifts)
            },
        )
    }
}

fn prepare_solution<I: Contract + Clone + PartialEq + Send + Sync + 'static>(
    option: &I,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    range: std::ops::RangeInclusive<f64>,
) -> Result<super::FiniteDifferenceSolution> {
    local_volatility::validate_model(context)?;
    super::solution::validate_range(&range)?;
    crate::cancellation::check()?;
    let anchor = super::solution::at_spot(context, *range.end())?;
    validate_life(option, &anchor)?;
    let terms = option.monitoring();
    let local = uses_local_diffusion(context);
    let mut pending = None;
    let mut vanilla = false;
    let levels = deterministic::barrier_probe_levels(
        &terms.monitoring,
        option.upper_barrier(),
        context,
        &range,
    )?;
    for spot in super::solution::probes(&range, levels) {
        let probe = super::solution::at_spot(context, spot)?;
        if local {
            let prefix =
                deterministic::barrier_prefix(&terms.monitoring, option.upper_barrier(), &probe)?;
            let (touched, finished) = (prefix.touched, prefix.finished);
            let knock_in = terms.barrier_type().is_knock_in();
            let fixed = terms.rebate() == 0.0 && option.payoff_requires_touch();
            if touched && !knock_in || !touched && finished && knock_in || fixed && !knock_in {
                local_resolved_with(
                    option,
                    &probe,
                    &prefix,
                    GreeksRequest::new([]),
                    Default::default(),
                    |_| Err(super::unstable()),
                )?;
            } else if touched || finished || fixed {
                vanilla = true;
            } else {
                pending = Some(prefix.end);
            }
        } else if option.resolved_price(&probe)?.is_none() {
            pending = Some(context.valuation_time());
        }
    }
    let continuation = if vanilla {
        Some(super::prepare_vanilla_state(
            terms.vanilla(),
            Payoff::vanilla(terms.option_type(), terms.strike(), false),
            &anchor,
            settings,
            false,
            *range.start(),
        )?)
    } else {
        None
    };
    let grid = if let Some(end) = pending {
        if crate::engines::deterministic::enabled_until(context, option.monitoring().expiry_time())?
        {
            None
        } else {
            let (rolled, carry, _) = continuation_context(&anchor, end)?;
            let rolled = rolled.as_ref();
            Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
                option,
                rolled,
                settings,
                diffusion,
                scaled(*range.start(), carry),
                [None; 3]
            ))?)
        }
    } else {
        None
    };
    let state = BarrierState {
        grid,
        vanilla: continuation,
    };
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
macro_rules! barrier_solution {
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
barrier_solution!(FiniteDifferenceBarrierEngine, BarrierOption);
barrier_solution!(FiniteDifferenceDoubleBarrierEngine, DoubleBarrierOption);
