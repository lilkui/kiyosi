use super::grid::{SpatialGrid, Spline, TimeGrid, concentrate, project_payoff, spot_floor};
use super::{Payoff, WORKSPACE_BUDGET, invalid_grid, prepare_rate_curves, unstable, validate_work};
use crate::engines::analytic::heston::integrated_variance;
use crate::engines::deterministic::{self, AmericanExercise, exercise_value};
use crate::instruments::monitoring::validate_life;
use crate::numerics::{checked_with_scale, scaled_cash};
use crate::{
    AmericanOption, EuropeanOption, GreeksRequest, HestonParameters, OptionContract,
    PricingContext, PricingEngine, PricingResult, Result,
};

/// Configures fixed Heston grids; see [`FiniteDifferenceHestonVanillaEngine::new`].
///
/// Counts specify intervals, not nodes. Curve knots are added to the time grid.
/// Pricing limits estimated workspace and rollback work. Successful pricing
/// does not guarantee a particular accuracy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HestonFiniteDifferenceSettings {
    /// Sets at least three asset intervals; defaults to 400.
    pub asset_step_count: i32,
    /// Sets at least three variance intervals; defaults to 100.
    pub variance_step_count: i32,
    /// Sets at least one base time interval; defaults to 400.
    pub time_step_count: i32,
    /// Sets a finite positive asset upper bound, or selects it automatically.
    ///
    /// When a numerical grid is required, must exceed valuation spot and strike.
    pub asset_upper_boundary: Option<f64>,
    /// Sets a finite positive variance upper bound, or selects it automatically.
    ///
    /// When a variance grid is required, must exceed initial variance.
    pub variance_upper_boundary: Option<f64>,
}

impl Default for HestonFiniteDifferenceSettings {
    fn default() -> Self {
        Self {
            asset_step_count: 400,
            variance_step_count: 100,
            time_step_count: 400,
            asset_upper_boundary: None,
            variance_upper_boundary: None,
        }
    }
}

impl HestonFiniteDifferenceSettings {
    /// Validates context-independent controls; see [`FiniteDifferenceHestonVanillaEngine::new`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for insufficient counts or nonpositive or
    /// non-finite explicit bounds.
    pub fn validate(&self) -> Result<()> {
        for (name, count, minimum) in [
            ("asset_step_count", self.asset_step_count, 3),
            ("variance_step_count", self.variance_step_count, 3),
            ("time_step_count", self.time_step_count, 1),
        ] {
            if count < minimum {
                return Err(crate::Error::InvalidInput(
                    format!("{name} must be at least {minimum}").into(),
                ));
            }
        }
        for (name, boundary) in [
            ("asset_upper_boundary", self.asset_upper_boundary),
            ("variance_upper_boundary", self.variance_upper_boundary),
        ] {
            if let Some(value) = boundary {
                crate::error::finite(value, true, name)?;
            }
        }
        Ok(())
    }
}

/// Prices European and American vanilla options under the Heston model.
///
/// The asset grid uses log spot and an automatically selected positive lower boundary.
/// Uses the modified Craig-Sneyd (MCS) alternating direction implicit (ADI)
/// scheme with a fixed splitting parameter of 1/3, nine-point mixed derivatives,
/// nonuniform price/variance grids and Ikonen-Toivanen American splitting.
/// Accepts deterministic rate and dividend-yield curves, parameters that violate
/// the Feller condition, zero parameters and correlation endpoints. With zero
/// volatility of volatility, pricing uses the deterministic variance path in
/// one dimension. Absorbing zero variance uses optimal
/// deterministic exercise. Requested Greeks have [`crate::GreekStatus::Unsupported`];
/// implied-parameter solvers return [`crate::Error::UnsupportedOperation`].
///
/// Domain truncation and discretization both require convergence checks. No
/// automatic error estimate or refinement is performed. Shared and independent
/// instances support concurrent calls with request-local state. See [`Self::new`].
///
/// # References
///
/// In 't Hout and Foulon (2010), [ADI finite difference schemes for option pricing
/// in the Heston model with correlation][heston-adi].
///
/// [heston-adi]: https://www.math.ualberta.ca/ijnam/Volume-7-2010/No-2-10/2010-02-06.pdf
///
/// # Pricing
///
/// Prices a European or American call or put; see [`Self::new`].
///
/// Validates the model, permitted valuation bounds and curve coverage before expiry payoff.
/// European valuation may precede the effective time; American valuation must
/// be within the inclusive contract life. Both require rate/dividend coverage
/// from valuation through expiry; see [`OptionContract::valuation_start`].
/// Grid coverage is checked only when that grid is needed. Initial variance
/// is at valuation. Explicit domains must be checked by enlargement, as must
/// automatic domains. Only rounding-sized negative prices may be clipped.
///
/// ## Errors
///
/// Returns [`crate::Error::UnsupportedOperation`] for other models,
/// [`crate::Error::InvalidInput`] outside permitted valuation bounds or curve coverage, or for explicit
/// bounds that exclude spot, strike or initial variance, estimated workspace above 1 GiB,
/// more than one billion spatial grid points times time intervals, or American
/// exercise-boundary work estimated above 25 million curve-knot visits, and
/// [`crate::Error::ComputationFailure`] for unrepresentable grids, intermediates or
/// invalid prices. Fixed grids do not report error-target nonconvergence.
/// Inputs and settings remain unchanged on success and failure.
///
/// # Greeks
///
/// Returns the price with requested Greeks marked unsupported; see [`Self::new`].
///
/// Use [`PricingResult::require`] to reject an unavailable Greek.
///
/// ## Errors
///
/// Checks Heston applicability and propagates [`Self::price`] errors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FiniteDifferenceHestonVanillaEngine {
    settings: HestonFiniteDifferenceSettings,
}

impl FiniteDifferenceHestonVanillaEngine {
    /// Creates an engine with validated fixed-grid controls.
    ///
    /// # Errors
    ///
    /// Propagates [`HestonFiniteDifferenceSettings::validate`] errors; inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AmericanOption, Date, FiniteDifferenceHestonVanillaEngine,
    ///     HestonFiniteDifferenceSettings, HestonParameters, OptionType, PricingContext, PricingEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let model = HestonParameters::new(kiyosi::HestonParameterInputs {
    ///     risk_free_rate: 0.05.into(), dividend_yield: 0.02.into(),
    ///     initial_variance: 0.04, long_run_variance: 0.04,
    ///     mean_reversion_speed: 1.5, vol_of_vol: 0.3, correlation: -0.7,
    /// })?;
    /// let context = PricingContext::new(model, 100.0, start.into())?;
    /// let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings::default())?;
    /// assert_eq!(engine.settings().variance_step_count, 100);
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: HestonFiniteDifferenceSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned grid controls; see [`Self::new`].
    pub fn settings(&self) -> HestonFiniteDifferenceSettings {
        self.settings
    }
}

macro_rules! pricing {
    ($instrument:ty, $american:literal) => {
        impl PricingEngine<$instrument> for FiniteDifferenceHestonVanillaEngine {
            fn greek_method(
                &self,
                option: &$instrument,
                context: &PricingContext,
            ) -> Result<crate::GreekMethod> {
                <Self as PricingEngine<$instrument>>::validate_applicability(self, context)?;
                Ok(crate::GreekMethod::for_valuation(
                    option,
                    context,
                    crate::GreekMethod::Unsupported,
                ))
            }

            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                context.model_parameters().as_heston()?;
                Ok(())
            }
            fn price(&self, option: &$instrument, context: &PricingContext) -> Result<f64> {
                price(
                    option,
                    context,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    self.settings,
                )
            }

            fn price_with_greeks_with_settings(
                &self,
                option: &$instrument,
                context: &PricingContext,
                request: GreeksRequest,
                settings: crate::NumericalShiftSettings,
            ) -> Result<PricingResult> {
                <Self as PricingEngine<$instrument>>::validate_applicability(self, context)?;
                settings.validate()?;
                PricingResult::new(self.price(option, context)?)
                    .map(|result| result.with_request(request, GreeksRequest::new([])))
            }
        }
    };
}
pricing!(EuropeanOption, false);
pricing!(AmericanOption, true);

fn variance_at(model: &HestonParameters, time: f64) -> f64 {
    let x = model.mean_reversion_speed() * time;
    let rise = -(-x).exp_m1();
    let growth = if rise == 0.0 && time > 0.0 && model.mean_reversion_speed() > 0.0 {
        scaled_cash(
            model.long_run_variance(),
            model.mean_reversion_speed().ln() + time.ln(),
        )
    } else {
        model.long_run_variance() * rise
    };
    model.initial_variance() * (-x).exp() + growth
}

fn variance_nodes(count: i32, upper: f64, initial: f64, scale: f64) -> Result<Vec<f64>> {
    if !upper.is_finite() || upper <= initial {
        return Err(unstable());
    }
    let end = (upper / scale).asinh();
    let spacing = end / f64::from(count);
    let mut nodes = Vec::new();
    for i in 0..count {
        if i % 1024 == 0 {
            crate::cancellation::check()?;
        }
        nodes.push(scale * (spacing * f64::from(i)).sinh());
    }
    nodes.push(upper);
    if nodes
        .windows(2)
        .any(|pair| !pair[0].is_finite() || pair[0] >= pair[1])
    {
        return Err(unstable());
    }
    Ok(nodes)
}

fn price(
    option: &impl OptionContract,
    context: &PricingContext,
    payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
) -> Result<f64> {
    prepare(option, context, payoff, settings, context.spot_price())?.price(option, context)
}

fn prepare(
    option: &impl OptionContract,
    context: &PricingContext,
    mut payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
    minimum_spot: f64,
) -> Result<HestonState> {
    // Live values, multiplier, ADI stage buffers, operator coefficients and factors.
    const BYTES_PER_GRID_POINT: u64 = 112;
    // Axis coordinates, derivative coefficients, and terminal and intrinsic payoffs.
    const BYTES_PER_AXIS_NODE: u64 = 96;
    const BYTES_PER_TIME_STEP: u64 = 24;
    let model = context.model_parameters().as_heston()?;
    validate_life(option, context)?;
    let end = option.expiry_time();
    let time = context.valuation_time().years_until(end);
    context.validate_curve_coverage(end)?;
    if time == 0.0 {
        return Ok(HestonState::Expiry(payoff));
    }
    let absorbing = model.initial_variance() == 0.0
        && (model.mean_reversion_speed() == 0.0 || model.long_run_variance() == 0.0);
    if absorbing {
        if payoff.is_american() {
            return Ok(HestonState::Exercise(AmericanExercise::new(
                payoff, context, end,
            )?));
        }
        context.validate_curve_horizon(end)?;
        return Ok(HestonState::Direct(payoff));
    }
    let scale = payoff.strike.max(context.spot_price());
    let minimum_spot = minimum_spot / scale;
    payoff.strike /= scale;
    if minimum_spot == 0.0 || payoff.strike == 0.0 {
        return Err(unstable());
    }
    let integrated = integrated_variance(model, time);
    if !integrated.is_finite() || integrated < 0.0 {
        return Err(unstable());
    }
    if settings.asset_upper_boundary.is_some_and(|x| x <= scale) {
        return Err(invalid_grid(
            "Heston asset boundary must exceed spot and strike",
        ));
    }
    let deterministic = model.vol_of_vol() == 0.0;
    let assets = u64::try_from(settings.asset_step_count).map_err(|_| unstable())? + 1;
    let rows = if deterministic {
        1
    } else {
        u64::try_from(settings.variance_step_count).map_err(|_| unstable())? + 1
    };
    let steps = u64::try_from(settings.time_step_count).map_err(|_| unstable())?;
    let grid_bytes = (assets * rows)
        .saturating_mul(BYTES_PER_GRID_POINT)
        .saturating_add((assets + rows) * BYTES_PER_AXIS_NODE);
    let validate_grid = |time_steps, curve_bytes| {
        if grid_bytes
            .saturating_add(time_steps * BYTES_PER_TIME_STEP)
            .saturating_add(curve_bytes)
            > WORKSPACE_BUDGET
        {
            return Err(invalid_grid(
                "Heston finite-difference workspace exceeds 1 GiB; reduce grid step counts",
            ));
        }
        validate_work(assets * rows, time_steps)
    };
    let (risk, dividend) = prepare_rate_curves(context, end, |knots, bytes| {
        validate_grid(steps.max(knots), bytes)
    })?;
    let mut drift: f64 = 0.0;
    let mut lower_drift: f64 = 0.0;
    for t in risk.ends().chain(dividend.ends()) {
        crate::cancellation::check()?;
        let carry = dividend.log_factor(0.0, t) - risk.log_factor(0.0, t);
        drift = drift.max(carry);
        lower_drift = lower_drift.max(-carry);
    }
    let upper = settings.asset_upper_boundary.map_or_else(
        || (drift + 6.0 * integrated.sqrt()).exp().max(4.0),
        |x| x / scale,
    );
    let curve_bytes = risk.workspace_bytes() + dividend.workspace_bytes();
    let times = TimeGrid::with_events(
        time,
        settings.time_step_count,
        &risk,
        &dividend,
        &[],
        |steps| validate_grid(steps, curve_bytes),
    )?;
    if payoff.is_american() {
        let knots = risk.ends().len() + dividend.ends().len() - 2;
        if knots.saturating_mul(times.widths.len()).saturating_mul(2) > 25_000_000 {
            return Err(invalid_grid(
                "American Heston exercise boundary exceeds 25 million curve-knot visits; reduce time steps or curve nodes",
            ));
        }
    }
    let width = integrated.sqrt();
    let tail_width = width.max(0.35 * model.vol_of_vol() * time);
    let lower = spot_floor(
        minimum_spot,
        payoff.strike,
        lower_drift + 0.5 * integrated + 6.0 * tail_width,
    );
    let mut asset = SpatialGrid::new(settings.asset_step_count, [lower, upper], payoff.strike)?;
    let carry = dividend.log_factor(0.0, time) - risk.log_factor(0.0, time);
    if width > 0.0 && width < 0.04 && carry.abs() > 4.0 * width {
        asset.resolve_drift_band(payoff.strike, carry, width)?;
    } else {
        let asset_scale = if width < 0.04 {
            (0.3 * width).max(f64::EPSILON)
        } else {
            0.06
        };
        concentrate(&mut asset, vec![payoff.strike], asset_scale)?;
    }
    let mut valuation_row = 0;
    let variance = if deterministic {
        None
    } else {
        if settings
            .variance_upper_boundary
            .is_some_and(|x| x <= model.initial_variance())
        {
            return Err(invalid_grid(
                "Heston variance boundary must exceed initial variance",
            ));
        }
        let mean = model.initial_variance().max(variance_at(model, time));
        let kappa = model.mean_reversion_speed();
        let horizon = if kappa * time == 0.0 {
            time
        } else {
            -(-2.0 * kappa * time).exp_m1() / kappa / 2.0
        };
        let spread = model.vol_of_vol() * (mean * horizon).sqrt();
        let upper = settings
            .variance_upper_boundary
            .unwrap_or_else(|| (mean + 8.0 * spread).max(0.25));
        let variance_scale = (0.1 * mean).max(0.05 * spread);
        let mut nodes = variance_nodes(
            settings.variance_step_count,
            upper,
            model.initial_variance(),
            variance_scale.max(0.0001),
        )?;
        if model.initial_variance() > 0.0 {
            valuation_row = (1..nodes.len() - 1)
                .min_by(|&a, &b| {
                    (nodes[a] - model.initial_variance())
                        .abs()
                        .total_cmp(&(nodes[b] - model.initial_variance()).abs())
                })
                .ok_or_else(unstable)?;
            nodes[valuation_row] = model.initial_variance();
        }
        Some(nodes)
    };
    let nx = asset.count + 1;
    let rows = variance.as_ref().map_or(1, Vec::len);
    let length = nx
        .checked_mul(rows)
        .filter(|&n| n <= isize::MAX as usize / size_of::<[f64; 3]>())
        .ok_or_else(unstable)?;
    let terminal: Vec<_> = (0..nx)
        .map(|i| project_payoff(&asset, &[payoff.strike], i, |s| payoff.value(s)))
        .collect();
    let mut values: Vec<_> = (0..length).map(|k| terminal[k % nx]).collect();
    let intrinsic: Vec<_> = (0..nx).map(|i| payoff.value(asset.asset(i))).collect();
    let mut multiplier = vec![0.0; length];
    let mut first = vec![0.0; length];
    let mut second = vec![0.0; length];
    let mut predictor = vec![0.0; length];
    let mut stage = vec![0.0; length];
    let mut op = Operator::new(&asset, variance.as_deref());
    for step in (0..times.widths.len()).rev() {
        crate::cancellation::check()?;
        let a = times.times[step];
        let b = times.times[step + 1];
        let dt = times.widths[step];
        let middle = a.midpoint(b);
        let rate = risk.rate(middle);
        let q = dividend.rate(middle);
        let edges = [
            exercise_value(payoff, lower, a, time, &risk, &dividend)?,
            exercise_value(payoff, upper, a, time, &risk, &dividend)?,
        ];
        if edges.iter().any(|x| !x.is_finite()) {
            return Err(unstable());
        }
        let var = if deterministic {
            variance_at(model, middle)
        } else {
            0.0
        };
        op.prepare(&asset, variance.as_deref(), model, [var, rate, q, dt / 3.0])?;
        for j in 0..op.dynamic_rows() {
            crate::cancellation::check()?;
            for i in 1..nx - 1 {
                let k = j * nx + i;
                let [mixed, along_asset, along_variance] = op.components(&values, j, i);
                first[k] = along_asset;
                second[k] = along_variance;
                predictor[k] =
                    values[k] + dt * (mixed + along_asset + along_variance + multiplier[k]);
            }
        }
        op.boundaries(&mut predictor, edges);
        stage.copy_from_slice(&predictor);
        op.solve(&mut stage, &first, &second, edges)?;
        for (x, old) in stage.iter_mut().zip(&values) {
            *x -= old;
        }
        for j in 0..op.dynamic_rows() {
            crate::cancellation::check()?;
            for i in 1..nx - 1 {
                let [cross, along_asset, along_variance] = op.components(&stage, j, i);
                predictor[j * nx + i] += dt * (0.5 * cross + (along_asset + along_variance) / 6.0);
            }
        }
        op.solve(&mut predictor, &first, &second, edges)?;
        if payoff.is_american() {
            for k in 0..length {
                let raw = predictor[k];
                predictor[k] = (raw - dt * multiplier[k]).max(intrinsic[k % nx]);
                multiplier[k] = (multiplier[k] + (intrinsic[k % nx] - raw) / dt).max(0.0);
            }
        }
        op.boundaries(&mut predictor, edges);
        if predictor.iter().chain(&multiplier).any(|x| !x.is_finite()) {
            return Err(unstable());
        }
        std::mem::swap(&mut values, &mut predictor);
    }
    let spline = asset.spline(if variance.is_some() {
        values[valuation_row * nx..][..nx].to_vec()
    } else {
        values
    });
    Ok(HestonState::Grid {
        scale,
        asset,
        payoff,
        spline,
    })
}

#[derive(PartialEq)]
enum HestonState {
    Expiry(Payoff),
    Exercise(AmericanExercise),
    Direct(Payoff),
    Grid {
        scale: f64,
        asset: SpatialGrid,
        payoff: Payoff,
        spline: Spline,
    },
}
impl HestonState {
    fn price(&self, option: &impl OptionContract, context: &PricingContext) -> Result<f64> {
        let spot = context.spot_price();
        match self {
            Self::Expiry(payoff) => Ok(payoff.value(spot)),
            Self::Exercise(exercise) => exercise.price(context),
            Self::Direct(payoff) => deterministic::vanilla(option, *payoff, context),
            Self::Grid {
                scale,
                asset,
                payoff,
                spline,
            } => {
                let spot = spot / scale;
                if spot == 0.0 {
                    return Err(unstable());
                }
                let mut price = asset.sample(spline, spot)[0];
                if payoff.is_american() {
                    let exercise = payoff.value(spot);
                    price = if asset.exercise_cell(spline, spot, |s| payoff.value(s)) {
                        exercise
                    } else {
                        price.max(exercise)
                    };
                }
                checked_with_scale(scale * price, *scale)
            }
        }
    }
}
impl<I: OptionContract> super::solution::Backend<I> for HestonState {
    fn query_work(&self, _: &I, _: &PricingContext) -> usize {
        if let Self::Exercise(exercise) = self {
            exercise.interval_count()
        } else {
            0
        }
    }

    fn evaluate(
        &self,
        option: &I,
        context: &PricingContext,
        _: GreeksRequest,
        _: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        PricingResult::new(self.price(option, context)?)
    }
}
macro_rules! heston_solution {
    ($option:ty, $american:literal) => {
        impl super::FiniteDifferenceEngine<$option> for FiniteDifferenceHestonVanillaEngine {
            fn solve(
                &self,
                option: &$option,
                context: &PricingContext,
                range: std::ops::RangeInclusive<f64>,
            ) -> Result<super::FiniteDifferenceSolution> {
                context.model_parameters().as_heston()?;
                super::solution::validate_range(&range)?;
                crate::cancellation::check()?;
                let anchor = super::solution::at_spot(context, *range.end())?;
                let state = prepare(
                    option,
                    &anchor,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    self.settings,
                    *range.start(),
                )?;
                let low = super::solution::at_spot(context, *range.start())?;
                state.price(option, &low)?;
                crate::cancellation::check()?;
                Ok(super::FiniteDifferenceSolution::new(
                    option.clone(),
                    context,
                    range,
                    state,
                ))
            }
        }
    };
}
heston_solution!(EuropeanOption, false);
heston_solution!(AmericanOption, true);

// Generator-oriented upwinding preserves nonnegative directional off-diagonals.
fn directional(nodes: &[f64], i: usize, diffusion: f64, drift: f64, discount: f64) -> [f64; 3] {
    if i == 0 {
        let c = drift / (nodes[1] - nodes[0]);
        return [0.0, -c - discount, c];
    }
    let left = nodes[i] - nodes[i - 1];
    let right = nodes[i + 1] - nodes[i];
    let span = left + right;
    let mut a = (2.0 * diffusion - drift * right) / (left * span);
    let mut c = (2.0 * diffusion + drift * left) / (right * span);
    if a < 0.0 || c < 0.0 {
        a = 2.0 * diffusion / (left * span) + (-drift).max(0.0) / left;
        c = 2.0 * diffusion / (right * span) + drift.max(0.0) / right;
    }
    [a, -a - c - discount, c]
}

fn derivative(nodes: &[f64], i: usize) -> [f64; 3] {
    let left = nodes[i] - nodes[i - 1];
    let right = nodes[i + 1] - nodes[i];
    [
        -right / (left * (left + right)),
        (right - left) / (left * right),
        left / (right * (left + right)),
    ]
}

struct Operator {
    nx: usize,
    rows: usize,
    asset: Vec<[f64; 3]>,
    variance: Vec<[f64; 3]>,
    mixed: Vec<f64>,
    dx: Vec<[f64; 3]>,
    dv: Vec<[f64; 3]>,
    asset_factors: Vec<[f64; 3]>,
    variance_factors: Vec<[f64; 3]>,
    coefficients_key: Option<[u64; 3]>,
    weight: f64,
}

impl Operator {
    fn new(asset: &SpatialGrid, variance: Option<&[f64]>) -> Self {
        let nx = asset.nodes.len();
        let rows = variance.map_or(1, <[f64]>::len);
        let derivatives = |nodes: &[f64]| {
            let mut coefficients = vec![[0.0; 3]; nodes.len()];
            for (index, row) in coefficients
                .iter_mut()
                .enumerate()
                .take(nodes.len() - 1)
                .skip(1)
            {
                *row = derivative(nodes, index);
            }
            coefficients
        };
        Self {
            nx,
            rows,
            asset: vec![[0.0; 3]; nx * rows],
            variance: vec![[0.0; 3]; rows],
            mixed: vec![0.0; rows],
            dx: derivatives(&asset.nodes),
            dv: variance.map_or_else(Vec::new, derivatives),
            asset_factors: vec![[0.0; 3]; nx * rows],
            variance_factors: vec![[0.0; 3]; rows],
            coefficients_key: None,
            weight: f64::NAN,
        }
    }

    fn dynamic_rows(&self) -> usize {
        if self.rows == 1 { 1 } else { self.rows - 1 }
    }

    fn prepare(
        &mut self,
        asset_grid: &SpatialGrid,
        variance_grid: Option<&[f64]>,
        model: &HestonParameters,
        [deterministic, rate, dividend, weight]: [f64; 4],
    ) -> Result<()> {
        let key = [deterministic, rate, dividend].map(f64::to_bits);
        let rebuild = self.coefficients_key != Some(key);
        let rows = self.dynamic_rows();
        if rebuild {
            for j in 0..rows {
                crate::cancellation::check()?;
                let variance = variance_grid.map_or(deterministic, |nodes| nodes[j]);
                self.variance[j] = variance_grid.map_or([0.0, -rate / 2.0, 0.0], |nodes| {
                    directional(
                        nodes,
                        j,
                        0.5 * model.vol_of_vol().powi(2) * variance,
                        model.mean_reversion_speed() * (model.long_run_variance() - variance),
                        rate / 2.0,
                    )
                });
                self.mixed[j] = model.correlation() * model.vol_of_vol() * variance;
                let volatility = variance.sqrt();
                for i in 1..self.nx - 1 {
                    let mut row = asset_grid.coefficients(i, rate, dividend, volatility);
                    row[1] += rate / 2.0;
                    if row.iter().any(|x| !x.is_finite()) {
                        return Err(unstable());
                    }
                    self.asset[j * self.nx + i] = row;
                }
                if self.variance[j].iter().any(|x| !x.is_finite()) || !self.mixed[j].is_finite() {
                    return Err(unstable());
                }
            }
            self.coefficients_key = Some(key);
        }
        if rebuild || self.weight.to_bits() != weight.to_bits() {
            for (factors, coefficients) in self
                .asset_factors
                .chunks_exact_mut(self.nx)
                .zip(self.asset.chunks_exact(self.nx))
                .take(rows)
            {
                crate::cancellation::check()?;
                factors
                    .iter_mut()
                    .zip(coefficients)
                    .for_each(|(factor, &row)| *factor = implicit_row(row, weight));
                factor(factors)?;
            }
            for (factor, &row) in self
                .variance_factors
                .iter_mut()
                .zip(&self.variance)
                .take(rows)
            {
                *factor = implicit_row(row, weight);
            }
            if self.rows > 1 {
                self.variance_factors[rows] = [-1.0, 1.0, 0.0];
            }
            factor(&mut self.variance_factors)?;
            self.weight = weight;
        }
        Ok(())
    }

    fn boundaries(&self, values: &mut [f64], edges: [f64; 2]) {
        for row in values.chunks_exact_mut(self.nx) {
            row[0] = edges[0];
            row[self.nx - 1] = edges[1];
        }
        if self.rows > 1 {
            let top = (self.rows - 1) * self.nx;
            let (below, last) = values.split_at_mut(top);
            last.copy_from_slice(&below[top - self.nx..top]);
        }
    }

    fn components(&self, values: &[f64], j: usize, i: usize) -> [f64; 3] {
        let k = j * self.nx + i;
        let [a, b, c] = self.asset[k];
        let asset = a * values[k - 1] + b * values[k] + c * values[k + 1];
        let [a, b, c] = self.variance[j];
        let mut variance = b * values[k];
        if j > 0 {
            variance += a * values[k - self.nx];
        }
        if self.rows > 1 {
            variance += c * values[k + self.nx];
        }
        let mixed = if j > 0 && self.rows > 1 {
            // Difference across variance first so shared asset boundaries cancel exactly.
            let [left, center, right] = [k - 1, k, k + 1].map(|index| {
                self.dv[j][0] * (values[index - self.nx] - values[index])
                    + self.dv[j][2] * (values[index + self.nx] - values[index])
            });
            self.mixed[j] * (self.dx[i][0] * (left - center) + self.dx[i][2] * (right - center))
        } else {
            0.0
        };
        [mixed, asset, variance]
    }

    fn solve(
        &self,
        values: &mut [f64],
        first: &[f64],
        second: &[f64],
        edges: [f64; 2],
    ) -> Result<()> {
        let nx = self.nx;
        let rows = self.dynamic_rows();
        for ((row, first), factors) in values
            .chunks_exact_mut(nx)
            .zip(first.chunks_exact(nx))
            .zip(self.asset_factors.chunks_exact(nx))
            .take(rows)
        {
            crate::cancellation::check()?;
            row[0] = edges[0];
            row[nx - 1] = edges[1];
            for i in 1..nx - 1 {
                row[i] = row[i] - self.weight * first[i] - factors[i][0] * row[i - 1];
            }
            for i in (0..nx - 1).rev() {
                row[i] = (row[i] - factors[i][2] * row[i + 1]) / factors[i][1];
            }
            if row.iter().any(|value| !value.is_finite()) {
                return Err(unstable());
            }
        }

        // Previous and next variance rows are contiguous across the independent asset solves.
        for (j, &[multiplier, _, _]) in self.variance_factors.iter().enumerate() {
            crate::cancellation::check()?;
            let (previous, rest) = values.split_at_mut(j * nx);
            let row = &mut rest[1..nx - 1];
            if j < rows {
                let explicit = &second[j * nx + 1..(j + 1) * nx - 1];
                for (value, term) in row.iter_mut().zip(explicit) {
                    *value -= self.weight * term;
                }
            } else {
                row.fill(0.0);
            }
            if j > 0 {
                for (value, prior) in row.iter_mut().zip(&previous[(j - 1) * nx + 1..j * nx - 1]) {
                    *value -= multiplier * prior;
                }
            }
        }
        for (j, &[_, diagonal, upper]) in self.variance_factors.iter().enumerate().rev() {
            let (current, next) = values[j * nx..].split_at_mut(nx);
            let row = &mut current[1..nx - 1];
            if j + 1 < self.rows {
                for (value, next) in row.iter_mut().zip(&next[1..nx - 1]) {
                    *value -= upper * next;
                }
            }
            for value in row.iter_mut() {
                *value /= diagonal;
            }
            if row.iter().any(|value| !value.is_finite()) {
                return Err(unstable());
            }
        }
        self.boundaries(values, edges);
        Ok(())
    }
}

fn implicit_row([lower, diagonal, upper]: [f64; 3], weight: f64) -> [f64; 3] {
    [-weight * lower, 1.0 - weight * diagonal, -weight * upper]
}

fn factor(rows: &mut [[f64; 3]]) -> Result<()> {
    let mut previous = None;
    for [multiplier, diagonal, upper] in rows {
        if let Some((pivot, previous_upper)) = previous {
            *multiplier /= pivot;
            *diagonal -= *multiplier * previous_upper;
        }
        if !diagonal.is_finite() || *diagonal == 0.0 {
            return Err(unstable());
        }
        previous = Some((*diagonal, *upper));
    }
    Ok(())
}
