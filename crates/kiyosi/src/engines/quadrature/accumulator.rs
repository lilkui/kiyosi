use super::gaussian::{
    CubicInterpolation, Layer, Polynomial, TerminalPiece, TransitionCache, integrate_terminal,
    integration_nodes,
};
use super::{QuadratureSettings, flat_rates};
use crate::numerics::{Sum, finite_price, log_ratio, scaled_signed_cash};
use crate::{
    Accumulator, BlackScholesMertonParameters, Error, OptionType, PricingContext, PricingEngine,
    Result, Timestamp,
};

/// Prices accumulators and decumulators using affine layers and Gaussian integration.
///
/// Shared and distinct instances support concurrent pricing.
/// See [`Self::new`] for an example.
///
/// # Greeks
///
/// Computes requested scalar Greeks with shared core bump defaults; see [`Self::new`].
///
/// At expiry Greeks follow terminal payoffs, with time Greeks absent.
/// Spot/time stencils crossing active events are unavailable. Before expiry
/// vega and rho are defined at current thresholds. Request-local curve reuse
/// preserves the same prices as independent scalar repricing.
///
/// ## Errors
///
/// Checks flat-rate applicability before shift validation, then propagates
/// [`crate::numerical_greeks`] errors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureAccumulatorEngine {
    settings: QuadratureSettings,
}
impl QuadratureAccumulatorEngine {
    /// Creates an engine with validated settings.
    ///
    /// # Errors
    ///
    /// Propagates [`QuadratureSettings::validate`] errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # use kiyosi::OptionContract as _;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     start, Date::new(2025, 1, 6)?.start_of_day(), kiyosi::TimeOfDay::new(0, 0, 0, 0)?))?
    ///     .replace(AccumulatorChanges {
    ///         accumulated_quantity: Some(Some(3.0)), knock_out_settled: Some(Some(false)),
    ///         ..Default::default()
    ///     })?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     90.0, option.expiry_time().into())?;
    /// let engine = QuadratureAccumulatorEngine::new(QuadratureSettings::default())?;
    /// assert_eq!(engine.settings().asset_step_count, 512);
    /// assert_eq!(engine.price(&option, &context)?, -50.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: QuadratureSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }
    /// Returns owned settings; see [`Self::new`].
    pub fn settings(&self) -> QuadratureSettings {
        self.settings
    }
}
impl PricingEngine<Accumulator> for QuadratureAccumulatorEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    /// Prices outstanding cash flows, applying a current contractual event once.
    ///
    /// Observes knock-out only on the contract's specified dates, before accumulation.
    /// Acceleration and settlement follow the contract's direction. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Missing required prior history returns [`crate::Error::MissingHistory`].
    /// Supplied history covers events strictly before valuation.
    ///
    /// Checks flat rates ([`Error::UnsupportedOperation`]), inclusive life
    /// and impossible history ([`Error::InvalidInput`]), then required
    /// curve coverage ([`Error::InvalidInput`]). Non-finite results or unrepresentable grids
    /// and transitions return [`Error::ComputationFailure`].
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        price(option, context, self.settings, None)
    }
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<Accumulator> + '_>>> {
        Ok(Some(Box::new(Request {
            settings: self.settings,
            curve: std::cell::RefCell::new(None),
        })))
    }
}
struct Request {
    settings: QuadratureSettings,
    curve: std::cell::RefCell<Option<PreparedCurve>>,
}
impl PricingEngine<Accumulator> for Request {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        let mut curve = self.curve.borrow_mut();
        price(option, context, self.settings, Some(&mut curve))
    }
}

struct PreparedCurve {
    option: Accumulator,
    model: BlackScholesMertonParameters,
    valuation: Timestamp,
    nodes: Vec<f64>,
    layers: [Layer; 2],
    drift: f64,
    deviation: f64,
    discount: f64,
}
#[expect(
    clippy::cast_precision_loss,
    reason = "Observation indices are bounded by the supported calendar range and fit exactly in f64."
)]
fn price(
    option: &Accumulator,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: Option<&mut Option<PreparedCurve>>,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    if crate::engines::deterministic::enabled(context) {
        return crate::engines::deterministic::accumulator(option, context);
    }
    option.validate_pricing(context)?;
    let (quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return finite_price(value);
    }
    if let Some(price) = unmonitored_accumulator_value(option, context, quantity, [rate, dividend])?
    {
        return Ok(price);
    }
    context.validate_curve_horizon(option.expiry_time())?;
    let horizon = context.valuation_time().years_until(option.expiry_time());
    let strike = option.strike();
    let current = log_ratio(context.spot_price(), strike);
    let evaluate = |nodes: &[f64], layers: &[Layer; 2], drift, deviation, discount| {
        let value =
            |layer: &Layer| layer.evaluate(nodes, current, strike, drift, deviation, discount);
        finite_price(quantity * value(&layers[0]) + value(&layers[1]))
    };
    if let Some(curve) = prepared.as_deref().and_then(Option::as_ref).filter(|c| {
        c.option == *option
            && context
                .natural_bsm_parameters()
                .is_ok_and(|m| c.model == *m)
            && c.valuation == context.valuation_time()
    }) {
        return evaluate(
            &curve.nodes,
            &curve.layers,
            curve.drift,
            curve.deviation,
            curve.discount,
        );
    }
    let fixed = option.fixed_purchase_events(context)?;
    let dates = if let Some(events) = &fixed {
        events.iter().map(|event| event.time).collect()
    } else {
        option.future_observation_times(context)?
    };
    let knock_out = log_ratio(option.knock_out_level(), strike);
    let sign = match option.option_type() {
        OptionType::Call => 1.0,
        OptionType::Put => -1.0,
    };
    let knocks_out = |point: f64| sign * point >= sign * knock_out;
    let sigma = context.natural_bsm_parameters()?.volatility();
    let drift_rate = rate - dividend - 0.5 * sigma * sigma;
    let transition = |time| super::transition(time, drift_rate, sigma, rate);
    let count = if option.accrues(context) { 2 } else { 1 };
    let mut layers: [Layer; 2] = Default::default();
    layers[0].exact.push(TerminalPiece {
        lower: f64::NEG_INFINITY,
        upper: f64::INFINITY,
        cash: -sign * strike,
        asset: sign,
    });
    if let Some(last) = fixed.as_ref().and_then(|events| events.last()) {
        if last.before != 0.0 {
            layers[1].exact.push(TerminalPiece {
                lower: f64::NEG_INFINITY,
                upper: f64::INFINITY,
                cash: -sign * strike * last.before,
                asset: sign * last.before,
            });
        }
    }
    if count == 2 && option.accumulates_at(option.expiry_time(), option.calendar()) {
        let (live_lower, live_upper) = if option.observes_knock_out(option.expiry_time()) {
            if sign > 0.0 {
                (f64::NEG_INFINITY, knock_out)
            } else {
                (knock_out, f64::INFINITY)
            }
        } else {
            (f64::NEG_INFINITY, f64::INFINITY)
        };
        for (lower, upper, quantity) in [
            (f64::NEG_INFINITY, 0.0, option.purchase(0.0)),
            (0.0, f64::INFINITY, option.purchase(f64::INFINITY)),
        ] {
            let lower = lower.max(live_lower);
            let upper = upper.min(live_upper);
            if lower < upper {
                layers[1].exact.push(TerminalPiece {
                    lower,
                    upper,
                    cash: -sign * strike * quantity,
                    asset: sign * quantity,
                });
            }
        }
    }
    if dates.len() == 1 {
        let (drift, deviation, discount) = transition(horizon)?;
        return evaluate(&[], &layers, drift, deviation, discount);
    }
    let extent = settings.integration_standard_deviations * sigma * horizon.sqrt();
    // Cover both forward drift and the backward movement of future event boundaries.
    let drift_extent = (drift_rate * horizon).abs();
    let lower = knock_out.min(0.0) - drift_extent - extent;
    let upper = knock_out.max(0.0) + drift_extent + extent;
    let breaks = [0.0, knock_out];
    let mut asset_steps = settings.asset_step_count;
    let patch_radius = drift_extent + extent;
    let mut patches = Vec::new();
    if drift_extent > extent || extent < knock_out.abs() {
        let aligned_steps =
            ((upper - lower) / (2.0 * patch_radius) * f64::from(asset_steps)).ceil();
        if aligned_steps <= f64::from(asset_steps.saturating_mul(3)) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "The rounded count is already bounded by i32."
            )]
            let refined_steps = aligned_steps as i32;
            asset_steps = asset_steps.max(refined_steps);
        } else {
            // Keep the coarse bridge, with independent uniform grids around the payoff jumps.
            for boundary in [knock_out.min(0.0), knock_out.max(0.0)] {
                let bounds = (boundary - patch_radius)..=(boundary + patch_radius);
                if bounds.start() < bounds.end() {
                    patches.push((boundary, bounds));
                }
            }
        }
    }
    let mut nodes = integration_nodes(lower, upper, asset_steps, &breaks, count)?;
    let mut ranges = Vec::new();
    if !patches.is_empty() {
        nodes.retain(|node| patches.iter().all(|(_, bounds)| !bounds.contains(node)));
        for (boundary, bounds) in &patches {
            nodes.extend(integration_nodes(
                *bounds.start(),
                *bounds.end(),
                asset_steps,
                &[*boundary],
                count,
            )?);
        }
        nodes.sort_unstable_by(f64::total_cmp);
        ranges.extend(patches.iter().map(|(_, bounds)| {
            nodes.partition_point(|node| node < bounds.start())
                ..nodes.partition_point(|node| node <= bounds.end())
        }));
    }
    let terminal_pieces = layers[0].exact.len().max(layers[1].exact.len());
    let mut transitions = if ranges.is_empty() {
        TransitionCache::new(&nodes, nodes.capacity(), count, asset_steps)?
    } else {
        TransitionCache::with_patches(&nodes, nodes.capacity(), count, asset_steps, &ranges)?
    };
    transitions.validate_scheduled_work(&nodes, &dates, count, terminal_pieces, 0, transition)?;
    let interpolation = CubicInterpolation::new(&nodes);
    let mut continuation: [Vec<Polynomial>; 2] = Default::default();
    let mut next_knock_out = None;
    let mut remaining_purchases = 0_usize;
    let mut purchases_before_knock_out = 0_usize;
    let mut remaining_fixed = 0.0;
    let mut fixed_before_knock_out = 0.0;
    for event in (0..dates.len()).rev() {
        crate::cancellation::check()?;
        let final_event = event + 1 == dates.len();
        let observes_knock_out = option.observes_knock_out(dates[event]);
        let accumulates = option.accumulates_at(dates[event], option.calendar());
        let fixed_purchase = fixed.as_ref().map(|events| &events[event]);
        let before = fixed_purchase.map_or(0.0, |event| event.before);
        let fixed_amount = fixed_purchase.map_or(0.0, |event| event.before + event.at);
        remaining_purchases += usize::from(accumulates);
        remaining_fixed += fixed_amount;
        if observes_knock_out {
            next_knock_out = Some(event);
            purchases_before_knock_out = 0;
            fixed_before_knock_out = before;
        } else {
            purchases_before_knock_out += usize::from(accumulates);
            fixed_before_knock_out += fixed_amount;
        }
        if !final_event {
            for (layer, result) in layers.iter_mut().enumerate().take(count) {
                result.exact.clear();
                for (point, lower, upper, purchase) in [
                    (
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                        nodes[0],
                        option.purchase(0.0),
                    ),
                    (
                        f64::INFINITY,
                        nodes[nodes.len() - 1],
                        f64::INFINITY,
                        option.purchase(f64::INFINITY),
                    ),
                ] {
                    let knock_out_side = knocks_out(point);
                    if observes_knock_out && knock_out_side {
                        continue;
                    }
                    let settlement = if knock_out_side { next_knock_out } else { None };
                    let last = settlement.unwrap_or(dates.len() - 1);
                    let remaining = dates[event].years_until(dates[last]);
                    let quantity = if layer == 0 {
                        1.0
                    } else if fixed_purchase.is_some() {
                        if settlement.is_some() {
                            fixed_before_knock_out
                        } else {
                            remaining_fixed
                        }
                    } else {
                        (if settlement.is_some() {
                            purchases_before_knock_out
                        } else {
                            remaining_purchases
                        }) as f64
                            * purchase
                    };
                    // Extreme paths settle at the next eligible knock-out, or at expiry.
                    result.exact.push(TerminalPiece {
                        lower,
                        upper,
                        cash: quantity * scaled_signed_cash(-sign * strike, -rate * remaining),
                        asset: sign * quantity * (-dividend * remaining).exp(),
                    });
                }
                result.cells = vec![[0.0; 4]; nodes.len() - 1];
            }
            if observes_knock_out {
                let (lower, upper) = if sign > 0.0 {
                    (knock_out, f64::INFINITY)
                } else {
                    (f64::NEG_INFINITY, knock_out)
                };
                for (layer, quantity) in layers.iter_mut().zip([1.0, before]) {
                    if quantity != 0.0 {
                        layer.exact.push(TerminalPiece {
                            lower,
                            upper,
                            cash: -sign * strike * quantity,
                            asset: sign * quantity,
                        });
                    }
                }
            }
            for (cell, edges) in nodes.windows(2).enumerate() {
                let point = edges[0].midpoint(edges[1]);
                if !(observes_knock_out && knocks_out(point)) {
                    layers[0].cells[cell] = continuation[0][cell];
                    if count == 2 {
                        let purchase = if fixed_purchase.is_some() {
                            fixed_amount
                        } else if !accumulates {
                            0.0
                        } else if point < 0.0 {
                            option.purchase(0.0)
                        } else {
                            option.purchase(f64::INFINITY)
                        };
                        layers[1].cells[cell] = std::array::from_fn(|power| {
                            continuation[1][cell][power] + purchase * continuation[0][cell][power]
                        });
                    }
                }
            }
        }
        let time = if event == 0 {
            context.valuation_time().years_until(dates[event])
        } else {
            dates[event - 1].years_until(dates[event])
        };
        let (drift, deviation, discount) = transition(time)?;
        if event == 0 {
            let value = evaluate(&nodes, &layers, drift, deviation, discount)?;
            if let Some(prepared) = prepared {
                *prepared = Some(PreparedCurve {
                    option: option.clone(),
                    model: context.natural_bsm_parameters()?.clone(),
                    valuation: context.valuation_time(),
                    nodes,
                    layers,
                    drift,
                    deviation,
                    discount,
                });
            }
            return Ok(value);
        }
        let mut transition = if final_event {
            None
        } else {
            Some(transitions.get(time, &nodes, drift, deviation)?)
        };
        for layer in 0..count {
            let mut values = vec![0.0; nodes.len()];
            if let Some(transition) = transition.as_deref_mut() {
                transition.integrate(&nodes, &layers[layer].cells, discount, &mut values)?;
            }
            for (value, &node) in values.iter_mut().zip(&nodes) {
                *value += integrate_terminal(
                    &layers[layer].exact,
                    node,
                    strike,
                    drift,
                    deviation,
                    discount,
                );
            }
            if values.iter().any(|v| !v.is_finite()) {
                return Err(Error::ComputationFailure(
                    "quadrature continuation is non-finite".into(),
                ));
            }
            continuation[layer] = interpolation.interpolate(&values);
        }
    }
    Err(Error::ComputationFailure(
        "quadrature has no future observation".into(),
    ))
}

// Conditional on each fixing, the expected expiry spot is a fixed multiple of
// that fixing's spot. Quantities therefore contribute separate digital moments.
pub(in crate::engines) fn unmonitored_accumulator_value(
    option: &Accumulator,
    context: &PricingContext,
    quantity: f64,
    [rate, dividend]: [f64; 2],
) -> Result<Option<f64>> {
    if !option.knock_out_observation_times().is_some_and(|dates| {
        dates
            .last()
            .is_none_or(|&last| last <= context.valuation_time())
    }) {
        return Ok(None);
    }
    let valuation = context.valuation_time();
    let expiry = option.expiry_time();
    context.validate_curve_horizon(expiry)?;
    let strike = option.strike();
    let current = log_ratio(context.spot_price(), strike);
    let sigma = context.natural_bsm_parameters()?.volatility();
    let carry = rate - dividend;
    let horizon = valuation.years_until(expiry);
    let discount = -rate * horizon;
    let sign = if option.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let leg = |lower, upper| TerminalPiece {
        lower,
        upper,
        cash: -sign * strike,
        asset: sign,
    };
    let integrate = |piece, fixing: f64, remaining: f64, log_quantity| {
        let deviation = sigma * fixing.sqrt();
        integrate_terminal(
            &[piece],
            current,
            strike,
            carry.mul_add(remaining, (carry - 0.5 * sigma * sigma) * fixing),
            deviation,
            discount + log_quantity,
        )
    };
    let mut sum = Sum::default();
    if quantity != 0.0 {
        sum.add(integrate(
            leg(f64::NEG_INFINITY, f64::INFINITY),
            horizon,
            0.0,
            quantity.ln(),
        ));
    }
    if option.daily_quantity() != 0.0 {
        let start = valuation.max(option.effective_time());
        let fixings = crate::ObservationSchedule::daily_times(
            start,
            expiry,
            option.calendar(),
            valuation < start,
            option.observation_clock_time(),
        )?;
        let log_lower_quantity = option.purchase(0.0).ln();
        let log_upper_quantity = option.purchase(f64::INFINITY).ln();
        for at in fixings {
            crate::cancellation::check()?;
            let fixing = valuation.years_until(at);
            let remaining = at.years_until(expiry);
            let split = carry * remaining;
            if log_lower_quantity > f64::NEG_INFINITY {
                sum.add(integrate(
                    leg(f64::NEG_INFINITY, split),
                    fixing,
                    remaining,
                    log_lower_quantity,
                ));
            }
            if log_upper_quantity > f64::NEG_INFINITY {
                sum.add(integrate(
                    leg(split, f64::INFINITY),
                    fixing,
                    remaining,
                    log_upper_quantity,
                ));
            }
        }
    }
    finite_price(sum.total()).map(Some)
}
