use super::{
    Curve, Diffusion, FiniteDifferenceBarrierEngine, FiniteDifferenceDoubleBarrierEngine,
    FiniteDifferenceSettings, Greek, GreeksRequest, LocalDiffusion, NumericalShiftSettings,
    OptionContract, OptionType, Payoff, PricingContext, PricingEngine, PricingResult, Result,
    SpatialGrid, black_scholes_arguments, diffusion, diffusion_upper, local_volatility, scaled,
    step, weighted_cdf,
};
use crate::engines::analytic::barrier::resolved_price;
use crate::instruments::monitoring::{validate_life, validate_schedule};
use crate::{BarrierOption, DoubleBarrierOption, ObservationMode, RebateTiming};

impl FiniteDifferenceBarrierEngine {
    /// Prices a single barrier with continuous or scheduled monitoring and flat or nonconstant rate curves.
    ///
    /// Resolves current observations against actual spot before building a grid.
    /// Prior at-hit rebates are already paid; resolved vanilla payoffs use the selected model.
    /// Local volatility supports price, delta and gamma, holding the surface fixed.
    /// Continuous barriers are exact domain edges; scheduled dates are inserted unchanged.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve/surface coverage, for
    /// nontrading observations, missing or impossible history, clipped or
    /// degenerate grids, or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure]. Inputs remain unchanged.
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(native_barrier(
            option,
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
        )?
        .price())
    }

    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    ///
    /// Uses [`NumericalShiftSettings`] for bumps and fixes the central BSM spatial domain.
    /// Local volatility accepts only delta/gamma, holding the absolute surface fixed.
    /// Stencils crossing observations or barriers leave affected Greeks absent; all are
    /// based on terminal payoffs at expiry and absent at a current barrier equality without a prior touch.
    ///
    /// # Errors
    ///
    /// Leaves other local-volatility Greeks absent. Validates requests and shifts before pricing. Propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors; feasible bumped failures fail the operation.
    /// Inputs and immutable engine settings remain unchanged on success and failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings::default())?;
    /// assert_eq!(engine.settings().asset_step_count, 800);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert_eq!(result.price(), engine.price(&option, &context)?);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        analysis(option, context, self.settings, request, shifts)
    }
}

impl PricingEngine<BarrierOption> for FiniteDifferenceBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.price_with_greeks(option, context, request, shifts)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}

trait Contract: OptionContract {
    fn monitoring(&self) -> &BarrierOption;
    fn upper_barrier(&self) -> Option<f64> {
        None
    }
    fn resolved_price(&self, context: &PricingContext) -> Result<Option<f64>>;
    fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)>;
    fn payoff_requires_touch(&self) -> bool;
}
impl Contract for BarrierOption {
    fn monitoring(&self) -> &BarrierOption {
        self
    }
    fn resolved_price(&self, context: &PricingContext) -> Result<Option<f64>> {
        resolved_price(self, context)
    }
    fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        self.state(context)
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
        crate::engines::analytic::double_barrier::resolved_price(self, context)
    }
    fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        self.state(context)
    }
    fn payoff_requires_touch(&self) -> bool {
        self.payoff_requires_touch()
    }
}

fn analysis(
    option: &impl Contract,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    request.validate()?;
    shifts.validate()?;
    local_volatility::validate_request(context, request)?;
    let mut settings = settings;
    if settings.asset_upper_boundary.is_none() && local_volatility::local_model(context).is_none() {
        let upper = default_upper(option, context);
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
        native_barrier(option, context, settings, request, shifts)?,
        |context, request| native_barrier(option, context, settings, request, shifts),
    )
}

impl FiniteDifferenceDoubleBarrierEngine {
    /// Prices double barriers with continuous or scheduled monitoring and flat or nonconstant rate curves.
    ///
    /// Fits both continuous edges exactly, using all configured asset steps between them.
    /// Projects scheduled boundary events simultaneously on the full spatial grid.
    /// Resolves current touches and known history before grid validation.
    /// Local volatility supports price, delta and gamma, holding the surface fixed.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Reuses [`FiniteDifferenceBarrierEngine::price`] categories and validation timing,
    /// including rejection of boundaries that do not exceed spot, strike and both barriers.
    /// Inputs and engine settings remain unchanged on every outcome.
    pub fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(native_barrier(
            option,
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
        )?
        .price())
    }

    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    ///
    /// Fixes the central BSM spatial domain for bumps. Local volatility accepts only
    /// delta and gamma with the local-volatility surface held fixed. Stencils crossing
    /// either boundary or an observation omit affected measures; expiry Greeks follow terminal payoffs. Before expiry, current boundary
    /// equality without a prior touch suppresses Greeks.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Inputs remain unchanged on success and failure.
    ///
    /// # Examples
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
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), Default::default())?;
    /// assert!((result.price() - 1.0838814592583166).abs() < 0.001);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        analysis(option, context, self.settings, request, shifts)
    }
}
impl PricingEngine<DoubleBarrierOption> for FiniteDifferenceDoubleBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.price_with_greeks(option, context, request, shifts)
    }
    fn volatility_affects_price(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> bool {
        option.volatility_affects_price(context)
    }
}

fn default_upper(option: &impl Contract, context: &PricingContext) -> f64 {
    let terms = option.monitoring();
    let relevant = context.spot_price().max(terms.strike());
    (4.0 * relevant).max(option.upper_barrier().unwrap_or(terms.barrier_level()) + relevant)
}

fn native_barrier(
    option: &impl Contract,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    let request = local_volatility::validate_request(context, request)?;
    if let Some(model) = local_volatility::local_model(context) {
        native_with_model(
            option,
            context,
            settings,
            request,
            shifts,
            LocalDiffusion::new(context, model.surface()),
        )
    } else {
        native_with_model(
            option,
            context,
            settings,
            request,
            shifts,
            context.bsm_parameters()?.volatility(),
        )
    }
}

fn local_resolved(
    option: &impl Contract,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<Option<PricingResult>> {
    let terms = option.monitoring();
    let (prior, touched, finished) = option.state(context)?;
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
    if touched && !knock_in {
        return if terms.rebate_timing() == RebateTiming::AtHit {
            PricingResult::new(if prior { 0.0 } else { terms.rebate() }).map(Some)
        } else {
            PricingResult::new(cash()?).map(Some)
        };
    }
    if !touched && finished && knock_in {
        return PricingResult::new(cash()?).map(Some);
    }
    let fixed = terms.rebate() == 0.0 && option.payoff_requires_touch();
    if fixed && !knock_in {
        return PricingResult::new(0.0).map(Some);
    }
    if touched || finished || fixed {
        let h = crate::risk::numerical::symmetric_shift(context.spot_price(), shifts.spot_shift());
        let request =
            if option.all_greeks_unavailable(context) || option.at_spot_discontinuity(context, h) {
                GreeksRequest::new([])
            } else {
                request
            };
        return Ok(Some(
            super::native(
                terms.vanilla(),
                Payoff::vanilla(terms.option_type(), terms.strike(), false),
                context,
                settings,
                request,
                shifts,
                false,
            )?
            .0,
        ));
    }
    Ok(None)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Barrier positions intentionally round and saturate to nonnegative indices before capping at the grid size."
)]
fn native_with_model<D: Diffusion>(
    option: &impl Contract,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    volatility: D,
) -> Result<PricingResult> {
    let terms = option.monitoring();
    let upper_barrier = option.upper_barrier();
    validate_life(terms, context)?;
    validate_schedule(terms.observation_schedule(), context)?;
    if D::LOCAL {
        if let Some(result) = local_resolved(option, context, settings, request, shifts)? {
            return Ok(result);
        }
    } else if let Some(value) = option.resolved_price(context)? {
        return PricingResult::new(value);
    }
    let time = context.valuation_time().years_until(option.expiry_time());
    let model = context.model_parameters();
    let expiry = option.expiry_time();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let upper = if let Some(upper) = settings.asset_upper_boundary {
        upper
    } else {
        let default = default_upper(option, context);
        if D::LOCAL {
            default.max(diffusion_upper(
                context.spot_price().max(terms.strike()),
                context,
                time,
                &risk,
                &dividend,
            )?)
        } else {
            default
        }
    };
    let mut space = SpatialGrid::new(
        settings,
        upper,
        0.0,
        context
            .spot_price()
            .max(terms.strike())
            .max(upper_barrier.unwrap_or(terms.barrier_level())),
    )?;
    let scheduled = terms.observation_mode() == ObservationMode::Scheduled;
    let up = terms.barrier_type().is_up();
    if !scheduled {
        if let Some(upper) = upper_barrier {
            space.uniform_domain(terms.barrier_level(), upper)?;
        } else {
            space.uniform_domain(
                if up { 0.0 } else { terms.barrier_level() },
                if up { terms.barrier_level() } else { upper },
            )?;
        }
    }
    let observations: Vec<_> = terms
        .observation_times()
        .iter()
        .filter(|&&date| date > context.valuation_time())
        .map(|&date| context.valuation_time().years_until(date))
        .collect();
    diffusion::validate_event_resolution(
        context.valuation_time(),
        expiry,
        terms.observation_times(),
    )?;
    let grid = diffusion::time_grid(
        context,
        expiry,
        settings.time_step_count,
        &risk,
        &dividend,
        &observations,
    )?;
    let theta = settings.scheme.theta();
    if !D::LOCAL {
        grid.validate_stability(
            &space,
            volatility.sigma(0.0, 0.0),
            theta,
            scheduled,
            &risk,
            &dividend,
        )?;
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
    let vanilla_edges = |current: f64| {
        let tau = time - current;
        let r = risk.log_factor(current, time);
        let q = dividend.log_factor(current, time);
        let value = |asset| {
            if tau == 0.0 {
                return payoff(asset);
            }
            if asset == 0.0 {
                return if sign > 0.0 {
                    0.0
                } else {
                    scaled(terms.strike(), r)
                };
            }
            if D::LOCAL {
                return if sign > 0.0 {
                    scaled(asset, q) - scaled(terms.strike(), r)
                } else {
                    0.0
                };
            }
            let (d1, d2) = black_scholes_arguments(
                asset,
                terms.strike(),
                q - r,
                volatility.sigma(0.0, 0.0),
                tau.sqrt(),
            );
            sign * (weighted_cdf(asset, q, sign * d1) - weighted_cdf(terms.strike(), r, sign * d2))
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let edges = |current: f64| {
        let mut edges = vanilla_edges(current);
        let next = observations.iter().find(|&&event| event >= current);
        if !scheduled || next.is_some() {
            let rebate = if scheduled && terms.rebate_timing() == RebateTiming::AtHit {
                scaled(
                    terms.rebate(),
                    risk.log_factor(current, *next.unwrap_or(&time)),
                )
            } else {
                rebate_at(current)
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
    let initial: Vec<_> = (0..=space.count).map(|i| payoff(space.asset(i))).collect();
    let levels: Vec<_> = std::iter::once(terms.barrier_level())
        .chain(upper_barrier)
        .collect();
    let breaks: Vec<_> = levels.iter().copied().chain([terms.strike()]).collect();
    let observe = |values: &mut [f64], current, terminal| {
        let event = rebate_at(current);
        let projected: Vec<_> = levels
            .iter()
            .map(|level| {
                let cut = ((level / space.spacing).round() as usize).min(space.count);
                // Project both boundaries from the same continuation, including shared cells.
                let value = project(&space, &breaks, cut, |asset| {
                    if terminal || upper_barrier.is_some() {
                        if knocked(asset) {
                            event
                        } else if terminal {
                            payoff(asset)
                        } else {
                            space.linear(values, asset)
                        }
                    } else if knocked(asset) {
                        event - space.linear(values, asset)
                    } else {
                        0.0
                    }
                });
                (
                    cut,
                    if terminal || upper_barrier.is_some() {
                        value
                    } else {
                        values[cut] + value
                    },
                )
            })
            .collect();
        for (i, value) in values.iter_mut().enumerate() {
            if knocked(space.asset(i)) {
                *value = event;
            }
        }
        for (cut, value) in projected {
            values[cut] = value;
        }
    };
    let march = |mut values: Vec<f64>, events: bool| -> Result<Vec<f64>> {
        if events && (!scheduled || observations.contains(&time)) {
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
        step::march(
            values,
            &space,
            &grid,
            volatility,
            settings.scheme,
            [&risk, &dividend],
            if events && scheduled {
                &observations
            } else {
                &[]
            },
            |current| {
                if events {
                    edges(current)
                } else {
                    vanilla_edges(current)
                }
            },
            |values, current| observe(values, current, false),
        )
    };
    let local_in = D::LOCAL && terms.barrier_type().is_knock_in();
    let mut values = if local_in {
        // Evolve activation directly to avoid cancellation between independently meshed prices.
        let full_space = SpatialGrid::new(
            settings,
            upper,
            0.0,
            context.spot_price().max(terms.strike()),
        )?;
        let continuation: Vec<_> = (0..=full_space.count)
            .map(|i| payoff(full_space.asset(i)))
            .collect();
        let mut unactivated = vec![terms.rebate(); space.count + 1];
        let observe_in = |values: &mut [f64], vanilla: &[f64], _: f64| {
            let projected: Vec<_> = levels
                .iter()
                .map(|level| {
                    let cut = ((level / space.spacing).round() as usize).min(space.count);
                    let average = project(&space, &breaks, cut, |asset| {
                        if upper_barrier.is_some() {
                            if knocked(asset) {
                                full_space.linear(vanilla, asset)
                            } else {
                                space.linear(values, asset)
                            }
                        } else if knocked(asset) {
                            full_space.linear(vanilla, asset) - space.linear(values, asset)
                        } else {
                            0.0
                        }
                    });
                    (
                        cut,
                        if upper_barrier.is_some() {
                            average
                        } else {
                            values[cut] + average
                        },
                    )
                })
                .collect();
            for (i, value) in values.iter_mut().enumerate() {
                if knocked(space.asset(i)) {
                    *value = full_space.linear(vanilla, space.asset(i));
                }
            }
            for (cut, value) in projected {
                values[cut] = value;
            }
        };
        if scheduled && observations.contains(&time) {
            observe_in(&mut unactivated, &continuation, time);
        } else if !scheduled {
            for i in [0, space.count] {
                if knocked(space.asset(i)) {
                    unactivated[i] = payoff(space.asset(i));
                }
            }
        }
        step::march_with_continuation(
            unactivated,
            &space,
            continuation,
            &full_space,
            &grid,
            volatility,
            settings.scheme,
            [&risk, &dividend],
            if scheduled { &observations } else { &[] },
            |current| {
                let cash = scaled(terms.strike(), risk.log_factor(current, time));
                if sign > 0.0 {
                    [
                        0.0,
                        scaled(upper, dividend.log_factor(current, time)) - cash,
                    ]
                } else {
                    [cash, 0.0]
                }
            },
            |current, vanilla| {
                let remaining = !scheduled || observations.iter().any(|&event| event >= current);
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
        )?
    } else {
        march(initial.clone(), true)?
    };
    if terms.barrier_type().is_knock_in() && !local_in {
        let vanilla = march(initial, false)?;
        let cash = scaled(terms.rebate(), risk.log_factor(0.0, time));
        for (value, vanilla) in values.iter_mut().zip(vanilla) {
            *value = vanilla - *value + cash;
        }
    }
    let curvature = space.curvature(&values);
    let spot = context.spot_price();
    let sample = space.sample(&values, &curvature, spot);
    let scale = scaled(spot, dividend.log_factor(0.0, time)).max(scaled(
        terms.strike().max(terms.rebate()),
        risk.log_factor(0.0, time),
    ));
    let mut result = PricingResult::new(crate::numerics::checked_with_scale(sample[0], scale)?)?;
    let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift());
    if !option.at_spot_discontinuity(context, h) {
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

pub(super) fn project(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64) -> f64,
) -> f64 {
    let center = space.asset(index);
    if index == 0 || index == space.count {
        return value(center);
    }
    let lower = space.asset(index - 1).midpoint(center);
    let upper = space.asset(index + 1).midpoint(center);
    let mut points: Vec<_> = breaks
        .iter()
        .copied()
        .filter(|&v| v > lower && v < upper)
        .collect();
    if points.is_empty() {
        return value(center);
    }
    points.extend([lower, center, upper]);
    points.sort_by(f64::total_cmp);
    points.dedup();
    points
        .windows(2)
        .map(|pair| {
            let middle = pair[0].midpoint(pair[1]);
            let radius = 0.5 * (pair[1] - pair[0]) / 3.0_f64.sqrt();
            (pair[1] - pair[0]) / (upper - lower)
                * value(middle - radius).midpoint(value(middle + radius))
        })
        .sum()
}
