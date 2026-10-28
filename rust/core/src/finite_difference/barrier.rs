use super::*;
use crate::barrier::{resolved_price, validate_life, validate_schedule};
use crate::{BarrierOption, ObservationMode, RebateTiming};

impl FiniteDifferenceBarrierEngine {
    /// Prices a single barrier with continuous or scheduled monitoring and flat or node curves.
    /// Resolves current observations against actual spot before building a grid.
    /// Prior at-hit rebates are already paid; resolved vanilla payoffs use analytic pricing.
    /// Continuous barriers are exact domain edges; scheduled dates are inserted unchanged.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidTimeRange`] outside life or curve coverage,
    /// [`ErrorCategory::InvalidDate`] for non-trading observations,
    /// [`ErrorCategory::InvalidParameter`] for missing history, clipped/degenerate grids
    /// or unstable explicit steps, [`ErrorCategory::InvalidOption`] for impossible history,
    /// and [`ErrorCategory::InvalidResult`] for numerical failures. Inputs remain unchanged.
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(native_barrier(
            option,
            context,
            self.settings,
            GreeksRequest::default(),
            Default::default(),
        )?
        .price())
    }

    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    /// Uses [`NumericalShiftSettings`] for all bumps and fixes the central spatial domain.
    /// Stencils crossing observations or barriers leave affected Greeks absent; all are
    /// absent at expiry or a current barrier equality without a prior touch.
    ///
    /// # Errors
    /// Validates requests and shifts before pricing. Propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors; feasible bumped failures fail the operation.
    /// Inputs and immutable engine settings remain unchanged on success and failure.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut))?;
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
        request.validate()?;
        shifts.validate()?;
        let mut settings = self.settings;
        if settings.asset_upper_boundary.is_none() {
            let upper = default_upper(option, context);
            if upper.is_finite() {
                settings.asset_upper_boundary = Some(upper);
            }
        }
        crate::numerical::complete_greeks(
            option,
            context,
            request,
            shifts,
            true,
            |context, request| native_barrier(option, context, settings, request, shifts),
        )
    }
}

impl PricingEngine<BarrierOption> for FiniteDifferenceBarrierEngine {
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
        crate::barrier::volatility_affects_price(option, context)
    }
}

fn default_upper(option: &BarrierOption, context: &PricingContext) -> f64 {
    let relevant = context.spot_price().max(option.strike());
    (4.0 * relevant).max(option.barrier_level() + relevant)
}

fn native_barrier(
    option: &BarrierOption,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    validate_life(option, context)?;
    validate_schedule(option, context)?;
    if let Some(value) = resolved_price(option, context)? {
        return PricingResult::new(value);
    }
    let time = context
        .valuation_time()
        .years_until(option.expiry_date().start_of_day());
    let model = context.model_parameters();
    let expiry = option.expiry_date().start_of_day();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let upper = settings
        .asset_upper_boundary
        .unwrap_or_else(|| default_upper(option, context));
    let mut space = SpatialGrid::new(
        settings,
        upper,
        0.0,
        context
            .spot_price()
            .max(option.strike())
            .max(option.barrier_level()),
    )?;
    let scheduled = option.observation_mode() == ObservationMode::Scheduled;
    let up = option.barrier_type().is_up();
    if !scheduled {
        space.barrier_domain(
            if up { 0.0 } else { option.barrier_level() },
            if up { option.barrier_level() } else { upper },
        )?;
    }
    let observations: Vec<_> = option
        .observation_dates()
        .iter()
        .filter(|date| date.start_of_day() > context.valuation_time())
        .map(|date| context.valuation_time().years_until(date.start_of_day()))
        .collect();
    let grid = TimeGrid::with_events(
        time,
        settings.time_step_count,
        &risk,
        &dividend,
        &observations,
    );
    let sigma = model.volatility();
    let theta = settings.scheme.theta();
    if theta == 0.0 {
        for (index, &dt) in grid.widths.iter().enumerate() {
            let middle = grid.times[index].midpoint(grid.times[index + 1]);
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let decay = if scheduled {
                let n = space.count as f64;
                (sigma * sigma * n * n).max((rate - q).abs() * n) + rate
            } else {
                (1..space.count)
                    .map(|i| -space.coefficients(i, rate, q, sigma)[1])
                    .fold(0.0, f64::max)
            };
            if dt * decay > 1.0 {
                return Err(invalid_grid("explicit finite-difference grid is unstable"));
            }
        }
    }
    let sign = if option.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let payoff = |s: f64| (sign * (s - option.strike())).max(0.0);
    let knocked = |s: f64| {
        if up {
            s >= option.barrier_level()
        } else {
            s <= option.barrier_level()
        }
    };
    let rebate_at = |current| {
        scaled(
            option.rebate(),
            if option.rebate_timing() == RebateTiming::AtHit {
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
                    scaled(option.strike(), r)
                };
            }
            let (d1, d2) =
                black_scholes_arguments(asset, option.strike(), q - r, sigma, tau.sqrt());
            sign * (weighted_cdf(asset, q, sign * d1) - weighted_cdf(option.strike(), r, sign * d2))
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let edges = |current: f64| {
        let mut edges = vanilla_edges(current);
        let next = observations.iter().find(|&&event| event >= current);
        if !scheduled || next.is_some() {
            let rebate = if scheduled && option.rebate_timing() == RebateTiming::AtHit {
                scaled(
                    option.rebate(),
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
    let observe = |values: &mut [f64], current, terminal| {
        let cut = ((option.barrier_level() / space.spacing).round() as usize).min(space.count);
        let event = rebate_at(current);
        let projected = project(
            &space,
            &[option.barrier_level(), option.strike()],
            cut,
            |asset| {
                if terminal {
                    if knocked(asset) { event } else { payoff(asset) }
                } else if knocked(asset) {
                    event - space.linear(values, asset)
                } else {
                    0.0
                }
            },
        );
        let projected = if terminal {
            projected
        } else {
            values[cut] + projected
        };
        for (i, value) in values.iter_mut().enumerate() {
            if knocked(space.asset(i)) {
                *value = event;
            }
        }
        values[cut] = projected;
    };
    let march = |mut values: Vec<f64>, events: bool| -> Result<Vec<f64>> {
        if events && (!scheduled || observations.contains(&time)) {
            if scheduled {
                observe(&mut values, time, true);
            } else {
                for (i, value) in values.iter_mut().enumerate() {
                    if knocked(space.asset(i)) {
                        *value = option.rebate();
                    }
                }
            }
        }
        let mut next = vec![0.0; values.len()];
        let mut stepper = Stepper::new(space.count);
        let mut damping: u32 = 2;
        for step in (0..grid.widths.len()).rev() {
            let damp = theta == 0.5 && damping > 0;
            let count = if damp { 2 } else { 1 };
            let weight = if damp { 1.0 } else { theta };
            damping = damping.saturating_sub(1);
            let dt = grid.widths[step] / f64::from(count);
            for substep in 0..count {
                let current = grid.times[step] + dt * f64::from(count - substep - 1);
                let middle = current + 0.5 * dt;
                stepper.advance(
                    &values,
                    &mut next,
                    &space,
                    sigma,
                    [dt, weight, risk.rate(middle), dividend.rate(middle)],
                    if events {
                        edges(current)
                    } else {
                        vanilla_edges(current)
                    },
                )?;
                std::mem::swap(&mut values, &mut next);
            }
            if events && scheduled && observations.contains(&grid.times[step]) {
                observe(&mut values, grid.times[step], false);
                damping = 2;
            }
        }
        Ok(values)
    };
    let mut values = march(initial.clone(), true)?;
    if option.barrier_type().is_knock_in() {
        let vanilla = march(initial, false)?;
        let cash = scaled(option.rebate(), risk.log_factor(0.0, time));
        for (value, vanilla) in values.iter_mut().zip(vanilla) {
            *value = (vanilla - *value + cash).max(0.0);
        }
    }
    let curvature = space.curvature(&values);
    let spot = context.spot_price();
    let sample = space.sample(&values, &curvature, spot);
    let mut result = PricingResult::new(crate::barrier::checked(sample[0])?)?;
    let h = crate::numerical::symmetric_shift(spot, shifts.spot_shift);
    if !option.at_spot_discontinuity(context, h) {
        result.set(request, Greek::Delta, sample[1])?;
        result.set(request, Greek::Gamma, sample[2])?;
        if request.has(Greek::Speed)
            && h > 0.0
            && spot - h > space.asset(0)
            && spot + h < space.upper
        {
            result.set(
                request,
                Greek::Speed,
                (space.sample(&values, &curvature, spot + h)[2]
                    - space.sample(&values, &curvature, spot - h)[2])
                    / (2.0 * h),
            )?;
        }
    }
    Ok(result)
}

fn project(space: &SpatialGrid, breaks: &[f64], index: usize, value: impl Fn(f64) -> f64) -> f64 {
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
