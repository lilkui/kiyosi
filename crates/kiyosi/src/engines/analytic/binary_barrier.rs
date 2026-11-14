use crate::engines::analytic::barrier::{analytic_log_shift, barrier_terms, hit_value};
use crate::instruments::monitoring::validate_life;
use crate::numerics::{checked, log_ratio, scaled_cash, scaled_cdf};
use crate::{
    AnalyticDigitalEngine, DigitalOption, GreeksRequest, OptionContract, OptionType,
    PricingContext, PricingEngine, PricingResult, Result,
};
use crate::{BinaryBarrierOption, SettlementTiming, TouchOption};

use crate::instruments::binary_barrier::{Contract, Settlement};
/// Prices binary barriers and single-touch contracts analytically with flat rates.
///
/// See [`Self::price`] for usage and scheduled-monitoring restrictions.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticBinaryBarrierEngine;
impl AnalyticBinaryBarrierEngine {
    /// Prices a binary barrier or touch contract with strictly prior history.
    ///
    /// Equality counts as a touch before future monitoring. Unresolved scheduled
    /// monitoring uses the Broadie-Glasserman-Kou (BGK) continuity correction
    /// and must end at expiry. Terminal binary equality at strike pays zero.
    /// Settled cash flows do not require curve coverage through expiry.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for nonconstant rate curves or unsupported
    /// schedules. Returns [`crate::Error::InvalidInput`] outside life, for impossible history, or outside required curve coverage.
    /// Non-finite results return [`crate::Error::ComputationFailure`].
    /// Exhausted at-hit payment integration returns [`crate::Error::NonConvergence`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let mut terms = TouchOptionTerms::new(start, Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndIn);
    /// terms.settlement_timing = SettlementTiming::AtHit;
    /// let option = TouchOption::cash(terms, 10.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 120.0, start.into())?;
    /// assert_eq!(AnalyticBinaryBarrierEngine.price(&option, &context)?, 10.0);
    /// # Ok(()) }
    /// ```
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }
    /// Computes requested numerical Greeks with shared bump defaults; see [`Self::price`].
    ///
    /// Expiry Greeks follow terminal payoffs. Before expiry, Greeks at a current barrier equality are absent. Stencils crossing
    /// observations or barriers leave affected Greeks absent.
    ///
    /// # Errors
    ///
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing errors.
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        PricingEngine::price_with_greeks(self, option, context, request)
    }
}

impl Contract<'_> {
    fn vanilla(&self, context: &PricingContext, time: f64) -> Result<f64> {
        context.validate_curve_horizon(self.monitoring.expiry_time())?;
        if let Some(digital) = self.digital {
            AnalyticDigitalEngine.price(digital, context)
        } else {
            let model = context.bsm_parameters()?;
            checked(scaled_cash(
                self.payout.unwrap_or(context.spot_price()),
                -if self.payout.is_some() {
                    model.flat_risk_free_rate()?
                } else {
                    model.flat_dividend_yield()?
                } * time,
            ))
        }
    }
    fn price(&self, context: &PricingContext) -> Result<f64> {
        let option = self.monitoring;
        let model = context.bsm_parameters()?;
        let rate = model.flat_risk_free_rate()?;
        let dividend = model.flat_dividend_yield()?;
        let valuation = context.valuation_time();
        validate_life(option, context)?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::binary(self, context);
        }
        let t = valuation.years_until(option.expiry_time());
        let knock_in = option.barrier_type().is_knock_in();
        let at_hit = self.timing == SettlementTiming::AtHit;
        match self.settlement(context)? {
            Settlement::Value(value) => return Ok(value),
            Settlement::AtExpiry => return self.vanilla(context, t),
            Settlement::Pending => {}
        }
        if self.payoff_requires_touch() {
            return if knock_in {
                self.vanilla(context, t)
            } else {
                Ok(0.0)
            };
        }
        context.validate_curve_horizon(option.expiry_time())?;
        let spot = context.spot_price();
        let sigma = model.volatility();
        let upper = option.barrier_type().is_up();
        let shift = analytic_log_shift(option, spot, sigma)?;
        let ratio = log_ratio(option.barrier_level(), spot) + shift;
        if at_hit {
            return hit_value(
                self.payout.unwrap_or(option.barrier_level()),
                ratio.abs(),
                upper,
                rate - dividend - 0.5 * sigma * sigma,
                sigma,
                t,
                rate,
            );
        }
        if self.digital.is_none() && sigma * sigma == 0.0 {
            let probability = hit_value(1.0, ratio.abs(), upper, rate - dividend, sigma, t, 0.0)?;
            let weight = if knock_in {
                probability
            } else {
                1.0 - probability
            };
            return if weight == 0.0 {
                Ok(0.0)
            } else {
                checked(scaled_cash(
                    self.payout.unwrap_or(spot),
                    -if self.payout.is_some() {
                        rate
                    } else {
                        dividend
                    } * t
                        + weight.ln(),
                ))
            };
        }
        let strike = self
            .digital
            .map_or(option.barrier_level(), DigitalOption::strike);
        let strike_above = log_ratio(strike, option.barrier_level()) > shift;
        let direction = self.digital.map(DigitalOption::option_type);
        let call = direction == Some(OptionType::Call);
        let carry = rate - dividend;
        let ([width, x1, x2, y1, y2], reflected) =
            barrier_terms(log_ratio(spot, strike), ratio, carry, sigma, t);
        let barrier_strike = log_ratio(option.barrier_level(), strike) + shift;
        let phi = if direction.is_some() {
            if call { 1.0 } else { -1.0 }
        } else if knock_in == upper {
            1.0
        } else {
            -1.0
        };
        let eta = if upper { -1.0 } else { 1.0 };
        let asset = self.payout.is_none();
        let amount = self.payout.unwrap_or(spot);
        let discount = -if asset { dividend } else { rate } * t;
        let payoff_shift = if asset { 0.0 } else { width };
        let f1 = || scaled_cdf(amount, discount, phi * x1 - phi * payoff_shift);
        let f2 = || scaled_cdf(amount, discount, phi * x2 - phi * payoff_shift);
        let f3 = || {
            reflected(
                amount,
                discount,
                eta * y1 - eta * payoff_shift,
                barrier_strike,
                asset,
            )
        };
        let f4 = || reflected(amount, discount, eta * y2 - eta * payoff_shift, 0.0, asset);
        let value = if direction.is_none() {
            if knock_in { f2() + f4() } else { f2() - f4() }
        } else {
            match (call, knock_in, upper, strike_above) {
                (true, true, true, true) | (false, true, false, false) => f1(),
                (true, true, true, false) | (false, true, false, true) => f2() - f3() + f4(),
                (true, true, false, true) | (false, true, true, false) => f3(),
                (true, true, false, false) | (false, true, true, true) => f1() - f2() + f4(),
                (true, false, true, true) | (false, false, false, false) => 0.0,
                (true, false, true, false) | (false, false, false, true) => {
                    f1() - f2() + f3() - f4()
                }
                (true, false, false, true) | (false, false, true, false) => f1() - f3(),
                (true, false, false, false) | (false, false, true, true) => f2() - f4(),
            }
        };
        crate::numerics::checked_with_scale(value, scaled_cash(amount, discount))
    }
}

macro_rules! pricing_engine {
    ($contract:ty) => {
        impl PricingEngine<$contract> for AnalyticBinaryBarrierEngine {
            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                crate::engines::flat_rates(context).map(|_| ())
            }
            fn price(&self, option: &$contract, context: &PricingContext) -> Result<f64> {
                option.contract().price(context)
            }
        }
    };
}
pricing_engine!(BinaryBarrierOption);
pricing_engine!(TouchOption);
