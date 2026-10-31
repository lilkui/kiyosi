use super::double_barrier::AbsorbingDistribution;
use crate::instruments::monitoring::{validate_life, validate_schedule};
use crate::numerics::{checked, scaled_cash};
use crate::{
    DoubleTouchOption, Error, GreeksRequest, NumericalShiftSettings, ObservationMode,
    PricingContext, PricingEngine, PricingResult, Result, SettlementTiming,
};

/// Prices continuous cash double-touch and double-no-touch contracts with flat rates.
///
/// Supports expiry payments and double-touch at-hit payments. Either inclusive
/// boundary triggers a touch. This stateless engine follows the crate concurrency
/// contract; see [`Self::price`] for usage and applicability limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticDoubleTouchEngine;

impl AnalyticDoubleTouchEngine {
    /// Prices remaining cashflows using absorbing-density image and spectral series.
    ///
    /// At-hit payments integrate the discounted first-exit density adaptively.
    /// Prior at-hit payments have zero remaining value. Resolved payments are
    /// returned before rejecting scheduled monitoring; node curves are always rejected.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for node curves, then
    /// [`Error::InvalidInput`] outside life, for non-trading observations or missing
    /// or impossible history. Unresolved scheduled monitoring then returns
    /// [`Error::UnsupportedOperation`]. Unrepresentable calculations return
    /// [`Error::ComputationFailure`]; exhausted series or integration limits return
    /// [`Error::NonConvergence`]. Inputs remain unchanged on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DoubleTouchOption::cash(DoubleTouchOptionTerms::new(start,
    ///     Date::new(2026, 1, 1)?, 80.0, 120.0, DoubleBarrierType::KnockOut), 1.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = AnalyticDoubleTouchEngine;
    /// assert!((engine.price(&option, &context)? - 0.36726156351907174).abs() < 1e-12);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DoubleTouchOption, context: &PricingContext) -> Result<f64> {
        self.validate_applicability(context)?;
        validate_life(option, context)?;
        validate_schedule(option.observation_schedule(), context)?;
        let (prior, touched, finished) = option.contract().state(context)?;
        let at_hit = option.settlement_timing() == SettlementTiming::AtHit;
        if prior && at_hit {
            return Ok(0.0);
        }
        let rate = context.model_parameters().risk_free_rate()?;
        if touched || finished {
            if touched != option.is_one_touch() {
                return Ok(0.0);
            }
            return if at_hit {
                Ok(option.payout())
            } else {
                checked(scaled_cash(
                    option.payout(),
                    -rate
                        * context
                            .valuation_time()
                            .years_until(option.expiry_date().start_of_day()),
                ))
            };
        }
        if option.observation_mode() == ObservationMode::Scheduled {
            return Err(Error::UnsupportedOperation(
                "analytic double-touch pricing requires continuous monitoring".into(),
            ));
        }
        let density = AbsorbingDistribution::new(
            option.lower_barrier(),
            option.upper_barrier(),
            option.expiry_date(),
            context,
        )?;
        if at_hit {
            density.hit_value(option.payout(), rate)
        } else {
            density.cash_value(option.payout(), rate, option.is_one_touch())
        }
    }

    /// Computes requested numerical Greeks with shared bump defaults; see [`Self::price`].
    ///
    /// Greeks are absent at expiry or current barrier equality without a prior touch.
    /// Stencils crossing either boundary or changing monitoring or history regimes
    /// omit affected measures.
    ///
    /// # Errors
    ///
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing failures.
    /// Inputs remain unchanged on failure.
    pub fn price_with_greeks(
        &self,
        option: &DoubleTouchOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}

impl PricingEngine<DoubleTouchOption> for AnalyticDoubleTouchEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn price(&self, option: &DoubleTouchOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn volatility_affects_price(
        &self,
        option: &DoubleTouchOption,
        context: &PricingContext,
    ) -> bool {
        option.contract().volatility_affects_price(context)
    }
}
