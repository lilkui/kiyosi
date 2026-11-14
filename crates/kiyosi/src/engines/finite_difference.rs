use super::{Payoff, curve::Curve};
use crate::numerics::{scaled_cash as scaled, weighted_cdf};
use crate::{
    AmericanOption, DigitalOption, Error, EuropeanOption, Greek, GreeksRequest,
    NumericalShiftSettings, OptionContract, OptionType, PricingContext, PricingEngine,
    PricingResult, Result, engines::analytic::european::black_scholes_arguments,
};

mod accumulator;
mod autocallable;
mod barrier;
mod binary_barrier;
mod diffusion;
mod grid;
pub(crate) mod heston;
mod local_volatility;
mod payoff;
mod step;
use diffusion::Diffusion;
use grid::{SpatialGrid, TimeGrid};
use step::Stepper;

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

/// Configures the spatial domain and time grid, validated at engine construction.
///
/// Grid sizes have no additional cap; pricing is subject to memory and runtime.
/// Allocation exhaustion follows the library-wide allocator contract.
/// See [`FiniteDifferenceVanillaEngine::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiniteDifferenceSettings {
    /// Sets at least three spatial steps; defaults to 800.
    /// Continuous barrier grids use all intervals between their actual domain edges.
    pub asset_step_count: i32,
    /// Sets at least one time step; defaults to 400. Curve/surface knots and observations are added.
    pub time_step_count: i32,
    /// Selects time marching; defaults to [`FiniteDifferenceScheme::Rannacher`].
    pub scheme: FiniteDifferenceScheme,
    /// Sets a finite positive upper spot boundary, or uses the contract's automatic domain.
    ///
    /// Pricing requires the boundary to exceed spot, strike and any barrier level.
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
    /// fewer than one time interval, a nonpositive or non-finite boundary.
    /// Leaves the settings unchanged. Domain coverage and scheme stability are checked at pricing.
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
        if let Some(boundary) = self.asset_upper_boundary {
            crate::error::finite(boundary, true, "asset_upper_boundary")?;
        }
        Ok(())
    }
}

fn invalid_grid(message: &'static str) -> Error {
    Error::InvalidInput(message.into())
}
fn unstable() -> Error {
    Error::ComputationFailure("finite-difference system is numerically unstable".into())
}

fn time_to_expiry(option: &impl OptionContract, context: &PricingContext) -> Result<f64> {
    let valuation = context.valuation_time();
    let expiry = option.expiry_time();
    crate::instruments::monitoring::validate_life(option, context)?;
    Ok(valuation.years_until(expiry))
}

fn flat_rates(context: &PricingContext) -> Result<()> {
    local_volatility::validate_model(context)?;
    for curve in [
        context.model_parameters().risk_free_curve(),
        context.model_parameters().dividend_curve(),
    ] {
        if curve.exact_flat_rate().is_none() {
            return Err(Error::UnsupportedOperation(
                "digital finite differences require constant rates".into(),
            ));
        }
    }
    Ok(())
}

fn automatic_upper(
    payoff: Payoff,
    context: &PricingContext,
    time: f64,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    let relevant = context.spot_price().max(payoff.strike);
    if !payoff.is_american() {
        return Ok(4.0 * relevant);
    }
    diffusion_upper(relevant, context, time, risk, dividend)
}

fn diffusion_upper(
    relevant: f64,
    context: &PricingContext,
    time: f64,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    let sigma = match context.model_parameters() {
        crate::ModelParameters::Heston(_) => {
            return Err(Error::UnsupportedOperation(
                "finite differences do not support Heston".into(),
            ));
        }
        crate::ModelParameters::BlackScholesMerton(m) => m.volatility(),
        crate::ModelParameters::LocalVolatility(m) => {
            let surface = m.surface();
            let start = context.valuation_time();
            let offset = surface.times()[0].years_until(start);
            let mut maximum: f64 = 0.0;
            // Bilinear sigma peaks at a spot/time node or a clipped time endpoint.
            for &spot in surface.spots() {
                maximum = maximum
                    .max(surface.at_year(offset, spot))
                    .max(surface.at_year(offset + time, spot));
            }
            for (&node, row) in surface.times().iter().zip(surface.values()) {
                let elapsed = start.years_until(node);
                if elapsed > 0.0 && elapsed < time {
                    maximum = row.iter().copied().fold(maximum, f64::max);
                }
            }
            maximum
        }
    };
    let drift = risk
        .ends()
        .chain(dividend.ends())
        .map(|end| {
            (-risk.log_factor(0.0, end) + dividend.log_factor(0.0, end) - 0.5 * sigma * sigma * end)
                .abs()
        })
        .fold(0.0, f64::max);
    let width = 4.0 * sigma * time.sqrt() + drift;
    Ok(if width <= 4.0_f64.ln() {
        4.0 * relevant
    } else {
        (relevant.ln() + width).exp()
    })
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
    if crate::engines::deterministic::enabled(context) {
        return Ok((
            PricingResult::new(crate::engines::deterministic::vanilla(
                option, payoff, context,
            )?)?,
            None,
        ));
    }
    diffusion::with_diffusion!(context, |diffusion| {
        native_model(
            option,
            payoff,
            context,
            settings,
            request,
            shifts,
            concentrate,
            diffusion,
        )
    })
}

#[expect(
    clippy::float_cmp,
    clippy::too_many_arguments,
    reason = "Explicit payoff, grid policy, model and stencil inputs; exact weights select the digital correction."
)]
fn native_model<D: Diffusion>(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    concentrate: bool,
    diffusion: D,
) -> Result<(PricingResult, Option<f64>)> {
    let time = time_to_expiry(option, context)?;
    let spot = context.spot_price();
    if time == 0.0 {
        return Ok((PricingResult::new(payoff.value(spot))?, None));
    }
    let model = context.model_parameters();
    let expiry = option.expiry_time();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let upper = match settings.asset_upper_boundary {
        Some(upper) => upper,
        None if D::LOCAL => {
            diffusion_upper(spot.max(payoff.strike), context, time, &risk, &dividend)?
        }
        None => automatic_upper(payoff, context, time, &risk, &dividend)?,
    };
    let mut space = SpatialGrid::new(
        settings,
        upper,
        if payoff.is_american() {
            payoff.strike
        } else {
            0.0
        },
        spot.max(payoff.strike),
    )?;
    if !payoff.is_american()
        && (!payoff.is_digital() || D::LOCAL)
        && concentrate
        && settings.scheme != FiniteDifferenceScheme::ExplicitEuler
    {
        let scale = payoff.strike * (0.1 * diffusion.sigma(0.0, spot) * time.sqrt()).max(1e-4);
        if scale > 0.0 && scale.is_finite() {
            grid::concentrate(&mut space, vec![payoff.strike], scale)?;
        }
    }
    let time_grid = diffusion::time_grid(
        context,
        expiry,
        settings.time_step_count,
        &risk,
        &dividend,
        &[],
    )?;
    let theta = settings.scheme.theta();
    if !D::LOCAL {
        time_grid.validate_stability(
            &space,
            diffusion.sigma(0.0, 0.0),
            theta,
            !payoff.is_american(),
            &risk,
            &dividend,
        )?;
    }
    let mut old: Vec<_> = (0..=space.count)
        .map(|i| {
            if !space.nodes.is_empty() {
                return grid::project_in_coordinates(&space, &[payoff.strike], i, |asset| {
                    [payoff.value(asset), 0.0]
                })[0];
            }
            let asset = space.asset(i);
            if !payoff.is_digital() {
                return payoff.value(asset);
            }
            let low = asset - 0.5 * space.spacing;
            let high = asset + 0.5 * space.spacing;
            let left = if payoff.sign > 0.0 {
                low.max(payoff.strike)
            } else {
                low
            };
            let right = if payoff.sign > 0.0 {
                high
            } else {
                high.min(payoff.strike)
            };
            if left >= right {
                0.0
            } else {
                (right - left) / space.spacing
                    * payoff.payout().unwrap_or(f64::midpoint(left, right))
            }
        })
        .collect();
    let can_exercise = payoff.is_american()
        && (model.risk_free_curve().exact_flat_rate().is_none()
            || model.dividend_curve().exact_flat_rate().is_none()
            || payoff.sign < 0.0
            || dividend.rate(0.0) > 0.0
            || risk.rate(0.0) < 0.0);
    let exercise = if can_exercise {
        old.clone()
    } else {
        Vec::new()
    };
    let mut next = vec![0.0; old.len()];
    let mut stepper = diffusion.stepper(space.count);
    let mut previous_boundary_time = time;
    let mut put_boundary = payoff.strike;
    let mut log_carry_error = 0.0;
    let mut valid_carry_error = true;
    let mut damping = 2;
    for step in (0..time_grid.widths.len()).rev() {
        crate::cancellation::check()?;
        for (current, dt, weight) in time_grid.substeps(step, settings.scheme, &mut damping) {
            let carry = if payoff.payout().is_some() {
                risk.rate(if D::LOCAL { current + 0.5 * dt } else { 0.0 })
            } else {
                dividend.rate(if D::LOCAL { current + 0.5 * dt } else { 0.0 })
            };
            if payoff.is_digital() && theta == 0.5 && (weight != 0.5 || carry < 0.0) {
                let exponent = carry * dt;
                let implicit = weight * exponent;
                let explicit = -(1.0 - weight) * exponent;
                if !exponent.is_finite() || implicit <= -1.0 || explicit <= -1.0 {
                    valid_carry_error = false;
                } else {
                    log_carry_error += (exponent + explicit.ln_1p() - implicit.ln_1p()).max(0.0);
                }
            }
            let tau = time - current;
            let middle = current + 0.5 * dt;
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let risk_log = risk.log_factor(time - tau, time);
            let dividend_log = dividend.log_factor(time - tau, time);
            let high =
                payoff::boundary_value(payoff, upper, [risk_log, dividend_log], diffusion, tau);
            let low = if payoff.sign > 0.0 || (payoff.is_digital() && payoff.payout().is_none()) {
                0.0
            } else if payoff.is_american() {
                put_boundary = payoff.strike.max(scaled(
                    put_boundary,
                    risk.log_factor(current, previous_boundary_time),
                ));
                previous_boundary_time = current;
                put_boundary
            } else {
                scaled(payoff.payout().unwrap_or(payoff.strike), risk_log)
            };
            diffusion.advance(
                &mut stepper,
                &old,
                &mut next,
                &space,
                middle,
                [dt, weight, rate, q],
                [low, high],
            )?;
            if can_exercise {
                for i in 1..space.count {
                    next[i] = next[i].max(exercise[i]);
                }
            }
            std::mem::swap(&mut old, &mut next);
        }
    }
    let relative_carry_error = if valid_carry_error && log_carry_error.exp_m1().is_finite() {
        log_carry_error.exp_m1()
    } else {
        0.0
    };
    let curvature = space.curvature(&old);
    let price_at = |spot| {
        payoff::checked_price(
            payoff,
            spot,
            space.sample(&old, &curvature, spot)[0],
            [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)],
            relative_carry_error,
        )
    };
    let mut result = PricingResult::new(price_at(spot)?)?;
    if request.has(Greek::Delta) || request.has(Greek::Gamma) {
        let sample = space.sample(&old, &curvature, spot);
        result.set(request, Greek::Delta, sample[1])?;
        result.set(request, Greek::Gamma, sample[2])?;
    }
    if request.has(Greek::Speed) {
        let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
        if let Some(speed) = space.speed(&old, &curvature, spot, h) {
            price_at(spot + h)?;
            price_at(spot - h)?;
            result.set(request, Greek::Speed, speed)?;
        }
    }
    Ok((result, Some(upper)))
}

fn complete_greeks(
    option: &impl OptionContract,
    context: &PricingContext,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    mut result: PricingResult,
    native: impl Fn(&PricingContext, GreeksRequest) -> Result<PricingResult>,
) -> Result<PricingResult> {
    if local_volatility::local_model(context).is_none() {
        return crate::risk::numerical::complete_greeks(
            option, context, request, shifts, true, result, native,
        );
    }
    result = result.with_request(request, GreeksRequest::new([Greek::Delta, Greek::Gamma]));
    let request = local_volatility::validate_request(context, request)?;
    if context.valuation_time() == option.expiry_time() {
        return crate::risk::numerical::expiry_greeks(option, context, shifts, result, |context| {
            Ok(native(context, GreeksRequest::new([]))?.price())
        });
    }
    if option.all_greeks_unavailable(context) {
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

fn analysis<I: OptionContract>(
    option: &I,
    context: &PricingContext,
    mut settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    native: impl Fn(
        &I,
        &PricingContext,
        FiniteDifferenceSettings,
        GreeksRequest,
        NumericalShiftSettings,
    ) -> Result<(PricingResult, Option<f64>)>,
) -> Result<PricingResult> {
    shifts.validate()?;
    local_volatility::validate_request(context, request)?;
    let (result, upper) = native(option, context, settings, request, shifts)?;
    settings.asset_upper_boundary = upper.or(settings.asset_upper_boundary);
    complete_greeks(
        option,
        context,
        request,
        shifts,
        result,
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
    native: impl Fn(&PricingContext, FiniteDifferenceSettings, GreeksRequest) -> Result<PricingResult>,
) -> Result<PricingResult> {
    shifts.validate()?;
    local_volatility::validate_request(context, request)?;
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
        /// See [`Self::price_with_greeks`] for usage.
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
            /// nonpositive time steps, or a nonpositive or non-finite upper boundary or spot bump.
            /// Validation precedes storing settings; no grid is allocated here.
            /// See [`Self::price_with_greeks`] for engine-specific usage.
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
    /// Accepts flat or nonconstant rate curves. American automatic domains expand with volatility
    /// and maturity and use sinh coordinates. With automatic domains, non-explicit
    /// European grids concentrate nodes around the strike and average the payoff kink.
    /// Explicit upper boundaries retain uniform European spot grids.
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
    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    ///
    /// Local-volatility results provide delta and gamma; other requested Greeks remain
    /// absent. Bumped contexts share the central spatial domain.
    /// At expiry, supported Greeks are exact payoff derivatives away from the strike;
    /// spot Greeks are undefined at the strike and time Greeks are absent.
    /// Uses [`NumericalShiftSettings`] for bumps; unavailable stencils leave Greeks absent.
    ///
    /// ## Errors
    ///
    /// Validates shifts before pricing; reuses [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Feasible bumped failures propagate.
    /// Inputs and engine settings remain unchanged on success and failure.
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
    "Prices European cash-or-nothing and asset-or-nothing contracts with flat-rate finite differences.",
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
    /// Checks flat-rate applicability before shift validation, then reuses
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
    /// Expiry Greeks follow terminal payoffs. Before expiry, a current barrier equality without a prior touch suppresses Greeks.
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
    /// follows the remaining stochastic horizon, not a previously settled payment.
    /// See [`crate::DoubleTouchOption::cash`] for construction and pricing.
    ///
    /// ## Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside life or required curve coverage,
    /// for unknown or impossible history, invalid grids,
    /// or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`][crate::Error::ComputationFailure].
    ///
    /// # Greeks
    ///
    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing either boundary or an observation omit affected measures.
    /// Expiry Greeks follow terminal payoffs. Before expiry, current boundary equality without a prior touch suppresses Greeks.
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
                shifts.validate()?;
                local_volatility::validate_request(context, request)?;
                if context.valuation_time() == option.expiry_time() {
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
                analysis(
                    option,
                    context,
                    self.settings,
                    request,
                    shifts,
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

impl FiniteDifferenceDigitalEngine {
    /// Prices a digital using cell-averaged terminal payoffs, settling strictly in the money at expiry.
    ///
    /// Rejects nonconstant rate curves before life validation, including at expiry.
    /// Accepts BSM or local volatility, with cell-averaged digital payoffs in both models.
    /// Local-volatility automatic domains concentrate nodes around the strike for
    /// non-explicit schemes. BSM and explicit upper boundaries retain uniform spot grids.
    /// See [`FiniteDifferenceVanillaEngine::price_with_greeks`] for context construction.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves, then reuses
    /// [`FiniteDifferenceVanillaEngine::price`] errors. Payoff-bound violations return
    /// [`Error::ComputationFailure`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{FiniteDifferenceDigitalEngine, DigitalOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// assert!((FiniteDifferenceDigitalEngine::default().price(&option, &context)? - 4.945810910532236).abs() < 0.001);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        flat_rates(context)?;
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
}

impl PricingEngine<DigitalOption> for FiniteDifferenceDigitalEngine {
    fn greek_method(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<DigitalOption>>::validate_applicability(self, context)?;
        local_volatility::greek_method(option, context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context)
    }
    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
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
