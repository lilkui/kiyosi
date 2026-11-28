use super::{Payoff, RollbackWork, curve::Curve, validate_rollback_work as validate_work};
use crate::instruments::monitoring::validate_life;
use crate::numerics::{scaled_cash as scaled, weighted_cdf};
use crate::{
    AmericanOption, DigitalOption, Error, EuropeanOption, Greek, GreeksRequest,
    NumericalShiftSettings, OptionContract, OptionType, PricingContext, PricingEngine,
    PricingResult, Result, Timestamp, engines::analytic::european::black_scholes_arguments,
};

mod accumulator;
mod autocallable;
mod barrier;
mod binary_barrier;
mod diffusion;
mod edge_weights;
mod grid;
pub(crate) mod heston;
mod local_volatility;
mod payoff;
mod solution;
mod step;
mod strike_batch;
use diffusion::Diffusion;
use grid::{SpatialGrid, TimeGrid};
pub use solution::{FiniteDifferenceEngine, FiniteDifferenceSolution};
use step::Stepper;
pub use strike_batch::VanillaBatchEngine;

/// Selects the finite-difference time-marching scheme.
///
/// See [`FiniteDifferenceVanillaEngine::new`] for a settings example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FiniteDifferenceScheme {
    /// Uses first-order explicit Euler, subject to grid stability validation at pricing.
    ExplicitEuler,
    /// Uses first-order implicit Euler.
    ImplicitEuler,
    /// Uses standard Crank-Nicolson without time-step damping.
    CrankNicolson,
    /// Uses Crank-Nicolson with Rannacher time-step damping (the default).
    ///
    /// Replaces each of the first two intervals with two implicit Euler half-steps,
    /// restarting after scheduled barrier, accumulator and autocallable observations.
    /// Continuous double barriers retry a Crank-Nicolson step that produces a
    /// negative interior price and continue the rollback with implicit Euler.
    #[default]
    Rannacher,
}

impl FiniteDifferenceScheme {
    fn theta(self) -> f64 {
        match self {
            Self::ExplicitEuler => 0.0,
            Self::ImplicitEuler => 1.0,
            Self::CrankNicolson | Self::Rannacher => 0.5,
        }
    }
}

/// Configures the log-spot domain and time grid, validated at engine construction.
///
/// Grids are limited to 1 GiB of estimated workspace and one billion spatial intervals times time intervals.
/// See [`FiniteDifferenceVanillaEngine::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiniteDifferenceSettings {
    /// Sets at least three spatial steps; defaults to 800.
    /// Continuous barrier grids use all intervals between their actual domain edges.
    pub asset_step_count: i32,
    /// Sets at least one base time step; defaults to 400. The grid includes curve
    /// and surface knots, model-calendar transitions and observations.
    pub time_step_count: i32,
    /// Selects time marching; defaults to [`FiniteDifferenceScheme::Rannacher`].
    pub scheme: FiniteDifferenceScheme,
    /// Sets a finite positive upper spot boundary, or uses the contract's automatic domain.
    ///
    /// Pricing requires the boundary to exceed spot, strike and any barrier level.
    /// The lower spot boundary is selected automatically.
    /// Automatic local-volatility domains use the maximum interpolated volatility
    /// over the remaining stochastic horizon.
    /// Explicit domains need convergence checks.
    /// Continuous barriers fix the corresponding domain edges: an up barrier or
    /// double barrier makes this bound inactive for the barrier grid. Local-volatility
    /// knock-in vanilla components and scheduled monitoring use the full domain.
    pub asset_upper_boundary: Option<f64>,
}

impl Default for FiniteDifferenceSettings {
    fn default() -> Self {
        Self {
            asset_step_count: 800,
            time_step_count: 400,
            scheme: Default::default(),
            asset_upper_boundary: None,
        }
    }
}

impl FiniteDifferenceSettings {
    /// Validates context-independent grid controls.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for fewer than three asset intervals,
    /// fewer than one time interval, an estimated grid workspace above 1 GiB, more than
    /// one billion spatial intervals times time intervals, or an invalid explicit boundary.
    /// Pricing includes curve buffers and event steps in the workspace estimate and
    /// damping half-steps in the work limit. It also checks domain coverage
    /// and scheme stability.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::FiniteDifferenceSettings;
    /// # fn main() -> kiyosi::Result<()> {
    /// let settings = FiniteDifferenceSettings { asset_step_count: 1600, ..Default::default() };
    /// settings.validate()?;
    /// # Ok(()) }
    /// ```
    pub fn validate(&self) -> Result<()> {
        if self.asset_step_count < 3 {
            return Err(invalid_grid("asset_step_count must be at least 3"));
        }
        if self.time_step_count < 1 {
            return Err(invalid_grid("time_step_count must be positive"));
        }
        validate_workspace(
            self.scheme,
            u64::from(self.asset_step_count.unsigned_abs()) + 1,
            u64::from(self.time_step_count.unsigned_abs()),
            0,
        )?;
        validate_work(
            u64::from(self.asset_step_count.unsigned_abs()),
            u64::from(self.time_step_count.unsigned_abs()),
        )?;
        if let Some(boundary) = self.asset_upper_boundary {
            crate::error::finite(boundary, true, "asset_upper_boundary")?;
        }
        Ok(())
    }
}

const WORKSPACE_BUDGET: u64 = 1 << 30;

/// Checks the workspace limit for finite-difference strike batches.
///
/// See [`FiniteDifferenceVanillaEngine::price_strikes`] for a pricing example.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] when the strike count exceeds the workspace limit.
#[doc(hidden)]
pub fn validate_finite_difference_strike_batch_size(strikes: usize) -> Result<()> {
    // Allow for the input, ordering indices, and both the grouped and final quotes.
    let bytes_per_strike = (size_of::<usize>() + 3 * size_of::<f64>()) as u64;
    if (strikes as u64).saturating_mul(bytes_per_strike) > WORKSPACE_BUDGET / 4 {
        return Err(invalid_grid(
            "finite-difference strike batch exceeds workspace budget",
        ));
    }
    Ok(())
}

/// Checks the surface workspace budget before collecting or pricing its inputs.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] when the requested dimensions exceed the budget.
#[doc(hidden)]
pub fn validate_finite_difference_surface_size(expiries: usize, strikes: usize) -> Result<()> {
    let row_bytes = 64_u64.saturating_add((strikes as u64).saturating_mul(16));
    // Even an empty expiry axis must bound the strike inputs.
    if (expiries.max(1) as u64).saturating_mul(row_bytes) > WORKSPACE_BUDGET / 4 {
        return Err(invalid_grid(
            "finite-difference surface output exceeds workspace budget",
        ));
    }
    Ok(())
}

fn validate_workspace(
    scheme: FiniteDifferenceScheme,
    asset_nodes: u64,
    time_steps: u64,
    curve_bytes: u64,
) -> Result<()> {
    const BYTES_PER_TIME_STEP: u64 = 24;
    // Allow for rollback layers, interpolation buffers and the spatial axis as well as the stepper.
    let bytes_per_asset_node = Stepper::PEAK_BYTES_PER_NODE
        + if scheme == FiniteDifferenceScheme::ExplicitEuler {
            72
        } else {
            112
        };
    if (asset_nodes * bytes_per_asset_node + time_steps * BYTES_PER_TIME_STEP)
        .saturating_add(curve_bytes)
        > WORKSPACE_BUDGET
    {
        return Err(invalid_grid(
            "finite-difference workspace exceeds 1 GiB; reduce grid step counts",
        ));
    }
    Ok(())
}

fn prepare_rate_curves(
    context: &PricingContext,
    horizon: Timestamp,
    validate_grid: impl FnOnce(u64, u64) -> Result<()>,
) -> Result<(Curve, Curve)> {
    context.validate_curve_coverage(horizon)?;
    crate::cancellation::check()?;
    let model = context.model_parameters();
    let (risk, dividend) = (model.risk_free_curve(), model.dividend_curve());
    let start = context.valuation_time();
    let [risk_count, dividend_count] =
        [risk, dividend].map(|curve| Curve::interval_count(curve, start, horizon));
    // Each curve's interior knots must appear even when the two curves share knots.
    validate_grid(
        risk_count.max(dividend_count),
        Curve::bytes_for_intervals(risk_count + dividend_count),
    )?;
    Ok((
        Curve::new(risk, start, horizon)?,
        Curve::new(dividend, start, horizon)?,
    ))
}

fn invalid_grid(message: &'static str) -> Error {
    Error::InvalidInput(message.into())
}
fn unstable() -> Error {
    Error::ComputationFailure("finite-difference system is numerically unstable".into())
}

fn diffusion_upper(relevant: f64, width: f64) -> f64 {
    if width <= 4.0_f64.ln() {
        4.0 * relevant
    } else {
        (relevant.ln() + width).exp()
    }
}

fn diffusion_log_width(
    context: &PricingContext,
    horizon: Timestamp,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    let start = context.valuation_time();
    let time = start.years_until(horizon);
    let sigma = match context.model_parameters() {
        crate::ModelParameters::Heston(_) => {
            return Err(Error::UnsupportedOperation(
                "finite differences do not support Heston".into(),
            ));
        }
        crate::ModelParameters::BlackScholesMerton(m) => m.volatility(),
        crate::ModelParameters::LocalVolatility(m) => {
            m.surface().maximum_between(start, horizon)?
        }
    };
    let sigma = sigma
        * context
            .model_parameters()
            .variance_time_convention()
            .calendar()
            .map_or(1.0, |calendar| {
                (365.0 / f64::from(calendar.trading_days_per_year())).sqrt()
            });
    let mut drift: f64 = 0.0;
    for (i, end) in risk.ends().chain(dividend.ends()).enumerate() {
        if i % 1024 == 0 {
            crate::cancellation::check()?;
        }
        drift = drift.max(
            (-risk.log_factor(0.0, end) + dividend.log_factor(0.0, end)
                - 0.5 * sigma * sigma * end)
                .abs(),
        );
    }
    Ok(4.0 * sigma * time.sqrt() + drift)
}

fn native(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    concentrate: bool,
) -> Result<(PricingResult, Option<f64>)> {
    let request = local_volatility::validate_request(context, request)?;
    validate_life(option, context)?;
    let expiry = option.expiry_time();
    if crate::engines::deterministic::enabled_until(context, expiry)? {
        return Ok((
            PricingResult::new(crate::engines::deterministic::vanilla_to_expiry(
                payoff, context, expiry,
            )?)?,
            None,
        ));
    }
    diffusion::with_diffusion!(context, Some(expiry), |diffusion| {
        if context.valuation_time() == expiry {
            return Ok((
                PricingResult::new(payoff.value(context.spot_price()))?,
                None,
            ));
        }
        let saved = solve_model(
            expiry,
            payoff,
            context,
            settings,
            concentrate,
            context.spot_price(),
            diffusion,
            None,
            None,
        )?;
        let result = saved.evaluate(context, request, shifts)?;
        let result = saved.european_market_greeks(context, expiry, request, shifts, result)?;
        Ok((result, Some(saved.grid.space.upper)))
    })
}

struct TimeSlices {
    before_at: Timestamp,
    before: f64,
    after: f64,
    before_values: Option<Vec<f64>>,
    after_values: Option<Vec<f64>>,
    remove_carry: bool,
}

#[expect(
    clippy::too_many_arguments,
    reason = "The optional snapshots use the same rollback and grid."
)]
fn solve_model<D: Diffusion>(
    expiry: Timestamp,
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    concentrate: bool,
    minimum_spot: f64,
    diffusion: D,
    mut slices: Option<&mut TimeSlices>,
    mut surface: Option<&mut strike_batch::AmericanSurface<'_>>,
) -> Result<VanillaGrid> {
    let time = context.valuation_time().years_until(expiry);
    let spot = context.spot_price();
    let model = context.model_parameters();
    let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
    let log_width = diffusion_log_width(context, expiry, &risk, &dividend)?;
    let level = spot.max(payoff.strike);
    let discount = [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)];
    let carry = discount[1] - discount[0];
    // Roll back in spot units before forward growth. Cash uses the risk-free
    // discount; removing growth from asset payoffs leaves the dividend discount.
    let mut rollback_payoff = payoff;
    let adjusted_strike = scaled(payoff.strike, -carry);
    let remove_carry = !payoff.is_american()
        && !D::LOCAL
        && concentrate
        && settings.scheme == FiniteDifferenceScheme::Rannacher
        && adjusted_strike.is_normal();
    if let Some(slices) = slices.as_deref_mut() {
        slices.remove_carry = remove_carry;
    }
    if remove_carry {
        rollback_payoff.strike = adjusted_strike;
    }
    let width = diffusion.sigma(
        0.0,
        if payoff.is_american() {
            payoff.strike
        } else {
            spot
        },
    ) * time.sqrt();
    let margin = 1.5 * log_width;
    let narrow = width < 0.04;
    let compact = concentrate && narrow && settings.scheme != FiniteDifferenceScheme::ExplicitEuler;
    let domain_width = if compact {
        margin.max(1e-3)
    } else if !D::LOCAL && margin < 2.5 {
        4.0_f64.ln()
    } else {
        margin.max(4.0_f64.ln())
    };
    let upper = settings.asset_upper_boundary.unwrap_or_else(|| {
        if compact || D::LOCAL || payoff.is_american() {
            scaled(level, domain_width)
        } else {
            4.0 * level
        }
    });
    if upper <= level {
        return Err(invalid_grid(
            "finite-difference upper bound must exceed spot and strike",
        ));
    }
    let lower = grid::spot_floor(minimum_spot, rollback_payoff.strike, domain_width);
    let mut space = SpatialGrid::new(
        settings.asset_step_count,
        [lower, upper],
        rollback_payoff.strike,
    )?;
    let resolve_drift = !remove_carry && narrow && carry.abs() > 4.0 * width;
    if settings.scheme != FiniteDifferenceScheme::ExplicitEuler && settings.asset_step_count >= 16 {
        if width > 0.0 && resolve_drift {
            space.resolve_drift_band(payoff.strike, carry, width)?;
        } else if narrow || concentrate || payoff.is_american() || !D::LOCAL {
            let floor = if payoff.is_digital() {
                1e-9
            } else {
                f64::EPSILON
            };
            let scale = (0.3 * width).max(floor);
            grid::concentrate(&mut space, vec![rollback_payoff.strike], scale)?;
        }
    }
    let mut startup = Vec::new();
    if resolve_drift && settings.scheme == FiniteDifferenceScheme::Rannacher {
        // Shorten the damped steps so implicit advection adds less numerical variance.
        let dt = time / f64::from(settings.time_step_count);
        startup.extend([time - 0.25 * dt, time - 0.125 * dt]);
    }
    if let Some(surface) = &surface {
        startup.extend_from_slice(surface.times());
    }
    let time_grid = diffusion::time_grid(context, expiry, settings, &risk, &dividend, &startup)?;
    let rollback_steps = time_grid.rollback_steps(settings.scheme, &[])?;
    if slices.is_some() {
        // Bound up to sixteen added substeps and two copied value slices.
        validate_work(space.count as u64, rollback_steps + 17)?;
        validate_workspace(
            settings.scheme,
            2 * (space.count as u64 + 1),
            time_grid.widths.len() as u64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )?;
    }
    validate_work(space.count as u64, rollback_steps)?;
    if let Some(surface) = &surface {
        surface.charge(space.count as u64, rollback_steps)?;
    }
    let can_exercise = payoff.is_american()
        && (model.risk_free_curve().exact_flat_rate().is_none()
            || model.dividend_curve().exact_flat_rate().is_none()
            || payoff.sign < 0.0
            || dividend.rate(0.0) > 0.0
            || risk.rate(0.0) < 0.0);
    let mut exercise_boundaries = if can_exercise {
        Some(crate::engines::deterministic::ExerciseBoundaries::new(
            payoff,
            [lower, upper],
            time,
            [&risk, &dividend],
            usize::try_from(rollback_steps).unwrap_or(usize::MAX),
        )?)
    } else {
        None
    };
    let theta = settings.scheme.theta();
    if !D::LOCAL && !payoff.is_digital() {
        time_grid.validate_stability(&space, diffusion.sigma(0.0, 0.0), theta, &risk, &dividend)?;
    }
    let is_cash_digital = payoff.payout().is_some();
    let is_asset_digital = payoff.is_digital() && !is_cash_digital;
    let mut old: Vec<_> = (0..=space.count)
        .map(|i| {
            grid::project_payoff(&space, &[rollback_payoff.strike], i, |asset| {
                rollback_payoff.value(asset)
            })
        })
        .collect();
    let exercise = if can_exercise {
        (0..=space.count)
            .map(|i| payoff.value(space.asset(i)))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut next = vec![0.0; old.len()];
    let mut stepper = diffusion.stepper(space.count);
    let before = slices.as_ref().map_or(0.0, |slices| slices.before);
    let earlier_curves = slices
        .as_ref()
        .filter(|slices| slices.before < 0.0 && can_exercise)
        .map(|slices| {
            Ok::<_, Error>([
                Curve::new(model.risk_free_curve(), slices.before_at, expiry)?,
                Curve::new(model.dividend_curve(), slices.before_at, expiry)?,
            ])
        })
        .transpose()?;
    let mut earlier_boundaries = earlier_curves
        .as_ref()
        .map(|[risk, dividend]| {
            crate::engines::deterministic::ExerciseBoundaries::new(
                payoff,
                [lower, upper],
                time - before,
                [risk, dividend],
                16,
            )
        })
        .transpose()?;
    let mut advance =
        |old: &[f64], next: &mut [f64], current: f64, dt: f64, weight: f64| -> Result<()> {
            let tau = time - current;
            let middle = current + 0.5 * dt;
            let [rate, q, risk_log, dividend_log] = if remove_carry {
                [0.0; 4]
            } else {
                let rate = risk.rate(middle);
                let q = dividend.rate(middle);
                let risk_log = if current < 0.0 {
                    -rate * tau
                } else {
                    risk.log_factor(current, time)
                };
                let dividend_log = if current < 0.0 {
                    -q * tau
                } else {
                    dividend.log_factor(current, time)
                };
                // Factor out the discount so time stepping preserves the digital payout cap.
                if is_cash_digital {
                    [0.0, q - rate, 0.0, dividend_log - risk_log]
                } else if is_asset_digital {
                    [rate - q, 0.0, risk_log - dividend_log, 0.0]
                } else {
                    [rate, q, risk_log, dividend_log]
                }
            };
            let mut edges = [lower, upper].map(|asset| {
                payoff::boundary_value(
                    rollback_payoff,
                    asset,
                    [risk_log, dividend_log],
                    diffusion,
                    tau,
                )
            });
            let (boundaries, date) = if current < 0.0 {
                (&mut earlier_boundaries, current - before)
            } else {
                (&mut exercise_boundaries, current)
            };
            if let Some(boundaries) = boundaries {
                for (edge, floor) in edges.iter_mut().zip(boundaries.at(date)?) {
                    *edge = edge.max(floor);
                }
            }
            diffusion.advance(
                &mut stepper,
                old,
                next,
                &space,
                middle,
                [dt, weight, rate, q],
                edges,
            )?;
            if can_exercise {
                for i in 1..space.count {
                    next[i] = next[i].max(exercise[i]);
                }
            }
            Ok(())
        };
    let mut damping = 2;
    for step in (0..time_grid.widths.len()).rev() {
        crate::cancellation::check()?;
        for (current, dt, weight) in time_grid.substeps(step, settings.scheme, &mut damping) {
            if let Some(slices) = slices.as_deref_mut().filter(|slices| {
                slices.after > 0.0 && slices.after >= current && slices.after_values.is_none()
            }) {
                // Use the same next time slice and split off only the extra query.
                let width = current + dt - slices.after;
                if width > 16.0 * f64::EPSILON * time {
                    let mut sampled = vec![0.0; old.len()];
                    advance(&old, &mut sampled, slices.after, width, weight)?;
                    slices.after_values = Some(sampled);
                } else {
                    slices.after_values = Some(old.clone());
                }
            }
            advance(&old, &mut next, current, dt, weight)?;
            std::mem::swap(&mut old, &mut next);
            if let Some(surface) = &mut surface {
                surface.capture(current, &space, &old, payoff)?;
            }
        }
    }
    if let Some(slices) = slices.filter(|slices| slices.before < 0.0) {
        let mut previous = old.clone();
        let width = time / f64::from(settings.time_step_count);
        let mut current = 0.0;
        while current > slices.before {
            crate::cancellation::check()?;
            let at = (current - width).max(slices.before);
            advance(
                &previous,
                &mut next,
                at,
                current - at,
                settings.scheme.theta(),
            )?;
            std::mem::swap(&mut previous, &mut next);
            current = at;
        }
        slices.before_values = Some(previous);
    }
    if is_asset_digital {
        for (i, value) in old.iter_mut().enumerate() {
            *value /= space.asset(i);
        }
    }
    // Allow a few ulps per rollback, including up to two substeps per interval.
    #[expect(
        clippy::cast_precision_loss,
        reason = "Allocated time grids fit exactly in f64."
    )]
    let roundoff = 8.0 * f64::EPSILON * time_grid.widths.len() as f64;
    Ok(VanillaGrid {
        grid: solution::Grid::new(space, old),
        payoff,
        discount,
        value_log_discount: if is_cash_digital {
            discount[0]
        } else if remove_carry {
            discount[1]
        } else {
            0.0
        },
        relative_upper_error: roundoff,
    })
}

#[derive(PartialEq)]
struct VanillaGrid {
    grid: solution::Grid,
    payoff: Payoff,
    discount: [f64; 2],
    value_log_discount: f64,
    relative_upper_error: f64,
}

#[derive(PartialEq)]
struct VanillaState {
    payoff: Payoff,
    grid: Option<VanillaGrid>,
    exercise: Option<crate::engines::deterministic::AmericanExercise>,
}

impl<I: OptionContract> solution::Backend<I> for VanillaState {
    fn evaluate(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        if let Some(grid) = &self.grid {
            return grid.evaluate(context, request, shifts);
        }
        if let Some(exercise) = &self.exercise {
            return PricingResult::new(exercise.price(context)?);
        }
        if context.valuation_time() == option.expiry_time() {
            let result = PricingResult::new(self.payoff.value(context.spot_price()))?;
            if !self.payoff.is_digital() {
                return crate::risk::analytics::vanilla_expiry_greeks(
                    if self.payoff.sign > 0.0 {
                        OptionType::Call
                    } else {
                        OptionType::Put
                    },
                    self.payoff.strike,
                    context,
                    result.with_request(request, request),
                );
            }
            return Ok(result);
        }
        PricingResult::new(crate::engines::deterministic::vanilla(
            option,
            self.payoff,
            context,
        )?)
    }

    fn query_work(&self, _: &I, _: &PricingContext) -> usize {
        self.exercise.as_ref().map_or(
            0,
            crate::engines::deterministic::AmericanExercise::interval_count,
        )
    }

    fn queries_may_scan_calendar(&self) -> bool {
        self.grid.is_none()
    }

    fn at_spot_discontinuity(&self, option: &I, context: &PricingContext, radius: f64) -> bool {
        // A stored grid already establishes that variance remains before expiry.
        if self.grid.is_some() {
            return false;
        }
        if let Some(exercise) = &self.exercise {
            exercise
                .at_spot_discontinuity(context, radius)
                .unwrap_or(false)
        } else {
            option.at_spot_discontinuity(context, radius)
        }
    }
}

fn prepare_vanilla_state(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    concentrate: bool,
    minimum_spot: f64,
) -> Result<VanillaState> {
    local_volatility::validate_model(context)?;
    validate_life(option, context)?;
    let expiry = option.expiry_time();
    let at_expiry = context.valuation_time() == expiry;
    let grid = if at_expiry || crate::engines::deterministic::enabled_until(context, expiry)? {
        None
    } else {
        Some(diffusion::with_diffusion!(
            context,
            Some(expiry),
            |diffusion| {
                solve_model(
                    expiry,
                    payoff,
                    context,
                    settings,
                    concentrate,
                    minimum_spot,
                    diffusion,
                    None,
                    None,
                )
            }
        )?)
    };
    let exercise = if grid.is_none() && payoff.is_american() && !at_expiry {
        Some(crate::engines::deterministic::AmericanExercise::new(
            payoff, context, expiry,
        )?)
    } else {
        None
    };
    let state = VanillaState {
        payoff,
        grid,
        exercise,
    };
    solution::Backend::evaluate(
        &state,
        option,
        context,
        GreeksRequest::new([]),
        Default::default(),
    )?;
    Ok(state)
}

macro_rules! vanilla_solution {
    ($engine:ty, $option:ty, $payoff:expr) => {
        impl FiniteDifferenceEngine<$option> for $engine {
            fn solve(
                &self,
                option: &$option,
                context: &PricingContext,
                range: std::ops::RangeInclusive<f64>,
            ) -> Result<FiniteDifferenceSolution> {
                <Self as PricingEngine<$option>>::validate_applicability(self, context)?;
                solution::validate_range(&range)?;
                crate::cancellation::check()?;
                let payoff: Payoff = ($payoff)(option);
                let anchor = solution::at_spot(context, *range.end())?;
                let state = prepare_vanilla_state(
                    option,
                    payoff,
                    &anchor,
                    self.settings,
                    self.settings.asset_upper_boundary.is_none(),
                    *range.start(),
                )?;
                crate::cancellation::check()?;
                Ok(FiniteDifferenceSolution::new(
                    option.clone(),
                    context,
                    range,
                    state,
                ))
            }
        }
    };
}
vanilla_solution!(
    FiniteDifferenceVanillaEngine,
    EuropeanOption,
    |option: &EuropeanOption| Payoff::vanilla(option.option_type(), option.strike(), false)
);
vanilla_solution!(
    FiniteDifferenceVanillaEngine,
    AmericanOption,
    |option: &AmericanOption| Payoff::vanilla(option.option_type(), option.strike(), true)
);
vanilla_solution!(
    FiniteDifferenceDigitalEngine,
    DigitalOption,
    Payoff::digital
);

impl VanillaGrid {
    fn european_market_greeks(
        &self,
        context: &PricingContext,
        expiry: Timestamp,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
        mut result: PricingResult,
    ) -> Result<PricingResult> {
        let measures = [
            Greek::Vega,
            Greek::Volga,
            Greek::Vanna,
            Greek::Zomma,
            Greek::Rho,
            Greek::DividendRho,
        ];
        if self.payoff.is_american()
            || self.payoff.is_digital()
            || !measures.into_iter().any(|greek| request.has(greek))
        {
            return Ok(result);
        }
        let Ok(model) = context.natural_bsm_parameters() else {
            return Ok(result);
        };
        if model.risk_free_curve().exact_flat_rate().is_none()
            || model.dividend_curve().exact_flat_rate().is_none()
            || !model.risk_free_curve().nodes().is_empty()
            || !model.dividend_curve().nodes().is_empty()
        {
            return Ok(result);
        }
        let spot = context.spot_price();
        let sigma = model.volatility();
        let time = context.valuation_time().years_until(expiry);
        let root = time.sqrt();
        // A coarse spatial grid can resolve price well before its curvature
        // is suitable for mixed sensitivities.
        if sigma * root <= 32.0 * self.grid.space.spacing {
            return Ok(result);
        }
        let moneyness = context.log_forward_moneyness(
            expiry,
            self.payoff.strike,
            self.discount[1] - self.discount[0],
        );
        let (d1, d2) = super::analytic::european::black_scholes_arguments(moneyness, sigma, root);
        if d1.abs().max(d2.abs()) > 4.0 {
            return Ok(result);
        }
        let [_, delta, gamma] = self
            .grid
            .sample(spot)
            .map(|value| crate::numerics::scaled_signed_cash(value, self.value_log_discount));
        // In the BSM equation, rate sensitivities differ only by forward-spot
        // and discount scaling. Volatility sensitivities follow from the same
        // solution's curvature and the derivative of the lognormal density.
        if shifts.volatility_shift.is_none()
            && shifts.spot_shift.is_none()
            && crate::risk::numerical::symmetric_shift(sigma, shifts.volatility_shift_at(sigma))
                > 0.0
            && gamma > 0.0
        {
            let scaled_gamma = gamma * spot;
            let vega = scaled_gamma * spot * sigma * time;
            for (greek, value) in [
                (Greek::Vega, vega),
                (Greek::Volga, vega * (d1 * d2 / sigma)),
                (Greek::Vanna, -scaled_gamma * root * d2),
                (Greek::Zomma, gamma * ((d1 * d2 - 1.0) / sigma)),
            ] {
                if value.is_finite() {
                    result.set(request, greek, value)?;
                }
            }
        }
        if shifts.rate_shift.to_bits() == NumericalShiftSettings::default().rate_shift.to_bits() {
            for (greek, curve, value) in [
                (
                    Greek::Rho,
                    model.risk_free_curve(),
                    (spot * delta - result.price()) * time,
                ),
                (
                    Greek::DividendRho,
                    model.dividend_curve(),
                    -spot * delta * time,
                ),
            ] {
                if value.is_finite()
                    && crate::risk::numerical::symmetric_shift(
                        curve.shift_origin(),
                        shifts.rate_shift,
                    ) > 0.0
                {
                    result.set(request, greek, value)?;
                }
            }
        }
        Ok(result)
    }

    fn evaluate(
        &self,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let spot = context.spot_price();
        let payoff = self.payoff;
        let sample_at = |spot| -> Result<[f64; 3]> {
            let [raw_price, delta, gamma] = if payoff.is_digital() && payoff.payout().is_none() {
                let [fraction, first, second] = self.grid.space.sample_log(&self.grid.spline, spot);
                let discount = self.discount[1];
                let scale = scaled(spot, discount);
                let price = if scale.is_normal() {
                    fraction * scale
                } else {
                    crate::numerics::scaled_signed_cash(fraction, spot.ln() + discount)
                };
                [
                    price,
                    crate::numerics::scaled_signed_cash(fraction + first, discount),
                    crate::numerics::scaled_signed_cash((first + second) / spot, discount),
                ]
            } else {
                let values = self.grid.sample(spot);
                if self.value_log_discount == 0.0 {
                    values
                } else {
                    values.map(|value| {
                        crate::numerics::scaled_signed_cash(value, self.value_log_discount)
                    })
                }
            };
            let price = payoff::checked_price(
                payoff,
                spot,
                raw_price,
                self.discount,
                self.relative_upper_error,
            )?;
            if payoff.is_american()
                && (payoff.value(spot) > raw_price
                    || self
                        .grid
                        .space
                        .exercise_cell(&self.grid.spline, spot, |s| payoff.value(s)))
            {
                let slope = if payoff.sign * (spot - payoff.strike) > 0.0 {
                    payoff.sign
                } else {
                    0.0
                };
                Ok([payoff.value(spot), slope, 0.0])
            } else {
                Ok([price, delta, gamma])
            }
        };
        let [price, delta, gamma] = sample_at(spot)?;
        let mut result = PricingResult::new(price)?;
        result.set(request, Greek::Delta, delta)?;
        result.set(request, Greek::Gamma, gamma)?;
        if request.has(Greek::Speed) {
            let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
            let space = &self.grid.space;
            if h > 0.0 && spot - h > space.asset(0) && spot + h < space.upper {
                let up = sample_at(spot + h)?[2];
                let down = sample_at(spot - h)?[2];
                result.set(request, Greek::Speed, (up - down) / (2.0 * h))?;
            }
        }
        Ok(result)
    }
}

fn parallel_american_pairs(
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
) -> bool {
    let assets = u64::from(settings.asset_step_count.unsigned_abs());
    let steps = u64::from(settings.time_step_count.unsigned_abs());
    payoff.is_american()
        && assets * steps >= 65_536
        && context.natural_bsm_parameters().is_ok_and(|model| {
            model.volatility() > 0.0
                && [model.risk_free_curve(), model.dividend_curve()]
                    .into_iter()
                    .all(|curve| curve.nodes().is_empty() && curve.calendar_quote().is_none())
        })
        // Allow for two simultaneous legs and the saved central time slices.
        && validate_workspace(settings.scheme, 3 * (assets + 1), 3 * (steps + 4), 0).is_ok()
}

fn complete_greeks(
    option: &impl OptionContract,
    context: &PricingContext,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    mut result: PricingResult,
    parallel_pairs: bool,
    native: impl Fn(&PricingContext, GreeksRequest) -> Result<PricingResult> + Sync,
) -> Result<PricingResult> {
    if local_volatility::local_model(context).is_none() {
        return crate::risk::numerical::complete_greeks_with_pair(
            option,
            context,
            request,
            shifts,
            true,
            result,
            &native,
            |high, low, measures| {
                if !parallel_pairs || high.model_parameters() == low.model_parameters() {
                    return Ok(None);
                }
                let Some((pool, _)) = super::rollback_pool() else {
                    return Ok(None);
                };
                if pool.current_thread_index().is_some() {
                    return Ok(None);
                }
                let tokens = crate::cancellation::current();
                let evaluate = |scenario: &PricingContext| {
                    crate::cancellation::with_tokens(tokens.clone(), || native(scenario, measures))
                };
                let (high, low) = pool.install(|| rayon::join(|| evaluate(high), || evaluate(low)));
                Ok(Some((high?, low?)))
            },
        );
    }
    result = result.with_request(request, GreeksRequest::new([Greek::Delta, Greek::Gamma]));
    let request = local_volatility::validate_request(context, request)?;
    if context.valuation_time() == option.expiry_time() {
        return crate::risk::numerical::expiry_greeks(option, context, shifts, result, |context| {
            Ok(native(context, GreeksRequest::new([]))?.price())
        });
    }
    if request == GreeksRequest::new([]) || option.all_greeks_unavailable(context) {
        return Ok(result);
    }
    let missing_delta = request.has(Greek::Delta) && !result.has(Greek::Delta);
    let missing_gamma = request.has(Greek::Gamma) && !result.has(Greek::Gamma);
    let spot = context.spot_price();
    let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
    if (missing_delta || missing_gamma)
        && h > 0.0
        && spot > h
        && !option.at_spot_discontinuity(context, h)
    {
        let price = |spot| -> Result<f64> {
            let shifted = context.replace(crate::PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })?;
            Ok(native(&shifted, GreeksRequest::new([]))?.price())
        };
        let up = price(spot + h)?;
        let down = price(spot - h)?;
        if missing_delta {
            result.set(request, Greek::Delta, (up - down) / h / 2.0)?;
        }
        if missing_gamma {
            let gamma = ((up - result.price()) - (result.price() - down)) / h / h;
            result.set(request, Greek::Gamma, gamma)?;
        }
    }
    Ok(if option.at_spot_discontinuity(context, 0.0) {
        result
    } else {
        result.with_unavailable_stencils(request)
    })
}

fn vanilla_time_analysis(
    option: &(impl OptionContract + Sync),
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<Option<PricingResult>> {
    // An explicit time shift retains the scalar repricing contract and its grids.
    if shifts.time_shift.is_some()
        || diffusion::uses_business_time(context)
        || settings.scheme == FiniteDifferenceScheme::ExplicitEuler
        || ![Greek::Theta, Greek::Charm, Greek::Color]
            .into_iter()
            .any(|g| request.has(g))
    {
        return Ok(None);
    }
    let Ok(model) = context.bsm_parameters() else {
        return Ok(None);
    };
    if [model.risk_free_curve(), model.dividend_curve()]
        .into_iter()
        .any(|curve| !curve.nodes().is_empty())
    {
        return Ok(None);
    }
    let (Some(rate), Some(dividend)) = (
        model.risk_free_curve().exact_flat_rate(),
        model.dividend_curve().exact_flat_rate(),
    ) else {
        return Ok(None);
    };
    if model.volatility() == 0.0 {
        return Ok(None);
    }
    shifts.validate()?;
    validate_life(option, context)?;
    let Some((before, after)) = crate::risk::numerical::time_stencil(option, context, shifts)?
    else {
        return Ok(None);
    };
    let valuation = context.valuation_time();
    let expiry = option.expiry_time();
    let time = valuation.years_until(expiry);
    let mut slices = TimeSlices {
        before_at: before,
        before: valuation.years_until(before),
        after: valuation.years_until(after),
        before_values: None,
        after_values: None,
        remove_carry: false,
    };
    let dt = time / f64::from(settings.time_step_count);
    if slices.after >= time - 2.0 * dt || -slices.before > 15.0 * dt || slices.after > time / 8.0 {
        return Ok(None);
    }
    let concentrate = settings.asset_upper_boundary.is_none();
    let saved = match solve_model(
        expiry,
        payoff,
        context,
        settings,
        concentrate,
        context.spot_price(),
        model.volatility(),
        Some(&mut slices),
        None,
    ) {
        Ok(grid) => grid,
        Err(error @ Error::Io(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let mut result = saved.european_market_greeks(
        context,
        expiry,
        request,
        shifts,
        saved.evaluate(context, request, shifts)?,
    )?;
    let stencil = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let sample = |values: Option<Vec<f64>>, at: Timestamp| -> Result<[f64; 3]> {
        let (result, log_spot_scale) = if let Some(values) = values {
            let horizon = at.years_until(expiry);
            let elapsed = valuation.years_until(at);
            let (scenario, log_scale, log_spot) = if slices.remove_carry {
                let log_spot = (dividend - rate) * elapsed;
                let scenario = solution::at_spot(context, scaled(context.spot_price(), log_spot))?;
                (Some(scenario), rate * elapsed - dividend * time, log_spot)
            } else {
                (None, 0.0, 0.0)
            };
            let result = VanillaGrid {
                grid: solution::Grid::new(saved.grid.space.clone(), values),
                payoff,
                discount: [-rate * horizon, -dividend * horizon],
                value_log_discount: log_scale,
                relative_upper_error: saved.relative_upper_error,
            }
            .evaluate(scenario.as_ref().unwrap_or(context), stencil, shifts)?;
            (result, log_spot)
        } else if at == valuation {
            (saved.evaluate(context, stencil, shifts)?, 0.0)
        } else {
            return Err(unstable());
        };
        Ok([
            result.price(),
            crate::numerics::scaled_signed_cash(result.require(Greek::Delta)?, log_spot_scale),
            crate::numerics::scaled_signed_cash(
                result.require(Greek::Gamma)?,
                2.0 * log_spot_scale,
            ),
        ])
    };
    let (low, high) = match (
        sample(slices.before_values, before),
        sample(slices.after_values, after),
    ) {
        (Ok(low), Ok(high)) => (low, high),
        (Err(error @ Error::Io(_)), _) | (_, Err(error @ Error::Io(_))) => return Err(error),
        _ => return Ok(None),
    };
    let duration = before.years_until(after);
    for (index, greek) in [Greek::Theta, Greek::Charm, Greek::Color]
        .into_iter()
        .enumerate()
    {
        result.set(request, greek, (high[index] - low[index]) / duration)?;
    }
    let bumped_settings = FiniteDifferenceSettings {
        asset_upper_boundary: Some(saved.grid.space.upper),
        ..settings
    };
    complete_greeks(
        option,
        context,
        request,
        shifts,
        result,
        parallel_american_pairs(payoff, context, bumped_settings),
        |context, request| {
            native(
                option,
                payoff,
                context,
                bumped_settings,
                request,
                shifts,
                concentrate,
            )
            .map(|(result, _)| result)
        },
    )
    .map(Some)
}

fn analysis<I: OptionContract + Sync>(
    option: &I,
    context: &PricingContext,
    mut settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    parallel_pairs: bool,
    native: impl Fn(
        &I,
        &PricingContext,
        FiniteDifferenceSettings,
        GreeksRequest,
        NumericalShiftSettings,
    ) -> Result<(PricingResult, Option<f64>)>
    + Sync,
) -> Result<PricingResult> {
    shifts.validate()?;
    let (result, upper) = native(option, context, settings, request, shifts)?;
    if !diffusion::uses_business_time(context) {
        settings.asset_upper_boundary = upper.or(settings.asset_upper_boundary);
    }
    complete_greeks(
        option,
        context,
        request,
        shifts,
        result,
        parallel_pairs,
        |context, request| Ok(native(option, context, settings, request, shifts)?.0),
    )
}

fn barrier_analysis(
    option: &impl OptionContract,
    context: &PricingContext,
    mut settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    default_upper: impl FnOnce() -> f64,
    native: impl Fn(&PricingContext, FiniteDifferenceSettings, GreeksRequest) -> Result<PricingResult>
    + Sync,
) -> Result<PricingResult> {
    shifts.validate()?;
    if settings.asset_upper_boundary.is_none() && local_volatility::local_model(context).is_none() {
        let upper = default_upper();
        if upper.is_finite() {
            settings.asset_upper_boundary = Some(upper);
        }
    }
    complete_greeks(
        option,
        context,
        request,
        shifts,
        native(context, settings, request)?,
        false,
        |context, request| native(context, settings, request),
    )
}

macro_rules! engine_settings {
    ($engine:ident, $description:literal $(, $(#[$doc:meta])*)?) => {
        #[doc = $description]
        $($(#[$doc])*)?
        ///
        /// Distinct and shared instances support concurrent pricing.
        ///
        /// See [`Self::price`] for usage.
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct $engine {
            settings: FiniteDifferenceSettings,
        }
        impl $engine {
            /// Creates an engine after validating grid sizes and any explicit upper boundary.
            ///
            /// # Errors
            ///
            /// Returns [`Error::InvalidInput`] for fewer than three asset steps,
            /// nonpositive time steps, a grid exceeding the workspace or rollback work limit,
            /// or a nonpositive or non-finite upper boundary.
            /// Validation precedes storing settings; no grid is allocated here.
            /// See [`Self::price`] for engine-specific usage.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi::{FiniteDifferenceVanillaEngine, FiniteDifferenceSettings, FiniteDifferenceScheme};
            /// # fn main() -> kiyosi::Result<()> {
            /// let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            ///     asset_step_count: 400, scheme: FiniteDifferenceScheme::ImplicitEuler,
            ///     ..Default::default()
            /// })?;
            /// assert_eq!(engine.settings().asset_step_count, 400);
            /// # Ok(()) }
            /// ```
            pub fn new(settings: FiniteDifferenceSettings) -> Result<Self> {
                settings.validate()?;
                Ok(Self { settings })
            }
            /// Returns copied settings. See [`Self::new`] for an example.
            pub fn settings(&self) -> FiniteDifferenceSettings {
                self.settings
            }
        }
    };
}
engine_settings!(
    FiniteDifferenceAccumulatorEngine,
    "Prices trading-date accumulators using affine quantity layers and projected events."
);

engine_settings!(
    FiniteDifferenceVanillaEngine,
    "Prices European and American vanilla contracts with uniform or concentrated finite-difference grids.",
    ///
    /// Use [`VanillaBatchEngine`] for strikes or strike–expiry surfaces. Compatible BSM
    /// strikes share a rescaled solution with automatic boundaries. Compatible European
    /// expiries share a log-spot grid and propagation with separate damping. Compatible
    /// American surfaces share rollback on a normalized grid for flat-rate BSM or step
    /// factors on an absolute grid for local volatility. Other strikes or expiries may
    /// use separate solves. Check convergence across the strike range and expiries.
    ///
    /// # Pricing
    ///
    /// Prices a supported vanilla contract, returning intrinsic value at expiry.
    ///
    /// Local volatility supports European and American price, delta and gamma, holding the
    /// absolute surface fixed. Time knots are inserted; sigma is evaluated at
    /// substep midpoints. Time coverage and explicit-step stability are validated.
    /// Local-volatility boundaries use asymptotic values with American exercise where applicable; automatic domains
    /// use the surface maximum volatility and require domain convergence checks.
    /// Unsupported combinations return [`Error::UnsupportedOperation`], even at expiry.
    /// Accepts flat or nonconstant rate curves. Automatic domains concentrate nodes
    /// around the strike for non-explicit schemes. The grid uses log spot and an
    /// automatically selected positive lower boundary. Time marching starts from
    /// payoff kinks averaged in log spot. Automatic Rannacher grids for European
    /// contracts with constant volatility shift the terminal strike by the
    /// deterministic carry and apply the corresponding discount after rollback.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// ## Errors
    ///
    /// Returns [`Error::InvalidInput`] outside permitted valuation bounds or curve coverage,
    /// for clipped/degenerate grids or unstable explicit steps,
    /// and [`Error::ComputationFailure`] for numerical failures, including materially
    /// negative prices. Only rounding-sized negative prices are clipped to zero.
    ///
    /// # Greeks
    ///
    /// Returns spline delta/gamma and fixed-grid speed. For resolved flat-rate BSM
    /// European grids, default rate and volatility sensitivities can use the
    /// same solution's price and spot derivatives. Other Greeks use numerical differences.
    ///
    /// Local-volatility results provide delta and gamma only. Bumped contexts
    /// share the central spatial domain.
    /// At expiry, supported Greeks are exact payoff derivatives away from the strike;
    /// spot Greeks are undefined at the strike and time Greeks are absent.
    /// Uses [`NumericalShiftSettings`] for bumps; unavailable stencils leave Greeks absent.
    ///
    /// ## Errors
    ///
    /// Validates shifts before pricing; reuses [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Feasible bumped failures propagate.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use kiyosi::{FiniteDifferenceVanillaEngine, EuropeanOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext, PricingEngine, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let result = FiniteDifferenceVanillaEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]))?;
    /// assert!((result.price() - 9.227005508154036).abs() < 0.002);
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
);
engine_settings!(
    FiniteDifferenceDigitalEngine,
    "Prices European cash-or-nothing and asset-or-nothing contracts with finite differences.",
    ///
    /// # Greeks
    ///
    /// Returns spline spot Greeks and numerical completion with the shared central domain.
    ///
    /// Expiry Greeks follow terminal payoffs; time Greeks are absent. See [`Self::price`] for a pricing example.
    /// Local volatility supports only delta and gamma with the absolute surface fixed;
    /// other requested Greeks are absent.
    ///
    /// ## Errors
    ///
    /// Checks model applicability before shift validation, then reuses
    /// [`Self::price`] and [`crate::numerical_greeks`] errors.
);

engine_settings!(
    FiniteDifferenceBarrierEngine,
    "Prices single-barrier contracts with exact continuous edges and scheduled event projection."
);

engine_settings!(
    FiniteDifferenceDoubleBarrierEngine,
    "Prices double-barrier contracts with exact continuous edges and simultaneous scheduled events."
);

engine_settings!(
    FiniteDifferenceBinaryBarrierEngine,
    "Prices binary barriers and single touch/no-touch contracts with finite differences.",
    ///
    /// # Pricing
    ///
    /// Prices a binary barrier or single touch with BSM or local volatility.
    ///
    /// Continuous barriers are exact grid edges; scheduled observations project event cells.
    /// Prior at-hit payments have no remaining value; resolved cash flows use exact discounts.
    /// Accepts flat or nonconstant rate curves. Local volatility requires surface coverage only
    /// through the remaining stochastic horizon, including the last at-hit observation.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// ## Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve coverage,
    /// for impossible history, invalid grids,
    /// or unstable explicit steps. Returns [`Error::ComputationFailure`][crate::Error::ComputationFailure] for
    /// numerical failures.
    ///
    /// # Greeks
    ///
    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing observations or barriers leave affected measures absent.
    /// Expiry Greeks follow terminal payoffs. Before expiry at positive BSM volatility,
    /// barrier equality without a prior touch leaves spot derivatives absent; rate
    /// and volatility Greeks follow the resolved payoff.
    /// Local volatility supports only delta and gamma with the local-volatility surface held fixed;
    /// other requested Greeks are absent.
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
    /// # use kiyosi::OptionContract as _;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = TouchOption::cash(TouchOptionTerms::new(start, Date::new(2026, 1, 1)?.start_of_day(),
    ///     120.0, BarrierType::UpAndIn), 10.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceBinaryBarrierEngine::default();
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]))?;
    /// assert!(result.price() > 0.0);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
);

engine_settings!(
    FiniteDifferenceDoubleTouchEngine,
    "Prices cash double-touch and double-no-touch contracts with finite differences.",
    ///
    /// # Pricing
    ///
    /// Prices cash double-touch contracts using both inclusive boundaries and shared history.
    ///
    /// Fits continuous boundaries exactly and projects scheduled events simultaneously.
    /// Prior at-hit payments have zero remaining value; resolved cash uses exact discounts.
    /// Accepts BSM or local volatility with flat or nonconstant rate curves. Surface coverage
    /// follows the stochastic horizon after valuation.
    /// See [`crate::DoubleTouchOption::cash`] for construction and pricing.
    ///
    /// ## Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve coverage,
    /// for impossible history, invalid grids,
    /// or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure].
    ///
    /// # Greeks
    ///
    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing either boundary or an observation omit affected measures.
    /// Expiry Greeks follow terminal payoffs. Before expiry at positive BSM volatility,
    /// boundary equality without a prior touch leaves spot derivatives absent; rate
    /// and volatility Greeks follow the resolved payoff.
    /// Local volatility supports only delta and gamma with the local-volatility surface held fixed;
    /// other requested Greeks are absent.
    ///
    /// ## Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Validates shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # use kiyosi::OptionContract as _;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let mut terms = DoubleTouchOptionTerms::new(start, Date::new(2026, 1, 1)?.start_of_day(),
    ///     80.0, 120.0, DoubleBarrierType::KnockOut);
    /// terms.touch_state = Some(BarrierTouchState::Untouched);
    /// let option = DoubleTouchOption::cash(terms, 1.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let result = FiniteDifferenceDoubleTouchEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]))?;
    /// assert!((result.price() - 0.36726156351907174).abs() < 0.00001);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
);

engine_settings!(
    FiniteDifferenceAutocallableEngine,
    "Prices snowball variants and Phoenix notes with event-projected finite differences; defaults to 800 asset and 400 time steps."
);

macro_rules! vanilla_pricing {
    ($option:ty, $american:literal) => {
        impl PricingEngine<$option> for FiniteDifferenceVanillaEngine {
            fn greek_method(
                &self,
                option: &$option,
                context: &PricingContext,
            ) -> Result<crate::GreekMethod> {
                let method = local_volatility::greek_method(option, context)?;
                if context.valuation_time() == option.expiry_time() {
                    return Ok(crate::GreekMethod::Native);
                }
                Ok(method)
            }

            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                local_volatility::validate_model(context)
            }
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                Ok(native(
                    option,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    context,
                    self.settings,
                    GreeksRequest::new([]),
                    Default::default(),
                    self.settings.asset_upper_boundary.is_none(),
                )?
                .0
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
                if context.valuation_time() == option.expiry_time() {
                    shifts.validate()?;
                    let supported = if local_volatility::local_model(context).is_some() {
                        GreeksRequest::new([Greek::Delta, Greek::Gamma])
                    } else {
                        GreeksRequest::all()
                    };
                    let result = PricingResult::new(self.price(option, context)?)?
                        .with_request(request, supported);
                    return crate::risk::analytics::vanilla_expiry_greeks(
                        option.option_type(),
                        option.strike(),
                        context,
                        result,
                    );
                }
                let payoff = Payoff::vanilla(option.option_type(), option.strike(), $american);
                if let Some(result) =
                    vanilla_time_analysis(option, payoff, context, self.settings, request, shifts)?
                {
                    return Ok(result);
                }
                analysis(
                    option,
                    context,
                    self.settings,
                    request,
                    shifts,
                    parallel_american_pairs(payoff, context, self.settings),
                    |option, context, settings, request, shifts| {
                        native(
                            option,
                            payoff,
                            context,
                            settings,
                            request,
                            shifts,
                            self.settings.asset_upper_boundary.is_none(),
                        )
                    },
                )
            }
        }
    };
}
vanilla_pricing!(EuropeanOption, false);
vanilla_pricing!(AmericanOption, true);

impl PricingEngine<DigitalOption> for FiniteDifferenceDigitalEngine {
    fn greek_method(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        local_volatility::greek_method(option, context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    /// Prices a digital, settling strictly in the money at expiry.
    ///
    /// Accepts BSM or local volatility with flat or nonconstant rate curves.
    /// Time marching uses cell-averaged payoffs.
    /// Automatic Rannacher grids with constant volatility shift the terminal strike
    /// by the deterministic carry and apply the corresponding discount after rollback.
    /// The grid uses log spot with an automatically selected positive lower boundary.
    /// Automatic domains with non-explicit schemes concentrate nodes around the strike.
    /// See [`FiniteDifferenceVanillaEngine::price_with_greeks`] for context construction.
    ///
    /// # Errors
    ///
    /// Reuses [`FiniteDifferenceVanillaEngine::price`] errors, including
    /// [`Error::UnsupportedOperation`] for unsupported models. Payoff-bound violations return
    /// [`Error::ComputationFailure`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{PricingEngine, FiniteDifferenceDigitalEngine, DigitalOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// assert!((FiniteDifferenceDigitalEngine::default().price(&option, &context)? - 4.945810910532236).abs() < 0.001);
    /// # Ok(()) }
    /// ```
    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Ok(native(
            option,
            Payoff::digital(option),
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
            self.settings.asset_upper_boundary.is_none(),
        )?
        .0
        .price())
    }
    fn price_with_greeks_with_settings(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<DigitalOption>>::validate_applicability(self, context)?;
        let payoff = Payoff::digital(option);
        analysis(
            option,
            context,
            self.settings,
            request,
            shifts,
            false,
            |option, context, settings, request, shifts| {
                native(
                    option,
                    payoff,
                    context,
                    settings,
                    request,
                    shifts,
                    self.settings.asset_upper_boundary.is_none(),
                )
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strike_batch_size_checks_workspace_before_allocating() -> Result<()> {
        let bytes_per_strike = size_of::<usize>() + 3 * size_of::<f64>();
        let limit =
            usize::try_from(WORKSPACE_BUDGET / 4).expect("budget fits usize") / bytes_per_strike;
        validate_finite_difference_strike_batch_size(limit)?;
        assert!(matches!(
            validate_finite_difference_strike_batch_size(limit + 1),
            Err(Error::InvalidInput(message)) if message.contains("strike batch exceeds workspace")
        ));
        Ok(())
    }

    #[test]
    fn surface_size_bounds_both_axes_and_empty_surfaces() {
        assert!(validate_finite_difference_surface_size(1000, 16_773).is_ok());
        assert!(matches!(
            validate_finite_difference_surface_size(1000, 16_774),
            Err(Error::InvalidInput(_))
        ));
        for (expiries, strikes) in [(usize::MAX, 0), (0, usize::MAX), (usize::MAX, usize::MAX)] {
            assert!(matches!(
                validate_finite_difference_surface_size(expiries, strikes),
                Err(Error::InvalidInput(_))
            ));
        }
        assert!(validate_finite_difference_surface_size(0, 0).is_ok());
    }
    use crate::{CancellationToken, Date, LocalVolatilityParameters, LocalVolatilitySurface};

    #[test]
    fn partial_surface_bound_scan_checks_cancellation() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let horizon = start.add_days(180)?;
        let surface = LocalVolatilitySurface::new(
            vec![start, start.add_days(365)?],
            vec![80.0, 120.0],
            vec![vec![0.2, 0.3], vec![0.4, 0.5]],
        )?;
        let model = LocalVolatilityParameters::new(0.0, 0.0, surface)?;
        let context = PricingContext::new(model, 100.0, start)?;
        let curve = Curve::new(context.model_parameters().risk_free_curve(), start, horizon)?;
        let token = CancellationToken::default();
        let result = token.run(|| {
            token.cancel();
            let result = diffusion_log_width(&context, horizon, &curve, &curve);
            assert!(
                matches!(result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
            );
            Ok(())
        });
        assert!(
            matches!(result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        Ok(())
    }
}
