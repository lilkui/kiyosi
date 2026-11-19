use super::{
    Curve, Diffusion, FiniteDifferenceAccumulatorEngine, FiniteDifferenceSettings, Greek,
    GreeksRequest, NumericalShiftSettings, OptionContract, PricingContext, PricingEngine,
    PricingResult, Result, SpatialGrid, analysis, diffusion, diffusion_upper, grid::Spline,
    local_volatility, validate_work,
};
use crate::{Accumulator, Timestamp, engines::deterministic};

impl FiniteDifferenceAccumulatorEngine {
    /// Prices outstanding units and future trading-date purchases, settling at knock-out or expiry.
    ///
    /// Applies current events and events before variance resumes on the deterministic
    /// spot path, with inclusive knock-out and direction-dependent acceleration.
    /// Supports flat and nonconstant rate curves.
    /// Accepts BSM or local volatility; surface coverage is required only for
    /// remaining stochastic evolution, after immediate settlement is resolved.
    /// See [`Self::price`] for an example.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::InvalidInput`][crate::Error::InvalidInput] outside contract life or curve coverage,
    /// for impossible history, clipped domains, unstable explicit steps or more than one
    /// billion spatial intervals times time intervals across both active rollback layers.
    /// Numerical failures return [`Error::ComputationFailure`][crate::Error::ComputationFailure].
    ///
    /// # Greeks
    ///
    /// Computes spline spot Greeks and completes other sensitivities with core bump defaults.
    ///
    /// Fixes the central upper domain for bumped prices. At expiry Greeks follow
    /// terminal payoffs, with time Greeks absent. Stencils crossing current
    /// thresholds or trading events are unavailable.
    /// Local volatility supports only delta and gamma with the absolute surface fixed.
    ///
    /// ## Errors
    ///
    /// Validates shifts, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors without changing inputs or settings.
    /// Unsupported local-volatility Greeks are absent.
    ///
    /// ## Examples
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
}

impl PricingEngine<Accumulator> for FiniteDifferenceAccumulatorEngine {
    fn greek_method(
        &self,
        option: &Accumulator,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        local_volatility::greek_method(option, context)
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        local_volatility::validate_model(context)
    }
    fn volatility_affects_price(&self, option: &Accumulator, context: &PricingContext) -> bool {
        option.volatility_affects_price(context)
            && !context
                .model_parameters()
                .variance_time_convention()
                .variance_start(context.valuation_time(), option.expiry_time())
                .is_ok_and(|start| {
                    start > context.valuation_time()
                        && deterministic::accumulator_through(option, context, start)
                            .is_ok_and(|(_, settlement)| settlement.is_some())
                })
    }

    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn price_with_greeks_with_settings(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<Accumulator>>::validate_applicability(self, context)?;
        analysis(option, context, self.settings, request, shifts, native)
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
    option.validate_pricing(context)?;
    let start = context.deterministic_until(option.expiry_time())?;
    let mut upper = None;
    let result = evaluate_after_observations(
        option,
        context,
        start,
        request,
        shifts,
        |rolled, quantity, request, shifts| {
            let grid = diffusion::with_diffusion!(rolled, |diffusion| {
                solve_grid(option, rolled, settings, diffusion)
            })?;
            upper = Some(grid.space.upper);
            grid.evaluate(option, rolled, quantity, request, shifts)
        },
    )?;
    Ok((result, upper))
}

fn evaluate_after_observations(
    option: &Accumulator,
    context: &PricingContext,
    start: Timestamp,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    evaluate: impl FnOnce(
        &PricingContext,
        f64,
        GreeksRequest,
        NumericalShiftSettings,
    ) -> Result<PricingResult>,
) -> Result<PricingResult> {
    let (quantity, settlement) = deterministic::accumulator_through(option, context, start)?;
    if let Some(value) = settlement {
        return PricingResult::new(value);
    }
    super::barrier::evaluate_after_prefix(
        context,
        start,
        request,
        shifts,
        |radius| option.at_spot_discontinuity(context, radius),
        |rolled, request, shifts| evaluate(rolled, quantity, request, shifts),
    )
}

fn solve_grid<D: Diffusion>(
    option: &Accumulator,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    diffusion: D,
) -> Result<AccumulatorGrid> {
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
    let trading = diffusion::observation_times(context.valuation_time(), expiry, &dates)?;
    let grid = diffusion::time_grid(context, expiry, settings, &risk, &dividend, &trading)?;
    let count = if option.accrues(context) { 2 } else { 1 };
    validate_work(
        space.count as u64 * count as u64,
        grid.rollback_steps(settings.scheme, &trading)?,
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
        let value = super::grid::project_in_coordinates(&space, &breaks, i, |asset| {
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
                let correction = super::grid::project_in_coordinates(&space, &breaks, i, |asset| {
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
    let layers = layers.map(|layer| space.spline(layer));
    Ok(AccumulatorGrid { space, layers })
}

#[derive(PartialEq)]
struct AccumulatorGrid {
    space: SpatialGrid,
    layers: [Spline; 2],
}
impl AccumulatorGrid {
    fn evaluate(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        quantity: f64,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        let Self { space, layers } = self;
        let upper = space.upper;
        let sample = |spot| {
            let slope = space.sample(&layers[0], spot);
            let intercept = space.sample(&layers[1], spot);
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
        if request != GreeksRequest::new([]) && !option.at_spot_discontinuity(context, h) {
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
        Ok(result)
    }
}

#[derive(PartialEq)]
struct AccumulatorState {
    start: Timestamp,
    grid: Option<AccumulatorGrid>,
}
impl super::solution::Backend<Accumulator> for AccumulatorState {
    fn evaluate(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        evaluate_after_observations(
            option,
            context,
            self.start,
            request,
            shifts,
            |rolled, quantity, request, shifts| {
                self.grid
                    .as_ref()
                    .ok_or_else(super::unstable)?
                    .evaluate(option, rolled, quantity, request, shifts)
            },
        )
    }
}
impl super::FiniteDifferenceEngine<Accumulator> for FiniteDifferenceAccumulatorEngine {
    fn solve(
        &self,
        option: &Accumulator,
        context: &PricingContext,
        range: std::ops::RangeInclusive<f64>,
    ) -> Result<super::FiniteDifferenceSolution> {
        local_volatility::validate_model(context)?;
        super::solution::validate_range(&range)?;
        crate::cancellation::check()?;
        let anchor = super::solution::at_spot(context, *range.end())?;
        option.validate_pricing(&anchor)?;
        let start = anchor.deterministic_until(option.expiry_time())?;
        let points = super::solution::probes(&range, [option.strike(), option.knock_out_level()]);
        let mut pending = false;
        for spot in points {
            let probe = super::solution::at_spot(context, spot)?;
            if deterministic::accumulator_through(option, &probe, start)?
                .1
                .is_none()
            {
                pending = true;
                break;
            }
        }
        let grid = if pending {
            let (rolled, _, _) = deterministic::continuation_context(&anchor, start)?;
            let rolled = rolled.as_ref();
            Some(diffusion::with_diffusion!(rolled, |diffusion| solve_grid(
                option,
                rolled,
                self.settings,
                diffusion
            ))?)
        } else {
            None
        };
        let state = AccumulatorState { start, grid };
        super::solution::Backend::evaluate(
            &state,
            option,
            &anchor,
            GreeksRequest::new([]),
            Default::default(),
        )?;
        crate::cancellation::check()?;
        Ok(super::FiniteDifferenceSolution::new(
            option.clone(),
            context,
            range,
            state,
        ))
    }
}
