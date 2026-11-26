use super::{
    Diffusion, FiniteDifferenceAutocallableEngine, FiniteDifferenceScheme,
    FiniteDifferenceSettings, Greek, GreeksRequest, NumericalShiftSettings, OptionContract,
    PricingContext, PricingEngine, PricingResult, Result, SpatialGrid, TimeGrid, analysis,
    diffusion, diffusion_upper,
    grid::{Spline, project_grid_node, project_in_coordinates},
    local_volatility,
    solution::Backend,
    validate_work, validate_workspace,
};
use crate::engines::deterministic::{self, continuation_context};
use crate::instruments::autocallable::ObservationEvent;
use crate::instruments::autocallable::pricing::{
    Initial, expiry_payment, fixed_value_at, validate,
};
use crate::instruments::monitoring::validate_life;
use crate::numerics::{scaled_cash, scaled_signed_cash};
use crate::risk::analytics::CouponPricer;
use crate::{AutocallableBarrierState, AutocallableNote, AutocallableTerms, Timestamp};

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
    ///     autocallable_observations(&[end], &[100.0], &[0.08])?, kiyosi::TradingCalendar::weekdays()))?;
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
    fn prepare_coupon_pricer<'a>(
        &'a self,
        lower: &AutocallableNote<T>,
        upper: &AutocallableNote<T>,
        context: &'a PricingContext,
    ) -> Result<Option<CouponPricer<'a, AutocallableNote<T>>>> {
        local_volatility::validate_model(context)?;
        validate(lower, context)?;
        validate(upper, context)?;
        let expiry = lower.expiry_time();
        if deterministic::enabled_until(context, expiry)? {
            return Ok(None);
        }
        let start = context.deterministic_until(expiry)?;
        let continuing = move |note: &AutocallableNote<T>| -> Result<Option<Initial>> {
            let state = deterministic::autocallable_through(note, context, start)?;
            Ok((state.settlement.is_none()
                && fixed_value_at(note, context, start, state.knocked_in).is_none())
            .then_some(state))
        };
        let (Some(low), Some(high)) = (continuing(lower)?, continuing(upper)?) else {
            return Ok(None);
        };
        let affected = lower.knock_in_affects_payoff();
        if low.knocked_in != high.knocked_in || affected != upper.knock_in_affects_payoff() {
            return Ok(None);
        }
        let (rolled, _, discount) = continuation_context(context, start)?;
        let rolled = rolled.as_ref();
        let spot = rolled.spot_price();
        let (space, [[lower_values, _], [upper_values, _]]) =
            diffusion::with_diffusion!(rolled, |diffusion| solve_layers(
                [lower, upper],
                rolled,
                self.settings,
                diffusion,
                low.knocked_in,
                spot,
            ))?;
        Ok(Some(Box::new(move |note, fraction| {
            let Some(state) = continuing(note)? else {
                return Ok(None);
            };
            if state.knocked_in != low.knocked_in || note.knock_in_affects_payoff() != affected {
                return Ok(None);
            }
            let mut values = Vec::with_capacity(lower_values.len());
            for (index, (&lo, &hi)) in lower_values.iter().zip(&upper_values).enumerate() {
                if index % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                let value = if fraction <= 0.0 {
                    lo
                } else if fraction >= 1.0 {
                    hi
                } else {
                    lo * (1.0 - fraction) + hi * fraction
                };
                values.push(value);
            }
            let spline = space.spline(values);
            let price = space.sample(&spline, spot)[0];
            Ok(Some(crate::numerics::finite_price(
                state.coupon + scaled_signed_cash(price, discount),
            )?))
        })))
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
    validate_life(note, context)?;
    if deterministic::enabled_until(context, note.expiry_time())? {
        return Ok((
            PricingResult::new(deterministic::autocallable(note, context)?)?,
            None,
        ));
    }
    note.prior(context.valuation_time())?;
    let start = context.deterministic_until(note.expiry_time())?;
    let state = deterministic::autocallable_through(note, context, start)?;
    if let Some(value) = state.settlement {
        return Ok((PricingResult::new(value)?, None));
    }
    let fixed = fixed_value_at(note, context, start, state.knocked_in);
    crate::cancellation::check()?;
    let grid = if fixed.is_none() {
        let (rolled, _, _) = continuation_context(context, start)?;
        let rolled = rolled.as_ref();
        Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
            note,
            rolled,
            settings,
            diffusion,
            state.knocked_in,
            rolled.spot_price()
        ))?)
    } else {
        None
    };
    let upper = grid.as_ref().map(|grid| grid.space.upper);
    let result = AutocallableState { start, grid }
        .evaluate_after_observations(note, context, &state, fixed, request, shifts)?;
    Ok((result, upper))
}

fn solve_grid<T: AutocallableTerms, D: Diffusion>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    diffusion: D,
    starting_knocked: bool,
    minimum_spot: f64,
) -> Result<AutocallableGrid> {
    let (space, [[alive, knocked]]) = solve_layers(
        [note],
        context,
        settings,
        diffusion,
        starting_knocked,
        minimum_spot,
    )?;
    let alive = space.spline(alive);
    let knocked = (!knocked.is_empty()).then(|| space.spline(knocked));
    Ok(AutocallableGrid {
        space,
        alive,
        knocked,
        starting_knocked,
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Validated grid step counts fit exactly in f64."
)]
fn solve_layers<T: AutocallableTerms, D: Diffusion, const N: usize>(
    notes: [&AutocallableNote<T>; N],
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    diffusion: D,
    starting_knocked: bool,
    minimum_spot: f64,
) -> Result<(SpatialGrid, [[Vec<f64>; 2]; N])> {
    let note = notes[0];
    let t = note.view();
    let daily = t.daily_knock_in();
    let layered = daily && !starting_knocked && note.knock_in_affects_payoff();
    let expiry = note.expiry_time();
    let time = context.valuation_time().years_until(expiry);
    let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
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
    observations.sort_unstable_by(f64::total_cmp);
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
        validate_workspace(
            settings.scheme,
            u64::from(settings.asset_step_count.unsigned_abs()) + 1,
            (grid.widths.len() - intervals + near_steps) as u64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )?;
        let mut times: Vec<_> = (0..near_steps)
            .map(|i| nearest * i as f64 / near_steps as f64)
            .collect();
        times.extend_from_slice(&grid.times[intervals..]);
        let mut widths = vec![nearest / near_steps as f64; near_steps];
        widths.extend_from_slice(&grid.widths[intervals..]);
        grid = TimeGrid { times, widths };
    }
    let layers = 1 + usize::from(layered);
    validate_work(
        u64::from(settings.asset_step_count.unsigned_abs()) * layers as u64 * N as u64,
        grid.rollback_steps(settings.scheme, &observations)?,
    )?;
    if N > 1 {
        validate_workspace(
            settings.scheme,
            (u64::from(settings.asset_step_count.unsigned_abs()) + 1) * N as u64,
            grid.widths.len() as u64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )?;
    }
    let mut levels = Vec::new();
    if let Some(ki) = t.knock_in {
        levels.extend([t.reference_spot(), ki.level]);
    }
    if let Some(d) = t.downside() {
        levels.extend([d.lower_strike, d.upper_strike]);
    }
    let events = note.events();
    let first = events.partition_point(|r| r.time() <= context.valuation_time());
    for r in &events[first..] {
        crate::cancellation::check()?;
        if r.has_knock_out() {
            levels.push(r.knock_out_level());
        }
        levels.extend(r.coupon_barrier_level());
    }
    let highest = levels
        .iter()
        .copied()
        .reduce(f64::max)
        .unwrap_or_else(|| context.spot_price());
    let width = super::diffusion_log_width(context, expiry, &risk, &dividend)?;
    let relevant = highest.max(context.spot_price());
    let upper = settings
        .asset_upper_boundary
        .unwrap_or_else(|| diffusion_upper(relevant, width));
    if upper <= relevant {
        return Err(super::invalid_grid(
            "finite-difference upper bound must exceed spot and contract levels",
        ));
    }
    let lowest = levels
        .iter()
        .copied()
        .filter(|level| *level > 0.0)
        .reduce(f64::min)
        .unwrap_or(highest);
    let mut lower = super::grid::spot_floor(
        minimum_spot,
        lowest,
        (1.5 * width).max(std::f64::consts::LN_2),
    );
    if !D::LOCAL && !layered && t.knock_in.is_none() && event_times.iter().all(|&at| at == expiry) {
        // A very distant terminal fixing has no representable lower-tail contribution.
        let distance = crate::numerics::log_ratio(minimum_spot, highest);
        let sigma_width = diffusion.sigma(0.0, minimum_spot) * time.sqrt();
        let carry = dividend.log_factor(0.0, time) - risk.log_factor(0.0, time);
        if 0.5 * distance > 50.0 * sigma_width + sigma_width * sigma_width + carry.abs() {
            lower = super::scaled(minimum_spot, -0.5 * distance);
        }
    }
    let mut space = SpatialGrid::new(settings.asset_step_count, [lower, upper], highest)?;
    if settings.scheme != FiniteDifferenceScheme::ExplicitEuler {
        // Avoid resolving a tiny fraction of the diffusion width and amplifying roundoff in gamma.
        let diffusion_scale = 0.01 * diffusion.sigma(0.0, highest) * nearest.sqrt();
        let scale = (0.001 * (nearest * 365.0).min(1.0).sqrt())
            .max(diffusion_scale)
            .max(64.0 * f64::EPSILON);
        super::grid::concentrate(&mut space, levels, scale)?;
    }
    if !D::LOCAL {
        grid.validate_stability(
            &space,
            diffusion.sigma(0.0, 0.0),
            settings.scheme.theta(),
            &risk,
            &dividend,
        )?;
    }
    let size = space.count + 1;
    let knock_in_size = if layered { size } else { 0 };
    let buffers = || std::array::from_fn(|_| [vec![0.0; size], vec![0.0; knock_in_size]]);
    let mut values = buffers();
    let mut next = buffers();
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
    for (note, [alive, knocked]) in notes.into_iter().zip(&mut values) {
        let end_event = event(note, time, context);
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
            let dividend_discount = (-q * dt).exp();
            let mut old: [[&[f64]; 2]; N] = [[&[]; 2]; N];
            let mut outputs: [[&mut [f64]; 2]; N] =
                std::array::from_fn(|_| std::array::from_fn(|_| &mut [] as &mut [f64]));
            let mut edges = [[[0.0; 2]; 2]; N];
            let mut count = 0;
            for (value, output) in values.iter().flatten().zip(next.iter_mut().flatten()) {
                if !value.is_empty() {
                    old.as_flattened_mut()[count] = value;
                    outputs.as_flattened_mut()[count] = output;
                    edges.as_flattened_mut()[count] =
                        space.discount_affine_edges(value, discount, dividend_discount);
                    count += 1;
                }
            }
            diffusion.advance_batch(
                &mut stepper,
                &old.as_flattened()[..count],
                &mut outputs.as_flattened_mut()[..count],
                &space,
                middle,
                key,
                &edges.as_flattened()[..count],
            )?;
            std::mem::swap(&mut values, &mut next);
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
            for ((note, [alive, knocked]), [next_alive, next_knocked]) in
                notes.into_iter().zip(&values).zip(&mut next)
            {
                let record = event(note, time, context).filter(|_| time > 0.0);
                // Project only the jump correction, leaving smooth daily continuation unchanged.
                for i in 0..size {
                    let correction = project_grid_node(&space, &breaks, i, |asset, node| {
                        let value = |layer: &[f64]| {
                            node.map_or_else(|| space.linear(layer, asset), |index| layer[index])
                        };
                        let a = value(alive);
                        let k = if layered { value(knocked) } else { 0.0 };
                        let coupon = record
                            .map_or(0.0, |r| note.observation_coupon(r, |level| asset >= level));
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
            }
            std::mem::swap(&mut values, &mut next);
            damping = 2;
        }
    }
    Ok((space, values))
}

#[derive(PartialEq)]
struct AutocallableGrid {
    space: SpatialGrid,
    alive: Spline,
    knocked: Option<Spline>,
    starting_knocked: bool,
}

#[derive(PartialEq)]
struct AutocallableState {
    start: Timestamp,
    grid: Option<AutocallableGrid>,
}
impl AutocallableState {
    fn evaluate_after_observations<T: AutocallableTerms>(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        state: &Initial,
        fixed: Option<f64>,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let h = crate::risk::numerical::symmetric_shift(
            context.spot_price(),
            shifts.spot_shift_at(context.spot_price()),
        );
        let spot_greeks =
            request.has(Greek::Delta) || request.has(Greek::Gamma) || request.has(Greek::Speed);
        let spot_greeks = spot_greeks && !note.at_spot_discontinuity(context, h);
        crate::cancellation::check()?;
        if let Some(value) = fixed {
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
        let layer = if state.knocked_in && !grid.starting_knocked {
            grid.knocked.as_ref().unwrap_or(&grid.alive)
        } else {
            &grid.alive
        };
        let space = &grid.space;
        let spot = rolled.spot_price();
        let [price, delta, gamma] = space.sample(layer, spot);
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
            if let Some(speed) = space.speed(layer, spot, h) {
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

impl<T: AutocallableTerms> Backend<AutocallableNote<T>> for AutocallableState {
    fn query_work(&self, note: &AutocallableNote<T>, context: &PricingContext) -> usize {
        if context.valuation_time() == note.expiry_time()
            || note.barrier_state() == Some(AutocallableBarrierState::KnockedOut)
        {
            return 0;
        }
        let days = if note.view().daily_knock_in() {
            usize::try_from(
                context
                    .valuation_date()
                    .days_until(note.expiry_time().date()),
            )
            .unwrap_or(usize::MAX)
        } else {
            0
        };
        days.saturating_add(note.events().len())
    }

    fn evaluate(
        &self,
        note: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let state = deterministic::autocallable_through(note, context, self.start)?;
        if let Some(value) = state.settlement {
            return PricingResult::new(value);
        }
        let fixed = fixed_value_at(note, context, self.start, state.knocked_in);
        self.evaluate_after_observations(note, context, &state, fixed, request, shifts)
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
        let start = context.deterministic_until(note.expiry_time())?;
        let mut pending = false;
        let mut starting_knocked = true;
        if start < note.expiry_time() {
            let low = super::solution::at_spot(context, *range.start())?;
            let state = deterministic::autocallable_through(note, &low, start)?;
            if state.settlement.is_none() {
                pending = fixed_value_at(note, &low, start, state.knocked_in).is_none();
                starting_knocked = state.knocked_in;
                if starting_knocked
                    && note.knock_in_affects_payoff()
                    && note.barrier_state() != Some(AutocallableBarrierState::KnockedIn)
                {
                    // Positive f64 bit patterns preserve spot order. A higher spot can avoid
                    // knock-in or trigger knock-out, so the search takes at most 64 replays.
                    let mut lo = range.start().to_bits();
                    let mut hi = range.end().to_bits();
                    let mut bits = hi;
                    while bits > lo {
                        crate::cancellation::check()?;
                        let probe = super::solution::at_spot(context, f64::from_bits(bits))?;
                        let state = deterministic::autocallable_through(note, &probe, start)?;
                        if state.settlement.is_some() {
                            hi = bits - 1;
                        } else if state.knocked_in {
                            lo = bits;
                        } else {
                            if fixed_value_at(note, &probe, start, false).is_none() {
                                pending = true;
                                starting_knocked = false;
                            }
                            break;
                        }
                        bits = hi - (hi - lo) / 2;
                    }
                }
            }
        }
        let grid = if pending {
            let (rolled, carry, _) = continuation_context(&anchor, start)?;
            let rolled = rolled.as_ref();
            Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
                note,
                rolled,
                self.settings,
                diffusion,
                starting_knocked,
                super::scaled(*range.start(), carry)
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
            autocallable_observations(&times, &[110.0; 3], &[0.08; 3])?,
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
