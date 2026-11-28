use super::gaussian::{
    CubicInterpolation, Layer, Polynomial, TerminalPiece, TransitionCache, integrate_terminal,
    integration_nodes,
};
use super::{QuadratureSettings, flat_rates, simpson};
use crate::engines::Payoff;
use crate::instruments::monitoring::validate_life;
use crate::numerics::{
    Sum, checked, checked_with_scale, discounted_product, log_interval_mass, log_ratio, scaled_cash,
};
use crate::{
    AmericanOption, AmericanOptionChanges, Error, EuropeanOption, PricingContext, PricingEngine,
    Result,
};

/// Configures vanilla spatial quadrature and American exercise intervals.
///
/// American pricing checks Gaussian workspace and rollback work at pricing time.
/// See [`QuadratureVanillaEngine::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadratureVanillaSettings {
    /// Sets at least four spatial steps; defaults to 512. Simpson panels round up to even.
    pub asset_step_count: i32,
    /// Sets at least one American time interval; defaults to 200. European pricing ignores it.
    pub time_step_count: i32,
    /// Sets a finite positive integration extent in standard deviations; defaults to 8.
    pub integration_standard_deviations: f64,
}

impl Default for QuadratureVanillaSettings {
    fn default() -> Self {
        let spatial = QuadratureSettings::default();
        Self {
            asset_step_count: spatial.asset_step_count,
            time_step_count: 200,
            integration_standard_deviations: spatial.integration_standard_deviations,
        }
    }
}

impl QuadratureVanillaSettings {
    /// Validates spatial controls and the time count; see [`QuadratureVanillaEngine::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for fewer than four spatial steps, a nonpositive
    /// or non-finite extent, or fewer than one time interval.
    pub fn validate(&self) -> Result<()> {
        QuadratureSettings {
            asset_step_count: self.asset_step_count,
            integration_standard_deviations: self.integration_standard_deviations,
        }
        .validate()?;
        if self.time_step_count < 1 {
            return Err(Error::InvalidInput(
                "time_step_count must be positive".into(),
            ));
        }
        Ok(())
    }
}

/// Prices European and American vanilla options by flat-rate BSM quadrature.
///
/// European pricing uses composite Simpson integration split at the strike.
/// American calls use the analytic European price when rates are nonnegative
/// and dividend yields are nonpositive. With one exercise interval, American pricing
/// takes the greater of immediate exercise and the analytic European price without a grid.
/// Other American pricing integrates cubic continuation curves on a strike-centered
/// log-price grid, with linear interpolation in cells whose cubic control values
/// exceed the endpoint range, and exercise at valuation, uniform time points and expiry.
/// American prices are bounded below by immediate exercise and the analytic European price.
/// The last interval integrates the terminal payoff with exact Gaussian moments.
/// In-the-money tails extend the edge values linearly in the asset; opposite
/// tails are zero. This boundary approximation requires domain convergence checks.
///
/// Time, space and extent require separate convergence checks; fixed-grid
/// completion provides no error estimate. Numerical Greeks, especially higher
/// derivatives near exercise boundaries, also require bump convergence checks.
/// Distinct and shared instances support concurrent pricing with request-local working storage.
/// See [`Self::new`] for usage.
///
/// # Pricing
///
/// Prices a European or American option, settling intrinsic value at expiry.
///
/// See [`Self::new`] for an example.
///
/// ## Errors
///
/// Returns [`Error::UnsupportedOperation`] for other models or nonconstant rate curves,
/// before life validation. Returns [`Error::InvalidInput`] outside permitted
/// valuation bounds or curve coverage, or when the estimated Gaussian grid or transition
/// workspace exceeds 256 MiB or American rollback exceeds one billion node and cell visits.
/// Returns [`Error::ComputationFailure`] for unrepresentable grids, transitions, tails or prices.
///
/// # Greeks
///
/// Computes requested numerical Greeks with shared bump controls. American spot bumps
/// reuse the continuation curve for matching market parameters and valuation time.
///
/// Expiry Greeks follow terminal payoffs; time Greeks are absent. Infeasible stencils leave affected measures
/// absent; feasible bumped failures propagate. See [`Self::new`] for usage.
///
/// ## Errors
///
/// Checks flat-rate BSM applicability before shift validation, then
/// propagates [`crate::numerical_greeks`] and [`Self::price`] errors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureVanillaEngine {
    settings: QuadratureVanillaSettings,
}

impl QuadratureVanillaEngine {
    /// Creates an engine after validating all integration and exercise controls.
    ///
    /// # Errors
    ///
    /// Propagates [`QuadratureVanillaSettings::validate`] errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let settings = QuadratureVanillaSettings::default();
    /// settings.validate()?;
    /// let engine = QuadratureVanillaEngine::new(settings)?;
    /// assert_eq!(engine.settings(), settings);
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]))?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: QuadratureVanillaSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Prices American options across strikes sharing one expiry and market.
    ///
    /// Rolls back a unit-strike continuation curve and samples it at each
    /// spot-to-strike ratio. Results preserve input order and duplicates.
    /// Check spatial and time convergence as for individual pricing.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = start.add_days(365)?;
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    /// let market = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start,
    /// )?;
    /// let prices = QuadratureVanillaEngine::default()
    ///     .price_strikes(&put, &market, &[90.0, 110.0])?;
    /// assert!(prices[0] < prices[1]);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Propagates invalid input, unsupported market, numerical and
    /// cancellation errors. A failed request returns no partial prices.
    pub fn price_strikes(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        let (rate, dividend) = flat_rates(context)?;
        validate_life(option, context)?;
        let expiry = option.expiry_time();
        context.validate_curve_horizon(expiry)?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        let mut prices = Vec::new();
        prices.try_reserve_exact(strikes.len()).map_err(|_| {
            Error::ComputationFailure("cannot reserve quadrature strike batch output".into())
        })?;
        let time = context.valuation_time().years_until(expiry);
        let unit = Payoff::vanilla(option.option_type(), 1.0, true);
        let curve = if strikes.len() > 1
            && time > 0.0
            && self.settings.time_step_count > 1
            && !crate::engines::deterministic::enabled(context)
            && !(unit.sign > 0.0 && rate >= 0.0 && dividend <= 0.0)
        {
            let carry = (rate - dividend) * time;
            let (width, half_variance) = log_transition(context, time, carry)?;
            match american_curve(
                unit,
                time,
                carry - half_variance,
                width,
                rate,
                self.settings,
            ) {
                Ok(curve) => Some(curve),
                Err(error @ Error::Io(_)) => return Err(error),
                Err(_) => None,
            }
        } else {
            None
        };
        let spot = context.spot_price();
        for &strike in strikes {
            crate::cancellation::check()?;
            let contract = option.replace(AmericanOptionChanges {
                strike: Some(strike),
                ..Default::default()
            })?;
            let ratio = spot / strike;
            if let Some(curve) = curve.as_ref().filter(|_| ratio.is_normal()) {
                let value = match curve
                    .price(unit, ratio)
                    .and_then(|price| checked(price * strike))
                {
                    Ok(price) => Some(price),
                    Err(error @ Error::Io(_)) => return Err(error),
                    Err(_) => None,
                };
                if let Some(value) = value {
                    let floor = crate::AnalyticEuropeanEngine.price(contract.vanilla(), context)?;
                    let intrinsic = Payoff { strike, ..unit }.value(spot);
                    prices.push(value.max(floor).max(intrinsic));
                    continue;
                }
            }
            prices.push(self.price(&contract, context)?);
        }
        Ok(prices)
    }

    /// Returns owned settings; see [`Self::new`] for an example.
    pub fn settings(&self) -> QuadratureVanillaSettings {
        self.settings
    }
}

impl PricingEngine<EuropeanOption> for QuadratureVanillaEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }

    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let (rate, dividend) = flat_rates(context)?;
        validate_life(option, context)?;
        let expiry = option.expiry_time();
        context.validate_curve_horizon(expiry)?;
        let payoff = Payoff::vanilla(option.option_type(), option.strike(), false);
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::vanilla(option, payoff, context);
        }
        let time = context.valuation_time().years_until(expiry);
        if time == 0.0 {
            return checked(payoff.value(context.spot_price()));
        }
        let carry = (rate - dividend) * time;
        let (width, half_variance) = log_transition(context, time, carry)?;
        let forward = context.log_forward_moneyness(expiry, payoff.strike, carry);
        let discount = if payoff.sign > 0.0 {
            (context.spot_price(), -dividend * time)
        } else {
            (payoff.strike, -rate * time)
        };
        european_price(
            payoff.sign,
            forward - half_variance,
            width,
            discount,
            self.settings,
        )
    }
}

impl PricingEngine<AmericanOption> for QuadratureVanillaEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AmericanOption> + '_>>> {
        Ok(Some(Box::new(AmericanRequest {
            settings: self.settings,
            curves: std::cell::RefCell::new(Vec::new()),
        })))
    }

    fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        price_american(option, context, self.settings, None)
    }
}

struct AmericanRequest {
    settings: QuadratureVanillaSettings,
    curves: std::cell::RefCell<Vec<(AmericanCurveKey, AmericanCurve)>>,
}

impl PricingEngine<AmericanOption> for AmericanRequest {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }

    fn price(&self, option: &AmericanOption, context: &PricingContext) -> Result<f64> {
        price_american(
            option,
            context,
            self.settings,
            Some(&mut self.curves.borrow_mut()),
        )
    }
}

#[derive(PartialEq)]
struct AmericanCurveKey {
    option: AmericanOption,
    valuation: crate::Timestamp,
    inputs: [u64; 3],
}

fn price_american(
    option: &AmericanOption,
    context: &PricingContext,
    settings: QuadratureVanillaSettings,
    cache: Option<&mut Vec<(AmericanCurveKey, AmericanCurve)>>,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    validate_life(option, context)?;
    let expiry = option.expiry_time();
    context.validate_curve_horizon(expiry)?;
    let payoff = Payoff::vanilla(option.option_type(), option.strike(), true);
    if crate::engines::deterministic::enabled(context) {
        return crate::engines::deterministic::vanilla(option, payoff, context);
    }
    let spot = context.spot_price();
    let valuation = context.valuation_time();
    let time = valuation.years_until(expiry);
    if time == 0.0 {
        return checked(payoff.value(spot));
    }
    let european = || crate::AnalyticEuropeanEngine.price(option.vanilla(), context);
    if payoff.sign > 0.0 && rate >= 0.0 && dividend <= 0.0 {
        return european();
    }
    if settings.time_step_count == 1 {
        return Ok(european()?.max(payoff.value(spot)));
    }
    let carry = (rate - dividend) * time;
    let (width, half_variance) = log_transition(context, time, carry)?;
    let build = || american_curve(payoff, time, carry - half_variance, width, rate, settings);
    let price = if let Some(cache) = cache {
        // The numerical stencils alternate between at most two active scenarios.
        let key = AmericanCurveKey {
            option: option.clone(),
            valuation,
            inputs: [width, rate, dividend].map(f64::to_bits),
        };
        if let Some(index) = cache.iter().position(|(candidate, _)| *candidate == key) {
            let entry = cache.remove(index);
            let price = entry.1.price(payoff, spot)?;
            cache.push(entry);
            price
        } else {
            if cache.len() == 2 {
                cache.remove(0);
            }
            let curve = build()?;
            let price = curve.price(payoff, spot)?;
            cache.push((key, curve));
            price
        }
    } else {
        build()?.price(payoff, spot)?
    };
    Ok(price.max(european()?))
}

fn log_transition(context: &PricingContext, time: f64, carry: f64) -> Result<(f64, f64)> {
    let width = context.natural_bsm_parameters()?.volatility() * time.sqrt();
    let half_variance = (0.5 * width) * width;
    if !(carry - half_variance).is_finite() || !width.is_finite() || width <= 0.0 {
        return Err(invalid_transition());
    }
    Ok((width, half_variance))
}

fn invalid_transition() -> Error {
    Error::ComputationFailure(
        "vanilla quadrature grid, transition or tail is not representable".into(),
    )
}

fn european_price(
    sign: f64,
    log_moneyness: f64,
    width: f64,
    (amount, log_discount): (f64, f64),
    settings: QuadratureVanillaSettings,
) -> Result<f64> {
    let extent = settings.integration_standard_deviations;
    let call = sign > 0.0;
    let density_center = if call { width } else { 0.0 };
    let upper = extent + density_center;
    if !upper.is_finite() {
        return Err(invalid_transition());
    }
    let kink = -log_moneyness / width;
    let mut breaks = vec![-extent, upper];
    if kink > -extent && kink < upper {
        breaks.insert(1, kink);
    }
    // Factor out the largest density on the paying side to preserve scaled tails.
    let edge = kink.clamp(-extent, upper);
    let peak = if call {
        (edge - density_center).max(0.0)
    } else {
        edge.min(0.0)
    };
    let integrand = |z: f64| {
        let distance = width.mul_add(z, log_moneyness);
        if sign * distance <= 0.0 {
            return 0.0;
        }
        // Call payoffs are fractions of the terminal asset, put payoffs of the strike.
        let normalized = -(-distance.abs()).exp_m1();
        let offset = z - density_center;
        normalized * (-0.5 * (offset - peak) * (offset + peak)).exp()
    };
    let mass = simpson(&breaks, settings.asset_step_count, integrand)?;
    checked(discounted_product(
        amount,
        mass,
        log_discount - 0.5 * peak * peak - 0.918_938_533_204_672_7,
    ))
}

struct AmericanCurve {
    nodes: Vec<f64>,
    layer: Layer,
    transition: [f64; 3],
}

impl AmericanCurve {
    fn price(&self, payoff: Payoff, spot: f64) -> Result<f64> {
        let [drift, deviation, discount] = self.transition;
        let continuation = self.layer.evaluate(
            &self.nodes,
            log_ratio(spot, payoff.strike),
            payoff.strike,
            drift,
            deviation,
            discount,
        );
        Ok(checked_with_scale(continuation, spot + payoff.strike)?.max(payoff.value(spot)))
    }
}

fn american_curve(
    payoff: Payoff,
    time: f64,
    drift: f64,
    width: f64,
    rate: f64,
    settings: QuadratureVanillaSettings,
) -> Result<AmericanCurve> {
    let intervals = f64::from(settings.time_step_count);
    let deviation = width / intervals.sqrt();
    let drift_step = drift / intervals;
    let discount = -rate * time / intervals;
    let extent = settings.integration_standard_deviations * width + drift.abs();
    if !extent.is_finite() || !discount.is_finite() || deviation <= 0.0 {
        return Err(invalid_transition());
    }
    let nodes = integration_nodes(-extent, extent, settings.asset_step_count, &[0.0], 1)?;
    let mut cache = TransitionCache::new(&nodes, nodes.capacity(), 1, settings.asset_step_count)?;
    // Narrow transitions can require the strike-anchored correction in integrate_terminal.
    let cache_tails = settings.time_step_count > 2
        && deviation >= 1e-5
        && cache.reserve_workspace(nodes.len() * size_of::<TailWeights>());
    cache.validate_rollback_work(&nodes, drift_step, deviation, settings.time_step_count)?;
    let interpolation = CubicInterpolation::new(&nodes);
    let intrinsic: Vec<_> = nodes
        .iter()
        .map(|&x| checked(log_intrinsic(payoff, x)))
        .collect::<Result<_>>()?;
    let mut layer = Layer {
        cells: Vec::new(),
        exact: vec![TerminalPiece {
            lower: if payoff.sign > 0.0 {
                0.0
            } else {
                f64::NEG_INFINITY
            },
            upper: if payoff.sign > 0.0 {
                f64::INFINITY
            } else {
                0.0
            },
            cash: -payoff.sign * payoff.strike,
            asset: payoff.sign,
        }],
    };
    let mut values = vec![0.0; nodes.len()];
    let mut tail_weights = Vec::new();
    for step in 1..settings.time_step_count {
        crate::cancellation::check()?;
        if layer.cells.is_empty() {
            values.fill(0.0);
        } else {
            cache
                .get(time / intervals, &nodes, drift_step, deviation)?
                .integrate(&nodes, &layer.cells, discount, &mut values)?;
        }
        for (index, ((value, &node), &exercise)) in
            values.iter_mut().zip(&nodes).zip(&intrinsic).enumerate()
        {
            let tail = if let Some(weights) = tail_weights.get(index) {
                TailWeights::value(weights, &layer.exact[0])
            } else {
                integrate_terminal(
                    &layer.exact,
                    node,
                    payoff.strike,
                    drift_step,
                    deviation,
                    discount,
                )
            };
            let continuation = *value + tail;
            *value = checked_with_scale(continuation, exercise + payoff.strike)?.max(exercise);
        }
        layer.cells = continuation_curve(&interpolation, &values);
        layer.exact[0] = tail(payoff, &nodes, &values)?;
        if step == 1 && cache_tails {
            let anchor = payoff.strike.ln();
            tail_weights.reserve_exact(nodes.len());
            for &node in &nodes {
                crate::cancellation::check()?;
                tail_weights.push(TailWeights::new(
                    &layer.exact[0],
                    node + drift_step,
                    anchor,
                    deviation,
                    discount,
                ));
            }
        }
    }
    Ok(AmericanCurve {
        nodes,
        layer,
        transition: [drift_step, deviation, discount],
    })
}

fn continuation_curve(interpolation: &CubicInterpolation, values: &[f64]) -> Vec<Polynomial> {
    let mut cells = interpolation.interpolate(values);
    for (cell, endpoints) in cells.iter_mut().zip(values.windows(2)) {
        let lower = endpoints[0].min(endpoints[1]);
        let upper = endpoints[0].max(endpoints[1]);
        let controls = [
            cell[0] + cell[1] / 3.0,
            cell[0] + (2.0 * cell[1] + cell[2]) / 3.0,
        ];
        // Bernstein control values bound the entire cubic, including nearly worthless tails.
        if controls
            .iter()
            .any(|&value| !value.is_finite() || value < lower || value > upper)
        {
            *cell = [endpoints[0], endpoints[1] - endpoints[0], 0.0, 0.0];
        }
    }
    cells
}

fn log_intrinsic(payoff: Payoff, x: f64) -> f64 {
    if payoff.sign * x <= 0.0 {
        0.0
    } else if x > 0.0 {
        scaled_cash(payoff.strike, x + (-(-x).exp_m1()).ln())
    } else {
        payoff.strike * -x.exp_m1()
    }
}

struct TailWeights([[f64; 2]; 2]);

impl TailWeights {
    fn new(
        piece: &TerminalPiece,
        mean: f64,
        log_anchor: f64,
        deviation: f64,
        discount: f64,
    ) -> Self {
        let lower = (piece.lower - mean) / deviation;
        let upper = (piece.upper - mean) / deviation;
        let cash = discount + log_interval_mass(lower, upper);
        let asset = log_anchor
            + mean
            + 0.5 * deviation * deviation
            + discount
            + log_interval_mass(lower - deviation, upper - deviation);
        Self([cash, asset].map(|log_weight| [log_weight, log_weight.exp()]))
    }

    fn value(&self, piece: &TerminalPiece) -> f64 {
        let mut sum = Sum::default();
        for ([log_weight, weight], amount) in self.0.into_iter().zip([piece.cash, piece.asset]) {
            if amount != 0.0 {
                let value = amount * weight;
                sum.add(if weight.is_normal() && value.is_normal() {
                    value
                } else {
                    (amount.abs().ln() + log_weight).exp().copysign(amount)
                });
            }
        }
        sum.total()
    }
}

fn tail(payoff: Payoff, nodes: &[f64], values: &[f64]) -> Result<TerminalPiece> {
    let first = if payoff.sign > 0.0 {
        nodes.len() - 2
    } else {
        0
    };
    let asset = scaled_cash(payoff.strike, nodes[first]);
    let change = scaled_cash(
        payoff.strike,
        nodes[first] + (nodes[first + 1] - nodes[first]).exp_m1().ln(),
    );
    let slope = (values[first + 1] - values[first]) / change;
    let cash = values[first] - slope * asset;
    if !slope.is_finite() || !cash.is_finite() || payoff.sign * slope < 0.0 {
        return Err(invalid_transition());
    }
    Ok(TerminalPiece {
        lower: if payoff.sign > 0.0 {
            nodes[nodes.len() - 1]
        } else {
            f64::NEG_INFINITY
        },
        upper: if payoff.sign > 0.0 {
            f64::INFINITY
        } else {
            nodes[0]
        },
        cash,
        asset: slope,
    })
}
