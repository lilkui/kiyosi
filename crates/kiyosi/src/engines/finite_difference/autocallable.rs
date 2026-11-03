use super::{
    Curve, Diffusion, FiniteDifferenceAutocallableEngine, FiniteDifferenceScheme,
    FiniteDifferenceSettings, Greek, GreeksRequest, LocalDiffusion, NumericalShiftSettings,
    OptionContract, PricingContext, PricingEngine, PricingResult, Result, SpatialGrid, TimeGrid,
    analysis, diffusion, diffusion_upper, local_volatility,
};
use crate::instruments::autocallable::Coupon;
use crate::instruments::autocallable::pricing::{
    fixed_value, initial, validate, volatility_affects_price,
};
use crate::{AutocallableNote, AutocallableObservation, AutocallableTerms};

impl FiniteDifferenceAutocallableEngine {
    /// Prices remaining snowball or Phoenix cash flows on exact trading and observation dates.
    ///
    /// Accepts flat or node-based rate curves, retains daily knock-in layers and projects future events.
    /// Current events use actual spot with strict knock-in and inclusive knock-out/coupon levels.
    /// Accepts BSM or local volatility. Surface coverage is required only for remaining
    /// stochastic evolution, after settlement and fixed cash flows are resolved.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or curve coverage, for
    /// nontrading observations or expiry, missing or impossible history, clipped
    /// grids, or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure]. Inputs remain unchanged.
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        Ok(native(
            option,
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
        )?
        .0
        .price())
    }

    /// Computes spline spot Greeks and completes other sensitivities with core bump defaults.
    ///
    /// Native delta/gamma do not require feasible spot bumps; speed requires a symmetric
    /// in-domain stencil. Fixes the central upper domain for bumps. Expiry Greeks and event-crossing stencils
    /// are absent; BSM vega and rho remain available at current thresholds.
    /// Local volatility supports only delta and gamma with the absolute surface fixed.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors without changing inputs or settings.
    /// Unsupported local-volatility Greeks remain absent.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
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
        analysis(option, context, self.settings, request, shifts, native)
    }
}
impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>>
    for FiniteDifferenceAutocallableEngine
{
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
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
        volatility_affects_price(option, context)
    }
}

fn highest<T: AutocallableTerms>(note: &AutocallableNote<T>) -> f64 {
    let t = note.view();
    let mut level = t.reference_spot();
    if let Some(ki) = t.knock_in {
        level = level.max(ki.level);
    }
    if let Some(d) = t.downside() {
        level = level.max(d.upper_strike).max(d.lower_strike);
    }
    note.observations().iter().fold(level, |level, r| {
        level.max(r.knock_out_level).max(r.coupon_barrier_level)
    })
}
fn default_upper<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    diffusion_upper(
        highest(note).max(context.spot_price()),
        context,
        context
            .valuation_time()
            .years_until(note.expiry_date().into()),
        risk,
        dividend,
    )
}

#[expect(
    clippy::float_cmp,
    reason = "Event times are recomputed identically from the same timestamps and must match exactly."
)]
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
    let coordinates = || breaks.iter().map(|&b| space.coordinate(b));
    if !coordinates().any(|b| b > lower && b < upper) {
        return value_at(center);
    }
    let mut edges = vec![lower, center, upper];
    edges.extend(coordinates().filter(|&b| b > lower && b < upper));
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
) -> Result<(PricingResult, Option<f64>)> {
    let request = local_volatility::validate_request(context, request)?;
    match context.model_parameters() {
        crate::ModelParameters::Heston(_) => Err(crate::Error::UnsupportedOperation(
            "finite differences do not support Heston".into(),
        )),
        crate::ModelParameters::BlackScholesMerton(model) => {
            native_with_diffusion(note, context, settings, request, shifts, model.volatility())
        }
        crate::ModelParameters::LocalVolatility(model) => native_with_diffusion(
            note,
            context,
            settings,
            request,
            shifts,
            LocalDiffusion::new(context, model.surface()),
        ),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Near-barrier step counts are bounded by the validated i32 grid size and fit exactly in f64."
)]
fn native_with_diffusion<T: AutocallableTerms, D: Diffusion>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    diffusion: D,
) -> Result<(PricingResult, Option<f64>)> {
    validate(note, context)?;
    let state = initial(note, context);
    let h = crate::risk::numerical::symmetric_shift(context.spot_price(), shifts.spot_shift());
    let spot_greeks = !note.at_spot_discontinuity(context, h);
    if let Some(value) = state.settlement {
        return Ok((PricingResult::new(value)?, None));
    }
    if let Some(value) = fixed_value(note, context, state.knocked_in) {
        let mut result = PricingResult::new(value + state.coupon)?;
        for greek in [Greek::Delta, Greek::Gamma] {
            if spot_greeks && request.has(greek) {
                result.set(request, greek, 0.0)?;
            }
        }
        return Ok((result, None));
    }
    let t = note.view();
    let daily = t.daily_knock_in();
    let layered = daily && !state.knocked_in && note.knock_in_affects_payoff();
    let model = context.model_parameters();
    let expiry = note.expiry_date().start_of_day();
    let time = context.valuation_time().years_until(expiry);
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
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
    let mut grid = diffusion::time_grid(
        context,
        expiry,
        settings.time_step_count,
        &risk,
        &dividend,
        &observations,
    )?;
    let mut nearest = observations
        .iter()
        .copied()
        .chain(risk.ends())
        .chain(dividend.ends())
        .fold(time, f64::min);
    if let Some(model) = local_volatility::local_model(context) {
        // Preserve surface knots when refining the interval nearest valuation.
        nearest = model
            .surface()
            .times()
            .iter()
            .filter(|&&t| t > context.valuation_time() && t < expiry)
            .map(|&t| context.valuation_time().years_until(t))
            .fold(nearest, f64::min);
    }
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
        None => default_upper(note, context, &risk, &dividend)?,
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
        let mut levels = vec![t.reference_spot()];
        if let Some(ki) = t.knock_in {
            levels.push(ki.level);
        }
        if let Some(d) = t.downside() {
            levels.extend([d.lower_strike, d.upper_strike]);
        }
        for r in note
            .observations()
            .iter()
            .filter(|r| r.date.start_of_day() > context.valuation_time())
        {
            levels.push(r.knock_out_level);
            if matches!(t.coupon, Coupon::Phoenix(_)) {
                levels.push(r.coupon_barrier_level);
            }
        }
        let scale =
            highest(note) * (0.001 * (nearest * 365.0).min(1.0).sqrt()).max(64.0 * f64::EPSILON);
        super::grid::concentrate(&mut space, levels, scale)?;
    }
    if !D::LOCAL {
        grid.validate_stability(
            &space,
            diffusion.sigma(0.0, 0.0),
            settings.scheme.theta(),
            space.scale == 0.0 && space.nodes.is_empty(),
            &risk,
            &dividend,
        )?;
    }
    let size = space.count + 1;
    let mut alive = vec![0.0; size];
    let mut knocked = vec![0.0; size];
    let mut next_alive = vec![0.0; size];
    let mut next_knocked = vec![0.0; size];
    let mut breaks = Vec::new();
    if let Some(ki) = t.knock_in {
        breaks.push(ki.level);
    }
    if let Some(d) = t.downside() {
        breaks.extend([d.lower_strike, d.upper_strike]);
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
                note.terminal(asset, knocked_in || t.below_knock_in(asset))
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
    let mut stepper = diffusion.stepper(space.count);
    let mut damping: u32 = 2;
    for step in (0..grid.widths.len()).rev() {
        for (current, dt, weight) in grid.substeps(step, settings.scheme, &mut damping) {
            let middle = current + 0.5 * dt;
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let key = [dt, weight, rate, q];
            let discount = (-rate * dt).exp();
            diffusion.advance(
                &mut stepper,
                &alive,
                &mut next_alive,
                &space,
                middle,
                key,
                [alive[0] * discount, alive[size - 1] * discount],
            )?;
            std::mem::swap(&mut alive, &mut next_alive);
            if layered {
                diffusion.advance(
                    &mut stepper,
                    &knocked,
                    &mut next_knocked,
                    &space,
                    middle,
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
            if let Some(ki) = t.knock_in.filter(|_| ki) {
                breaks.push(ki.level);
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
                            (if ki && t.below_knock_in(asset) {
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
        let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift());
        if h > 0.0 && spot > h && spot + h < upper {
            let up = space.sample(&alive, &curvature, spot + h)[2];
            let down = space.sample(&alive, &curvature, spot - h)[2];
            result.set(request, Greek::Speed, (up - down) / (2.0 * h))?;
        }
    }
    Ok((result, Some(upper)))
}
