use super::{
    Curve, Diffusion, FiniteDifferenceAccumulatorEngine, FiniteDifferenceSettings, Greek,
    GreeksRequest, LocalDiffusion, NumericalShiftSettings, OptionContract, PricingContext,
    PricingEngine, PricingResult, Result, SpatialGrid, analysis, diffusion, diffusion_upper,
    local_volatility,
};
use crate::Accumulator;

impl FiniteDifferenceAccumulatorEngine {
    /// Prices outstanding units and future trading-date purchases, settling at knock-out or expiry.
    ///
    /// Applies a current contractual event exactly once, with inclusive knock-out and
    /// direction-dependent acceleration. Supports flat and nonconstant rate curves.
    /// Accepts BSM or local volatility; surface coverage is required only for
    /// remaining stochastic evolution, after immediate settlement is resolved.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside contract life or curve coverage,
    /// for missing or impossible history, clipped domains or unstable explicit steps.
    /// Numerical failures return [`Error::ComputationFailure`][crate::Error::ComputationFailure]. Inputs remain unchanged.
    pub fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        Ok(native(
            option,
            context,
            self.settings,
            GreeksRequest::new([]),
            Default::default(),
        )?
        .0
        .price())
    }

    /// Computes spline spot Greeks and completes other sensitivities with core bump defaults.
    ///
    /// Fixes the central upper domain for bumped prices. At expiry Greeks follow
    /// terminal payoffs, with time Greeks absent. Stencils crossing current
    /// thresholds or trading events are unavailable.
    /// Local volatility supports only delta and gamma with the absolute surface fixed.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors without changing inputs or settings.
    /// Unsupported local-volatility Greeks are absent.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     start, Date::new(2025, 2, 3)?.start_of_day(), kiyosi::TimeOfDay::new(0, 0, 0, 0)?))?;
    /// let market = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     101.0, start.into())?;
    /// let engine = FiniteDifferenceAccumulatorEngine::default();
    /// let result = engine.price_with_greeks(&option, &market,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]))?;
    /// assert_eq!(result.price(), engine.price(&option, &market)?);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let shifts = self.settings.greek_shifts();
        analysis(option, context, self.settings, request, shifts, native)
    }
}

impl PricingEngine<Accumulator> for FiniteDifferenceAccumulatorEngine {
    fn greek_method(
        &self,
        option: &Accumulator,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<Accumulator>>::validate_applicability(self, context)?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            if matches!(
                context.model_parameters(),
                crate::ModelParameters::LocalVolatility(_)
            ) {
                crate::GreekMethod::NativeWithSpotControl
            } else {
                crate::GreekMethod::Mixed
            },
        ))
    }

    fn default_spot_shift(&self, _option: &Accumulator, context: &PricingContext) -> f64 {
        self.settings
            .greek_shifts()
            .spot_shift_at(context.spot_price())
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn greek_settings(&self, _option: &Accumulator) -> crate::NumericalShiftSettings {
        self.settings.greek_shifts()
    }
    fn price_with_greeks_with_settings(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<Accumulator>>::validate_applicability(self, context)?;
        shifts.validate()?;
        let engine = Self {
            settings: self.settings.with_greek_shifts(shifts),
        };
        <Self as PricingEngine<Accumulator>>::price_with_greeks(&engine, option, context, request)
    }
    fn price_with_greeks(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.price_with_greeks(option, context, request)
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
    }
}

fn default_upper(
    option: &Accumulator,
    context: &PricingContext,
    risk: &Curve,
    dividend: &Curve,
) -> Result<f64> {
    diffusion_upper(
        option
            .strike()
            .max(option.knock_out_level())
            .max(context.spot_price()),
        context,
        context.valuation_time().years_until(option.expiry_time()),
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
    let request = local_volatility::validate_request(context, request)?;
    if crate::engines::deterministic::enabled(context) {
        return Ok((
            PricingResult::new(crate::engines::deterministic::accumulator(option, context)?)?,
            None,
        ));
    }
    match context.model_parameters() {
        crate::ModelParameters::Heston(_) => Err(crate::Error::UnsupportedOperation(
            "finite differences do not support Heston".into(),
        )),
        crate::ModelParameters::BlackScholesMerton(model) => native_with_diffusion(
            option,
            context,
            settings,
            request,
            shifts,
            model.volatility(),
        ),
        crate::ModelParameters::LocalVolatility(model) => native_with_diffusion(
            option,
            context,
            settings,
            request,
            shifts,
            LocalDiffusion::new(context, model.surface()),
        ),
    }
}

fn native_with_diffusion<D: Diffusion>(
    option: &Accumulator,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    diffusion: D,
) -> Result<(PricingResult, Option<f64>)> {
    option.validate_pricing(context)?;
    let (quantity, settlement) = option.initial(context);
    if let Some(value) = settlement {
        return Ok((PricingResult::new(value)?, None));
    }
    let expiry = option.expiry_time();
    let model = context.model_parameters();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let upper = match settings.asset_upper_boundary {
        Some(value) => value,
        None => default_upper(option, context, &risk, &dividend)?,
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
    let dates = option.future_observation_times(context)?;
    let trading: Vec<_> = dates
        .iter()
        .map(|date| context.valuation_time().years_until(*date))
        .collect();
    diffusion::validate_event_resolution(context.valuation_time(), expiry, &dates)?;
    let grid = diffusion::time_grid(
        context,
        expiry,
        settings.time_step_count,
        &risk,
        &dividend,
        &trading,
    )?;
    if !D::LOCAL {
        grid.validate_stability(
            &space,
            diffusion.sigma(0.0, 0.0),
            settings.scheme.theta(),
            true,
            &risk,
            &dividend,
        )?;
    }
    let count = if option.accrues(context) { 2 } else { 1 };
    let mut layers = [vec![0.0; space.count + 1], vec![0.0; space.count + 1]];
    let mut next = layers.clone();
    let mut breaks = vec![option.strike(), option.knock_out_level()];
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let event = |asset: f64, slope: f64, intercept: f64, observe: bool, accumulate: bool| {
        if observe && option.is_knock_out(asset) {
            [option.unit_payoff(asset), 0.0]
        } else {
            [
                slope,
                intercept
                    + if accumulate {
                        slope * option.purchase(asset)
                    } else {
                        0.0
                    },
            ]
        }
    };
    let [slope, intercept] = &mut layers;
    for (i, (slope, intercept)) in slope.iter_mut().zip(intercept).enumerate() {
        let value = super::autocallable::project(&space, &breaks, i, |asset| {
            event(
                asset,
                option.unit_payoff(asset),
                0.0,
                option.observes_knock_out(option.expiry_time()),
                option.accumulates_at(option.expiry_time(), option.calendar()),
            )
        });
        *slope = value[0];
        *intercept = value[1];
    }
    let mut stepper = diffusion.stepper(space.count);
    let mut damping: u32 = 2;
    for step in (0..grid.widths.len()).rev() {
        crate::cancellation::check()?;
        for (current, dt, weight) in grid.substeps(step, settings.scheme, &mut damping) {
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
                diffusion.advance(
                    &mut stepper,
                    values,
                    &mut next[layer],
                    &space,
                    middle,
                    [dt, weight, rate, q],
                    boundaries,
                )?;
                std::mem::swap(&mut layers[layer], &mut next[layer]);
            }
        }
        if let Ok(event_index) = trading.binary_search_by(|t| t.total_cmp(&grid.times[step])) {
            let observe = option.observes_knock_out(dates[event_index]);
            for i in 0..=space.count {
                // Project only the event jump to preserve smooth continuation.
                let correction = super::autocallable::project(&space, &breaks, i, |asset| {
                    let slope = space.linear(&layers[0], asset);
                    let intercept = space.linear(&layers[1], asset);
                    let value = event(
                        asset,
                        slope,
                        intercept,
                        observe,
                        option.accumulates_at(dates[event_index], option.calendar()),
                    );
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
    let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
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
