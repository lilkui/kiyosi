use super::gaussian::{
    CubicInterpolation, Polynomial, TerminalPiece, TransitionCache, integrate_curve,
    integrate_terminal, integration_nodes, scaled,
};
use super::{QuadratureSettings, flat_rates};
use crate::engines::analytic::barrier::log_ratio;
use crate::instruments::autocallable::Coupon;
use crate::instruments::autocallable::pricing::{
    fixed_value, future_event_dates, initial, validate, volatility_affects_price,
};
use crate::{
    AutocallableNote, AutocallableObservation, AutocallableTerms, BlackScholesMertonParameters,
    Date, Error, GreeksRequest, NumericalShiftSettings, PricingContext, PricingEngine,
    PricingResult, Result, Timestamp,
};

/// Prices snowball variants and Phoenix notes by cubic Gaussian integration between exact event dates.
///
/// Retains separate intact and knocked-in continuation layers for daily monitoring.
/// Preserves cash and asset payments below the spatial grid, including zero-lower-strike losses.
/// Owns immutable settings; shared and distinct instances support concurrent pricing.
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureAutocallableEngine {
    settings: QuadratureSettings,
}
impl QuadratureAutocallableEngine {
    /// Creates an engine with validated spatial budget and integration extent.
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
    /// let end = Date::new(2025, 4, 1)?;
    /// let mut terms = SnowballTerms::new(0.08, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     autocallable_observations(&[end], &[110.0], Some(&[0.08]), None)?);
    /// terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    /// let note = SnowballOption::new(terms)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = QuadratureAutocallableEngine::new(QuadratureSettings::default())?;
    /// assert_eq!(engine.settings().asset_step_count, 512);
    /// let result = engine.price_with_greeks(&note, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Vega]), Default::default())?;
    /// assert!(result.price().is_finite());
    /// assert!(result.has(Greek::Delta));
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
    /// Prices remaining cashflows, resolving current events before future integration.
    ///
    /// Knock-in is strict below its level; knock-out and Phoenix coupon barriers
    /// are inclusive. Phoenix coupons accrue over contractual observation periods
    /// and are paid independently of knock-out, principal and downside loss.
    /// Past knock-outs return zero, and terminal payments use normalized units.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Checks flat rates ([`Error::UnsupportedOperation`]), then inclusive life,
    /// observation trading dates, missing or impossible prior history, and trading
    /// expiry ([`Error::InvalidInput`]), in that order. Non-finite prices or
    /// unrepresentable transitions return [`Error::ComputationFailure`].
    /// Inputs remain unchanged on success and failure.
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        price(option, context, self.settings, &mut None)
    }
    /// Computes requested scalar sensitivities using shared core bump defaults.
    ///
    /// Expiry Greeks are absent. Spot and time stencils crossing active events
    /// are unavailable; vega and rho remain defined at current thresholds.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request/shift validation, then
    /// propagates [`crate::numerical_greeks`] errors. Inputs remain unchanged.
    pub fn price_with_greeks<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}
impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>> for QuadratureAutocallableEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<AutocallableNote<T>> + '_>>> {
        Ok(Some(Box::new(Request {
            settings: self.settings,
            curve: std::cell::RefCell::new(None),
        })))
    }
    fn volatility_affects_price(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> bool {
        volatility_affects_price(option, context)
    }
}
struct Request<T: AutocallableTerms> {
    settings: QuadratureSettings,
    curve: std::cell::RefCell<Option<PreparedCurve<T>>>,
}
impl<T: AutocallableTerms> PricingEngine<AutocallableNote<T>> for Request<T> {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &AutocallableNote<T>, context: &PricingContext) -> Result<f64> {
        let mut curve = self.curve.borrow_mut();
        price(option, context, self.settings, &mut curve)
    }
    fn volatility_affects_price(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> bool {
        volatility_affects_price(option, context)
    }
}

fn checked(value: f64) -> Result<f64> {
    Ok(PricingResult::new(value)?.price())
}

struct Curve {
    cells: Vec<Polynomial>,
    low: [f64; 2],
    high: f64,
}
impl Curve {
    fn evaluate(
        &self,
        nodes: &[f64],
        mean: f64,
        anchor: f64,
        deviation: f64,
        discount: f64,
    ) -> f64 {
        integrate_curve(&self.cells, nodes, mean, deviation, discount)
            + integrate_terminal(&self.tails(nodes), mean, anchor, 0.0, deviation, discount)
    }
    fn tails(&self, nodes: &[f64]) -> [TerminalPiece; 2] {
        [
            TerminalPiece {
                lower: f64::NEG_INFINITY,
                upper: nodes[0],
                cash: self.low[0],
                asset: self.low[1],
            },
            TerminalPiece {
                lower: nodes[nodes.len() - 1],
                upper: f64::INFINITY,
                cash: self.high,
                asset: 0.0,
            },
        ]
    }
}
struct PreparedCurve<T: AutocallableTerms> {
    option: AutocallableNote<T>,
    model: BlackScholesMertonParameters,
    valuation: Timestamp,
    knocked_in: bool,
    nodes: Vec<f64>,
    dates: Vec<Date>,
    curve: Curve,
    drift: f64,
    deviation: f64,
    discount: f64,
}
impl<T: AutocallableTerms> PreparedCurve<T> {
    fn evaluate(&self, spot: f64) -> Result<f64> {
        let anchor = self.option.view().reference_spot();
        checked(self.curve.evaluate(
            &self.nodes,
            log_ratio(spot, anchor) + self.drift,
            anchor,
            self.deviation,
            self.discount,
        ))
    }
}
fn event<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    date: Date,
) -> Option<&AutocallableObservation> {
    note.observations()
        .binary_search_by_key(&date, |r| r.date)
        .ok()
        .map(|i| &note.observations()[i])
}
fn terminal<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    record: Option<&AutocallableObservation>,
    breaks: &[f64],
    knocked_in: bool,
) -> Vec<TerminalPiece> {
    let t = note.view();
    let anchor = t.reference_spot();
    let mut edges = vec![f64::NEG_INFINITY];
    edges.extend_from_slice(breaks);
    edges.push(f64::INFINITY);
    edges
        .windows(2)
        .map(|e| {
            let point = if e[0].is_infinite() {
                e[1] - 1.0
            } else if e[1].is_infinite() {
                e[0] + 1.0
            } else {
                e[0].midpoint(e[1])
            };
            let mut cash;
            let mut asset = 0.0;
            if let Some(r) = record.filter(|r| point > log_ratio(r.knock_out_level, anchor)) {
                cash = note.knock_out_payment(r);
            } else if knocked_in
                || t.knock_in
                    .is_some_and(|ki| point < log_ratio(ki.level, anchor))
            {
                if let Some(d) = t.downside() {
                    if d.lower_strike > 0.0 && point < log_ratio(d.lower_strike, anchor) {
                        cash = t.principal_ratio - d.upper_strike / d.initial_spot;
                        cash += d.lower_strike / d.initial_spot;
                    } else if point > log_ratio(d.upper_strike, anchor) {
                        cash = t.principal_ratio;
                    } else {
                        cash = t.principal_ratio - d.upper_strike / d.initial_spot;
                        asset = 1.0 / d.initial_spot;
                    }
                } else {
                    cash = note.terminal(anchor, true);
                }
            } else {
                cash = note.terminal(anchor, false);
            }
            if let Some(r) = record.filter(|r| {
                r.coupon_barrier_level == 0.0 || point > log_ratio(r.coupon_barrier_level, anchor)
            }) {
                cash += note.period_coupon(r);
            }
            TerminalPiece {
                lower: e[0],
                upper: e[1],
                cash,
                asset,
            }
        })
        .collect()
}
fn apply_event<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    date: Date,
    nodes: &[f64],
    alive: &mut Curve,
    knocked: Option<&Curve>,
) {
    let t = note.view();
    let anchor = t.reference_spot();
    let record = event(note, date);
    let amount = record.map_or(0.0, |r| note.period_coupon(r));
    let coupon = |point: f64| {
        record
            .filter(|r| {
                r.coupon_barrier_level == 0.0 || point >= log_ratio(r.coupon_barrier_level, anchor)
            })
            .map_or(0.0, |_| amount)
    };
    if let Some(r) = record {
        alive.high = note.knock_out_payment(r) + coupon(f64::INFINITY);
    }
    if let Some(knocked) = knocked {
        alive.low = knocked.low;
    }
    alive.low[0] += coupon(f64::NEG_INFINITY);
    for (i, cell) in alive.cells.iter_mut().enumerate() {
        let point = nodes[i].midpoint(nodes[i + 1]);
        if let Some(r) = record.filter(|r| point > log_ratio(r.knock_out_level, anchor)) {
            *cell = [note.knock_out_payment(r), 0.0, 0.0, 0.0];
        } else if let Some(knocked) = knocked.filter(|_| {
            t.knock_in
                .is_some_and(|ki| point < log_ratio(ki.level, anchor))
        }) {
            *cell = knocked.cells[i];
        }
        cell[0] += coupon(point);
    }
}
fn price<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: &mut Option<PreparedCurve<T>>,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    validate(note, context)?;
    let state = initial(note, context);
    if let Some(value) = state
        .settlement
        .or_else(|| fixed_value(note, context, state.knocked_in))
    {
        return checked(
            value
                + if state.settlement.is_none() {
                    state.coupon
                } else {
                    0.0
                },
        );
    }
    let layers =
        note.view().daily_knock_in() && !state.knocked_in && note.knock_in_affects_payoff();
    let dates = future_event_dates(note, context, layers)?;
    if let Some(curve) = prepared.as_ref().filter(|c| {
        c.option == *note
            && c.model == *context.model_parameters()
            && c.valuation == context.valuation_time()
            && c.knocked_in == state.knocked_in
            && c.dates == dates
    }) {
        return checked(state.coupon + curve.evaluate(context.spot_price())?);
    }
    let t = note.view();
    let anchor = t.reference_spot();
    let mut breaks = Vec::new();
    if let Some(ki) = t.knock_in {
        breaks.push(log_ratio(ki.level, anchor));
    }
    if let Some(d) = t.downside() {
        breaks.push(log_ratio(d.upper_strike, anchor));
        if d.lower_strike > 0.0 {
            breaks.push(log_ratio(d.lower_strike, anchor));
        }
    }
    for r in note
        .observations()
        .iter()
        .filter(|r| r.date.start_of_day() > context.valuation_time())
    {
        breaks.push(log_ratio(r.knock_out_level, anchor));
        if matches!(t.coupon, Coupon::Phoenix(_)) && r.coupon_barrier_level > 0.0 {
            breaks.push(log_ratio(r.coupon_barrier_level, anchor));
        }
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let sigma = context.model_parameters().volatility();
    let drift_rate = rate - dividend - 0.5 * sigma * sigma;
    let horizon = context
        .valuation_time()
        .years_until(note.expiry_date().into());
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
    let terminal = terminal(
        note,
        event(note, note.expiry_date()),
        &breaks,
        state.knocked_in,
    );
    if dates.len() == 1 {
        let (drift, deviation, discount) = transition(horizon)?;
        return checked(
            state.coupon
                + integrate_terminal(
                    &terminal,
                    log_ratio(context.spot_price(), anchor),
                    anchor,
                    drift,
                    deviation,
                    discount,
                ),
        );
    }
    let extent = settings.integration_standard_deviations * sigma * horizon.sqrt();
    let lower = 0.0_f64.min(breaks[0]) + (drift_rate * horizon).min(0.0) - extent;
    let upper = 0.0_f64.max(breaks[breaks.len() - 1]) + (drift_rate * horizon).max(0.0) + extent;
    let nodes = integration_nodes(lower, upper, settings.asset_step_count, &breaks)?;
    let interpolation = CubicInterpolation::new(&nodes);
    let terminal_in = if layers {
        self::terminal(note, event(note, note.expiry_date()), &breaks, true)
    } else {
        Vec::new()
    };
    let mut alive = Curve {
        cells: Vec::new(),
        low: [terminal[0].cash, terminal[0].asset],
        high: terminal[terminal.len() - 1].cash,
    };
    let mut knocked = Curve {
        cells: Vec::new(),
        low: terminal_in.first().map_or([0.0; 2], |p| [p.cash, p.asset]),
        high: terminal_in.last().map_or(0.0, |p| p.cash),
    };
    let mut transitions = TransitionCache::default();
    for index in (0..dates.len()).rev() {
        let final_event = index == dates.len() - 1;
        if !final_event {
            apply_event(
                note,
                dates[index],
                &nodes,
                &mut alive,
                layers.then_some(&knocked),
            );
            if layers {
                apply_event(note, dates[index], &nodes, &mut knocked, None);
            }
        }
        let time = if index == 0 {
            context.valuation_time().years_until(dates[index].into())
        } else {
            dates[index - 1]
                .start_of_day()
                .years_until(dates[index].into())
        };
        let (drift, deviation, discount) = transition(time)?;
        if index == 0 {
            let curve = PreparedCurve {
                option: note.clone(),
                model: context.model_parameters().clone(),
                valuation: context.valuation_time(),
                knocked_in: state.knocked_in,
                nodes,
                dates,
                curve: alive,
                drift,
                deviation,
                discount,
            };
            let value = checked(state.coupon + curve.evaluate(context.spot_price())?)?;
            *prepared = Some(curve);
            return Ok(value);
        }
        let mut values = vec![0.0; nodes.len()];
        let mut values_in = vec![0.0; if layers { nodes.len() } else { 0 }];
        if final_event {
            for (i, &node) in nodes.iter().enumerate() {
                values[i] = integrate_terminal(&terminal, node, anchor, drift, deviation, discount);
                if layers {
                    values_in[i] =
                        integrate_terminal(&terminal_in, node, anchor, drift, deviation, discount);
                }
            }
        } else {
            let transition = transitions.get(time, &nodes, drift, deviation);
            transition.integrate(&alive.cells, discount, &mut values);
            if layers {
                transition.integrate(&knocked.cells, discount, &mut values_in);
            }
            let tails = alive.tails(&nodes);
            let tails_in = knocked.tails(&nodes);
            for (i, &node) in nodes.iter().enumerate() {
                values[i] += integrate_terminal(&tails, node, anchor, drift, deviation, discount);
                if layers {
                    values_in[i] +=
                        integrate_terminal(&tails_in, node, anchor, drift, deviation, discount);
                }
            }
        }
        if values.iter().chain(&values_in).any(|v| !v.is_finite()) {
            return Err(Error::ComputationFailure(
                "quadrature continuation is non-finite".into(),
            ));
        }
        alive = Curve {
            cells: interpolation.interpolate(&values),
            low: [
                scaled(alive.low[0], discount),
                scaled(alive.low[1], -dividend * time),
            ],
            high: scaled(alive.high, discount),
        };
        if layers {
            knocked = Curve {
                cells: interpolation.interpolate(&values_in),
                low: [
                    scaled(knocked.low[0], discount),
                    scaled(knocked.low[1], -dividend * time),
                ],
                high: scaled(knocked.high, discount),
            };
        }
    }
    Err(Error::ComputationFailure(
        "quadrature has no future event".into(),
    ))
}
