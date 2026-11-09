use crate::numerics::{checked, checked_with_scale, log_ratio, scaled_cash};
use crate::{ArithmeticAveragePriceOption, GeometricAveragePriceOption};
use crate::{Error, GreeksRequest, PricingContext, PricingEngine, PricingResult, Result};

/// Prices continuously monitored geometric average-price Asian options under BSM.
///
/// Supports deterministic risk-free-rate and dividend-yield curves.
/// Shared instances support concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticGeometricAveragePriceEngine;

/// Approximates continuously monitored arithmetic average-price Asian option prices.
///
/// Requires constant rates before expiry, including single-fixing contracts.
/// Fits a lognormal distribution to the first two moments of the remaining average,
/// using the Turnbull-Wakeman (Levy-compatible) moment-matching approximation.
/// Shared instances support concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnbullWakemanArithmeticAveragePriceEngine;

impl AnalyticGeometricAveragePriceEngine {
    /// Prices a geometric average-price option, including forward-start averaging and history.
    ///
    /// # Errors
    ///
    /// Missing or stale history returns [`crate::Error::MissingHistory`] or
    /// [`crate::Error::StaleHistory`], respectively; see the [history contract](crate).
    ///
    /// Returns [`Error::UnsupportedOperation`] for non-BSM models,
    /// [`Error::InvalidInput`] outside life or curve coverage,
    /// for inconsistent history, and
    /// [`Error::ComputationFailure`] for non-finite calculations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AnalyticGeometricAveragePriceEngine, GeometricAveragePriceOption,
    ///     BlackScholesMertonParameters, PricingContext, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = GeometricAveragePriceOption::new(OptionType::Call, 100.0,
    ///     start, start, Date::new(2026, 1, 1)?.start_of_day(), None, None)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// assert!((AnalyticGeometricAveragePriceEngine.price(&option, &context)? - 5.035867546515384).abs() < 1e-12);
    /// # Ok(()) }
    /// ```
    pub fn price(
        &self,
        option: &GeometricAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        context.bsm_parameters()?;
        let o = &option.0;
        let tau = o.validate(context)?;
        if tau == 0.0 {
            return o.settlement(context);
        }
        o.coverage(context)?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::average(o, context, true);
        }
        if o.period() == 0.0 {
            return crate::AnalyticEuropeanEngine.price(&o.terms, context);
        }
        let valuation = context.valuation_time();
        let start = o.averaging_start;
        let expiry = o.terms.expiry_time();
        let period = o.period();
        let lead = valuation.years_until(start).max(0.0);
        let future = valuation.max(start).years_until(expiry);
        let weight = future / period;
        let model = context.bsm_parameters()?;
        let sigma = model.volatility();
        let mut times = vec![valuation, start.max(valuation), expiry];
        for curve in [model.risk_free_curve(), model.dividend_curve()] {
            times.extend(
                curve
                    .nodes()
                    .iter()
                    .copied()
                    .filter(|t| *t > valuation && *t < expiry),
            );
        }
        times.sort_unstable();
        times.dedup();
        let mut carry = 0.0;
        for pair in times.windows(2) {
            let from = pair[0];
            let to = pair[1];
            let interval = -model.risk_free_curve().log_factor(from, to)?
                + model.dividend_curve().log_factor(from, to)?;
            let midpoint = valuation.years_until(from) + from.years_until(to) / 2.0;
            carry += interval * (tau - lead.max(midpoint)) / period;
        }
        let variance_time = lead + future / 3.0;
        let deviation = sigma * weight * variance_time.sqrt();
        let mut log_forward = carry - weight * 0.5 * sigma * sigma * (lead + future / 2.0)
            + 0.5 * deviation * deviation;
        if valuation > start {
            log_forward +=
                (1.0 - weight) * log_ratio(o.realized.unwrap_or(0.0), context.spot_price());
        }
        let log_discount = model.risk_free_curve().log_factor(valuation, expiry)?;
        checked(crate::engines::analytic::european::lognormal_price(
            context.spot_price(),
            o.terms.strike(),
            log_forward,
            (log_forward + log_discount, log_discount),
            sigma * weight,
            variance_time.sqrt(),
            o.sign(),
        )?)
    }
}

fn exprel(x: f64) -> f64 {
    if x == 0.0 { 1.0 } else { x.exp_m1() / x }
}
fn divided_exprel(x: f64, y: f64) -> f64 {
    if x.abs().max(y.abs()) < 0.5 {
        let (mut sum, mut homogeneous, mut power, mut factorial) = (0.5, 1.0, 1.0, 2.0);
        for n in 1..=16 {
            power *= y;
            homogeneous = x * homogeneous + power;
            factorial *= f64::from(n + 2);
            sum += homogeneous / factorial;
        }
        sum
    } else if (x - y).abs() < 1e-5 {
        let midpoint = f64::midpoint(x, y);
        ((midpoint - 1.0) * midpoint.exp() + 1.0) / (midpoint * midpoint)
    } else {
        (exprel(x) - exprel(y)) / (x - y)
    }
}
fn divided_exprel_increment_ratio(x: f64, y: f64, h: f64) -> f64 {
    // Factor out h rather than subtracting nearly equal moments.
    if (x + h).abs().max(y.abs()) < 0.5 {
        let (mut sum, mut two, mut three, mut power, mut factorial) =
            (1.0 / 6.0, 1.0, 1.0, 1.0, 6.0);
        for n in 1..=16 {
            power *= y;
            two = x * two + power;
            three = (x + h) * three + two;
            factorial *= f64::from(n + 3);
            sum += three / factorial;
        }
        sum
    } else {
        let increment = (x.exp() * exprel(h) - exprel(x)) / (x + h);
        (increment - divided_exprel(x, y)) / (x + h - y)
    }
}

impl TurnbullWakemanArithmeticAveragePriceEngine {
    fn require_flat(context: &PricingContext) -> Result<()> {
        let model = context.bsm_parameters()?;
        if model.risk_free_curve().exact_flat_rate().is_none()
            || model.dividend_curve().exact_flat_rate().is_none()
        {
            return Err(Error::UnsupportedOperation(
                "Turnbull-Wakeman requires flat rates".into(),
            ));
        }
        Ok(())
    }

    /// Prices the arithmetic approximation, settling realized history at expiry.
    ///
    /// See [`AnalyticGeometricAveragePriceEngine::price`] for the market setup.
    ///
    /// # Errors
    ///
    /// Missing or stale history returns [`crate::Error::MissingHistory`] or
    /// [`crate::Error::StaleHistory`], respectively; see the [history contract](crate).
    ///
    /// Reuses geometric life/history errors, then returns
    /// [`Error::UnsupportedOperation`] for nonconstant rate curves before expiry,
    /// [`Error::InvalidInput`] for curve coverage, or
    /// [`Error::ComputationFailure`] for invalid moments or non-finite results.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{TurnbullWakemanArithmeticAveragePriceEngine, ArithmeticAveragePriceOption,
    ///     BlackScholesMertonParameters, PricingContext, Date, OptionType, GreeksRequest, Greek};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = ArithmeticAveragePriceOption::new(OptionType::Call, 100.0,
    ///     start, start, Date::new(2026, 1, 1)?.start_of_day(), None, None)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let result = TurnbullWakemanArithmeticAveragePriceEngine.price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]))?;
    /// assert!((result.price() - 5.246501860362777).abs() < 1e-12);
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        context.bsm_parameters()?;
        let o = &option.0;
        let tau = o.validate(context)?;
        if tau == 0.0 {
            return o.settlement(context);
        }
        let model = context.bsm_parameters()?;
        Self::require_flat(context)?;
        o.coverage(context)?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::average(o, context, false);
        }
        if o.period() == 0.0 {
            return crate::AnalyticEuropeanEngine.price(&o.terms, context);
        }
        let rate = model.flat_risk_free_rate()?;
        let carry = rate - model.flat_dividend_yield()?;
        let sigma = model.volatility();
        let period = o.period();
        let lead = (tau - period).max(0.0);
        let elapsed = period - tau;
        let delta = tau - lead;
        let m1 = (carry * lead).exp() * exprel(carry * delta);
        if !m1.is_finite() || m1 <= 0.0 {
            return Err(Error::ComputationFailure(
                "invalid arithmetic moment".into(),
            ));
        }
        let strike = o.adjusted_strike(tau);
        let scale = if elapsed > 0.0 { tau / period } else { 1.0 };
        if elapsed > 0.0 && strike <= 0.0 {
            if o.sign() < 0.0 {
                return Ok(0.0);
            }
            let accrued =
                (o.realized.unwrap_or(0.0) * (elapsed / period) - o.terms.strike()).max(0.0);
            return checked(
                scaled_cash(accrued, -rate * tau)
                    + scaled_cash(context.spot_price(), m1.ln() + scale.ln() - rate * tau),
            );
        }
        let overflow = !strike.is_finite();
        let cash_strike = if overflow {
            (elapsed / period).mul_add(
                o.terms.strike() - o.realized.unwrap_or(0.0),
                scale * o.terms.strike(),
            )
        } else {
            strike
        };
        let log_scale = if overflow { scale.ln() } else { 0.0 };
        let price_scale = if overflow { 1.0 } else { scale };
        let vol2 = sigma * sigma;
        let adjusted_vol = if vol2 * tau < 1e-4 {
            let mean = exprel(carry * delta);
            let coefficient = 2.0
                * delta
                * divided_exprel_increment_ratio(2.0 * carry * delta, carry * delta, vol2 * delta)
                / (mean * mean);
            let excess = vol2 * coefficient;
            let variance_time = lead
                + coefficient
                    * if excess == 0.0 {
                        1.0
                    } else {
                        excess.ln_1p() / excess
                    };
            let log_variance = vol2 * variance_time;
            if !log_variance.is_finite() || log_variance < -1e-12 {
                return Err(Error::ComputationFailure(
                    "invalid arithmetic variance".into(),
                ));
            }
            sigma * (variance_time / tau).max(0.0).sqrt()
        } else {
            let m2 = 2.0
                * ((2.0 * carry + vol2) * lead).exp()
                * divided_exprel((2.0 * carry + vol2) * delta, carry * delta);
            let variance = m2.ln() - 2.0 * m1.ln();
            if !variance.is_finite() || variance < -1e-12 {
                return Err(Error::ComputationFailure(
                    "invalid arithmetic variance".into(),
                ));
            }
            (variance.max(0.0) / tau).sqrt()
        };
        if !adjusted_vol.is_finite() {
            return Err(Error::ComputationFailure(
                "invalid arithmetic variance".into(),
            ));
        }
        if adjusted_vol == 0.0 {
            if o.sign() * (log_ratio(context.spot_price(), cash_strike) + log_scale + m1.ln())
                <= 0.0
            {
                return Ok(0.0);
            }
            let asset = scaled_cash(context.spot_price(), log_scale + m1.ln() - rate * tau);
            let cash = scaled_cash(cash_strike, -rate * tau);
            return checked_with_scale(
                price_scale * o.sign() * (asset - cash),
                price_scale * asset.max(cash),
            );
        }
        checked(
            price_scale
                * crate::engines::analytic::european::lognormal_price(
                    context.spot_price(),
                    cash_strike,
                    log_scale + m1.ln(),
                    (log_scale + m1.ln() - rate * tau, -rate * tau),
                    adjusted_vol,
                    tau.sqrt(),
                    o.sign(),
                )?,
        )
    }
}
macro_rules! asian_analytics {
    ($engine:ident, $option:ident) => {
        impl $engine {
            /// Returns requested Greeks using shared scalar finite differences.
            ///
            /// Time Greeks are unavailable when a stencil crosses the history boundary;
            /// expiry Greeks follow terminal payoffs, with time Greeks absent. See [`Self::price`] for a setup example.
            ///
            /// # Errors
            ///
            /// Returns [`Error::ComputationFailure`] for non-finite derivatives,
            /// and propagates pricing and feasible bumped-pricing errors.
            /// Inputs remain unchanged; unavailable stencils leave measures absent.
            pub fn price_with_greeks(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
            ) -> Result<PricingResult> {
                let settings = crate::NumericalShiftSettings::default();
                context.bsm_parameters()?;
                crate::numerical_greeks(self, option, context, request, settings)
            }
        }
    };
}
asian_analytics!(
    AnalyticGeometricAveragePriceEngine,
    GeometricAveragePriceOption
);
asian_analytics!(
    TurnbullWakemanArithmeticAveragePriceEngine,
    ArithmeticAveragePriceOption
);
impl PricingEngine<GeometricAveragePriceOption> for AnalyticGeometricAveragePriceEngine {
    fn price(&self, option: &GeometricAveragePriceOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
}
impl PricingEngine<ArithmeticAveragePriceOption> for TurnbullWakemanArithmeticAveragePriceEngine {
    fn greek_method(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        self.validate_applicability(context)?;
        if context.valuation_time() != option.expiry_time() {
            Self::require_flat(context)?;
        }
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            crate::GreekMethod::Numerical,
        ))
    }

    fn price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn volatility_affects_price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> bool {
        let tau = context.valuation_time().years_until(option.expiry_time());
        option.0.adjusted_strike(tau) > 0.0
    }
}
