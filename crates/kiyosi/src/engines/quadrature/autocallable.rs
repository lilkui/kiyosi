use super::gaussian::{
    CubicInterpolation, CubicPiece, Layer, TerminalPiece, TransitionCache, integrate_cubics,
    integrate_terminal, integration_nodes,
};
use super::{QuadratureSettings, flat_rates};
use crate::instruments::autocallable::ObservationEvent;
use crate::instruments::autocallable::pricing::{
    fixed_value, future_event_times, initial, validate,
};
use crate::numerics::{finite_price, log_ratio, scaled_signed_cash};
use crate::{
    AutocallableNote, AutocallableTerms, BlackScholesMertonParameters, Error, PricingContext,
    PricingEngine, Result, Timestamp,
};

/// Prices snowball variants and Phoenix notes by quadrature between event dates.
///
/// Integrates piecewise cubic continuation values against Gaussian transition densities.
/// Rolls back cash flows on event dates and the remaining knock-in exposure on daily dates.
/// Preserves cash and asset payments below the spatial grid, including zero-lower-strike losses.
/// Extends the spatial domain for forward drift and the backward movement of event boundaries.
/// Spatial resolution and integration extent require separate convergence checks.
/// Shared and distinct instances support concurrent pricing.
/// See [`Self::new`] for an example.
///
/// # Greeks
///
/// Computes requested scalar sensitivities using shared core bump defaults.
///
/// Expiry Greeks follow terminal payoffs; time Greeks are absent. Spot and time stencils crossing active events
/// are unavailable; vega and rho remain defined at current thresholds.
/// See [`Self::new`] for an example.
///
/// ## Errors
///
/// Checks flat-rate applicability before shift validation, then
/// propagates [`crate::numerical_greeks`] errors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureAutocallableEngine {
    settings: QuadratureSettings,
}
impl QuadratureAutocallableEngine {
    /// Creates an engine with validated spatial resolution and integration extent.
    ///
    /// # Errors
    ///
    /// Propagates [`QuadratureSettings::validate`] errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2025, 4, 1)?.start_of_day();
    /// let terms = SnowballTerms::new(0.08, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     autocallable_observations(&[end], &[110.0], &[0.08])?, KnockInObservation::AtExpiry, kiyosi::TradingCalendar::weekdays());
    /// let note = SnowballOption::new(terms)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = QuadratureAutocallableEngine::new(QuadratureSettings::default())?;
    /// assert_eq!(engine.settings().asset_step_count, 512);
    /// let result = engine.price_with_greeks(&note, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Vega]))?;
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
    /// Prices remaining cash flows, resolving current events before future integration.
    ///
    /// Knock-in is strict below its level; knock-out and Phoenix coupon barriers
    /// are inclusive. Phoenix coupons pay a fixed proportion of initial notional
    /// per qualifying observation without time accrual. Coupons are paid
    /// independently of knock-out, principal and downside loss.
    /// Past knock-outs return zero, and terminal payments use normalized units.
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Checks flat rates ([`Error::UnsupportedOperation`]), then inclusive life,
    /// impossible prior history, and required curve coverage
    /// ([`Error::InvalidInput`]), in that order. Non-finite prices or
    /// unrepresentable transitions return [`Error::ComputationFailure`].
    ///
    pub fn price<T: AutocallableTerms>(
        &self,
        option: &AutocallableNote<T>,
        context: &PricingContext,
    ) -> Result<f64> {
        price(option, context, self.settings, None)
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
        price(option, context, self.settings, Some(&mut curve))
    }
}

struct PreparedCurve<T: AutocallableTerms> {
    option: AutocallableNote<T>,
    model: BlackScholesMertonParameters,
    valuation: Timestamp,
    knocked_in: bool,
    nodes: Vec<f64>,
    legs: Vec<PreparedLayer>,
}

struct PreparedLayer {
    curve: Layer,
    split: Vec<CubicPiece>,
    drift: f64,
    deviation: f64,
    discount: f64,
}
impl PreparedLayer {
    fn price(&self, nodes: &[f64], current: f64, anchor: f64) -> f64 {
        self.curve.evaluate(
            nodes,
            current,
            anchor,
            self.drift,
            self.deviation,
            self.discount,
        ) + integrate_cubics(
            &self.split,
            current + self.drift,
            self.deviation,
            self.discount,
        )
    }
}

fn terminal<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    record: Option<&ObservationEvent>,
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
            if let Some(r) = record
                .filter(|r| r.has_knock_out() && point > log_ratio(r.knock_out_level(), anchor))
            {
                cash = note.knock_out_payment(r);
            } else if knocked_in
                || note.observes_knock_in(note.expiry_time())
                    && t.knock_in
                        .is_some_and(|ki| point < log_ratio(ki.level, anchor))
            {
                if let Some(d) = t.downside() {
                    if d.lower_strike > 0.0 && point < log_ratio(d.lower_strike, anchor) {
                        cash = t.principal_redemption_ratio - d.upper_strike / d.initial_spot;
                        cash += d.lower_strike / d.initial_spot;
                    } else if point > log_ratio(d.upper_strike, anchor) {
                        cash = t.principal_redemption_ratio;
                    } else {
                        cash = t.principal_redemption_ratio - d.upper_strike / d.initial_spot;
                        asset = 1.0 / d.initial_spot;
                    }
                    cash += note.rebate_payment(note.expiry_time());
                } else {
                    cash = note.terminal(anchor, true);
                }
            } else {
                cash = note.terminal(anchor, false);
            }
            if record.is_some_and(|r| {
                r.coupon_barrier_level()
                    .is_some_and(|level| level == 0.0 || point > log_ratio(level, anchor))
            }) {
                cash += note.period_coupon();
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
struct Event {
    knock_in: Option<f64>,
    knock_out: Option<f64>,
    coupon: Option<f64>,
    redemption: f64,
    amount: f64,
}

impl Event {
    fn new<T: AutocallableTerms>(
        note: &AutocallableNote<T>,
        record: Option<&ObservationEvent>,
        has_knock_in: bool,
    ) -> Self {
        let terms = note.view();
        let anchor = terms.reference_spot();
        let knock_out = record.filter(|r| r.has_knock_out());
        Self {
            knock_in: has_knock_in
                .then_some(terms.knock_in)
                .flatten()
                .map(|ki| log_ratio(ki.level, anchor)),
            knock_out: knock_out.map(|r| log_ratio(r.knock_out_level(), anchor)),
            coupon: record
                .and_then(ObservationEvent::coupon_barrier_level)
                .map(|level| log_ratio(level, anchor)),
            redemption: knock_out.map_or(0.0, |r| note.knock_out_payment(r)),
            amount: note.period_coupon(),
        }
    }

    fn breaks(&self) -> [Option<f64>; 3] {
        [self.knock_in, self.knock_out, self.coupon]
    }

    fn coupon_at(&self, point: f64) -> f64 {
        if self.coupon.is_some_and(|level| point >= level) {
            self.amount
        } else {
            0.0
        }
    }

    fn apply(&self, nodes: &[f64], alive: &mut Layer, pieces: &mut Vec<CubicPiece>) {
        pieces.clear();
        if self.knock_out.is_some() {
            alive.exact[1].cash = self.redemption;
        }
        alive.exact[1].cash += self.coupon_at(f64::INFINITY);
        if self.knock_in.is_some() {
            alive.exact[0].cash = 0.0;
            alive.exact[0].asset = 0.0;
        }
        alive.exact[0].cash += self.coupon_at(f64::NEG_INFINITY);
        let mut breaks = self.breaks().map(|level| level.unwrap_or(f64::INFINITY));
        breaks.sort_unstable_by(f64::total_cmp);
        for (i, cell) in alive.cells.iter_mut().enumerate() {
            let [left, right] = [nodes[i], nodes[i + 1]];
            let polynomial = |lower, upper| {
                let mut value = if self.knock_out.is_some_and(|level| lower >= level) {
                    [self.redemption, 0.0, 0.0, 0.0]
                } else if self.knock_in.is_some_and(|level| upper <= level) {
                    [0.0; 4]
                } else {
                    *cell
                };
                value[0] += self.coupon_at(lower);
                value
            };
            if breaks.iter().any(|&cut| left < cut && cut < right) {
                let mut last = left;
                for end in breaks
                    .into_iter()
                    .filter(|&cut| left < cut && cut < right)
                    .chain([right])
                {
                    if last < end {
                        pieces.push(CubicPiece::restricted(
                            [last, end],
                            [left, right],
                            polynomial(last, end),
                        ));
                        last = end;
                    }
                }
                // Exclude the whole split cell from the FFT to avoid subtracting
                // large nearly equal contributions at the price tails.
                *cell = [0.0; 4];
            } else {
                *cell = polynomial(left, right);
            }
        }
    }
}

fn price<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    settings: QuadratureSettings,
    prepared: Option<&mut Option<PreparedCurve<T>>>,
) -> Result<f64> {
    let (rate, dividend) = flat_rates(context)?;
    if crate::engines::deterministic::enabled(context) {
        return crate::engines::deterministic::autocallable(note, context);
    }
    validate(note, context)?;
    let state = initial(note, context);
    if let Some(value) = state
        .settlement
        .or_else(|| fixed_value(note, context, state.knocked_in))
    {
        return finite_price(
            value
                + if state.settlement.is_none() {
                    state.coupon
                } else {
                    0.0
                },
        );
    }
    context.validate_curve_horizon(note.expiry_time())?;
    let layers =
        note.view().daily_knock_in() && !state.knocked_in && note.knock_in_affects_payoff();
    let t = note.view();
    let anchor = t.reference_spot();
    let current = log_ratio(context.spot_price(), anchor);
    let evaluate = |nodes: &[f64], legs: &[PreparedLayer]| {
        let mut value = crate::numerics::Sum::default();
        value.add(state.coupon);
        for leg in legs {
            value.add(leg.price(nodes, current, anchor));
        }
        finite_price(value.total())
    };
    if let Some(curve) = prepared.as_deref().and_then(Option::as_ref).filter(|c| {
        c.option == *note
            && context
                .natural_bsm_parameters()
                .is_ok_and(|m| c.model == *m)
            && c.valuation == context.valuation_time()
            && c.knocked_in == state.knocked_in
    }) {
        return evaluate(&curve.nodes, &curve.legs);
    }
    let mut dates = future_event_times(note, context, layers)?;
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
        .events()
        .iter()
        .filter(|r| r.time() > context.valuation_time())
    {
        if r.has_knock_out() {
            breaks.push(log_ratio(r.knock_out_level(), anchor));
        }
        if let Some(level) = r.coupon_barrier_level().filter(|&level| level > 0.0) {
            breaks.push(log_ratio(level, anchor));
        }
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let sigma = context.natural_bsm_parameters()?.volatility();
    let drift_rate = rate - dividend - 0.5 * sigma * sigma;
    let horizon = context.valuation_time().years_until(note.expiry_time());
    let transition = |time| super::transition(time, drift_rate, sigma, rate);
    let expiry_event = note.event_at(note.expiry_time());
    let mut terminal_breaks: Vec<_> = Event::new(
        note,
        expiry_event,
        note.observes_knock_in(note.expiry_time()),
    )
    .breaks()
    .into_iter()
    .flatten()
    .filter(|level| level.is_finite())
    .chain(t.downside().into_iter().flat_map(|downside| {
        [downside.lower_strike, downside.upper_strike]
            .into_iter()
            .filter(|&strike| strike > 0.0)
            .map(|strike| log_ratio(strike, anchor))
    }))
    .collect();
    terminal_breaks.sort_unstable_by(f64::total_cmp);
    terminal_breaks.dedup();
    let terminal_breaks = if breaks.len() <= 16 {
        &breaks
    } else {
        &terminal_breaks
    };
    let terminal_value = terminal(note, expiry_event, terminal_breaks, state.knocked_in);
    if dates.len() == 1 {
        let (drift, deviation, discount) = transition(horizon)?;
        return finite_price(
            state.coupon
                + integrate_terminal(&terminal_value, current, anchor, drift, deviation, discount),
        );
    }
    if layers {
        let mut write = 0;
        for i in 0..dates.len() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let date = dates[i];
            if date == note.expiry_time()
                || note.daily_time(date, note.calendar())
                || note
                    .event_at(date)
                    .is_some_and(ObservationEvent::has_knock_out)
            {
                dates[write] = date;
                write += 1;
            }
        }
        dates.truncate(write);
    }
    let extent = settings.integration_standard_deviations * sigma * horizon.sqrt();
    let drift_extent = (drift_rate * horizon).abs();
    let lower = 0.0_f64.min(breaks.first().copied().unwrap_or(0.0)) - drift_extent - extent;
    let upper = 0.0_f64.max(breaks.last().copied().unwrap_or(0.0)) + drift_extent + extent;
    let layer_count = 1 + usize::from(layers);
    let grid_breaks = if breaks.len() <= 16 {
        breaks.as_slice()
    } else {
        &[]
    };
    let nodes = integration_nodes(
        lower,
        upper,
        settings.asset_step_count,
        grid_breaks,
        layer_count,
    )?;
    let mut transitions = TransitionCache::new(
        &nodes,
        nodes.capacity(),
        layer_count,
        settings.asset_step_count,
    )?;
    let interpolation = CubicInterpolation::new(&nodes);

    // Knock-in only changes redemption. Coupons and knock-outs cancel in the difference.
    let mut legs = Vec::with_capacity(layer_count);
    let mut plans = Vec::with_capacity(layer_count);
    let cash_dates;
    if layers {
        let knocked = terminal(note, expiry_event, terminal_breaks, true);
        let difference = terminal_value
            .into_iter()
            .zip(&knocked)
            .map(|(mut intact, hit)| {
                intact.cash -= hit.cash;
                intact.asset -= hit.asset;
                intact
            })
            .collect();
        cash_dates = future_event_times(note, context, false)?;
        // Use the same interpolation dates when only a small number of daily
        // fixings can be skipped, so the two layers cancel on the same mesh.
        let cash_dates = if dates.len() <= cash_dates.len() + 2 {
            &dates
        } else {
            &cash_dates
        };
        plans.push((cash_dates, knocked, false));
        plans.push((&dates, difference, true));
    } else {
        plans.push((&dates, terminal_value, false));
    }
    for (dates, payoff, _) in &plans {
        transitions.validate_scheduled_work(
            &nodes,
            dates,
            1,
            payoff.len(),
            dates.len().saturating_mul(6),
            transition,
        )?;
    }
    for (dates, payoff, premium) in plans {
        legs.push(rollback(
            note,
            context.valuation_time(),
            dates,
            &nodes,
            &interpolation,
            payoff,
            premium,
            dividend,
            transition,
            &mut transitions,
        )?);
    }
    let value = evaluate(&nodes, &legs)?;
    if let Some(prepared) = prepared {
        *prepared = Some(PreparedCurve {
            option: note.clone(),
            model: context.natural_bsm_parameters()?.clone(),
            valuation: context.valuation_time(),
            knocked_in: state.knocked_in,
            nodes,
            legs,
        });
    }
    Ok(value)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Both payoff layers share the grid and its transition cache."
)]
fn rollback<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    valuation: Timestamp,
    dates: &[Timestamp],
    nodes: &[f64],
    interpolation: &CubicInterpolation,
    terminal: Vec<TerminalPiece>,
    premium: bool,
    dividend: f64,
    transition: impl Fn(f64) -> Result<(f64, f64, f64)>,
    transitions: &mut TransitionCache,
) -> Result<PreparedLayer> {
    let anchor = note.view().reference_spot();
    let mut curve = Layer {
        cells: Vec::new(),
        exact: vec![
            TerminalPiece {
                lower: f64::NEG_INFINITY,
                upper: nodes[0],
                cash: terminal.first().map_or(0.0, |p| p.cash),
                asset: terminal.first().map_or(0.0, |p| p.asset),
            },
            TerminalPiece {
                lower: nodes[nodes.len() - 1],
                upper: f64::INFINITY,
                cash: terminal.last().map_or(0.0, |p| p.cash),
                asset: 0.0,
            },
        ],
    };
    let mut split = Vec::with_capacity(6);
    let mut values = vec![0.0; nodes.len()];
    for index in (0..dates.len()).rev() {
        crate::cancellation::check()?;
        let date = dates[index];
        let final_event = index == dates.len() - 1;
        let time = if index == 0 {
            valuation
        } else {
            dates[index - 1]
        }
        .years_until(date);
        let (drift, deviation, discount) = transition(time)?;
        if !final_event {
            let mut event = Event::new(
                note,
                note.event_at(date),
                premium && note.daily_time(date, note.calendar()),
            );
            if premium {
                event.coupon = None;
                event.redemption = 0.0;
            }
            event.apply(nodes, &mut curve, &mut split);
        }
        if index == 0 {
            if final_event {
                curve.exact = terminal;
            }
            return Ok(PreparedLayer {
                curve,
                split,
                drift,
                deviation,
                discount,
            });
        }
        if final_event {
            for (value, &node) in values.iter_mut().zip(nodes) {
                *value = integrate_terminal(&terminal, node, anchor, drift, deviation, discount);
            }
        } else {
            let operator = transitions.get(time, nodes, drift, deviation)?;
            operator.integrate(nodes, &curve.cells, discount, &mut values)?;
            for (value, &node) in values.iter_mut().zip(nodes) {
                *value +=
                    integrate_terminal(&curve.exact, node, anchor, drift, deviation, discount)
                        + integrate_cubics(&split, node + drift, deviation, discount);
            }
        }
        if values.iter().any(|v| !v.is_finite()) {
            return Err(Error::ComputationFailure(
                "quadrature continuation is non-finite".into(),
            ));
        }
        curve.cells = interpolation.interpolate(&values);
        for piece in &mut curve.exact {
            piece.cash = scaled_signed_cash(piece.cash, discount);
            piece.asset = scaled_signed_cash(piece.asset, -dividend * time);
        }
    }
    Err(Error::ComputationFailure(
        "quadrature has no future event".into(),
    ))
}
