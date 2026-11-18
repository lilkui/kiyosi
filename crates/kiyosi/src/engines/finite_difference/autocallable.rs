use super::{
    Curve, Diffusion, FiniteDifferenceAutocallableEngine, FiniteDifferenceScheme,
    FiniteDifferenceSettings, Greek, GreeksRequest, NumericalShiftSettings, OptionContract,
    PricingContext, PricingEngine, PricingResult, Result, SpatialGrid, TimeGrid, analysis,
    diffusion, diffusion_upper, grid::project_in_coordinates, local_volatility, validate_work,
};
use crate::engines::deterministic::{self, continuation_context};
use crate::instruments::autocallable::Coupon;
use crate::instruments::autocallable::ObservationEvent;
use crate::instruments::autocallable::pricing::{
    event_times_through, expiry_payment, fixed_value_at, validate,
};
use crate::numerics::{scaled_cash, scaled_signed_cash};
use crate::{AutocallableNote, AutocallableTerms, Timestamp};

impl FiniteDifferenceAutocallableEngine {
    /// Prices remaining snowball or Phoenix cash flows on exact trading and observation dates.
    ///
    /// Accepts flat or nonconstant rate curves, retains daily knock-in layers and projects future events.
    /// Current events and events before variance starts use the deterministic spot path,
    /// with strict knock-in and inclusive knock-out/coupon levels.
    /// Accepts BSM or local volatility. Surface coverage is required only for remaining
    /// stochastic evolution, after settlement and fixed cash flows are resolved.
    /// See [`Self::price`] for an example.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or curve coverage, for
    /// impossible history, clipped
    /// grids, or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure].
    ///
    /// # Greeks
    ///
    /// Computes spline spot Greeks and completes other sensitivities with core bump defaults.
    ///
    /// Native delta/gamma do not require feasible spot bumps; speed requires a symmetric
    /// in-domain stencil. Fixes the central upper domain for bumps. At expiry Greeks
    /// follow terminal payoffs, with time Greeks absent. Event-crossing stencils
    /// are unavailable; before expiry BSM vega and rho are available at current thresholds.
    /// Local volatility supports only delta and gamma with the absolute surface fixed.
    ///
    /// ## Errors
    ///
    /// Validates shifts, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors without changing inputs or settings.
    /// Unsupported local-volatility Greeks are absent.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.02, start, end,
    ///     autocallable_observations(&[end], &[100.0], Some(&[0.08]), None)?, kiyosi::TradingCalendar::weekdays()))?;
    /// let market = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceAutocallableEngine::default();
    /// let result = engine.price_with_greeks(&note, &market,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]))?;
    /// assert_eq!(result.price(), engine.price(&note, &market)?);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
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
}
impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>>
    for FiniteDifferenceAutocallableEngine
{
    fn greek_method(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        local_volatility::greek_method(option, context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn volatility_affects_price(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> bool {
        let now = context.valuation_time();
        if let Ok(start) = context
            .model_parameters()
            .variance_time_convention()
            .variance_start(now, note.expiry_time())
        {
            if start > now
                && deterministic::autocallable_through(note, context, start).is_ok_and(|state| {
                    state.settlement.is_some()
                        || fixed_value_at(note, context, start, state.knocked_in).is_some()
                })
            {
                return false;
            }
        }
        note.volatility_affects_price(context)
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks_with_settings(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<AutocallableNote<T>>>::validate_applicability(self, context)?;
        analysis(option, context, self.settings, request, shifts, native)
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
    note.events().iter().fold(level, |level, r| {
        level
            .max(if r.has_knock_out() {
                r.knock_out_level()
            } else {
                0.0
            })
            .max(r.coupon_barrier_level().unwrap_or(0.0))
    })
}

fn event<'a, T: AutocallableTerms>(
    note: &'a AutocallableNote<T>,
    time: f64,
    context: &PricingContext,
) -> Option<&'a ObservationEvent> {
    let observations = note.events();
    observations
        .binary_search_by(|r| {
            context
                .valuation_time()
                .years_until(r.time())
                .total_cmp(&time)
        })
        .ok()
        .map(|index| &observations[index])
}

fn native<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<(PricingResult, Option<f64>)> {
    let request = local_volatility::validate_request(context, request)?;
    if deterministic::enabled_until(context, note.expiry_time())? {
        return Ok((
            PricingResult::new(deterministic::autocallable(note, context)?)?,
            None,
        ));
    }
    validate(note, context)?;
    let start = context.deterministic_until(note.expiry_time())?;
    let state = deterministic::autocallable_through(note, context, start)?;
    if let Some(value) = state.settlement {
        return Ok((PricingResult::new(value)?, None));
    }
    let mut grid = None;
    if fixed_value_at(note, context, start, state.knocked_in).is_none() {
        let (rolled, _, _) = continuation_context(context, start)?;
        let rolled = rolled.as_ref();
        grid = Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
            note,
            rolled,
            settings,
            diffusion,
            state.knocked_in
        ))?);
    }
    let upper = grid.as_ref().map(|grid| grid.space.upper);
    let result = AutocallableState { start, grid }.evaluate(note, context, request, shifts)?;
    Ok((result, upper))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Validated grid step counts fit exactly in f64."
)]
fn solve_grid<T: AutocallableTerms, D: Diffusion>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    diffusion: D,
    starting_knocked: bool,
) -> Result<AutocallableGrid> {
    let t = note.view();
    let daily = t.daily_knock_in();
    let layered = daily && !starting_knocked && note.knock_in_affects_payoff();
    let model = context.model_parameters();
    let expiry = note.expiry_time();
    let time = context.valuation_time().years_until(expiry);
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let event_times =
        crate::instruments::autocallable::pricing::future_event_times(note, context, daily)?;
    let mut observations: Vec<_> = note
        .events()
        .iter()
        .filter(|r| r.time() > context.valuation_time())
        .map(|r| context.valuation_time().years_until(r.time()))
        .collect();
    let mut trading = Vec::new();
    if daily && (layered || settings.scheme == FiniteDifferenceScheme::ExplicitEuler) {
        trading.extend(
            event_times
                .iter()
                .copied()
                .filter(|&time| note.daily_time(time, note.calendar()))
                .map(|time| context.valuation_time().years_until(time)),
        );
    }
    observations.extend_from_slice(&trading);
    diffusion::validate_event_resolution(context.valuation_time(), expiry, &event_times)?;
    let mut grid =
        diffusion::time_grid(context, expiry, settings, &risk, &dividend, &observations)?;
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
    nearest = context
        .model_parameters()
        .variance_time_convention()
        .transitions(context.valuation_time(), expiry)?
        .into_iter()
        .map(|t| context.valuation_time().years_until(t))
        .fold(nearest, f64::min);
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
    let layers = if layered { 2 } else { 1 };
    validate_work(
        u64::from(settings.asset_step_count.unsigned_abs()) * layers,
        grid.widths.len() as u64,
    )?;
    let highest = highest(note);
    let upper = match settings.asset_upper_boundary {
        Some(upper) => upper,
        None => diffusion_upper(
            highest.max(context.spot_price()),
            context,
            time,
            &risk,
            &dividend,
        )?,
    };
    let mut space = SpatialGrid::new(
        settings,
        upper,
        if upper > 4.0 * highest { highest } else { 0.0 },
        highest.max(context.spot_price()),
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
            .events()
            .iter()
            .filter(|r| r.time() > context.valuation_time())
        {
            if r.has_knock_out() {
                levels.push(r.knock_out_level());
            }
            if matches!(t.coupon, Coupon::Phoenix(_)) {
                levels.push(r.coupon_barrier_level().unwrap_or(0.0));
            }
        }
        let scale = highest * (0.001 * (nearest * 365.0).min(1.0).sqrt()).max(64.0 * f64::EPSILON);
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
    let knock_in_size = if layered { size } else { 0 };
    let mut alive = vec![0.0; size];
    let mut knocked = vec![0.0; knock_in_size];
    let mut next_alive = vec![0.0; size];
    let mut next_knocked = vec![0.0; knock_in_size];
    let mut breaks = Vec::new();
    if let Some(ki) = t.knock_in {
        breaks.push(ki.level);
    }
    if let Some(d) = t.downside() {
        breaks.extend([d.lower_strike, d.upper_strike]);
    }
    let end_event = event(note, time, context);
    if let Some(r) = end_event {
        if r.has_knock_out() {
            breaks.push(r.knock_out_level());
        }
        breaks.push(r.coupon_barrier_level().unwrap_or(0.0));
    }
    for i in 0..size {
        let value = project_in_coordinates(&space, &breaks, i, |asset| {
            [
                expiry_payment(note, asset, starting_knocked, end_event),
                if layered {
                    expiry_payment(note, asset, true, end_event)
                } else {
                    0.0
                },
            ]
        });
        alive[i] = value[0];
        if layered {
            knocked[i] = value[1];
        }
    }
    let mut stepper = diffusion.stepper(space.count);
    let mut damping: u32 = 2;
    for step in (0..grid.widths.len()).rev() {
        crate::cancellation::check()?;
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
                if r.has_knock_out() {
                    breaks.push(r.knock_out_level());
                }
                breaks.push(r.coupon_barrier_level().unwrap_or(0.0));
            }
            // Project only the jump correction, leaving smooth daily continuation unchanged.
            for i in 0..size {
                let correction = project_in_coordinates(&space, &breaks, i, |asset| {
                    let a = space.linear(&alive, asset);
                    let k = if layered {
                        space.linear(&knocked, asset)
                    } else {
                        0.0
                    };
                    let coupon = record.map_or(0.0, |r| note.observation_coupon(r, asset));
                    if let Some(r) =
                        record.filter(|r| r.has_knock_out() && asset >= r.knock_out_level())
                    {
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
    let curvatures = [
        space.curvature(&alive),
        if layered {
            space.curvature(&knocked)
        } else {
            Vec::new()
        },
    ];
    Ok(AutocallableGrid {
        space,
        layers: [alive, knocked],
        curvatures,
        starting_knocked,
    })
}

#[derive(PartialEq)]
struct AutocallableGrid {
    space: SpatialGrid,
    layers: [Vec<f64>; 2],
    curvatures: [Vec<f64>; 2],
    starting_knocked: bool,
}

#[derive(PartialEq)]
struct AutocallableState {
    start: Timestamp,
    grid: Option<AutocallableGrid>,
}
impl AutocallableState {
    fn evaluate<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        if self.start == note.expiry_time() && deterministic::enabled_until(context, self.start)? {
            return PricingResult::new(deterministic::autocallable(note, context)?);
        }
        let state = deterministic::autocallable_through(note, context, self.start)?;
        let h = crate::risk::numerical::symmetric_shift(
            context.spot_price(),
            shifts.spot_shift_at(context.spot_price()),
        );
        let spot_greeks =
            request.has(Greek::Delta) || request.has(Greek::Gamma) || request.has(Greek::Speed);
        let spot_greeks = spot_greeks && !note.at_spot_discontinuity(context, h);
        if let Some(value) = state.settlement {
            return PricingResult::new(value);
        }
        if let Some(value) = fixed_value_at(note, context, self.start, state.knocked_in) {
            let discount = deterministic::discount(context, self.start)?;
            let mut result =
                PricingResult::new(scaled_signed_cash(value, discount) + state.coupon)?;
            for greek in [Greek::Delta, Greek::Gamma] {
                if spot_greeks {
                    result.set(request, greek, 0.0)?;
                }
            }
            return Ok(result);
        }
        let (rolled, carry, discount) = continuation_context(context, self.start)?;
        let grid = self.grid.as_ref().ok_or_else(super::unstable)?;
        let layer =
            usize::from(state.knocked_in && !grid.starting_knocked && !grid.layers[1].is_empty());
        let alive = &grid.layers[layer];
        let curvature = &grid.curvatures[layer];
        let space = &grid.space;
        let spot = rolled.spot_price();
        let [price, delta, gamma] = space.sample(alive, curvature, spot);
        let mut result = PricingResult::new(state.coupon + scaled_signed_cash(price, discount))?;
        for (greek, value, powers) in [(Greek::Delta, delta, 1.0), (Greek::Gamma, gamma, 2.0)] {
            if spot_greeks && request.has(greek) {
                result.set(
                    request,
                    greek,
                    scaled_signed_cash(value, discount + powers * carry),
                )?;
            }
        }
        if spot_greeks && request.has(Greek::Speed) {
            let h = crate::risk::numerical::symmetric_shift(spot, scaled_cash(h, carry));
            if let Some(speed) = space.speed(alive, curvature, spot, h) {
                result.set(
                    request,
                    Greek::Speed,
                    scaled_signed_cash(speed, discount + 3.0 * carry),
                )?;
            }
        }
        Ok(result)
    }
}

impl<T: AutocallableTerms> super::solution::Backend<AutocallableNote<T>> for AutocallableState {
    fn evaluate(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.evaluate(note, context, request, shifts)
    }
}
impl<T: AutocallableTerms> super::FiniteDifferenceEngine<AutocallableNote<T>>
    for FiniteDifferenceAutocallableEngine
{
    fn solve(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        range: std::ops::RangeInclusive<f64>,
    ) -> Result<super::FiniteDifferenceSolution> {
        local_volatility::validate_model(context)?;
        super::solution::validate_range(&range)?;
        crate::cancellation::check()?;
        let anchor = super::solution::at_spot(context, *range.end())?;
        validate(note, &anchor)?;
        let now = context.valuation_time();
        let start = context.deterministic_until(note.expiry_time())?;
        let low = super::solution::at_spot(context, *range.start())?;
        let mut levels = vec![note.view().reference_spot()];
        let mut pending = false;
        let mut starting_knocked = true;
        if start < note.expiry_time()
            && deterministic::autocallable_through(note, &low, start)?
                .settlement
                .is_none()
        {
            if let Some(ki) = note.view().knock_in {
                levels.push(ki.level);
                if start > now && note.view().daily_knock_in() {
                    for time in event_times_through(note, context, true, start)? {
                        if note.daily_time(time, note.calendar()) {
                            levels.push(scaled_cash(
                                ki.level,
                                -deterministic::log_return(context, time)?,
                            ));
                        }
                    }
                }
            }
            let events = note.events();
            for record in events[events.partition_point(|r| r.time() < now)..]
                .iter()
                .take_while(|r| r.time() <= start)
            {
                if record.has_knock_out() {
                    levels.push(scaled_cash(
                        record.knock_out_level(),
                        -deterministic::log_return(context, record.time())?,
                    ));
                }
            }
            for spot in super::solution::probes(&range, levels) {
                let probe = super::solution::at_spot(context, spot)?;
                let state = deterministic::autocallable_through(note, &probe, start)?;
                if state.settlement.is_none()
                    && fixed_value_at(note, &probe, start, state.knocked_in).is_none()
                {
                    pending = true;
                    starting_knocked &= state.knocked_in;
                }
            }
        }
        let grid = if pending {
            let (rolled, _, _) = continuation_context(&anchor, start)?;
            let rolled = rolled.as_ref();
            Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
                note,
                rolled,
                self.settings,
                diffusion,
                starting_knocked
            ))?)
        } else {
            None
        };
        let state = AutocallableState { start, grid };
        state.evaluate(note, &anchor, GreeksRequest::new([]), Default::default())?;
        crate::cancellation::check()?;
        Ok(super::FiniteDifferenceSolution::new(
            note.clone(),
            context,
            range,
            state,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BinarySnowballOption, BinarySnowballTerms, BlackScholesMertonParameters, Date,
        TradingCalendar, autocallable_observations,
    };

    #[test]
    fn events_match_only_exact_observation_times() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let times = [
            start.add_micros(11)?,
            start.add_micros(13)?,
            start.add_micros(17)?,
        ];
        let note = BinarySnowballOption::new(BinarySnowballTerms::new(
            0.02,
            start,
            start.add_micros(20)?,
            autocallable_observations(&times, &[110.0; 3], Some(&[0.08; 3]), None)?,
            TradingCalendar::all_days(),
        ))?;
        for valuation in [start, times[0], start.add_micros(12)?] {
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
                100.0,
                valuation,
            )?;
            for micros in 0..=20 {
                let query = start.add_micros(micros)?;
                let expected = match micros {
                    11 | 13 | 17 => Some(query),
                    _ => None,
                };
                assert_eq!(
                    event(&note, valuation.years_until(query), &context)
                        .map(ObservationEvent::time),
                    expected,
                    "valuation={valuation:?}, query={query:?}",
                );
            }
        }
        Ok(())
    }
}
