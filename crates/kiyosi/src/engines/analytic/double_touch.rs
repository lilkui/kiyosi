use super::double_barrier::AbsorbingDistribution;
use crate::instruments::monitoring::validate_life;
use crate::numerics::{checked, scaled_cash};
use crate::{
    DoubleTouchOption, Error, GreeksRequest, ObservationMode, PricingContext, PricingEngine,
    PricingResult, Result, SettlementTiming,
};

/// Prices continuous cash double-touch and double-no-touch contracts with flat rates.
///
/// Supports expiry payments and double-touch at-hit payments. Either inclusive
/// boundary triggers a touch. This stateless engine follows the crate concurrency
/// contract; see [`Self::price`] for usage and applicability limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticDoubleTouchEngine;

impl AnalyticDoubleTouchEngine {
    /// Prices remaining cash flows using absorbing-density image and spectral series.
    ///
    /// At-hit payments integrate the discounted first-exit density adaptively.
    /// Prior at-hit payments have zero remaining value. Resolved payments and exact
    /// zero-volatility paths are returned before rejecting scheduled monitoring;
    /// nonconstant rate curves are always rejected.
    /// Settled cash flows do not require curve coverage through expiry.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves, then
    /// [`Error::InvalidInput`] outside life, for missing or impossible history,
    /// or outside required curve coverage. Unresolved scheduled monitoring at positive volatility returns
    /// [`Error::UnsupportedOperation`]. Unrepresentable calculations return
    /// [`Error::ComputationFailure`]; exhausted series or integration limits return
    /// [`Error::NonConvergence`]. Inputs remain unchanged on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DoubleTouchOption::cash(DoubleTouchOptionTerms::new(start,
    ///     Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0, DoubleBarrierType::KnockOut), 1.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// let engine = AnalyticDoubleTouchEngine;
    /// assert!((engine.price(&option, &context)? - 0.36726156351907174).abs() < 1e-12);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]))?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DoubleTouchOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        self.validate_applicability(context)?;
        validate_life(option, context)?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::binary(&option.contract(), context);
        }
        let (prior, touched, finished) = option.contract().state(context)?;
        let at_hit = option.settlement_timing() == SettlementTiming::AtHit;
        if prior && at_hit {
            return Ok(0.0);
        }
        let rate = context.bsm_parameters()?.flat_risk_free_rate()?;
        if touched || finished {
            if touched != option.is_one_touch() {
                return Ok(0.0);
            }
            return if at_hit {
                Ok(option.payout())
            } else {
                context.validate_curve_horizon(option.expiry_time())?;
                checked(scaled_cash(
                    option.payout(),
                    -rate * context.valuation_time().years_until(option.expiry_time()),
                ))
            };
        }
        if option.observation_mode() == ObservationMode::Scheduled {
            return Err(Error::UnsupportedOperation(
                "analytic double-touch pricing requires continuous monitoring".into(),
            ));
        }
        context.validate_curve_horizon(option.expiry_time())?;
        let density = AbsorbingDistribution::new(
            option.lower_barrier(),
            option.upper_barrier(),
            option.expiry_time(),
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
    /// Expiry Greeks follow terminal payoffs. Current touches retain rate and volatility Greeks
    /// of the resolved payoff; spot derivatives remain absent at equality.
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
    ) -> Result<PricingResult> {
        let settings = crate::NumericalShiftSettings::default();
        context.bsm_parameters()?;
        crate::numerical_greeks(self, option, context, request, settings)
    }
}

impl PricingEngine<DoubleTouchOption> for AnalyticDoubleTouchEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.bsm_parameters()?;
        context.bsm_parameters()?.flat_risk_free_rate()?;
        context.bsm_parameters()?.flat_dividend_yield()?;
        Ok(())
    }
    fn price(&self, option: &DoubleTouchOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
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
