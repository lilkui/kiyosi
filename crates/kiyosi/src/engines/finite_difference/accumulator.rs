use super::*;
use crate::Accumulator;

impl FiniteDifferenceAccumulatorEngine {
    /// Prices outstanding units and future trading-date purchases, settling at knock-out or expiry.
    ///
    /// Applies a current midnight event exactly once, with inclusive knock-out and
    /// acceleration strictly below strike. Supports flat and node rate curves.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside contract life or curve coverage,
    /// for non-trading expiry, clipped domains or unstable explicit steps.
    /// Numerical failures return [`Error::ComputationFailure`]. Inputs remain unchanged.
    pub fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        Ok(native(
            option,
            context,
            self.settings,
            GreeksRequest::default(),
            Default::default(),
        )?
        .0
        .price())
    }

    /// Computes spline spot Greeks and completes other sensitivities with core bump defaults.
    ///
    /// Fixes the central upper domain for bumped prices. Expiry Greeks and stencils
    /// crossing current thresholds or trading events are unavailable.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors without changing inputs or settings.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     start, Date::new(2025, 2, 3)?))?;
    /// let market = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     101.0, start.into())?;
    /// let engine = FiniteDifferenceAccumulatorEngine::default();
    /// let result = engine.price_with_greeks(&option, &market,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), Default::default())?;
    /// assert_eq!(result.price(), engine.price(&option, &market)?);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        request.validate()?;
        shifts.validate()?;
        let mut settings = self.settings;
        let (result, upper) = native(option, context, settings, request, shifts)?;
        settings.asset_upper_boundary = upper.or(settings.asset_upper_boundary);
        crate::risk::numerical::complete_greeks(
            option,
            context,
            request,
            shifts,
            true,
            result,
            |context, request| Ok(native(option, context, settings, request, shifts)?.0),
        )
    }
}

impl PricingEngine<Accumulator> for FiniteDifferenceAccumulatorEngine {
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.price_with_greeks(option, context, request, shifts)
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}

fn validate(option: &Accumulator, context: &PricingContext) -> Result<()> {
    time_to_expiry(option, context)?;
    if !context.calendar().is_trading_day(option.expiry_date()) {
        return Err(Error::InvalidInput("expiry must be a trading day".into()));
    }
    Ok(())
}

fn default_upper(
    option: &Accumulator,
    context: &PricingContext,
    risk: &Curve,
    dividend: &Curve,
) -> f64 {
    diffusion_upper(
        option
            .strike()
            .max(option.knock_out_level())
            .max(context.spot_price()),
        context,
        context
            .valuation_time()
            .years_until(option.expiry_date().into()),
        risk,
        dividend,
    )
}

fn native(
    option: &Accumulator,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<(PricingResult, Option<f64>)> {
    validate(option, context)?;
    let (quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return Ok((PricingResult::new(value)?, None));
    }
    let expiry = option.expiry_date().start_of_day();
    let time = context.valuation_time().years_until(expiry);
    let model = context.model_parameters();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let sigma = model.volatility();
    let upper = match settings.asset_upper_boundary {
        Some(value) => value,
        None => default_upper(option, context, &risk, &dividend),
    };
    let space = SpatialGrid::new(
        settings,
        upper,
        0.0,
        context
            .spot_price()
            .max(option.strike())
            .max(option.knock_out_level()),
    )?;
    let mut trading = Vec::new();
    let mut date = context.valuation_date();
    loop {
        if date.start_of_day() > context.valuation_time() && context.calendar().is_trading_day(date)
        {
            trading.push(context.valuation_time().years_until(date.into()));
        }
        if date == option.expiry_date() {
            break;
        }
        date = date.add_days(1)?;
    }
    let grid = TimeGrid::with_events(time, settings.time_step_count, &risk, &dividend, &trading);
    let theta = settings.scheme.theta();
    grid.validate_stability(&space, sigma, theta, true, &risk, &dividend)?;
    let count = if option.accrues() { 2 } else { 1 };
    let mut layers = [vec![0.0; space.count + 1], vec![0.0; space.count + 1]];
    let mut next = layers.clone();
    let mut breaks = vec![option.strike(), option.knock_out_level()];
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let event = |asset: f64, slope: f64, intercept: f64| {
        if asset >= option.knock_out_level() {
            [asset - option.strike(), 0.0]
        } else {
            [slope, intercept + slope * option.purchase(asset)]
        }
    };
    let [slope, intercept] = &mut layers;
    for (i, (slope, intercept)) in slope.iter_mut().zip(intercept).enumerate() {
        let value = super::autocallable::project(&space, &breaks, i, |asset| {
            event(asset, asset - option.strike(), 0.0)
        });
        *slope = value[0];
        *intercept = value[1];
    }
    let mut stepper = Stepper::new(space.count);
    let mut damping: usize = 2;
    for step in (0..grid.widths.len()).rev() {
        let damp = theta == 0.5 && damping > 0;
        damping = damping.saturating_sub(1);
        let substeps = if damp { 2 } else { 1 };
        let dt = grid.widths[step] / f64::from(substeps);
        for substep in 0..substeps {
            let current = grid.times[step] + dt * f64::from(substeps - substep - 1);
            let middle = current + 0.5 * dt;
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            for layer in 0..count {
                let values = &layers[layer];
                let slope = (values[space.count] - values[space.count - 1]) / space.spacing;
                let intercept = values[space.count] - slope * upper;
                let boundaries = [
                    values[0] * (-rate * dt).exp(),
                    slope * upper * (-q * dt).exp() + intercept * (-rate * dt).exp(),
                ];
                stepper.advance(
                    values,
                    &mut next[layer],
                    &space,
                    sigma,
                    [dt, if damp { 1.0 } else { theta }, rate, q],
                    boundaries,
                )?;
                std::mem::swap(&mut layers[layer], &mut next[layer]);
            }
        }
        if trading
            .binary_search_by(|t| t.total_cmp(&grid.times[step]))
            .is_ok()
        {
            for i in 0..=space.count {
                // Project only the event jump to preserve smooth continuation.
                let correction = super::autocallable::project(&space, &breaks, i, |asset| {
                    let slope = space.linear(&layers[0], asset);
                    let intercept = space.linear(&layers[1], asset);
                    let value = event(asset, slope, intercept);
                    [value[0] - slope, value[1] - intercept]
                });
                for layer in 0..count {
                    next[layer][i] = layers[layer][i] + correction[layer];
                }
            }
            for layer in 0..count {
                std::mem::swap(&mut layers[layer], &mut next[layer]);
            }
            damping = 2;
        }
    }
    let curvatures = [space.curvature(&layers[0]), space.curvature(&layers[1])];
    let sample = |spot| {
        let slope = space.sample(&layers[0], &curvatures[0], spot);
        let intercept = space.sample(&layers[1], &curvatures[1], spot);
        [
            quantity * slope[0] + intercept[0],
            quantity * slope[1] + intercept[1],
            slope[2].mul_add(quantity, intercept[2]),
        ]
    };
    let spot = context.spot_price();
    let value = sample(spot);
    let mut result = PricingResult::new(value[0])?;
    let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift);
    if !option.at_spot_discontinuity(context, h) {
        result.set(request, Greek::Delta, value[1])?;
        result.set(request, Greek::Gamma, value[2])?;
        if request.has(Greek::Speed) && h > 0.0 && spot > h && spot + h < upper {
            let up = sample(spot + h);
            let down = sample(spot - h);
            PricingResult::new(up[0])?;
            PricingResult::new(down[0])?;
            result.set(request, Greek::Speed, (up[2] - down[2]) / (2.0 * h))?;
        }
    }
    Ok((result, Some(upper)))
}
