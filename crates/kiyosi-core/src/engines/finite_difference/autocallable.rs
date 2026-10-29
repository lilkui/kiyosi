use super::*;
use crate::instruments::autocallable::pricing::{fixed_value, initial, validate};
use crate::{AutocallableNote, AutocallableObservation, AutocallableTerms, KnockInObservationMode};

impl FiniteDifferenceAutocallableEngine {
    /// Prices remaining snowball or Phoenix cashflows on exact trading and observation dates.
    /// Accepts flat or node curves, retains daily knock-in layers and projects future events.
    /// Current events use actual spot with strict knock-in and inclusive knock-out/coupon levels.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidTimeRange`] outside life or curve coverage,
    /// [`ErrorCategory::InvalidDate`] for non-trading observations or expiry,
    /// [`ErrorCategory::InvalidParameter`] for missing history, clipped grids or unstable
    /// explicit steps, [`ErrorCategory::InvalidOption`] for impossible history, and
    /// [`ErrorCategory::InvalidResult`] for numerical failures. Inputs remain unchanged.
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        Ok(native(
            option,
            context,
            self.settings,
            GreeksRequest::default(),
            Default::default(),
        )?
        .price())
    }

    /// Computes spline spot Greeks and completes other sensitivities with core bump defaults.
    /// Native delta/gamma do not require feasible spot bumps; speed requires a symmetric
    /// in-domain stencil. Fixes the central upper domain for bumps. Expiry Greeks and event-crossing stencils
    /// are absent; vega and rho remain available at current thresholds.
    ///
    /// # Errors
    /// Validates requests and shifts, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors without changing inputs or settings.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let end = Date::new(2026, 1, 1)?;
    /// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.02, start, end,
    ///     autocallable_observations(&[end], &[100.0], Some(&[0.08]), None)?))?;
    /// let market = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceAutocallableEngine::default();
    /// let result = engine.price_with_greeks(&note, &market,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), Default::default())?;
    /// assert_eq!(result.price(), engine.price(&note, &market)?);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        request.validate()?;
        shifts.validate()?;
        let mut settings = self.settings;
        validate(option, context)?;
        let state = initial(option, context);
        if settings.asset_upper_boundary.is_none()
            && state.settlement.is_none()
            && fixed_value(option, context, state.knocked_in).is_none()
        {
            let upper = default_upper(option, context)?;
            if upper.is_finite() {
                settings.asset_upper_boundary = Some(upper);
            }
        }
        crate::risk::numerical::complete_greeks(
            option,
            context,
            request,
            shifts,
            true,
            |context, request| native(option, context, settings, request, shifts),
        )
    }
}
impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>>
    for FiniteDifferenceAutocallableEngine
{
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.price_with_greeks(option, context, request, shifts)
    }
    fn volatility_affects_price(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> bool {
        let state = initial(option, context);
        state.settlement.is_none() && fixed_value(option, context, state.knocked_in).is_none()
    }
}

fn highest<T: AutocallableTerms>(note: &AutocallableNote<T>) -> f64 {
    let t = note.view();
    let mut level = t.initial_spot;
    if t.has_knock_in {
        level = level.max(t.knock_in_level);
    }
    if t.fixed_knocked_in_coupon.is_none() {
        level = level.max(t.upper_strike).max(t.lower_strike);
    }
    note.observations().iter().fold(level, |level, r| {
        level.max(r.knock_out_level).max(r.coupon_barrier_level)
    })
}
fn default_upper<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
) -> Result<f64> {
    let time = time_to_expiry(note, context)?;
    if time == 0.0 {
        return Ok(4.0 * highest(note).max(context.spot_price()));
    }
    let model = context.model_parameters();
    let risk = Curve::new(
        model.risk_free_curve(),
        context.valuation_time(),
        note.expiry_date().into(),
    )?;
    let dividend = Curve::new(
        model.dividend_curve(),
        context.valuation_time(),
        note.expiry_date().into(),
    )?;
    let sigma = model.volatility();
    let maximum_drift = risk
        .ends()
        .chain(dividend.ends())
        .map(|end| {
            (dividend.log_factor(0.0, end) - risk.log_factor(0.0, end) - 0.5 * sigma * sigma * end)
                .abs()
        })
        .fold(0.0, f64::max);
    let width = 4.0 * sigma * time.sqrt() + maximum_drift;
    let relevant = highest(note).max(context.spot_price());
    Ok(if width <= 4.0_f64.ln() {
        4.0 * relevant
    } else {
        (relevant.ln() + width).exp()
    })
}

fn concentrate(space: &mut SpatialGrid, mut levels: Vec<f64>, scale: f64) -> Result<()> {
    levels.retain(|&l| l > 0.0 && l < space.upper);
    levels.sort_by(f64::total_cmp);
    let gap = scale * (space.upper / scale).asinh() / space.count as f64;
    let mut previous = 0.0;
    levels.retain(|&l| {
        if l - previous < gap || space.upper - l < gap {
            false
        } else {
            previous = l;
            true
        }
    });
    if levels.is_empty() {
        return Ok(());
    }
    let mut segments = vec![(0.0, levels[0], false, 1_usize, 0.0)];
    for pair in levels.windows(2) {
        let middle = pair[0].midpoint(pair[1]);
        segments.push((pair[0], middle, true, 1, 0.0));
        segments.push((middle, pair[1], false, 1, 0.0));
    }
    segments.push((*levels.last().unwrap_or(&0.0), space.upper, true, 1, 0.0));
    if segments.len() > space.count {
        return Ok(());
    }
    let width = |a: f64, b: f64| ((b - a) / scale).asinh();
    let total: f64 = segments.iter().map(|&(a, b, _, _, _)| width(a, b)).sum();
    let available = space.count - segments.len();
    let mut allocated = 0;
    for (a, b, _, steps, remainder) in &mut segments {
        let count = available as f64 * width(*a, *b) / total;
        *steps += count as usize;
        *remainder = count.fract();
        allocated += count as usize;
    }
    for _ in allocated..available {
        let mut best = 0;
        for i in 1..segments.len() {
            if segments[i].4 > segments[best].4 {
                best = i;
            }
        }
        segments[best].3 += 1;
        segments[best].4 = -1.0;
    }
    let mut spacing = total / space.count as f64;
    for &(a, b, _, steps, _) in &segments {
        if steps > 1 {
            spacing = spacing.min(width(a, b) / (steps as f64 - 0.5));
        }
    }
    let mut nodes = vec![0.0];
    for (a, b, from_left, steps, _) in segments {
        for i in 1..=steps {
            let x = spacing * if from_left { i } else { steps - i } as f64;
            let value = if i == steps {
                b
            } else if from_left {
                a + scale * x.sinh()
            } else {
                b - scale * x.sinh()
            };
            if value > nodes[nodes.len() - 1] {
                nodes.push(value);
            }
        }
    }
    if nodes.iter().any(|n| !n.is_finite()) {
        return Err(invalid_grid("concentrated grid is not representable"));
    }
    space.count = nodes.len() - 1;
    space.spacing = 1.0;
    space.scale = 0.0;
    space.nodes = nodes;
    Ok(())
}

fn event<'a, T: AutocallableTerms>(
    note: &'a AutocallableNote<T>,
    time: f64,
    context: &PricingContext,
) -> Option<&'a AutocallableObservation> {
    note.observations()
        .iter()
        .find(|r| context.valuation_time().years_until(r.date.into()) == time)
}

// Project only the jump correction, leaving smooth daily continuation unchanged.
pub(super) fn project(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64) -> [f64; 2],
) -> [f64; 2] {
    let value_at = |x| value(space.asset_of_coordinate(x));
    let center = space.coordinate(space.asset(index));
    if index == 0 || index == space.count {
        return value_at(center);
    }
    let lower = space.coordinate(space.asset(index - 1)).midpoint(center);
    let upper = center.midpoint(space.coordinate(space.asset(index + 1)));
    let breaks: Vec<_> = breaks.iter().map(|&b| space.coordinate(b)).collect();
    if !breaks.iter().any(|&b| b > lower && b < upper) {
        return value_at(center);
    }
    let mut edges = vec![lower, center, upper];
    edges.extend(breaks.iter().copied().filter(|&b| b > lower && b < upper));
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    let mut result = [0.0; 2];
    for e in edges.windows(2) {
        let middle = e[0].midpoint(e[1]);
        let radius = (e[1] - e[0]) / (2.0 * 3.0_f64.sqrt());
        let low = value_at(middle - radius);
        let high = value_at(middle + radius);
        for layer in 0..2 {
            result[layer] += (e[1] - e[0]) / (upper - lower) * low[layer].midpoint(high[layer]);
        }
    }
    result
}

fn native<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    validate(note, context)?;
    let state = initial(note, context);
    let h = crate::risk::numerical::symmetric_shift(context.spot_price(), shifts.spot_shift);
    let spot_greeks = !note.at_spot_discontinuity(context, h);
    if let Some(value) = state.settlement {
        return PricingResult::new(value);
    }
    if let Some(value) = fixed_value(note, context, state.knocked_in) {
        let mut result = PricingResult::new(value + state.coupon)?;
        for greek in [Greek::Delta, Greek::Gamma] {
            if spot_greeks && request.has(greek) {
                result.set(request, greek, 0.0)?;
            }
        }
        return Ok(result);
    }
    let t = note.view();
    let daily = t.knock_in_observation_mode == KnockInObservationMode::EveryTradingDay;
    let layered = daily && !state.knocked_in && note.knock_in_affects_payoff();
    let model = context.model_parameters();
    let expiry = note.expiry_date().start_of_day();
    let time = context.valuation_time().years_until(expiry);
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let sigma = model.volatility();
    let mut observations: Vec<_> = note
        .observations()
        .iter()
        .filter(|r| r.date.start_of_day() > context.valuation_time())
        .map(|r| context.valuation_time().years_until(r.date.into()))
        .collect();
    let mut trading = Vec::new();
    if daily && (layered || settings.scheme == FiniteDifferenceScheme::ExplicitEuler) {
        let mut date = context.valuation_date();
        loop {
            if date.start_of_day() > context.valuation_time()
                && context.calendar().is_trading_day(date)
            {
                trading.push(context.valuation_time().years_until(date.into()));
            }
            if date == note.expiry_date() {
                break;
            }
            date = date.add_days(1)?;
        }
    }
    observations.extend_from_slice(&trading);
    let mut grid = TimeGrid::with_events(
        time,
        settings.time_step_count,
        &risk,
        &dividend,
        &observations,
    );
    let nearest = observations
        .iter()
        .copied()
        .chain(risk.ends())
        .chain(dividend.ends())
        .fold(time, f64::min);
    let near_steps = if layered { 64 } else { 128 };
    let intervals = grid.times.partition_point(|&v| v < nearest);
    if intervals < near_steps {
        let mut times: Vec<_> = (0..near_steps)
            .map(|i| nearest * i as f64 / near_steps as f64)
            .collect();
        times.extend_from_slice(&grid.times[intervals..]);
        let mut widths = vec![nearest / near_steps as f64; near_steps];
        widths.extend_from_slice(&grid.widths[intervals..]);
        grid = TimeGrid { times, widths };
    }
    let upper = match settings.asset_upper_boundary {
        Some(upper) => upper,
        None => default_upper(note, context)?,
    };
    let mut space = SpatialGrid::new(
        settings,
        upper,
        if upper > 4.0 * highest(note) {
            highest(note)
        } else {
            0.0
        },
        highest(note).max(context.spot_price()),
    )?;
    if settings.scheme != FiniteDifferenceScheme::ExplicitEuler {
        let mut levels = vec![t.initial_spot];
        if t.has_knock_in {
            levels.push(t.knock_in_level);
        }
        if t.fixed_knocked_in_coupon.is_none() {
            levels.extend([t.lower_strike, t.upper_strike]);
        }
        for r in note
            .observations()
            .iter()
            .filter(|r| r.date.start_of_day() > context.valuation_time())
        {
            levels.push(r.knock_out_level);
            if t.phoenix {
                levels.push(r.coupon_barrier_level);
            }
        }
        let scale =
            highest(note) * (0.001 * (nearest * 365.0).min(1.0).sqrt()).max(64.0 * f64::EPSILON);
        concentrate(&mut space, levels, scale)?;
    }
    let theta = settings.scheme.theta();
    grid.validate_barrier_stability(
        &space,
        sigma,
        theta,
        space.scale == 0.0 && space.nodes.is_empty(),
        &risk,
        &dividend,
    )?;
    let size = space.count + 1;
    let mut alive = vec![0.0; size];
    let mut knocked = vec![0.0; size];
    let mut next_alive = vec![0.0; size];
    let mut next_knocked = vec![0.0; size];
    let mut breaks = Vec::new();
    if t.has_knock_in {
        breaks.push(t.knock_in_level);
    }
    if t.fixed_knocked_in_coupon.is_none() {
        breaks.extend([t.lower_strike, t.upper_strike]);
    }
    let end_event = event(note, time, context);
    if let Some(r) = end_event {
        breaks.extend([r.knock_out_level, r.coupon_barrier_level]);
    }
    let payment = |asset, knocked_in, record: Option<&AutocallableObservation>| {
        let coupon = record.map_or(0.0, |r| note.observation_coupon(r, asset));
        coupon
            + if let Some(r) = record.filter(|r| asset >= r.knock_out_level) {
                note.knock_out_payment(r)
            } else {
                note.terminal(
                    asset,
                    knocked_in || t.has_knock_in && asset < t.knock_in_level,
                )
            }
    };
    for i in 0..size {
        let value = project(&space, &breaks, i, |asset| {
            [
                payment(asset, state.knocked_in, end_event),
                payment(asset, true, end_event),
            ]
        });
        alive[i] = value[0];
        knocked[i] = value[1];
    }
    let mut stepper = Stepper::new(space.count);
    let mut damping: usize = 2;
    for step in (0..grid.widths.len()).rev() {
        let damp = theta == 0.5 && damping > 0;
        damping = damping.saturating_sub(1);
        let count = if damp { 2 } else { 1 };
        let dt = grid.widths[step] / f64::from(count);
        for substep in 0..count {
            let current = grid.times[step] + dt * f64::from(count - substep - 1);
            let middle = current + 0.5 * dt;
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let key = [dt, if damp { 1.0 } else { theta }, rate, q];
            let discount = (-rate * dt).exp();
            stepper.advance(
                &alive,
                &mut next_alive,
                &space,
                sigma,
                key,
                [alive[0] * discount, alive[size - 1] * discount],
            )?;
            std::mem::swap(&mut alive, &mut next_alive);
            if layered {
                stepper.advance(
                    &knocked,
                    &mut next_knocked,
                    &space,
                    sigma,
                    key,
                    [knocked[0] * discount, knocked[size - 1] * discount],
                )?;
                std::mem::swap(&mut knocked, &mut next_knocked);
            }
        }
        let time = grid.times[step];
        let record = event(note, time, context).filter(|_| time > 0.0);
        let ki = layered && trading.binary_search_by(|v| v.total_cmp(&time)).is_ok();
        if record.is_some() || ki {
            breaks.clear();
            if ki {
                breaks.push(t.knock_in_level);
            }
            if let Some(r) = record {
                breaks.extend([r.knock_out_level, r.coupon_barrier_level]);
            }
            for i in 0..size {
                let correction = project(&space, &breaks, i, |asset| {
                    let a = space.linear(&alive, asset);
                    let k = if layered {
                        space.linear(&knocked, asset)
                    } else {
                        0.0
                    };
                    let coupon = record.map_or(0.0, |r| note.observation_coupon(r, asset));
                    if let Some(r) = record.filter(|r| asset >= r.knock_out_level) {
                        let payment = note.knock_out_payment(r) + coupon;
                        [payment - a, payment - k]
                    } else {
                        [
                            (if ki && asset < t.knock_in_level {
                                k - a
                            } else {
                                0.0
                            }) + coupon,
                            coupon,
                        ]
                    }
                });
                next_alive[i] = alive[i] + correction[0];
                if layered {
                    next_knocked[i] = knocked[i] + correction[1];
                }
            }
            std::mem::swap(&mut alive, &mut next_alive);
            if layered {
                std::mem::swap(&mut knocked, &mut next_knocked);
            }
            damping = 2;
        }
    }
    let curvature = space.curvature(&alive);
    let spot = context.spot_price();
    let [price, delta, gamma] = space.sample(&alive, &curvature, spot);
    let mut result = PricingResult::new(state.coupon + price)?;
    for (greek, value) in [(Greek::Delta, delta), (Greek::Gamma, gamma)] {
        if spot_greeks && request.has(greek) {
            result.set(request, greek, value)?;
        }
    }
    if spot_greeks && request.has(Greek::Speed) {
        let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift);
        if h > 0.0 && spot > h && spot + h < upper {
            let up = space.sample(&alive, &curvature, spot + h)[2];
            let down = space.sample(&alive, &curvature, spot - h)[2];
            result.set(request, Greek::Speed, (up - down) / (2.0 * h))?;
        }
    }
    Ok(result)
}
