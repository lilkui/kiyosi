use crate::{
    AmericanOption, DigitalOption, Error, EuropeanOption, Greek, GreeksRequest,
    NumericalShiftSettings, OptionContract, OptionType, PricingContext, PricingEngine,
    PricingResult, Result,
    engines::analytic::{
        barrier::scaled_cash as scaled,
        european::{black_scholes_arguments, weighted_cdf},
    },
};

mod accumulator;
mod autocallable;
mod barrier;
mod binary_barrier;
mod grid;
mod step;
use grid::{Curve, SpatialGrid, TimeGrid};
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
    /// Uses Crank-Nicolson with two initial intervals replaced by implicit half-steps.
    #[default]
    CrankNicolson,
}

impl FiniteDifferenceScheme {
    fn theta(self) -> f64 {
        match self {
            Self::ExplicitEuler => 0.0,
            Self::ImplicitEuler => 1.0,
            Self::CrankNicolson => 0.5,
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
    pub asset_step_count: i32,
    /// Sets at least one time step; defaults to 200. Curve knots and barrier observations are added.
    pub time_step_count: i32,
    /// Selects time marching; defaults to damped Crank-Nicolson.
    pub scheme: FiniteDifferenceScheme,
    /// Sets a finite positive upper spot boundary, or uses the contract's automatic domain.
    ///
    /// Pricing requires the boundary to exceed spot, strike and any barrier level.
    /// Explicit domains need convergence checks.
    pub asset_upper_boundary: Option<f64>,
}

impl Default for FiniteDifferenceSettings {
    fn default() -> Self {
        Self {
            asset_step_count: 800,
            time_step_count: 200,
            scheme: Default::default(),
            asset_upper_boundary: None,
        }
    }
}

impl FiniteDifferenceSettings {
    fn validate(self) -> Result<()> {
        if self.asset_step_count < 3
            || self.time_step_count < 1
            || self
                .asset_upper_boundary
                .is_some_and(|v| !v.is_finite() || v <= 0.0)
        {
            return Err(invalid_grid("finite-difference settings are invalid"));
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

#[derive(Clone, Copy)]
struct Payoff {
    strike: f64,
    sign: f64,
    american: bool,
    digital: bool,
    payout: Option<f64>,
}

impl Payoff {
    fn value(self, spot: f64) -> f64 {
        if self.digital {
            if self.sign * (spot - self.strike) > 0.0 {
                self.payout.unwrap_or(spot)
            } else {
                0.0
            }
        } else {
            (self.sign * (spot - self.strike)).max(0.0)
        }
    }
}

fn time_to_expiry(option: &impl OptionContract, context: &PricingContext) -> Result<f64> {
    let valuation = context.valuation_time();
    let expiry = option.expiry_date().start_of_day();
    if valuation < option.effective_date().start_of_day() || valuation > expiry {
        return Err(Error::InvalidInput(
            "valuation must be within contract life".into(),
        ));
    }
    Ok(valuation.years_until(expiry))
}

fn flat_rates(context: &PricingContext) -> Result<()> {
    context.model_parameters().risk_free_rate()?;
    context.model_parameters().dividend_yield()?;
    Ok(())
}

fn automatic_upper(
    payoff: Payoff,
    context: &PricingContext,
    time: f64,
    risk: &Curve,
    dividend: &Curve,
) -> f64 {
    let relevant = context.spot_price().max(payoff.strike);
    if !payoff.american {
        return 4.0 * relevant;
    }
    diffusion_upper(relevant, context, time, risk, dividend)
}

fn diffusion_upper(
    relevant: f64,
    context: &PricingContext,
    time: f64,
    risk: &Curve,
    dividend: &Curve,
) -> f64 {
    let sigma = context.model_parameters().volatility();
    let drift = risk
        .ends()
        .chain(dividend.ends())
        .map(|end| {
            (-risk.log_factor(0.0, end) + dividend.log_factor(0.0, end) - 0.5 * sigma * sigma * end)
                .abs()
        })
        .fold(0.0, f64::max);
    let width = 4.0 * sigma * time.sqrt() + drift;
    if width <= 4.0_f64.ln() {
        4.0 * relevant
    } else {
        (relevant.ln() + width).exp()
    }
}

fn native(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    let time = time_to_expiry(option, context)?;
    let spot = context.spot_price();
    if time == 0.0 {
        return PricingResult::new(payoff.value(spot));
    }
    let model = context.model_parameters();
    let sigma = model.volatility();
    let expiry = option.expiry_date().start_of_day();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let upper = settings
        .asset_upper_boundary
        .unwrap_or_else(|| automatic_upper(payoff, context, time, &risk, &dividend));
    let space = SpatialGrid::new(
        settings,
        upper,
        if payoff.american { payoff.strike } else { 0.0 },
        spot.max(payoff.strike),
    )?;
    let time_grid = TimeGrid::new(time, settings.time_step_count, &risk, &dividend);
    let theta = settings.scheme.theta();
    time_grid.validate_stability(&space, sigma, theta, !payoff.american, &risk, &dividend)?;
    let terminal = |asset| {
        if !payoff.digital {
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
            (right - left) / space.spacing * payoff.payout.unwrap_or(0.5 * (left + right))
        }
    };
    let mut old: Vec<_> = (0..=space.count)
        .map(|i| terminal(space.asset(i)))
        .collect();
    let can_exercise = payoff.american
        && (!model.risk_free_curve().is_flat()
            || !model.dividend_curve().is_flat()
            || payoff.sign < 0.0
            || dividend.rate(0.0) > 0.0
            || risk.rate(0.0) < 0.0);
    let exercise = if can_exercise {
        old.clone()
    } else {
        Vec::new()
    };
    let mut next = vec![0.0; old.len()];
    let mut stepper = Stepper::new(space.count);
    let mut previous_boundary_time = time;
    let mut put_boundary = payoff.strike;
    let mut log_carry_error = 0.0;
    let mut valid_carry_error = true;
    for step in (0..time_grid.widths.len()).rev() {
        let damp = theta == 0.5 && step + 2 >= time_grid.widths.len();
        let count = if damp { 2 } else { 1 };
        let weight = if damp { 1.0 } else { theta };
        let dt = time_grid.widths[step] / f64::from(count);
        let carry = if payoff.payout.is_some() {
            risk.rate(0.0)
        } else {
            dividend.rate(0.0)
        };
        if payoff.digital && theta == 0.5 && (weight != 0.5 || carry < 0.0) {
            let exponent = carry * dt;
            let implicit = weight * exponent;
            let explicit = -(1.0 - weight) * exponent;
            if !exponent.is_finite() || implicit <= -1.0 || explicit <= -1.0 {
                valid_carry_error = false;
            } else {
                log_carry_error +=
                    f64::from(count) * (exponent + explicit.ln_1p() - implicit.ln_1p()).max(0.0);
            }
        }
        for substep in 0..count {
            let offset = dt * f64::from(count - substep - 1);
            let current = time_grid.times[step] + offset;
            let tau = time - time_grid.times[step] - offset;
            let middle = current + 0.5 * dt;
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let risk_log = risk.log_factor(time - tau, time);
            let dividend_log = dividend.log_factor(time - tau, time);
            let (d1, d2) = black_scholes_arguments(
                upper,
                payoff.strike,
                dividend_log - risk_log,
                sigma,
                tau.sqrt(),
            );
            let high = if payoff.digital {
                if let Some(amount) = payoff.payout {
                    weighted_cdf(amount, risk_log, payoff.sign * d2)
                } else {
                    weighted_cdf(upper, dividend_log, payoff.sign * d1)
                }
            } else {
                let continuation = payoff.sign
                    * (weighted_cdf(upper, dividend_log, payoff.sign * d1)
                        - weighted_cdf(payoff.strike, risk_log, payoff.sign * d2));
                if payoff.american {
                    continuation.max(payoff.value(upper))
                } else {
                    continuation
                }
            };
            let low = if payoff.sign > 0.0 || (payoff.digital && payoff.payout.is_none()) {
                0.0
            } else if payoff.american {
                put_boundary = payoff.strike.max(scaled(
                    put_boundary,
                    risk.log_factor(current, previous_boundary_time),
                ));
                previous_boundary_time = current;
                put_boundary
            } else {
                scaled(payoff.payout.unwrap_or(payoff.strike), risk_log)
            };
            stepper.advance(
                &old,
                &mut next,
                &space,
                sigma,
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
    let price_at = |s| -> Result<f64> {
        let value = space.sample(&old, &curvature, s)[0];
        if !value.is_finite() {
            return Err(unstable());
        }
        if !payoff.digital {
            return Ok(value.max(if payoff.american {
                payoff.value(s)
            } else {
                0.0
            }));
        }
        let bound = scaled(
            payoff.payout.unwrap_or(s),
            if payoff.payout.is_some() {
                risk.log_factor(0.0, time)
            } else {
                dividend.log_factor(0.0, time)
            },
        );
        let tolerance = 64.0 * f64::EPSILON * bound;
        let carry_error = bound * relative_carry_error;
        if !value.is_finite()
            || value < -tolerance
            || value - bound
                > tolerance
                    + if carry_error.is_finite() {
                        carry_error
                    } else {
                        0.0
                    }
        {
            return Err(Error::ComputationFailure(
                "finite-difference digital price violates payoff bounds".into(),
            ));
        }
        Ok(value.clamp(0.0, bound))
    };
    let mut result = PricingResult::new(price_at(spot)?)?;
    if request.has(Greek::Delta) || request.has(Greek::Gamma) {
        let sample = space.sample(&old, &curvature, spot);
        result.set(request, Greek::Delta, sample[1])?;
        result.set(request, Greek::Gamma, sample[2])?;
    }
    if request.has(Greek::Speed) {
        let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift);
        if h > 0.0 && spot > h && spot + h < upper {
            price_at(spot + h)?;
            price_at(spot - h)?;
            result.set(
                request,
                Greek::Speed,
                (space.sample(&old, &curvature, spot + h)[2]
                    - space.sample(&old, &curvature, spot - h)[2])
                    / (2.0 * h),
            )?;
        }
    }
    Ok(result)
}

fn analysis(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    mut settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    request.validate()?;
    shifts.validate()?;
    let time = time_to_expiry(option, context)?;
    if settings.asset_upper_boundary.is_none() && time > 0.0 {
        let expiry = option.expiry_date().start_of_day();
        let model = context.model_parameters();
        let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
        let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
        let upper = automatic_upper(payoff, context, time, &risk, &dividend);
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
        |context, request| native(option, payoff, context, settings, request, shifts),
    )
}

macro_rules! engine_settings {
    ($engine:ident, $description:literal) => {
        engine_settings!($engine, $description, FiniteDifferenceSettings::default());
    };
    ($engine:ident, $description:literal, $defaults:expr) => {
        #[doc = $description]
        ///
        /// Owns immutable settings; distinct and shared instances support concurrent pricing.
        ///
        /// See [`Self::price_with_greeks`] for usage.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $engine {
            settings: FiniteDifferenceSettings,
        }
        impl Default for $engine {
            fn default() -> Self {
                Self {
                    settings: $defaults,
                }
            }
        }
        impl $engine {
            /// Creates an engine after validating grid sizes and any explicit upper boundary.
            ///
            /// # Errors
            ///
            /// Returns [`Error::InvalidInput`] for fewer than three asset steps,
            /// nonpositive time steps or a nonpositive/non-finite explicit upper boundary.
            /// Validation precedes storing settings; no grid is allocated here.
            /// See [`Self::price_with_greeks`] for engine-specific usage.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi_core::{FiniteDifferenceVanillaEngine, FiniteDifferenceSettings, FiniteDifferenceScheme};
            /// # fn main() -> kiyosi_core::Result<()> {
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
    "Prices European and American vanilla contracts with uniform or sinh finite-difference grids."
);
engine_settings!(
    FiniteDifferenceDigitalEngine,
    "Prices European cash-or-nothing and asset-or-nothing contracts with flat-rate finite differences."
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
    "Prices binary barriers and single touch/no-touch contracts with finite differences."
);

engine_settings!(
    FiniteDifferenceDoubleTouchEngine,
    "Prices cash double-touch and double-no-touch contracts with finite differences."
);

engine_settings!(
    FiniteDifferenceAutocallableEngine,
    "Prices snowball variants and Phoenix notes with event-projected finite differences; defaults to 800 asset and 2,000 time steps.",
    FiniteDifferenceSettings {
        time_step_count: 2000,
        ..Default::default()
    }
);

impl FiniteDifferenceVanillaEngine {
    /// Prices a supported vanilla contract, returning intrinsic value at expiry.
    ///
    /// Accepts flat or node curves. American automatic domains expand with volatility
    /// and maturity and use sinh coordinates; European grids remain uniform in spot.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside contract life or curve coverage,
    /// for clipped/degenerate grids or unstable explicit steps,
    /// and [`Error::ComputationFailure`] for numerical failures. Inputs remain unchanged.
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns spline delta/gamma and fixed-grid speed, completing other Greeks numerically.
    ///
    /// Bumped contexts share the central spatial domain. All Greeks are absent at expiry.
    /// Uses [`NumericalShiftSettings`] for bumps; unavailable stencils leave Greeks absent.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts before pricing; reuses [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Feasible bumped failures propagate.
    /// Inputs and engine settings remain unchanged on success and failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{FiniteDifferenceVanillaEngine, EuropeanOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let result = FiniteDifferenceVanillaEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!((result.price() - 9.227005508154036).abs() < 0.002);
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price_with_greeks(self, option, context, request, settings)
    }
}

macro_rules! vanilla_pricing {
    ($option:ty, $american:literal) => {
        impl PricingEngine<$option> for FiniteDifferenceVanillaEngine {
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                Ok(native(
                    option,
                    Payoff {
                        strike: option.strike(),
                        sign: if option.option_type() == OptionType::Call {
                            1.0
                        } else {
                            -1.0
                        },
                        american: $american,
                        digital: false,
                        payout: None,
                    },
                    context,
                    self.settings,
                    GreeksRequest::default(),
                    Default::default(),
                )?
                .price())
            }
            fn price_with_greeks(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                settings: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                analysis(
                    option,
                    Payoff {
                        strike: option.strike(),
                        sign: if option.option_type() == OptionType::Call {
                            1.0
                        } else {
                            -1.0
                        },
                        american: $american,
                        digital: false,
                        payout: None,
                    },
                    context,
                    self.settings,
                    request,
                    settings,
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
    /// Rejects node curves before life validation, including at expiry.
    /// See [`FiniteDifferenceVanillaEngine::price_with_greeks`] for context construction.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for node curves, then reuses
    /// [`FiniteDifferenceVanillaEngine::price`] errors. Payoff-bound violations return
    /// [`Error::ComputationFailure`]. Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{FiniteDifferenceDigitalEngine, DigitalOption, OptionType,
    ///     BlackScholesMertonParameters, Date, PricingContext};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// assert!((FiniteDifferenceDigitalEngine::default().price(&option, &context)? - 4.945810910532236).abs() < 0.001);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        flat_rates(context)?;
        Ok(native(
            option,
            Self::payoff(option),
            context,
            self.settings,
            GreeksRequest::default(),
            Default::default(),
        )?
        .price())
    }

    /// Returns spline spot Greeks and numerical completion with the shared central domain.
    ///
    /// All expiry Greeks are absent. See [`Self::price`] for a pricing example.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request and shift validation, then reuses
    /// [`Self::price`] and [`crate::numerical_greeks`] errors. Inputs remain unchanged.
    pub fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        flat_rates(context)?;
        analysis(
            option,
            Self::payoff(option),
            context,
            self.settings,
            request,
            shifts,
        )
    }

    fn payoff(option: &DigitalOption) -> Payoff {
        Payoff {
            strike: option.strike(),
            sign: if option.option_type() == OptionType::Call {
                1.0
            } else {
                -1.0
            },
            american: false,
            digital: true,
            payout: option.payout(),
        }
    }
}

impl PricingEngine<DigitalOption> for FiniteDifferenceDigitalEngine {
    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request, settings)
    }
}
