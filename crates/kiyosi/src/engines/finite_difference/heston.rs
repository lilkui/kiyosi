use super::grid::{Curve, SpatialGrid, TimeGrid, concentrate};
use super::{FiniteDifferenceSettings, Payoff, invalid_grid, time_to_expiry, unstable};
use crate::engines::analytic::heston::integrated_variance;
use crate::numerics::{checked_with_scale, log_ratio, scaled_cash};
use crate::{
    AmericanOption, EuropeanOption, GreeksRequest, HestonParameters, NumericalShiftSettings,
    OptionContract, PricingContext, PricingEngine, PricingResult, Result,
};

/// Configures fixed Heston grids; see [`FiniteDifferenceHestonVanillaEngine::new`].
///
/// Counts specify intervals, not nodes. Curve knots are added to the time grid.
/// There is no extra size cap; allocation exhaustion follows the library-wide
/// allocator contract. Successful pricing is not an estimated-error guarantee.
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
    /// non-finite explicit bounds, without changing inputs.
    pub fn validate(&self) -> Result<()> {
        if self.asset_step_count < 3
            || self.variance_step_count < 3
            || self.time_step_count < 1
            || [self.asset_upper_boundary, self.variance_upper_boundary]
                .into_iter()
                .flatten()
                .any(|x| !x.is_finite() || x <= 0.0)
        {
            return Err(invalid_grid(
                "Heston finite-difference settings are invalid",
            ));
        }
        Ok(())
    }
}

/// Prices European and American vanilla options under the Heston model.
///
/// Uses the modified Craig-Sneyd (MCS) alternating direction implicit (ADI)
/// scheme with a fixed splitting parameter of 1/3, nine-point mixed derivatives,
/// nonuniform price/variance grids and Ikonen-Toivanen American splitting.
/// Accepts deterministic rate and dividend-yield curves, parameters that violate
/// the Feller condition, zero parameters and correlation endpoints. With zero
/// volatility of volatility, pricing uses the deterministic variance path in
/// one dimension. Absorbing zero variance uses optimal
/// deterministic exercise. Requested Greeks remain absent; implied-parameter solvers are unsupported.
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
    ///     HestonFiniteDifferenceSettings, HestonParameters, OptionType, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let model = HestonParameters::new(0.05, 0.02, 0.04, 0.04, 1.5, 0.3, -0.7)?;
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

    /// Prices a European or American call or put; see [`Self::new`].
    ///
    /// Validates the model, contract life and curve coverage before expiry payoff.
    /// Grid coverage is checked only when that grid is needed. Initial variance
    /// is at valuation. Explicit domains must be checked by enlargement, as must
    /// automatic domains. Only rounding-sized negative prices may be clipped.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for other models,
    /// [`crate::Error::InvalidInput`] outside contract life/curve coverage or for explicit
    /// bounds that exclude spot, strike or initial variance, and
    /// [`crate::Error::ComputationFailure`] for unrepresentable grids, intermediates or
    /// invalid prices. Fixed grids do not report error-target nonconvergence.
    /// Inputs and settings remain unchanged on success and failure.
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns the price with all requested Greeks absent; see [`Self::new`].
    ///
    /// # Errors
    ///
    /// Rejects empty requests or invalid shifts with [`crate::Error::InvalidInput`] and propagates
    /// [`Self::price`] errors. Inputs remain unchanged; valid numerical shifts are unused.
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
        request.validate()?;
        settings.validate()?;
        PricingResult::new(self.price(option, context)?)
            .map(|result| result.with_request(request, GreeksRequest::new([])))
    }
}

macro_rules! pricing {
    ($instrument:ty, $american:literal) => {
        impl PricingEngine<$instrument> for FiniteDifferenceHestonVanillaEngine {
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
            fn price_with_greeks(
                &self,
                option: &$instrument,
                context: &PricingContext,
                request: GreeksRequest,
                settings: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                Self::price_with_greeks(self, option, context, request, settings)
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

// Discounted deterministic intrinsic values also supply the far-asset asymptote.
#[expect(
    clippy::float_cmp,
    reason = "Equal forward rates have no isolated stationary point."
)]
fn exercise_value(
    payoff: Payoff,
    spot: f64,
    start: f64,
    end: f64,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    let value = |t| {
        let dividend_log = dividend.log_factor(start, t);
        let risk_log = risk.log_factor(start, t);
        let price = if spot == 0.0 {
            if payoff.sign < 0.0 {
                scaled_cash(payoff.strike, risk_log)
            } else {
                0.0
            }
        } else if payoff.strike == 0.0 {
            if payoff.sign > 0.0 {
                scaled_cash(spot, dividend_log)
            } else {
                0.0
            }
        } else {
            let m = log_ratio(spot, payoff.strike) + dividend_log - risk_log;
            if m.is_nan() {
                return Err(unstable());
            }
            if payoff.sign * m <= 0.0 {
                0.0
            } else if payoff.sign > 0.0 {
                scaled_cash(spot, dividend_log + (-(-m).exp_m1()).ln())
            } else {
                scaled_cash(payoff.strike, risk_log + (-m.exp_m1()).ln())
            }
        };
        checked_with_scale(price, 0.0)
    };
    if !payoff.american {
        return value(end);
    }
    let mut knots: Vec<_> = risk
        .ends()
        .chain(dividend.ends())
        .filter(|&t| t > start && t < end)
        .chain([start, end])
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let mut best = value(start)?.max(value(end)?);
    for pair in knots.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        best = best.max(value(b)?);
        let middle = a.midpoint(b);
        let r = risk.rate(middle);
        let q = dividend.rate(middle);
        if spot > 0.0 && r != 0.0 && q != 0.0 && r.signum() == q.signum() && r != q {
            let log = log_ratio(r.abs(), q.abs())
                + log_ratio(payoff.strike, spot)
                + risk.log_factor(start, a)
                - dividend.log_factor(start, a);
            let t = a + log / (r - q);
            if t > a && t < b {
                best = best.max(value(t)?);
            }
        }
    }
    Ok(best)
}

fn space(count: i32, upper: f64, level: f64, scale: f64, levels: Vec<f64>) -> Result<SpatialGrid> {
    let mut grid = SpatialGrid::new(
        FiniteDifferenceSettings {
            asset_step_count: count,
            ..Default::default()
        },
        upper,
        scale,
        level,
    )
    .map_err(|_| unstable())?;
    concentrate(&mut grid, levels, scale).map_err(|_| unstable())?;
    // Materialize coordinates once: ADI revisits each node many times.
    if grid.nodes.is_empty() {
        grid.nodes = (0..=grid.count).map(|i| grid.asset(i)).collect();
    }
    if grid.count < 3
        || grid
            .nodes
            .windows(2)
            .any(|w| !w[1].is_finite() || w[1] <= w[0])
    {
        return Err(unstable());
    }
    Ok(grid)
}

fn price(
    option: &impl OptionContract,
    context: &PricingContext,
    mut payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
) -> Result<f64> {
    let model = context.model_parameters().as_heston()?;
    let time = time_to_expiry(option, context)?;
    let start = context.valuation_time();
    let end = option.expiry_time();
    let risk = Curve::new(model.risk_free_curve(), start, end)?;
    let dividend = Curve::new(model.dividend_curve(), start, end)?;
    if time == 0.0 {
        return Ok(payoff.value(context.spot_price()));
    }
    let scale = payoff.strike.max(context.spot_price());
    if model.initial_variance() == 0.0
        && (model.mean_reversion_speed() == 0.0 || model.long_run_variance() == 0.0)
    {
        return checked_with_scale(
            exercise_value(payoff, context.spot_price(), 0.0, time, &risk, &dividend)?,
            scale,
        );
    }
    let spot = context.spot_price() / scale;
    payoff.strike /= scale;
    if spot == 0.0 || payoff.strike == 0.0 {
        return Err(unstable());
    }
    let integrated = integrated_variance(model, time);
    if !integrated.is_finite() || integrated < 0.0 {
        return Err(unstable());
    }
    let mut drift: f64 = 0.0;
    for t in risk.ends().chain(dividend.ends()) {
        drift = drift.max(dividend.log_factor(0.0, t) - risk.log_factor(0.0, t));
    }
    let upper = settings.asset_upper_boundary.map_or_else(
        || (drift + 6.0 * integrated.sqrt()).exp().max(4.0),
        |x| x / scale,
    );
    if settings.asset_upper_boundary.is_some_and(|x| x <= scale) {
        return Err(invalid_grid(
            "Heston asset boundary must exceed spot and strike",
        ));
    }
    let asset = space(
        settings.asset_step_count,
        upper,
        spot.max(payoff.strike),
        (0.1 * spot.max(payoff.strike)).max(f64::MIN_POSITIVE),
        vec![spot, payoff.strike],
    )?;
    let deterministic = model.vol_of_vol() == 0.0;
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
        let upper = settings.variance_upper_boundary.unwrap_or_else(|| {
            (mean + 8.0 * model.vol_of_vol() * (mean * horizon).sqrt()).max(0.25)
        });
        let mut grid = space(
            settings.variance_step_count,
            upper,
            model.initial_variance(),
            (0.1 * mean).max(0.0001),
            Vec::new(),
        )?;
        if model.initial_variance() > 0.0 {
            let nearest = (1..grid.count)
                .min_by(|&a, &b| {
                    (grid.nodes[a] - model.initial_variance())
                        .abs()
                        .total_cmp(&(grid.nodes[b] - model.initial_variance()).abs())
                })
                .ok_or_else(unstable)?;
            grid.nodes[nearest] = model.initial_variance();
        }
        Some(grid)
    };
    let times = TimeGrid::with_events(time, settings.time_step_count, &risk, &dividend, &[]);
    let nx = asset.count + 1;
    let rows = variance.as_ref().map_or(1, |v| v.count + 1);
    let length = nx
        .checked_mul(rows)
        .filter(|&n| n <= isize::MAX as usize / size_of::<[f64; 3]>())
        .ok_or_else(unstable)?;
    let mut terminal = Vec::with_capacity(nx);
    for (i, &s) in asset.nodes.iter().enumerate() {
        let mut value = payoff.value(s);
        if i > 0 && i + 1 < nx {
            let a = asset.nodes[i - 1].midpoint(s);
            let b = s.midpoint(asset.nodes[i + 1]);
            if a < payoff.strike && payoff.strike < b {
                let h = if payoff.sign > 0.0 {
                    b - payoff.strike
                } else {
                    payoff.strike - a
                };
                value = 0.5 * h * (h / (b - a));
            }
        }
        terminal.push(value);
    }
    let mut values: Vec<_> = (0..length).map(|k| terminal[k % nx]).collect();
    let intrinsic: Vec<_> = asset.nodes.iter().map(|&s| payoff.value(s)).collect();
    let mut multiplier = vec![0.0; length];
    for step in (0..times.widths.len()).rev() {
        let a = times.times[step];
        let b = times.times[step + 1];
        let dt = times.widths[step];
        let middle = a.midpoint(b);
        let rate = risk.rate(middle);
        let q = dividend.rate(middle);
        let edges = [
            exercise_value(payoff, 0.0, a, time, &risk, &dividend)?,
            exercise_value(payoff, upper, a, time, &risk, &dividend)?,
        ];
        if edges.iter().any(|x| !x.is_finite()) {
            return Err(unstable());
        }
        let op = Operator::new(
            &asset.nodes,
            variance.as_ref().map(|v| v.nodes.as_slice()),
            model,
            if deterministic {
                variance_at(model, middle)
            } else {
                0.0
            },
            rate,
            q,
        )?;
        let [mixed, first, second] = op.apply(&values);
        let mut predictor: Vec<_> = (0..length)
            .map(|k| values[k] + dt * (mixed[k] + first[k] + second[k] + multiplier[k]))
            .collect();
        op.boundaries(&mut predictor, edges);
        let theta = dt / 3.0;
        let mut stage = predictor.clone();
        op.solve(&mut stage, &first, &second, theta, edges)?;
        for (x, old) in stage.iter_mut().zip(&values) {
            *x -= old;
        }
        let [cross, along_asset, along_variance] = op.apply(&stage);
        for k in 0..length {
            predictor[k] += dt * (0.5 * cross[k] + (along_asset[k] + along_variance[k]) / 6.0);
        }
        op.solve(&mut predictor, &first, &second, theta, edges)?;
        if payoff.american {
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
        values = predictor;
    }
    let price = if let Some(v) = &variance {
        let at_asset: Vec<_> = values
            .chunks_exact(nx)
            .map(|row| asset.linear(row, spot))
            .collect();
        v.linear(&at_asset, model.initial_variance())
    } else {
        asset.linear(&values, spot)
    };
    checked_with_scale(scale * price, scale)
}

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
}

impl Operator {
    fn new(
        s: &[f64],
        v: Option<&[f64]>,
        model: &HestonParameters,
        deterministic: f64,
        rate: f64,
        q: f64,
    ) -> Result<Self> {
        let nx = s.len();
        let rows = v.map_or(1, <[f64]>::len);
        let mut op = Self {
            nx,
            rows,
            asset: vec![[0.0; 3]; nx * rows],
            variance: vec![[0.0; 3]; rows],
            mixed: vec![0.0; nx * rows],
            dx: vec![[0.0; 3]; nx],
            dv: vec![[0.0; 3]; rows],
        };
        for i in 1..nx - 1 {
            op.dx[i] = derivative(s, i);
        }
        for j in 0..rows {
            let variance = v.map_or(deterministic, |v| v[j]);
            if let Some(v) = v {
                if j + 1 < rows {
                    op.variance[j] = directional(
                        v,
                        j,
                        0.5 * model.vol_of_vol().powi(2) * variance,
                        model.mean_reversion_speed() * (model.long_run_variance() - variance),
                        rate / 2.0,
                    );
                    if j > 0 {
                        op.dv[j] = derivative(v, j);
                    }
                }
            } else {
                op.variance[j] = [0.0, -rate / 2.0, 0.0];
            }
            for (i, &spot) in s.iter().enumerate().take(nx - 1).skip(1) {
                op.asset[j * nx + i] = directional(
                    s,
                    i,
                    0.5 * variance * spot * spot,
                    (rate - q) * spot,
                    rate / 2.0,
                );
                op.mixed[j * nx + i] = model.correlation() * model.vol_of_vol() * variance * spot;
            }
        }
        if op
            .asset
            .iter()
            .chain(&op.variance)
            .flatten()
            .chain(&op.mixed)
            .any(|x| !x.is_finite())
        {
            return Err(unstable());
        }
        Ok(op)
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

    fn apply(&self, values: &[f64]) -> [Vec<f64>; 3] {
        let mut mixed = vec![0.0; values.len()];
        let mut asset = vec![0.0; values.len()];
        let mut variance = vec![0.0; values.len()];
        let dynamic_rows = if self.rows == 1 { 1 } else { self.rows - 1 };
        for j in 0..dynamic_rows {
            for i in 1..self.nx - 1 {
                let k = j * self.nx + i;
                let [a, b, c] = self.asset[k];
                asset[k] = a * values[k - 1] + b * values[k] + c * values[k + 1];
                let [a, b, c] = self.variance[j];
                variance[k] = b * values[k];
                if j > 0 {
                    variance[k] += a * values[k - self.nx];
                }
                if self.rows > 1 {
                    variance[k] += c * values[k + self.nx];
                }
                if j > 0 && self.rows > 1 {
                    let mut cross = 0.0;
                    for (dj, &wj) in self.dv[j].iter().enumerate() {
                        let row = (j + dj - 1) * self.nx;
                        let slope = self.dx[i][0] * (values[row + i - 1] - values[row + i])
                            + self.dx[i][2] * (values[row + i + 1] - values[row + i]);
                        cross += wj * slope;
                    }
                    mixed[k] = self.mixed[k] * cross;
                }
            }
        }
        [mixed, asset, variance]
    }

    fn solve(
        &self,
        values: &mut [f64],
        first: &[f64],
        second: &[f64],
        weight: f64,
        edges: [f64; 2],
    ) -> Result<()> {
        let dynamic_rows = if self.rows == 1 { 1 } else { self.rows - 1 };
        let mut lower = vec![0.0; self.nx.max(self.rows)];
        let mut diagonal = lower.clone();
        let mut upper = lower.clone();
        let mut rhs = lower.clone();
        for j in 0..dynamic_rows {
            lower[0] = 0.0;
            diagonal[0] = 1.0;
            upper[0] = 0.0;
            rhs[0] = edges[0];
            for i in 1..self.nx - 1 {
                let k = j * self.nx + i;
                let [a, b, c] = self.asset[k];
                lower[i] = -weight * a;
                diagonal[i] = 1.0 - weight * b;
                upper[i] = -weight * c;
                rhs[i] = values[k] - weight * first[k];
            }
            let last = self.nx - 1;
            lower[last] = 0.0;
            diagonal[last] = 1.0;
            upper[last] = 0.0;
            rhs[last] = edges[1];
            tridiagonal(
                &lower[..self.nx],
                &mut diagonal[..self.nx],
                &upper[..self.nx],
                &mut rhs[..self.nx],
            )?;
            values[j * self.nx..(j + 1) * self.nx].copy_from_slice(&rhs[..self.nx]);
        }
        for i in 1..self.nx - 1 {
            for j in 0..dynamic_rows {
                let [a, b, c] = self.variance[j];
                lower[j] = -weight * a;
                diagonal[j] = 1.0 - weight * b;
                upper[j] = -weight * c;
                rhs[j] = values[j * self.nx + i] - weight * second[j * self.nx + i];
            }
            if self.rows > 1 {
                let last = self.rows - 1;
                lower[last] = -1.0;
                diagonal[last] = 1.0;
                upper[last] = 0.0;
                rhs[last] = 0.0;
            }
            tridiagonal(
                &lower[..self.rows],
                &mut diagonal[..self.rows],
                &upper[..self.rows],
                &mut rhs[..self.rows],
            )?;
            for j in 0..self.rows {
                values[j * self.nx + i] = rhs[j];
            }
        }
        self.boundaries(values, edges);
        Ok(())
    }
}

fn tridiagonal(lower: &[f64], diagonal: &mut [f64], upper: &[f64], rhs: &mut [f64]) -> Result<()> {
    for i in 0..rhs.len() {
        if i > 0 {
            let multiplier = lower[i] / diagonal[i - 1];
            diagonal[i] -= multiplier * upper[i - 1];
            rhs[i] -= multiplier * rhs[i - 1];
        }
        if !diagonal[i].is_finite() || diagonal[i] == 0.0 {
            return Err(unstable());
        }
    }
    for i in (0..rhs.len()).rev() {
        if i + 1 < rhs.len() {
            rhs[i] -= upper[i] * rhs[i + 1];
        }
        rhs[i] /= diagonal[i];
        if !rhs[i].is_finite() {
            return Err(unstable());
        }
    }
    Ok(())
}
