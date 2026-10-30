use super::flat_rates;
use crate::engines::analytic::barrier::{
    checked, hit_value, log_ratio, resolved_price, scaled_cash, validate_life, validate_schedule,
};
use crate::{
    BarrierOption, Error, GreeksRequest, NumericalShiftSettings, OptionType, PricingContext,
    PricingEngine, PricingResult, RebateTiming, Result,
};

/// Configures the spatial budget and integration extent for event-date quadrature.
///
/// Check spatial and domain convergence separately; see [`QuadratureBarrierEngine::new`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadratureSettings {
    /// Sets the asset grid budget (at least four); continuous integration rounds up to even panels.
    pub asset_step_count: i32,
    /// Sets the finite positive integration extent in standard deviations.
    pub integration_standard_deviations: f64,
}
impl Default for QuadratureSettings {
    fn default() -> Self {
        Self {
            asset_step_count: 512,
            integration_standard_deviations: 8.0,
        }
    }
}
impl QuadratureSettings {
    /// Validates the spatial budget and extent. See [`QuadratureBarrierEngine::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] below four steps or for a non-finite
    /// or nonpositive extent. Does not change the settings.
    pub fn validate(&self) -> Result<()> {
        if self.asset_step_count < 4
            || !self.integration_standard_deviations.is_finite()
            || self.integration_standard_deviations <= 0.0
        {
            return Err(Error::InvalidInput("quadrature requires at least four asset steps and a finite positive integration extent".into()));
        }
        Ok(())
    }
}

/// Prices single-barrier contracts by flat-rate quadrature with exact observation dates.
///
/// Uses Simpson integration for continuous monitoring and cubic Gaussian integration
/// between scheduled observations, including schedules ending before expiry.
/// Owns its settings; shared instances support concurrent pricing. See [`Self::new`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureBarrierEngine {
    settings: QuadratureSettings,
}
impl QuadratureBarrierEngine {
    /// Creates an engine after validating the spatial budget and extent.
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
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = QuadratureBarrierEngine::new(QuadratureSettings {
    ///     asset_step_count: 1024, integration_standard_deviations: 10.0,
    /// })?;
    /// assert_eq!(engine.settings().asset_step_count, 1024);
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn new(settings: QuadratureSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }
    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> QuadratureSettings {
        self.settings
    }

    /// Prices after validating flat rates, inclusive life, calendar and strictly prior history.
    ///
    /// Current boundary hits settle before future monitoring; prior at-hit rebates are paid.
    /// Resolved vanilla payoffs use analytic pricing. See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for node curves. Returns
    /// [`Error::InvalidInput`] outside life, for non-trading fixings, or for
    /// missing or impossible history, checked in that order. Unrepresentable grids,
    /// transitions or results return [`Error::ComputationFailure`]. Inputs remain unchanged.
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        price(option, context, self.settings, &mut None)
    }

    /// Computes requested scalar numerical Greeks with shared bump defaults.
    ///
    /// At expiry or a current barrier equality without a prior touch, all Greeks are absent.
    /// Stencils crossing observations or spot barriers leave affected measures absent.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request/shift validation and propagates
    /// [`crate::numerical_greeks`] and [`Self::price`] errors. Inputs remain unchanged.
    pub fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}
impl PricingEngine<BarrierOption> for QuadratureBarrierEngine {
    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<BarrierOption> + Send + Sync + '_>>> {
        Ok(Some(Box::new(BarrierPricer {
            settings: self.settings,
            prepared: std::sync::Mutex::new(None),
        })))
    }
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        crate::engines::analytic::barrier::volatility_affects_price(option, context)
    }
}

fn price(
    option: &BarrierOption,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: &mut Option<PreparedCurve>,
) -> Result<f64> {
    flat_rates(context)?;
    validate_life(option, context)?;
    validate_schedule(option, context)?;
    if let Some(value) = resolved_price(option, context)? {
        return Ok(value);
    }
    if option.observation_mode() == crate::ObservationMode::Continuous {
        continuous_price(option, context, settings)
    } else {
        scheduled_price(option, context, settings, prepared)
    }
}

struct BarrierPricer {
    settings: QuadratureSettings,
    prepared: std::sync::Mutex<Option<PreparedCurve>>,
}
impl PricingEngine<BarrierOption> for BarrierPricer {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        let mut prepared = self.prepared.lock().map_err(|_| {
            Error::ComputationFailure("quadrature request cache is poisoned".into())
        })?;
        price(option, context, self.settings, &mut prepared)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        crate::engines::analytic::barrier::volatility_affects_price(option, context)
    }
}

fn invalid_transition() -> Error {
    Error::ComputationFailure("quadrature transition is not representable".into())
}
fn continuous_price(
    option: &BarrierOption,
    context: &PricingContext,
    settings: QuadratureSettings,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    let time = context
        .valuation_time()
        .years_until(option.expiry_date().start_of_day());
    let sigma = context.model_parameters().volatility();
    let width = sigma * time.sqrt();
    let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
    let barrier = log_ratio(option.barrier_level(), context.spot_price());
    let strike = log_ratio(option.strike(), context.spot_price());
    if width <= 0.0 || !width.is_finite() || !drift.is_finite() {
        return Err(invalid_transition());
    }
    let extent = settings.integration_standard_deviations;
    let upper = extent
        + if option.option_type() == OptionType::Call {
            width
        } else {
            0.0
        };
    let mut breaks = vec![-extent, upper];
    for point in [(strike - drift) / width, (barrier - drift) / width] {
        if point > -extent && point < upper {
            breaks.push(point);
        }
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let knock_in = option.barrier_type().is_knock_in();
    let up = option.barrier_type().is_up();
    let sign = if option.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let log_discount = -rate * time;
    let integrand = |z: f64| {
        let terminal = drift + width * z;
        let live = if up {
            terminal < barrier
        } else {
            terminal > barrier
        };
        let exponent = if live {
            -2.0 * (barrier / width) * ((barrier - terminal) / width)
        } else {
            0.0
        };
        let log_survival = if live {
            (-exponent.exp_m1()).ln()
        } else {
            f64::NEG_INFINITY
        };
        let log_density = log_discount - 0.5 * z * z - 0.5 * (2.0 * std::f64::consts::PI).ln();
        let distance = terminal - strike;
        let mut value = 0.0;
        if sign * distance > 0.0 {
            let log_payoff = option.strike().ln()
                + if distance > 0.0 {
                    distance + (-(-distance).exp_m1()).ln()
                } else {
                    (-distance.exp_m1()).ln()
                };
            value =
                (log_payoff + if knock_in { exponent } else { log_survival } + log_density).exp();
        }
        if knock_in || option.rebate_timing() == RebateTiming::AtExpiry {
            value += scaled_cash(
                option.rebate(),
                if knock_in { log_survival } else { exponent } + log_density,
            );
        }
        value
    };
    let panels = settings.asset_step_count as usize + settings.asset_step_count as usize % 2;
    let mut result = super::gaussian::Sum::default();
    for section in breaks.windows(2) {
        let step = (section[1] - section[0]) / panels as f64;
        let mut sum = super::gaussian::Sum::default();
        sum.add(integrand(section[0]));
        sum.add(integrand(section[1]));
        for index in 1..panels {
            sum.add(
                if index % 2 == 0 { 2.0 } else { 4.0 }
                    * integrand(section[0] + index as f64 * step),
            );
        }
        result.add(sum.total() * step / 3.0);
    }
    if !knock_in && option.rebate_timing() == RebateTiming::AtHit && option.rebate() != 0.0 {
        result.add(hit_value(
            option.rebate(),
            barrier.abs(),
            up,
            drift / time,
            sigma,
            time,
            rate,
        )?);
    }
    checked(result.total())
}

fn terminal_layer(option: &BarrierOption) -> Vec<super::gaussian::TerminalPiece> {
    let barrier = log_ratio(option.barrier_level(), option.strike());
    let mut edges = vec![
        f64::NEG_INFINITY,
        0.0_f64.min(barrier),
        0.0_f64.max(barrier),
        f64::INFINITY,
    ];
    edges.dedup();
    edges
        .windows(2)
        .map(|edges| {
            let point = if edges[0] == f64::NEG_INFINITY {
                edges[1] - 1.0
            } else if edges[1] == f64::INFINITY {
                edges[0] + 1.0
            } else {
                edges[0].midpoint(edges[1])
            };
            let hit = if option.barrier_type().is_up() {
                point > barrier
            } else {
                point < barrier
            };
            let mut cash = 0.0;
            let mut asset = 0.0;
            if hit == option.barrier_type().is_knock_in() {
                let sign = if option.option_type() == OptionType::Call {
                    1.0
                } else {
                    -1.0
                };
                if sign * point > 0.0 {
                    cash = -sign * option.strike();
                    asset = sign;
                }
            } else {
                cash = option.rebate();
            }
            super::gaussian::TerminalPiece {
                lower: edges[0],
                upper: edges[1],
                cash,
                asset,
            }
        })
        .collect()
}

use super::gaussian::{
    CubicInterpolation, Polynomial, Sum, TerminalPiece, Transition, integrate_curve,
    integrate_terminal, integration_nodes, scaled,
};

#[derive(Default)]
struct Layer {
    curve: Vec<Polynomial>,
    exact: Vec<TerminalPiece>,
}
impl Layer {
    fn evaluate(
        &self,
        nodes: &[f64],
        current: f64,
        anchor: f64,
        drift: f64,
        deviation: f64,
        discount: f64,
    ) -> f64 {
        let mut sum = Sum::default();
        sum.add(integrate_terminal(
            &self.exact,
            current,
            anchor,
            drift,
            deviation,
            discount,
        ));
        if !self.curve.is_empty() {
            sum.add(integrate_curve(
                &self.curve,
                nodes,
                current + drift,
                deviation,
                discount,
            ));
        }
        sum.total()
    }
}

fn event_limit(
    option: &BarrierOption,
    remaining: f64,
    context: &PricingContext,
    high: bool,
) -> Result<[f64; 2]> {
    let (rate, dividend) = flat_rates(context)?;
    let hit = option.barrier_type().is_up() == high;
    if hit == option.barrier_type().is_knock_in() {
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        return Ok(if (sign > 0.0) != high {
            [0.0; 2]
        } else {
            [
                -sign * scaled(option.strike(), -rate * remaining),
                sign * (-dividend * remaining).exp(),
            ]
        });
    }
    let discount =
        if !option.barrier_type().is_knock_in() && option.rebate_timing() == RebateTiming::AtHit {
            0.0
        } else {
            -rate * remaining
        };
    Ok([scaled(option.rebate(), discount), 0.0])
}

fn event_layer(
    option: &BarrierOption,
    context: &PricingContext,
    date: crate::Date,
    nodes: &[f64],
    interpolation: &CubicInterpolation,
    continuation: &[f64],
) -> Result<Layer> {
    let remaining = date
        .start_of_day()
        .years_until(option.expiry_date().start_of_day());
    let knock_in = option.barrier_type().is_knock_in();
    let (rate, _) = flat_rates(context)?;
    let rebate = scaled(
        option.rebate(),
        if !knock_in && option.rebate_timing() == RebateTiming::AtHit {
            0.0
        } else {
            -rate * remaining
        },
    );
    let mut vanilla_curve = Vec::new();
    if knock_in || continuation.is_empty() {
        let vanilla = option.vanilla();
        let values = nodes
            .iter()
            .map(|&node| {
                let spot = scaled(option.strike(), node);
                if !spot.is_finite() || spot <= 0.0 {
                    return Err(Error::ComputationFailure(
                        "quadrature asset grid is not representable".into(),
                    ));
                }
                let shifted = context.replace(crate::PricingContextChanges {
                    spot_price: Some(spot),
                    valuation_time: Some(date.into()),
                    ..Default::default()
                })?;
                crate::AnalyticEuropeanEngine.price(vanilla, &shifted)
            })
            .collect::<Result<Vec<_>>>()?;
        vanilla_curve = interpolation.interpolate(&values);
    }
    let mut curve = if continuation.is_empty() {
        if knock_in {
            vec![[rebate, 0.0, 0.0, 0.0]; nodes.len() - 1]
        } else {
            vanilla_curve.clone()
        }
    } else {
        interpolation.interpolate(continuation)
    };
    let barrier = log_ratio(option.barrier_level(), option.strike());
    for (cell, polynomial) in curve.iter_mut().enumerate() {
        let point = nodes[cell].midpoint(nodes[cell + 1]);
        if if option.barrier_type().is_up() {
            point > barrier
        } else {
            point < barrier
        } {
            *polynomial = if knock_in {
                vanilla_curve[cell]
            } else {
                [rebate, 0.0, 0.0, 0.0]
            };
        }
    }
    let low = event_limit(option, remaining, context, false)?;
    let high = event_limit(option, remaining, context, true)?;
    Ok(Layer {
        curve,
        exact: vec![
            TerminalPiece {
                lower: f64::NEG_INFINITY,
                upper: nodes[0],
                cash: low[0],
                asset: low[1],
            },
            TerminalPiece {
                lower: nodes[nodes.len() - 1],
                upper: f64::INFINITY,
                cash: high[0],
                asset: high[1],
            },
        ],
    })
}

struct PreparedCurve {
    option: BarrierOption,
    model: crate::BlackScholesMertonParameters,
    valuation: crate::Timestamp,
    nodes: Vec<f64>,
    layer: Layer,
    drift: f64,
    deviation: f64,
    discount: f64,
}
impl PreparedCurve {
    fn evaluate(&self, spot: f64) -> Result<f64> {
        checked(self.layer.evaluate(
            &self.nodes,
            log_ratio(spot, self.option.strike()),
            self.option.strike(),
            self.drift,
            self.deviation,
            self.discount,
        ))
    }
}

fn scheduled_price(
    option: &BarrierOption,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: &mut Option<PreparedCurve>,
) -> Result<f64> {
    if let Some(curve) = prepared.as_ref() {
        if curve.option == *option
            && curve.model == *context.model_parameters()
            && curve.valuation == context.valuation_time()
        {
            return curve.evaluate(context.spot_price());
        }
    }
    let dates: Vec<_> = option
        .observation_dates()
        .iter()
        .copied()
        .filter(|date| date.start_of_day() > context.valuation_time())
        .collect();
    let (rate, dividend) = flat_rates(context)?;
    let sigma = context.model_parameters().volatility();
    let drift_rate = rate - dividend - 0.5 * sigma * sigma;
    let horizon = context
        .valuation_time()
        .years_until(option.expiry_date().start_of_day());
    let extent = settings.integration_standard_deviations * sigma * horizon.sqrt();
    let barrier = log_ratio(option.barrier_level(), option.strike());
    let current = log_ratio(context.spot_price(), option.strike());
    if dates.len() == 1 && dates[0] == option.expiry_date() {
        let deviation = sigma * horizon.sqrt();
        let drift = drift_rate * horizon;
        if deviation <= 0.0 || !deviation.is_finite() || !drift.is_finite() {
            return Err(invalid_transition());
        }
        return checked(integrate_terminal(
            &terminal_layer(option),
            current,
            option.strike(),
            drift,
            deviation,
            -rate * horizon,
        ));
    }
    let lower = 0.0_f64.min(barrier).min(current) + (drift_rate * horizon).min(0.0) - extent;
    let upper = 0.0_f64.max(barrier).max(current) + (drift_rate * horizon).max(0.0) + extent;
    let nodes = integration_nodes(lower, upper, settings.asset_step_count, &[0.0, barrier])?;
    let interpolation = CubicInterpolation::new(&nodes);
    let mut values = vec![0.0; nodes.len()];
    let mut transitions: Vec<(f64, Transition)> = Vec::new();
    for event in (0..dates.len()).rev() {
        let final_event = event + 1 == dates.len();
        let layer = if final_event && dates[event] == option.expiry_date() {
            Layer {
                curve: Vec::new(),
                exact: terminal_layer(option),
            }
        } else {
            event_layer(
                option,
                context,
                dates[event],
                &nodes,
                &interpolation,
                if final_event { &[] } else { &values },
            )?
        };
        let time = if event == 0 {
            context
                .valuation_time()
                .years_until(dates[event].start_of_day())
        } else {
            dates[event - 1]
                .start_of_day()
                .years_until(dates[event].start_of_day())
        };
        let drift = drift_rate * time;
        let deviation = sigma * time.sqrt();
        let discount = -rate * time;
        if deviation <= 0.0 || !deviation.is_finite() || !drift.is_finite() {
            return Err(invalid_transition());
        }
        if event == 0 {
            let curve = PreparedCurve {
                option: option.clone(),
                model: context.model_parameters().clone(),
                valuation: context.valuation_time(),
                nodes,
                layer,
                drift,
                deviation,
                discount,
            };
            let value = curve.evaluate(context.spot_price())?;
            *prepared = Some(curve);
            return Ok(value);
        }
        if !layer.curve.is_empty() {
            let index = if let Some(index) = transitions
                .iter()
                .position(|(duration, _)| *duration == time)
            {
                index
            } else {
                transitions.push((time, Transition::new(&nodes, drift, deviation)));
                transitions.len() - 1
            };
            transitions[index]
                .1
                .integrate(&layer.curve, discount, &mut values);
        } else {
            values.fill(0.0);
        }
        for (value, &node) in values.iter_mut().zip(&nodes) {
            *value += integrate_terminal(
                &layer.exact,
                node,
                option.strike(),
                drift,
                deviation,
                discount,
            );
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(Error::ComputationFailure(
                "quadrature continuation is non-finite".into(),
            ));
        }
    }
    Err(Error::ComputationFailure(
        "quadrature has no future observation".into(),
    ))
}
