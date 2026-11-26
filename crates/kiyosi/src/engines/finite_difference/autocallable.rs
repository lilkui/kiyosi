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
    /// For a single valuation layer with flat BSM rates, the default first-order
    /// market sensitivities reuse the price rollback and its matrix factors.
    /// Nondefault bumps use separate repricing.
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
        let (space, [[lower_values, _], [upper_values, _]], _) =
            diffusion::with_diffusion!(rolled, |diffusion| solve_layers(
                [lower, upper],
                rolled,
                self.settings,
                diffusion,
                low.knocked_in,
                spot,
                [None; 3],
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

// Keep the derivative order aligned with the three operator parameters.
pub(super) const MARKET_GREEKS: [Greek; 3] = [Greek::Rho, Greek::DividendRho, Greek::Vega];

pub(super) fn market_bumps(
    context: &PricingContext,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> [Option<f64>; 3] {
    if !MARKET_GREEKS.into_iter().any(|greek| request.has(greek)) {
        return [None; 3];
    }
    let Ok(model) = context.natural_bsm_parameters() else {
        return [None; 3];
    };
    let [risk, yield_curve] = [model.risk_free_curve(), model.dividend_curve()];
    if [risk, yield_curve]
        .into_iter()
        .any(|curve| curve.calendar_quote().is_some())
    {
        return [None; 3];
    }
    let (Some(rate), Some(dividend)) = (risk.exact_flat_rate(), yield_curve.exact_flat_rate())
    else {
        return [None; 3];
    };
    let vol = model.volatility();
    if rate.abs() > 2.0 || dividend.abs() > 2.0 || !(1e-3..=2.0).contains(&vol) {
        return [None; 3];
    }
    let automatic_rates =
        shifts.rate_shift.to_bits() == NumericalShiftSettings::default().rate_shift.to_bits();
    let simple_vega = shifts.volatility_shift.is_none()
        && ![Greek::Volga, Greek::Vanna, Greek::Zomma]
            .into_iter()
            .any(|g| request.has(g));
    let requested = [automatic_rates, automatic_rates, simple_vega];
    let widths = [
        shifts.rate_shift,
        shifts.rate_shift,
        shifts.volatility_shift_at(vol),
    ];
    let parameters = [rate, dividend, vol];
    std::array::from_fn(|i| {
        let bump = crate::risk::numerical::symmetric_shift(parameters[i], widths[i]);
        (requested[i] && request.has(MARKET_GREEKS[i]) && bump > 0.0).then_some(bump)
    })
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
        let bumps = if start == context.valuation_time() && !note.all_greeks_unavailable(context) {
            market_bumps(context, request, shifts)
        } else {
            [None; 3]
        };
        let solve = |bumps| {
            diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
                note,
                rolled,
                settings,
                diffusion,
                state.knocked_in,
                rolled.spot_price(),
                bumps
            ))
        };
        Some(match solve(bumps) {
            Err(crate::Error::ComputationFailure(_)) if bumps.iter().any(Option::is_some) => {
                solve([None; 3])?
            }
            result => result?,
        })
    } else {
        None
    };
    let upper = grid.as_ref().map(|grid| grid.space.upper);
    let result = AutocallableState {
        start,
        grid,
        prefix: None,
    }
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
    bumps: [Option<f64>; 3],
) -> Result<AutocallableGrid> {
    let (space, [[alive, knocked]], market) = solve_layers(
        [note],
        context,
        settings,
        diffusion,
        starting_knocked,
        minimum_spot,
        bumps,
    )?;
    let alive = space.spline(alive);
    let knocked = (!knocked.is_empty()).then(|| space.spline(knocked));
    let market = market.map(|values| (!values.is_empty()).then(|| space.spline(values)));
    Ok(AutocallableGrid {
        space,
        market,
        alive,
        knocked,
        starting_knocked,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RollbackLayer {
    Joint,
    Cash,
    Exposure,
}

type Layers<const N: usize> = (SpatialGrid, [[Vec<f64>; 2]; N], [Vec<f64>; 3]);

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
    bumps: [Option<f64>; 3],
) -> Result<Layers<N>> {
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
    let cash_grid = if layered
        && !D::LOCAL
        && settings.scheme != FiniteDifferenceScheme::ExplicitEuler
        && trading.len() > observations.len() + 2
        && validate_workspace(
            settings.scheme,
            (u64::from(settings.asset_step_count.unsigned_abs()) + 1) * N as u64,
            2 * u64::from(settings.time_step_count.unsigned_abs())
                + trading.len() as u64
                + 2 * (observations.len() + risk.ends().len() + dividend.ends().len()) as u64
                + 64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )
        .is_ok()
    {
        Some(diffusion::time_grid(
            context,
            expiry,
            settings,
            &risk,
            &dividend,
            &observations,
        )?)
    } else {
        None
    };
    let cash_observations = cash_grid.as_ref().map(|_| observations.clone());
    observations.extend_from_slice(&trading);
    observations.sort_unstable_by(f64::total_cmp);
    diffusion::validate_event_resolution(context.valuation_time(), expiry, &event_times)?;
    let time_grid = if layered && settings.scheme != FiniteDifferenceScheme::ExplicitEuler {
        diffusion::aligned_time_grid
    } else {
        diffusion::time_grid
    };
    let mut exposure_settings = settings;
    if let Some(cash_grid) = &cash_grid {
        if settings.scheme == FiniteDifferenceScheme::Rannacher {
            // Spend a fraction of the common leg's saved damping steps on
            // resolving the exposure, whose daily jumps drive spot gamma.
            let extra = i32::try_from(trading.len() / 2)
                .unwrap_or(i32::MAX)
                .min(settings.time_step_count / 8);
            let refined = settings.time_step_count.saturating_add(extra);
            let upper_steps = u64::from(refined.unsigned_abs())
                + cash_grid.widths.len() as u64
                + 3 * observations.len() as u64
                + 2 * (risk.ends().len() + dividend.ends().len()) as u64
                + 128;
            if validate_work(
                u64::from(settings.asset_step_count.unsigned_abs()) * N as u64,
                upper_steps,
            )
            .is_ok()
                && validate_workspace(
                    settings.scheme,
                    (u64::from(settings.asset_step_count.unsigned_abs()) + 1) * N as u64,
                    upper_steps,
                    risk.workspace_bytes() + dividend.workspace_bytes(),
                )
                .is_ok()
            {
                exposure_settings.time_step_count = refined;
            }
        }
    }
    let mut grid = time_grid(
        context,
        expiry,
        exposure_settings,
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
    let daily_steps = grid.rollback_steps(settings.scheme, &observations)?;
    let rollback_steps = if let Some((cash_grid, cash_observations)) =
        cash_grid.as_ref().zip(cash_observations.as_deref())
    {
        daily_steps.saturating_add(cash_grid.rollback_steps(settings.scheme, cash_observations)?)
    } else {
        daily_steps * (1 + u64::from(layered))
    };
    validate_work(
        u64::from(settings.asset_step_count.unsigned_abs()) * N as u64,
        rollback_steps,
    )?;
    if N > 1 || cash_grid.is_some() {
        validate_workspace(
            settings.scheme,
            (u64::from(settings.asset_step_count.unsigned_abs()) + 1) * N as u64,
            (grid.widths.len() + cash_grid.as_ref().map_or(0, |grid| grid.widths.len())) as u64,
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
    let tangent_count = bumps.iter().flatten().count() as u64;
    let bumps = if bumps.iter().any(Option::is_some)
        && N == 1
        && !D::LOCAL
        && settings.scheme != FiniteDifferenceScheme::ExplicitEuler
        && validate_work(
            u64::from(settings.asset_step_count.unsigned_abs()) * (1 + tangent_count),
            rollback_steps,
        )
        .is_ok()
        && validate_workspace(
            settings.scheme,
            (u64::from(settings.asset_step_count.unsigned_abs()) + 1)
                * (1 + tangent_count)
                * (1 + u64::from(layered)),
            (grid.widths.len() + cash_grid.as_ref().map_or(0, |grid| grid.widths.len())) as u64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )
        .is_ok()
    {
        bumps
    } else {
        [None; 3]
    };
    let mut tangent = bumps.map(|bump| {
        if bump.is_some() {
            [vec![0.0; size], vec![0.0; knock_in_size]]
        } else {
            [Vec::new(), Vec::new()]
        }
    });
    let mut tangent_next = tangent.clone();
    let sigma = if bumps.iter().any(Option::is_some) {
        diffusion.sigma(0.0, 0.0)
    } else {
        0.0
    };
    let parameters = [risk.rate(0.0), dividend.rate(0.0), sigma];
    let coefficients = tangent_coefficients(&space, parameters, bumps)?;
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
            alive[i] = if cash_grid.is_some() {
                value[0] - value[1]
            } else {
                value[0]
            };
            if layered {
                knocked[i] = value[1];
            }
        }
    }
    let first = if cash_grid.is_some() {
        RollbackLayer::Exposure
    } else {
        RollbackLayer::Joint
    };
    let passes = [
        Some((&grid, first)),
        cash_grid.as_ref().map(|g| (g, RollbackLayer::Cash)),
    ];
    let mut stepper = diffusion.stepper(space.count);
    for (grid, layer) in passes.into_iter().flatten() {
        let split = layer != RollbackLayer::Joint;
        let cash = layer == RollbackLayer::Cash;
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
                for (i, (value, output)) in values
                    .iter()
                    .flatten()
                    .zip(next.iter_mut().flatten())
                    .enumerate()
                {
                    if !value.is_empty() && (!split || (i % 2 == 1) == cash) {
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
                for (lane, derivative) in coefficients
                    .iter()
                    .enumerate()
                    .filter(|(_, derivative)| !derivative.is_empty())
                {
                    let [dr, dq] = [
                        if lane == 0 { -dt * discount } else { 0.0 },
                        if lane == 1 {
                            -dt * dividend_discount
                        } else {
                            0.0
                        },
                    ];
                    for (i, (old, new)) in tangent[lane]
                        .iter()
                        .zip(&mut tangent_next[lane])
                        .enumerate()
                    {
                        if old.is_empty() || (split && (i == 1) != cash) {
                            continue;
                        }
                        let [before, after] = [&values[0][i], &next[0][i]];
                        let edge = space.discount_affine_edges(old, discount, dividend_discount);
                        let source = space.discount_affine_edges(before, dr, dq);
                        stepper.advance_tangent(
                            [old, before, after],
                            new,
                            [dt, weight, rate, q, sigma],
                            [edge[0] + source[0], edge[1] + source[1]],
                            derivative,
                        )?;
                    }
                }
                swap_layers(&mut tangent, &mut tangent_next, layer);
                swap_layers(&mut values, &mut next, layer);
            }
            let time = grid.times[step];
            let record = event(note, time, context)
                .filter(|r| time > 0.0 && (layer != RollbackLayer::Exposure || r.has_knock_out()));
            let ki = !cash && layered && trading.binary_search_by(|v| v.total_cmp(&time)).is_ok();
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
                                node.map_or_else(
                                    || space.linear(layer, asset),
                                    |index| layer[index],
                                )
                            };
                            let a = if cash { 0.0 } else { value(alive) };
                            let k = if layered && layer != RollbackLayer::Exposure {
                                value(knocked)
                            } else {
                                0.0
                            };
                            let hit = record
                                .filter(|r| r.has_knock_out() && asset >= r.knock_out_level());
                            if layer == RollbackLayer::Exposure {
                                return [
                                    if hit.is_some() || (ki && t.below_knock_in(asset)) {
                                        -a
                                    } else {
                                        0.0
                                    },
                                    0.0,
                                ];
                            }
                            let coupon = record.map_or(0.0, |r| {
                                note.observation_coupon(r, |level| asset >= level)
                            });
                            if let Some(r) = hit {
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
                        if !cash {
                            next_alive[i] = alive[i] + correction[0];
                        }
                        if layered && layer != RollbackLayer::Exposure {
                            next_knocked[i] = knocked[i] + correction[1];
                        }
                    }
                }
                let knockout = record
                    .filter(|r| r.has_knock_out())
                    .map(ObservationEvent::knock_out_level);
                if ki || knockout.is_some() {
                    for ([alive, knocked], [next_alive, next_knocked]) in
                        tangent.iter().zip(&mut tangent_next)
                    {
                        if alive.is_empty() {
                            continue;
                        }
                        for i in 0..size {
                            let correction =
                                project_grid_node(&space, &breaks, i, |asset, node| {
                                    let value = |values: &[f64]| {
                                        node.map_or_else(
                                            || space.linear(values, asset),
                                            |index| values[index],
                                        )
                                    };
                                    let hit = knockout.is_some_and(|level| asset >= level);
                                    let transfer = ki && t.below_knock_in(asset);
                                    let a = if cash { 0.0 } else { value(alive) };
                                    let k = if layered && layer != RollbackLayer::Exposure {
                                        value(knocked)
                                    } else {
                                        0.0
                                    };
                                    if hit {
                                        [-a, -k]
                                    } else if transfer {
                                        [k - a, 0.0]
                                    } else {
                                        [0.0, 0.0]
                                    }
                                });
                            if !cash {
                                next_alive[i] = alive[i] + correction[0];
                            }
                            if layered && layer != RollbackLayer::Exposure {
                                next_knocked[i] = knocked[i] + correction[1];
                            }
                        }
                    }
                    swap_layers(&mut tangent, &mut tangent_next, layer);
                }
                swap_layers(&mut values, &mut next, layer);
                damping = 2;
            }
        }
    }
    if cash_grid.is_some() {
        for [alive, knocked] in &mut values {
            for (value, &cash) in alive.iter_mut().zip(knocked.iter()) {
                *value += cash;
            }
        }
        for [alive, knocked] in &mut tangent {
            for (value, &cash) in alive.iter_mut().zip(knocked.iter()) {
                *value += cash;
            }
        }
    }
    Ok((space, values, tangent.map(|[alive, _]| alive)))
}

pub(super) fn tangent_coefficients(
    space: &SpatialGrid,
    parameters: [f64; 3],
    bumps: [Option<f64>; 3],
) -> Result<[Vec<[f64; 3]>; 3]> {
    let mut derivatives = std::array::from_fn(|_| Vec::new());
    for (lane, bump) in bumps.into_iter().enumerate() {
        let Some(bump) = bump else { continue };
        let mut low = parameters;
        let mut high = parameters;
        low[lane] -= bump;
        high[lane] += bump;
        derivatives[lane].reserve_exact(space.count - 1);
        for i in 1..space.count {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let a = space.coefficients(i, low[0], low[1], low[2]);
            let b = space.coefficients(i, high[0], high[1], high[2]);
            derivatives[lane].push(std::array::from_fn(|j| (b[j] - a[j]) / (2.0 * bump)));
        }
    }
    Ok(derivatives)
}

fn swap_layers<const N: usize>(
    values: &mut [[Vec<f64>; 2]; N],
    next: &mut [[Vec<f64>; 2]; N],
    layer: RollbackLayer,
) {
    match layer {
        RollbackLayer::Joint => std::mem::swap(values, next),
        RollbackLayer::Exposure | RollbackLayer::Cash => {
            let index = usize::from(layer == RollbackLayer::Cash);
            for (value, next) in values.iter_mut().zip(next) {
                std::mem::swap(&mut value[index], &mut next[index]);
            }
        }
    }
}

#[derive(PartialEq)]
struct AutocallableGrid {
    space: SpatialGrid,
    market: [Option<Spline>; 3],
    alive: Spline,
    knocked: Option<Spline>,
    starting_knocked: bool,
}

#[derive(PartialEq)]
struct AutocallableState {
    start: Timestamp,
    grid: Option<AutocallableGrid>,
    prefix: Option<Vec<deterministic::AutocallableFixing>>,
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
        for (greek, values) in MARKET_GREEKS.into_iter().zip(&grid.market) {
            if let Some(values) = values {
                let derivative = space.sample(values, spot)[0];
                if derivative.is_finite() {
                    result.set(request, greek, derivative)?;
                }
            }
        }
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
        let state = deterministic::autocallable_through_prepared(
            note,
            context,
            self.start,
            self.prefix.as_deref(),
        )?;
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
        let prefix = deterministic::prepare_autocallable_prefix(note, context, start)?;
        let through = |context: &PricingContext| {
            deterministic::autocallable_through_prepared(note, context, start, prefix.as_deref())
        };
        let mut pending = false;
        let mut starting_knocked = true;
        if start < note.expiry_time() {
            let low = super::solution::at_spot(context, *range.start())?;
            let state = through(&low)?;
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
                        let state = through(&probe)?;
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
                super::scaled(*range.start(), carry),
                [None; 3],
            ))?)
        } else {
            None
        };
        let state = AutocallableState {
            start,
            grid,
            prefix,
        };
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
