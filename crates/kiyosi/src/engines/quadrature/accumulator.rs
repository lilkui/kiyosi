use super::gaussian::{
    CubicInterpolation, Layer, Polynomial, TerminalPiece, TransitionCache, integrate_terminal,
    integration_nodes, scaled,
};
use super::{QuadratureSettings, flat_rates};
use crate::numerics::log_ratio;
use crate::{
    Accumulator, BlackScholesMertonParameters, Date, Error, GreeksRequest, NumericalShiftSettings,
    OptionType, PricingContext, PricingEngine, PricingResult, Result, Timestamp,
};

/// Prices accumulators and decumulators using affine layers and Gaussian integration.
///
/// Owns immutable settings; shared and distinct instances support concurrent pricing.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureAccumulatorEngine {
    settings: QuadratureSettings,
}
impl QuadratureAccumulatorEngine {
    /// Creates an engine with validated settings.
    ///
    /// # Errors
    ///
    /// Propagates [`QuadratureSettings::validate`] errors without changing inputs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     start, Date::new(2025, 1, 6)?))?.replace(3.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     90.0, option.expiry_date().into())?;
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
    /// Prices outstanding cashflows, applying a current midnight trading event once.
    ///
    /// Observes knock-out only on the contract's specified dates, before accumulation.
    /// Acceleration and settlement follow the contract's direction. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Checks flat rates ([`Error::UnsupportedOperation`]), inclusive life
    /// ([`Error::InvalidInput`]), then trading expiry and knock-out dates
    /// ([`Error::InvalidInput`]). Non-finite results or unrepresentable grids
    /// and transitions return [`Error::ComputationFailure`]. Inputs remain unchanged.
    pub fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        price(option, context, self.settings, None)
    }
    /// Computes requested scalar Greeks with shared core bump defaults; see [`Self::new`].
    ///
    /// Expiry Greeks and spot/time stencils crossing active events are unavailable.
    /// Vega and rho remain defined at current thresholds. Request-local curve reuse
    /// preserves the same prices as independent scalar repricing.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request/shift validation, then propagates
    /// [`crate::numerical_greeks`] errors. Inputs remain unchanged on every outcome.
    pub fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}
impl PricingEngine<Accumulator> for QuadratureAccumulatorEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<Accumulator> + '_>>> {
        Ok(Some(Box::new(Request {
            settings: self.settings,
            curve: std::cell::RefCell::new(None),
        })))
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}
fn checked(value: f64) -> Result<f64> {
    Ok(PricingResult::new(value)?.price())
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
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}

struct PreparedCurve {
    option: Accumulator,
    model: BlackScholesMertonParameters,
    valuation: Timestamp,
    dates: Vec<Date>,
    nodes: Vec<f64>,
    layers: [Layer; 2],
    drift: f64,
    deviation: f64,
    discount: f64,
}
fn price(
    option: &Accumulator,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: Option<&mut Option<PreparedCurve>>,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    option.validate_pricing(context)?;
    let horizon = context
        .valuation_time()
        .years_until(option.expiry_date().into());
    let (quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return checked(value);
    }
    let strike = option.strike();
    let current = log_ratio(context.spot_price(), strike);
    let evaluate = |nodes: &[f64], layers: &[Layer; 2], drift, deviation, discount| {
        let value =
            |layer: &Layer| layer.evaluate(nodes, current, strike, drift, deviation, discount);
        checked(quantity * value(&layers[0]) + value(&layers[1]))
    };
    let dates = option.future_observation_dates(context)?;
    if let Some(curve) = prepared.as_deref().and_then(Option::as_ref).filter(|c| {
        c.option == *option
            && c.model == *context.model_parameters()
            && c.valuation == context.valuation_time()
            && c.dates == dates
    }) {
        return evaluate(
            &curve.nodes,
            &curve.layers,
            curve.drift,
            curve.deviation,
            curve.discount,
        );
    }
    let knock_out = log_ratio(option.knock_out_level(), strike);
    let sign = match option.option_type() {
        OptionType::Call => 1.0,
        OptionType::Put => -1.0,
    };
    let knocks_out = |point: f64| sign * point >= sign * knock_out;
    let sigma = context.model_parameters().volatility();
    let drift_rate = rate - dividend - 0.5 * sigma * sigma;
    let transition = |time: f64| -> Result<(f64, f64, f64)> {
        let drift = drift_rate * time;
        let deviation = sigma * time.sqrt();
        if !drift.is_finite() || !deviation.is_finite() || deviation <= 0.0 {
            return Err(Error::ComputationFailure(
                "quadrature transition is not representable".into(),
            ));
        }
        Ok((drift, deviation, -rate * time))
    };
    let count = if option.accrues(context) { 2 } else { 1 };
    let mut layers: [Layer; 2] = Default::default();
    layers[0].exact.push(TerminalPiece {
        lower: f64::NEG_INFINITY,
        upper: f64::INFINITY,
        cash: -sign * strike,
        asset: sign,
    });
    if count == 2 {
        let (live_lower, live_upper) = if option.observes_knock_out(option.expiry_date()) {
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
    let mut breaks = vec![0.0, knock_out];
    if drift_extent > extent || extent < knock_out.abs() {
        // Resolve each event boundary's drift and diffusion range without enlarging every cell.
        for boundary in [0.0, knock_out] {
            let lower = boundary - drift_extent - extent;
            let upper = boundary + drift_extent + extent;
            if lower < upper {
                breaks.extend(integration_nodes(
                    lower,
                    upper,
                    settings.asset_step_count,
                    &[],
                )?);
            }
        }
    }
    let nodes = integration_nodes(lower, upper, settings.asset_step_count, &breaks)?;
    let interpolation = CubicInterpolation::new(&nodes);
    let mut transitions = TransitionCache::default();
    let mut continuation: [Vec<Polynomial>; 2] = Default::default();
    let mut next_knock_out = None;
    for event in (0..dates.len()).rev() {
        let final_event = event + 1 == dates.len();
        let observes_knock_out = option.observes_knock_out(dates[event]);
        if observes_knock_out {
            next_knock_out = Some(event);
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
                    let remaining = dates[event].start_of_day().years_until(dates[last].into());
                    let quantity = if layer == 0 {
                        1.0
                    } else {
                        (last - event + usize::from(settlement.is_none())) as f64 * purchase
                    };
                    // Extreme paths settle at the next eligible knock-out, or at expiry.
                    result.exact.push(TerminalPiece {
                        lower,
                        upper,
                        cash: quantity * scaled(-sign * strike, -rate * remaining),
                        asset: sign * quantity * (-dividend * remaining).exp(),
                    });
                }
                result.cells = vec![[0.0; 4]; nodes.len() - 1];
            }
            if observes_knock_out {
                layers[0].exact.push(TerminalPiece {
                    lower: if sign > 0.0 {
                        knock_out
                    } else {
                        f64::NEG_INFINITY
                    },
                    upper: if sign > 0.0 { f64::INFINITY } else { knock_out },
                    cash: -sign * strike,
                    asset: sign,
                });
            }
            for (cell, edges) in nodes.windows(2).enumerate() {
                let point = edges[0].midpoint(edges[1]);
                if !(observes_knock_out && knocks_out(point)) {
                    layers[0].cells[cell] = continuation[0][cell];
                    if count == 2 {
                        let purchase = if point < 0.0 {
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
            context.valuation_time().years_until(dates[event].into())
        } else {
            dates[event - 1]
                .start_of_day()
                .years_until(dates[event].into())
        };
        let (drift, deviation, discount) = transition(time)?;
        if event == 0 {
            let value = evaluate(&nodes, &layers, drift, deviation, discount)?;
            if let Some(prepared) = prepared {
                *prepared = Some(PreparedCurve {
                    option: option.clone(),
                    model: context.model_parameters().clone(),
                    valuation: context.valuation_time(),
                    dates,
                    nodes,
                    layers,
                    drift,
                    deviation,
                    discount,
                });
            }
            return Ok(value);
        }
        let transition = if final_event {
            None
        } else {
            Some(transitions.get(time, &nodes, drift, deviation))
        };
        for layer in 0..count {
            let mut values = vec![0.0; nodes.len()];
            if let Some(transition) = transition {
                transition.integrate(&layers[layer].cells, discount, &mut values);
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
