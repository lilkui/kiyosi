use crate::engines::analytic::barrier::{
    analytic_log_shift, checked, hit_value, log_ratio, reflection_tail, scaled_cash, scaled_cdf,
    standardize,
};
use crate::{
    AnalyticDigitalEngine, Error, GreeksRequest, NumericalShiftSettings, OptionContract,
    OptionType, PricingContext, PricingEngine, PricingResult, Result,
};
use crate::{BinaryBarrierOption, SettlementTiming, TouchOption};

use crate::instruments::binary_barrier::Contract;
/// Stateless flat-rate binary-barrier and single-touch analytics; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticBinaryBarrierEngine;
impl AnalyticBinaryBarrierEngine {
    /// Prices a binary barrier or touch contract with strictly prior history.
    /// Equality touches before future monitoring; scheduled unresolved monitoring uses BGK
    /// and must end at expiry. Terminal binary equality at strike pays zero.
    ///
    /// # Errors
    /// Returns [`Error::UnsupportedOperation`] for node curves or unsupported
    /// schedules. Returns [`Error::InvalidInput`] outside life, for non-trading
    /// fixings, or for missing or impossible history, checked in that order.
    /// Non-finite results return [`Error::ComputationFailure`]. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let mut terms = TouchOptionTerms::new(start, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndIn);
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
    /// Greeks at expiry or a current barrier equality are absent. Stencils crossing
    /// observations or barriers leave affected Greeks absent.
    ///
    /// # Errors
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing errors.
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}

impl Contract<'_> {
    fn vanilla(&self, context: &PricingContext, time: f64) -> Result<f64> {
        if let Some(digital) = self.digital {
            AnalyticDigitalEngine.price(digital, context)
        } else {
            let model = context.model_parameters();
            checked(scaled_cash(
                self.payout.unwrap_or(context.spot_price()),
                -if self.payout.is_some() {
                    model.risk_free_rate()?
                } else {
                    model.dividend_yield()?
                } * time,
            ))
        }
    }
    pub(crate) fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let Ok((_, touched, finished)) = self.state(context) else {
            return false;
        };
        let knock_in = self.monitoring.barrier_type().is_knock_in();
        if touched || finished {
            return self.digital.is_some() && knock_in == touched;
        }
        !(!knock_in && self.digital.is_some() && self.monitoring.payoff_requires_touch())
    }
    fn price(&self, context: &PricingContext) -> Result<f64> {
        let option = self.monitoring;
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        let valuation = context.valuation_time();
        if valuation < option.effective_date().start_of_day()
            || valuation > option.expiry_date().start_of_day()
        {
            return Err(Error::InvalidInput(
                "valuation outside contract life".into(),
            ));
        }
        if option
            .observation_dates()
            .iter()
            .any(|&d| !context.calendar().is_trading_day(d))
        {
            return Err(Error::InvalidInput(
                "observation is not a trading day".into(),
            ));
        }
        let (prior, touched, finished) = option.state(context)?;
        let t = valuation.years_until(option.expiry_date().start_of_day());
        let knock_in = option.barrier_type().is_knock_in();
        let at_hit = self.timing == SettlementTiming::AtHit;
        if prior && at_hit {
            return Ok(0.0);
        }
        if t == 0.0 || touched || finished {
            if knock_in != touched {
                return Ok(0.0);
            }
            if at_hit {
                return Ok(self.payout.unwrap_or(context.spot_price()));
            }
            return self.vanilla(context, t);
        }
        if self.digital.is_some() && option.payoff_requires_touch() {
            return if knock_in {
                self.vanilla(context, t)
            } else {
                Ok(0.0)
            };
        }
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
        let strike = self.digital.map_or(option.barrier_level(), |d| d.strike());
        let strike_above = log_ratio(strike, option.barrier_level()) > shift;
        let direction = self.digital.map(|d| d.option_type());
        let call = direction == Some(OptionType::Call);
        let carry = rate - dividend;
        let width = sigma * t.sqrt();
        let mu = carry / sigma / sigma - 0.5;
        let moneyness = log_ratio(spot, strike);
        let x1 = if mu.is_finite() {
            moneyness / width + (1.0 + mu) * width
        } else {
            standardize(moneyness + carry * t, sigma, t.sqrt()) + 0.5 * width
        };
        let x2 = if mu.is_finite() {
            -ratio / width + (1.0 + mu) * width
        } else {
            standardize(-ratio + carry * t, sigma, t.sqrt()) + 0.5 * width
        };
        let y1 = (2.0 * ratio + moneyness) / width + (1.0 + mu) * width;
        let y2 = ratio / width + (1.0 + mu) * width;
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
        let reflection = 2.0 * (if asset { mu + 1.0 } else { mu }) * ratio;
        let payoff_shift = if asset { 0.0 } else { width };
        let f1 = || scaled_cdf(amount, discount, phi * x1 - phi * payoff_shift);
        let f2 = || scaled_cdf(amount, discount, phi * x2 - phi * payoff_shift);
        let reflected = |argument: f64, at_strike: bool| {
            if reflection.is_finite() {
                scaled_cdf(amount, discount + reflection, argument)
            } else {
                reflection_tail(
                    amount,
                    discount,
                    ratio,
                    if at_strike {
                        log_ratio(option.barrier_level(), strike) + shift
                    } else {
                        0.0
                    },
                    carry,
                    sigma,
                    t,
                    if asset { 0.5 } else { -0.5 },
                )
            }
        };
        let f3 = || reflected(eta * y1 - eta * payoff_shift, true);
        let f4 = || reflected(eta * y2 - eta * payoff_shift, false);
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
        checked(value)
    }
}

macro_rules! pricing_engine {
    ($contract:ty) => {
        impl PricingEngine<$contract> for AnalyticBinaryBarrierEngine {
            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                context.model_parameters().risk_free_rate()?;
                context.model_parameters().dividend_yield()?;
                Ok(())
            }
            fn price(&self, option: &$contract, context: &PricingContext) -> Result<f64> {
                option.contract().price(context)
            }
            fn volatility_affects_price(
                &self,
                option: &$contract,
                context: &PricingContext,
            ) -> bool {
                option.contract().volatility_affects_price(context)
            }
        }
    };
}
pricing_engine!(BinaryBarrierOption);
pricing_engine!(TouchOption);
