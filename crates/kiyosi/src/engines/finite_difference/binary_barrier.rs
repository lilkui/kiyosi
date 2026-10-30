use super::*;
use crate::engines::analytic::barrier::{checked, validate_life, validate_schedule};
use crate::instruments::binary_barrier::Contract;
use crate::{
    AnalyticDigitalEngine, BinaryBarrierOption, DoubleTouchOption, ObservationMode,
    SettlementTiming, TouchOption,
};

impl FiniteDifferenceBinaryBarrierEngine {
    /// Prices a binary barrier or single touch with flat or node curves.
    ///
    /// Continuous barriers are exact grid edges; scheduled observations project event cells.
    /// Prior at-hit payments have no remaining value; resolved cash flows use exact discounts.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside life or required curve coverage,
    /// for non-trading observations, missing or impossible history, invalid grids,
    /// or unstable explicit steps. Returns [`Error::ComputationFailure`] for
    /// numerical failures. Inputs remain unchanged.
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing observations or barriers leave affected measures absent.
    /// All Greeks are absent at expiry or a current barrier equality without a prior touch.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Inputs and engine settings remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = TouchOption::cash(TouchOptionTerms::new(start, Date::new(2026, 1, 1)?,
    ///     120.0, BarrierType::UpAndIn), 10.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = FiniteDifferenceBinaryBarrierEngine::default();
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.price() > 0.0);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price_with_greeks(self, option, context, request, shifts)
    }
}

macro_rules! binary_pricing {
    ($engine:ty, $option:ty) => {
        impl PricingEngine<$option> for $engine {
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                Ok(native(
                    option.contract(),
                    context,
                    self.settings,
                    GreeksRequest::default(),
                    Default::default(),
                )?
                .price())
            }
            fn price_with_greeks(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                shifts: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                request.validate()?;
                shifts.validate()?;
                let mut settings = self.settings;
                if settings.asset_upper_boundary.is_none() {
                    let upper = default_upper(&option.contract(), context);
                    if upper.is_finite() {
                        settings.asset_upper_boundary = Some(upper);
                    }
                }
                crate::risk::numerical::complete_greeks(
                    option,
                    context,
                    request,
                    shifts,
                    true,
                    native(option.contract(), context, settings, request, shifts)?,
                    |context, request| {
                        native(option.contract(), context, settings, request, shifts)
                    },
                )
            }
            fn volatility_affects_price(&self, option: &$option, context: &PricingContext) -> bool {
                option.contract().volatility_affects_price(context)
            }
        }
    };
}
binary_pricing!(FiniteDifferenceBinaryBarrierEngine, BinaryBarrierOption);
binary_pricing!(FiniteDifferenceBinaryBarrierEngine, TouchOption);
binary_pricing!(FiniteDifferenceDoubleTouchEngine, DoubleTouchOption);

impl FiniteDifferenceDoubleTouchEngine {
    /// Prices cash double-touch contracts using both inclusive boundaries and shared history.
    ///
    /// Fits continuous boundaries exactly and projects scheduled events simultaneously.
    /// Prior at-hit payments have zero remaining value; resolved cash uses exact discounts.
    /// See [`DoubleTouchOption::cash`] for construction and pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside life or required curve coverage,
    /// for non-trading observations, unknown or impossible history, invalid grids,
    /// or unstable explicit steps. Numerical failures return
    /// [`Error::ComputationFailure`]. Inputs remain unchanged.
    pub fn price(&self, option: &DoubleTouchOption, context: &PricingContext) -> Result<f64> {
        <Self as PricingEngine<DoubleTouchOption>>::price(self, option, context)
    }

    /// Returns spline spot Greeks and completes other sensitivities on the central domain.
    ///
    /// Stencils crossing either boundary or an observation omit affected measures.
    /// All Greeks are absent at expiry or current boundary equality without a prior touch.
    ///
    /// # Errors
    ///
    /// Validates requests and shifts before pricing, then propagates [`Self::price`] and
    /// [`crate::numerical_greeks`] errors. Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let mut terms = DoubleTouchOptionTerms::new(start, Date::new(2026, 1, 1)?,
    ///     80.0, 120.0, DoubleBarrierType::KnockOut);
    /// terms.touch_state = Some(BarrierTouchState::Untouched);
    /// let option = DoubleTouchOption::cash(terms, 1.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let result = FiniteDifferenceDoubleTouchEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!((result.price() - 0.36726156351907174).abs() < 0.00001);
    /// assert!(result.has(Greek::Delta));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DoubleTouchOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<DoubleTouchOption>>::price_with_greeks(
            self, option, context, request, shifts,
        )
    }
}

fn default_upper(contract: &Contract<'_>, context: &PricingContext) -> f64 {
    let relevant = context.spot_price().max(contract.monitoring.strike());
    (4.0 * relevant).max(
        contract
            .double
            .map_or(contract.monitoring.barrier_level(), |d| d.upper_barrier())
            + relevant,
    )
}

fn native(
    contract: Contract<'_>,
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
) -> Result<PricingResult> {
    let option = contract.monitoring;
    validate_life(option, context)?;
    validate_schedule(option, context)?;
    let (prior, touched, finished) = contract.state(context)?;
    let upper_barrier = contract.double.map(|d| d.upper_barrier());
    let knock_in = option.barrier_type().is_knock_in();
    let at_hit = contract.timing == SettlementTiming::AtHit;
    let spot = context.spot_price();
    let terminal = |asset| {
        if let Some(digital) = contract.digital {
            if (digital.option_type() == OptionType::Call && asset > digital.strike())
                || (digital.option_type() == OptionType::Put && asset < digital.strike())
            {
                contract.payout.unwrap_or(asset)
            } else {
                0.0
            }
        } else {
            contract.payout.unwrap_or(asset)
        }
    };
    if prior && at_hit {
        return PricingResult::new(0.0);
    }
    let expiry = option.expiry_date().start_of_day();
    if context.valuation_time() == expiry {
        return PricingResult::new(if touched == knock_in {
            terminal(spot)
        } else {
            0.0
        });
    }
    if touched || finished {
        if touched != knock_in {
            return PricingResult::new(0.0);
        }
        if at_hit {
            return PricingResult::new(contract.payout.unwrap_or(spot));
        }
        let value = if let Some(digital) = contract.digital {
            AnalyticDigitalEngine.price(digital, context)?
        } else {
            let model = context.model_parameters();
            let curve = if contract.payout.is_some() {
                model.risk_free_curve()
            } else {
                model.dividend_curve()
            };
            checked(scaled(
                contract.payout.unwrap_or(spot),
                curve.log_factor(context.valuation_time(), expiry)?,
            ))?
        };
        return PricingResult::new(value);
    }
    let scheduled = option.observation_mode() == ObservationMode::Scheduled;
    let horizon = if scheduled && at_hit {
        option
            .observation_dates()
            .last()
            .copied()
            .unwrap_or(option.expiry_date())
            .start_of_day()
    } else {
        expiry
    };
    let time = context.valuation_time().years_until(horizon);
    let model = context.model_parameters();
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), horizon)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), horizon)?;
    let upper = settings
        .asset_upper_boundary
        .unwrap_or_else(|| default_upper(&contract, context));
    let mut space = SpatialGrid::new(
        settings,
        upper,
        0.0,
        spot.max(option.strike())
            .max(upper_barrier.unwrap_or(option.barrier_level())),
    )?;
    let up = option.barrier_type().is_up();
    if !scheduled {
        space.uniform_domain(
            if up { 0.0 } else { option.barrier_level() },
            upper_barrier.unwrap_or(if up { option.barrier_level() } else { upper }),
        )?;
    }
    let observations: Vec<_> = option
        .observation_dates()
        .iter()
        .filter(|date| date.start_of_day() > context.valuation_time())
        .map(|date| context.valuation_time().years_until(date.start_of_day()))
        .collect();
    let grid = TimeGrid::with_events(
        time,
        settings.time_step_count,
        &risk,
        &dividend,
        &observations,
    );
    let sigma = model.volatility();
    let theta = settings.scheme.theta();
    grid.validate_stability(&space, sigma, theta, scheduled, &risk, &dividend)?;
    let knocked = |asset| {
        upper_barrier.is_some_and(|upper| asset >= upper)
            || if up {
                asset >= option.barrier_level()
            } else {
                asset <= option.barrier_level()
            }
    };
    let discount = |current, end| {
        if contract.payout.is_some() {
            risk.log_factor(current, end)
        } else {
            dividend.log_factor(current, end)
        }
    };
    let base = |asset, current| {
        let tau = time - current;
        if tau == 0.0 {
            return terminal(asset);
        }
        let log = discount(current, time);
        if let Some(digital) = contract.digital {
            let sign = if digital.option_type() == OptionType::Call {
                1.0
            } else {
                -1.0
            };
            if asset == 0.0 {
                return if sign < 0.0 {
                    scaled(contract.payout.unwrap_or(0.0), log)
                } else {
                    0.0
                };
            }
            let (d1, d2) = black_scholes_arguments(
                asset,
                digital.strike(),
                dividend.log_factor(current, time) - risk.log_factor(current, time),
                sigma,
                tau.sqrt(),
            );
            weighted_cdf(
                contract.payout.unwrap_or(asset),
                log,
                sign * if contract.payout.is_some() { d2 } else { d1 },
            )
        } else {
            scaled(contract.payout.unwrap_or(asset), log)
        }
    };
    let hit = |asset, current, event| {
        if contract.digital.is_some() || !knock_in {
            0.0
        } else {
            scaled(
                contract.payout.unwrap_or(asset),
                discount(current, if at_hit { event } else { time }),
            )
        }
    };
    let surviving = |asset| {
        if contract.digital.is_none() && knock_in {
            0.0
        } else {
            terminal(asset)
        }
    };
    let levels: Vec<_> = std::iter::once(option.barrier_level())
        .chain(upper_barrier)
        .collect();
    let breaks: Vec<_> = levels.iter().copied().chain([option.strike()]).collect();
    let edges = |current, events: bool| {
        let value = |asset| {
            if !events {
                return base(asset, current);
            }
            let next = observations.iter().find(|&&event| event >= current);
            if knocked(asset) && (!scheduled || next.is_some()) {
                hit(asset, current, next.copied().unwrap_or(current))
            } else if contract.digital.is_none() && knock_in {
                0.0
            } else {
                base(asset, current)
            }
        };
        [value(space.asset(0)), value(space.upper)]
    };
    let observe = |values: &mut [f64], current| {
        let projected: Vec<_> = levels
            .iter()
            .map(|level| {
                let cut = ((level / space.spacing).round() as usize).min(space.count);
                // Project both boundaries from the same continuation, including shared cells.
                let value = super::barrier::project(&space, &breaks, cut, |asset| {
                    if upper_barrier.is_some() {
                        if knocked(asset) {
                            hit(asset, current, current)
                        } else {
                            space.linear(values, asset)
                        }
                    } else if knocked(asset) {
                        hit(asset, current, current) - space.linear(values, asset)
                    } else {
                        0.0
                    }
                });
                (
                    cut,
                    if upper_barrier.is_some() {
                        value
                    } else {
                        values[cut] + value
                    },
                )
            })
            .collect();
        for (i, value) in values.iter_mut().enumerate() {
            if knocked(space.asset(i)) {
                *value = hit(space.asset(i), current, current);
            }
        }
        for (cut, value) in projected {
            values[cut] = value;
        }
    };
    let march = |events: bool| -> Result<Vec<f64>> {
        let mut values: Vec<_> = (0..=space.count)
            .map(|i| {
                super::barrier::project(&space, &breaks, i, |asset| {
                    if !events {
                        terminal(asset)
                    } else if (!scheduled || observations.contains(&time)) && knocked(asset) {
                        hit(asset, time, time)
                    } else {
                        surviving(asset)
                    }
                })
            })
            .collect();
        let mut next = vec![0.0; values.len()];
        let mut stepper = Stepper::new(space.count);
        let mut damping: u32 = 2;
        for step in (0..grid.widths.len()).rev() {
            let damp = theta == 0.5 && damping > 0;
            let count = if damp { 2 } else { 1 };
            let weight = if damp { 1.0 } else { theta };
            damping = damping.saturating_sub(1);
            let dt = grid.widths[step] / f64::from(count);
            for substep in 0..count {
                let current = grid.times[step] + dt * f64::from(count - substep - 1);
                let middle = current + 0.5 * dt;
                stepper.advance(
                    &values,
                    &mut next,
                    &space,
                    sigma,
                    [dt, weight, risk.rate(middle), dividend.rate(middle)],
                    edges(current, events),
                )?;
                std::mem::swap(&mut values, &mut next);
            }
            if events && scheduled && observations.contains(&grid.times[step]) {
                observe(&mut values, grid.times[step]);
                damping = 2;
            }
        }
        Ok(values)
    };
    let mut values = march(true)?;
    if contract.digital.is_some() && knock_in {
        for (value, vanilla) in values.iter_mut().zip(march(false)?) {
            *value = vanilla - *value;
        }
    }
    let curvature = space.curvature(&values);
    let sample = space.sample(&values, &curvature, spot);
    let scale = scaled(
        contract.payout.unwrap_or(spot),
        if contract.payout.is_some() {
            risk.log_factor(0.0, time)
        } else {
            dividend.log_factor(0.0, time)
        },
    );
    let mut result = PricingResult::new(crate::engines::analytic::barrier::checked_with_scale(
        sample[0], scale,
    )?)?;
    let h = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift);
    if !contract.double.map_or_else(
        || option.at_spot_discontinuity(context, h),
        |double| double.at_spot_discontinuity(context, h),
    ) {
        result.set(request, Greek::Delta, sample[1])?;
        result.set(request, Greek::Gamma, sample[2])?;
        if request.has(Greek::Speed)
            && h > 0.0
            && spot - h > space.asset(0)
            && spot + h < space.upper
        {
            result.set(
                request,
                Greek::Speed,
                (space.sample(&values, &curvature, spot + h)[2]
                    - space.sample(&values, &curvature, spot - h)[2])
                    / (2.0 * h),
            )?;
        }
    }
    Ok(result)
}
