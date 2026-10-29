use super::gaussian::{
    CubicInterpolation, Polynomial, Sum, TerminalPiece, Transition, integrate_curve,
    integrate_terminal, integration_nodes, scaled,
};
use super::{QuadratureSettings, flat_rates, time_to_expiry};
use crate::engines::analytic::barrier::log_ratio;
use crate::{
    Accumulator, BlackScholesMertonParameters, Date, Error, GreeksRequest, NumericalShiftSettings,
    PricingContext, PricingEngine, PricingResult, Result, Timestamp,
};

/// Prices trading-date accumulators using affine quantity layers and cubic Gaussian integration.
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
    /// use kiyosi_core::*;
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
    /// Knock-out is inclusive and precedes purchases; acceleration is strict below strike.
    /// Settles accrued units at knock-out or expiry. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Checks flat rates ([`Error::UnsupportedOperation`]), inclusive life
    /// ([`Error::InvalidInput`]), then trading expiry
    /// ([`Error::InvalidInput`]). Non-finite results or unrepresentable grids
    /// and transitions return [`Error::ComputationFailure`]. Inputs remain unchanged.
    pub fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        price(option, context, self.settings, &mut None)
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
    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<Accumulator> + Send + Sync + '_>>> {
        Ok(Some(Box::new(Request {
            settings: self.settings,
            curve: std::sync::Mutex::new(None),
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
    curve: std::sync::Mutex<Option<PreparedCurve>>,
}
impl PricingEngine<Accumulator> for Request {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        let mut curve = self.curve.lock().map_err(|_| {
            Error::ComputationFailure("quadrature request cache is poisoned".into())
        })?;
        price(option, context, self.settings, &mut curve)
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        QuadratureAccumulatorEngine::default().volatility_affects_price(option, context)
    }
}

#[derive(Default)]
struct Layer {
    cells: Vec<Polynomial>,
    exact: Vec<TerminalPiece>,
}
impl Layer {
    fn evaluate(
        &self,
        nodes: &[f64],
        spot: f64,
        strike: f64,
        drift: f64,
        deviation: f64,
        discount: f64,
    ) -> f64 {
        let mut sum = Sum::default();
        sum.add(integrate_terminal(
            &self.exact,
            spot,
            strike,
            drift,
            deviation,
            discount,
        ));
        if !self.cells.is_empty() {
            sum.add(integrate_curve(
                &self.cells,
                nodes,
                spot + drift,
                deviation,
                discount,
            ));
        }
        sum.total()
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
impl PreparedCurve {
    fn evaluate(&self, spot: f64, quantity: f64) -> Result<f64> {
        let current = log_ratio(spot, self.option.strike());
        let value = |layer: &Layer| {
            layer.evaluate(
                &self.nodes,
                current,
                self.option.strike(),
                self.drift,
                self.deviation,
                self.discount,
            )
        };
        checked(quantity * value(&self.layers[0]) + value(&self.layers[1]))
    }
}

fn price(
    option: &Accumulator,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: &mut Option<PreparedCurve>,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    let horizon = time_to_expiry(option, context)?;
    if !context.calendar().is_trading_day(option.expiry_date()) {
        return Err(Error::InvalidInput("expiry must be a trading day".into()));
    }
    let (quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return checked(value);
    }
    let mut dates = Vec::new();
    let mut date = context.valuation_date();
    loop {
        if date.start_of_day() > context.valuation_time() && context.calendar().is_trading_day(date)
        {
            dates.push(date);
        }
        if date == option.expiry_date() {
            break;
        }
        date = date.add_days(1)?;
    }
    if let Some(curve) = prepared.as_ref().filter(|c| {
        c.option == *option
            && c.model == *context.model_parameters()
            && c.valuation == context.valuation_time()
            && c.dates == dates
    }) {
        return curve.evaluate(context.spot_price(), quantity);
    }
    let strike = option.strike();
    let knock_out = log_ratio(option.knock_out_level(), strike);
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
    let count = if option.accrues() { 2 } else { 1 };
    let mut layers: [Layer; 2] = Default::default();
    layers[0].exact.push(TerminalPiece {
        lower: f64::NEG_INFINITY,
        upper: f64::INFINITY,
        cash: -strike,
        asset: 1.0,
    });
    if count == 2 {
        let accelerated = option.purchase(0.0);
        layers[1].exact.push(TerminalPiece {
            lower: f64::NEG_INFINITY,
            upper: knock_out.min(0.0),
            cash: -strike * accelerated,
            asset: accelerated,
        });
        if knock_out > 0.0 {
            layers[1].exact.push(TerminalPiece {
                lower: 0.0,
                upper: knock_out,
                cash: -strike * option.daily_quantity(),
                asset: option.daily_quantity(),
            });
        }
    }
    if dates.len() == 1 {
        let (drift, deviation, discount) = transition(horizon)?;
        let current = log_ratio(context.spot_price(), strike);
        let value =
            |layer: &Layer| layer.evaluate(&[], current, strike, drift, deviation, discount);
        return checked(quantity * value(&layers[0]) + value(&layers[1]));
    }
    let extent = settings.integration_standard_deviations * sigma * horizon.sqrt();
    let lower = knock_out.min(0.0) + (drift_rate * horizon).min(0.0) - extent;
    let upper = knock_out.max(0.0) + (drift_rate * horizon).max(0.0) + extent;
    let nodes = integration_nodes(lower, upper, settings.asset_step_count, &[0.0, knock_out])?;
    let interpolation = CubicInterpolation::new(&nodes);
    let mut transitions: Vec<(f64, Transition)> = Vec::new();
    let mut continuation: [Vec<Polynomial>; 2] = Default::default();
    for event in (0..dates.len()).rev() {
        let final_event = event + 1 == dates.len();
        if !final_event {
            let remaining = dates[event]
                .start_of_day()
                .years_until(option.expiry_date().into());
            for (layer, result) in layers.iter_mut().enumerate().take(count) {
                let low_quantity = if layer == 0 {
                    1.0
                } else {
                    (dates.len() - event) as f64 * option.purchase(0.0)
                };
                // The low-price limit retains the asset as well as all remaining purchase cash.
                result.exact = vec![TerminalPiece {
                    lower: f64::NEG_INFINITY,
                    upper: nodes[0],
                    cash: low_quantity * scaled(-strike, -rate * remaining),
                    asset: low_quantity * (-dividend * remaining).exp(),
                }];
                result.cells = vec![[0.0; 4]; nodes.len() - 1];
            }
            layers[0].exact.push(TerminalPiece {
                lower: knock_out,
                upper: f64::INFINITY,
                cash: -strike,
                asset: 1.0,
            });
            for (cell, edges) in nodes.windows(2).enumerate() {
                let point = edges[0].midpoint(edges[1]);
                if point < knock_out {
                    layers[0].cells[cell] = continuation[0][cell];
                    if count == 2 {
                        let purchase = if point < 0.0 {
                            option.purchase(0.0)
                        } else {
                            option.daily_quantity()
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
            let curve = PreparedCurve {
                option: option.clone(),
                model: context.model_parameters().clone(),
                valuation: context.valuation_time(),
                dates,
                nodes,
                layers,
                drift,
                deviation,
                discount,
            };
            let value = curve.evaluate(context.spot_price(), quantity)?;
            *prepared = Some(curve);
            return Ok(value);
        }
        let ti = if final_event {
            None
        } else {
            Some(
                if let Some(index) = transitions.iter().position(|(t, _)| *t == time) {
                    index
                } else {
                    transitions.push((time, Transition::new(&nodes, drift, deviation)));
                    transitions.len() - 1
                },
            )
        };
        for layer in 0..count {
            let mut values = vec![0.0; nodes.len()];
            if let Some(ti) = ti {
                transitions[ti]
                    .1
                    .integrate(&layers[layer].cells, discount, &mut values);
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
