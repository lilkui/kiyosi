use super::*;
use crate::{AnalyticEuropeanEngine, BarrierOption, RebateTiming};

/// Prices single-barrier vanilla contracts with a flat-rate CRR tree.
///
/// Continuous monitoring checks every tree level. Scheduled fixings must align exactly
/// with time steps; dates are never moved. Only node delta/gamma are returned.
/// Values own their settings and shared instances support concurrent pricing.
/// See [`Self::price`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoxRossRubinsteinBarrierEngine {
    settings: BinomialSettings,
}

impl CoxRossRubinsteinBarrierEngine {
    /// Creates an engine after validating its step count.
    ///
    /// See [`CoxRossRubinsteinVanillaEngine::new`] for a settings example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside 1 through 1,000,000 steps.
    pub fn new(settings: BinomialSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> BinomialSettings {
        self.settings
    }

    /// Prices after validating flat rates, inclusive life, calendar and prior history.
    ///
    /// Resolved payoffs settle before grid validation; resolved vanilla payoffs use
    /// analytic pricing. Prior at-hit rebates are already paid. Inputs remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for node curves or unsupported
    /// fixings. Returns [`Error::InvalidInput`] outside life, for non-trading
    /// fixings, or for missing or impossible history, checked in that order.
    /// Unstable tree parameters, nodes or results return [`Error::ComputationFailure`].
    /// Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{BarrierOption, BarrierOptionTerms, BarrierType, BinomialSettings,
    ///     BlackScholesMertonParameters, CoxRossRubinsteinBarrierEngine, Date, OptionType,
    ///     PricingContext};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let engine = CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 256 })?;
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(self
            .native(option, context, GreeksRequest::default())?
            .price())
    }

    /// Returns requested node delta/gamma; other Greeks remain absent.
    ///
    /// One-step gamma and expiry Greeks are absent. Current barrier equality also
    /// suppresses Greeks without a prior touch. Shifts are validated but unused.
    /// See [`Self::price`] for a pricing example.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request/shift validation, then reuses
    /// [`Self::price`] errors. Empty requests and invalid shifts return
    /// [`Error::InvalidInput`]. Inputs remain unchanged.
    pub fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        flat_rates(context)?;
        request.validate()?;
        settings.validate()?;
        self.native(option, context, request)
    }

    fn native(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let (rate, _) = flat_rates(context)?;
        crate::engines::analytic::barrier::validate_life(option, context)?;
        crate::engines::analytic::barrier::validate_schedule(option, context)?;
        let steps = self.settings.step_count as usize;
        let request = if context.valuation_time() == option.expiry_date().start_of_day()
            || option.at_spot_discontinuity(context, 0.0)
        {
            GreeksRequest::default()
        } else {
            GreeksRequest::new(
                [Greek::Delta, Greek::Gamma]
                    .into_iter()
                    .filter(|&g| request.has(g) && (g != Greek::Gamma || steps >= 2)),
            )
        };
        if let Some(value) = crate::engines::analytic::barrier::resolved_price(option, context)? {
            if crate::engines::analytic::barrier::volatility_affects_price(option, context)
                && (request.has(Greek::Delta) || request.has(Greek::Gamma))
            {
                return AnalyticEuropeanEngine.price_with_greeks(
                    option.vanilla(),
                    context,
                    request,
                );
            }
            let mut result = PricingResult::new(value)?;
            result.set(request, Greek::Delta, 0.0)?;
            result.set(request, Greek::Gamma, 0.0)?;
            return Ok(result);
        }
        let time = context.valuation_time();
        let duration = time.micros_until(option.expiry_date().start_of_day());
        let mut observed =
            vec![option.observation_mode() == crate::ObservationMode::Continuous; steps + 1];
        if option.observation_mode() == crate::ObservationMode::Scheduled {
            // Integer arithmetic preserves exact microsecond alignment without overflowing i64.
            for &date in option.observation_dates() {
                let offset = time.micros_until(date.start_of_day());
                if offset < 0 {
                    continue;
                }
                let scaled = i128::from(offset) * steps as i128;
                if scaled % i128::from(duration) != 0 {
                    return Err(Error::UnsupportedOperation(
                        "scheduled barrier observation does not coincide with a CRR tree step"
                            .into(),
                    ));
                }
                observed[(scaled / i128::from(duration)) as usize] = true;
            }
        }
        let dt = time_step(option, context, self.settings)?;
        let (width, up, down, discount, p) = parameters(context, dt)?;
        let spot = context.spot_price();
        let log_barrier =
            crate::engines::analytic::barrier::log_ratio(spot, option.barrier_level());
        let upper = option.barrier_type().is_up();
        let knock_in = option.barrier_type().is_knock_in();
        let breached = |level: usize, node: usize| {
            let offset = (2.0 * node as f64 - level as f64) * width;
            let distance = log_barrier + offset;
            if distance.abs() <= 4.0 * f64::EPSILON * (log_barrier.abs() + offset.abs()).max(1.0) {
                let asset = scaled(spot, offset);
                if upper {
                    asset >= option.barrier_level()
                } else {
                    asset <= option.barrier_level()
                }
            } else if upper {
                distance >= 0.0
            } else {
                distance <= 0.0
            }
        };
        let mut values = vec![0.0; steps + 1];
        let mut vanilla = if knock_in {
            vec![0.0; steps + 1]
        } else {
            Vec::new()
        };
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        for node in 0..=steps {
            let asset = scaled(spot, (2.0 * node as f64 - steps as f64) * width);
            if !asset.is_normal() {
                return Err(unstable());
            }
            let payoff = (sign * (asset - option.strike())).max(0.0);
            let touched = observed[steps] && breached(steps, node);
            values[node] = if touched == knock_in {
                payoff
            } else {
                option.rebate()
            };
            if knock_in {
                vanilla[node] = payoff;
            }
        }
        let mut first = [0.0; 2];
        let mut second = [0.0; 3];
        for level in (0..=steps).rev() {
            if level < steps {
                let rebate = if observed[level] && !knock_in {
                    if option.rebate_timing() == RebateTiming::AtHit {
                        option.rebate()
                    } else {
                        scaled(option.rebate(), -rate * (steps - level) as f64 * dt)
                    }
                } else {
                    0.0
                };
                for node in 0..=level {
                    values[node] = discount * ((1.0 - p) * values[node] + p * values[node + 1]);
                    if knock_in {
                        vanilla[node] =
                            discount * ((1.0 - p) * vanilla[node] + p * vanilla[node + 1]);
                    }
                    if observed[level] && breached(level, node) {
                        values[node] = if knock_in { vanilla[node] } else { rebate };
                    }
                    if !values[node].is_finite() || knock_in && !vanilla[node].is_finite() {
                        return Err(unstable());
                    }
                }
            }
            if level == 1 {
                first.copy_from_slice(&values[..2]);
            }
            if level == 2 {
                second.copy_from_slice(&values[..3]);
            }
        }
        let mut result = PricingResult::new(values[0])?;
        let denominator = spot * (up - down);
        if denominator.is_finite() && denominator != 0.0 {
            result.set(request, Greek::Delta, (first[1] - first[0]) / denominator)?;
        }
        if steps >= 2 {
            let high = spot * (up * up - 1.0);
            let low = spot * (1.0 - down * down);
            let denominator = 0.5 * spot * (up * up - down * down);
            if high.is_finite()
                && low.is_finite()
                && denominator.is_finite()
                && high != 0.0
                && low != 0.0
                && denominator != 0.0
            {
                result.set(
                    request,
                    Greek::Gamma,
                    ((second[2] - second[1]) / high - (second[1] - second[0]) / low) / denominator,
                )?;
            }
        }
        Ok(result)
    }
}

impl PricingEngine<BarrierOption> for CoxRossRubinsteinBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request, settings)
    }
    fn prepare_implied_volatility(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        settings: ImpliedVolatilitySettings,
    ) -> Result<ImpliedVolatilitySettings> {
        prepare_implied(option, context, self.settings, settings)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        crate::engines::analytic::barrier::volatility_affects_price(option, context)
    }
}
