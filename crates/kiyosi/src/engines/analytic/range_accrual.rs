use crate::RangeAccrualOption;
use crate::numerics::{Sum, checked, log_interval_mass, log_ratio, scaled_cash, standardize};
use crate::{
    Error, GreeksRequest, NumericalShiftSettings, ObservationSchedule, PricingContext,
    PricingEngine, PricingResult, Result,
};

/// Prices range coupons by summing exact BSM marginal probabilities with deterministic curves.
///
/// Owns no mutable state and supports concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticRangeAccrualEngine;

impl AnalyticRangeAccrualEngine {
    /// Computes requested numerical Greeks from exact prices with core bump defaults.
    ///
    /// All Greeks are absent at expiry. Spot stencils reaching current bounds and time
    /// stencils touching any observation are unavailable; vega/rho remain valid at bounds.
    /// See [`Self::price`] for contract construction and pricing.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::numerical_greeks`] validation and pricing errors.
    /// Inputs remain unchanged on success and failure.
    pub fn price_with_greeks(
        &self,
        option: &RangeAccrualOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, settings)
    }
    /// Prices the coupon, using spot for a current observation and explicit prior hit history.
    ///
    /// Valuation before the effective date is valid. Zero coupons still validate calendar/history,
    /// then return zero without requiring curve coverage. Payment occurs only at expiry.
    ///
    /// # Errors
    ///
    /// Checks valuation after expiry, non-trading expiry, missing or excessive
    /// prior hits, and required curve coverage, in that order
    /// ([`Error::InvalidInput`]). Unresolvable curve knots or non-finite
    /// transitions or prices return [`Error::ComputationFailure`].
    /// Inputs remain unchanged on every outcome.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = RangeAccrualOption::new(RangeAccrualTerms::new(1000.0, 0.12, 95.0, 110.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2025, 1, 3)?))?.replace(Some(1))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     95.0, option.expiry_date().into())?;
    /// assert!((AnalyticRangeAccrualEngine.price(&option, &context)? - 240.0 / 365.0).abs() < 1e-12);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &RangeAccrualOption, context: &PricingContext) -> Result<f64> {
        let valuation = context.valuation_time();
        let expiry = option.expiry_date().start_of_day();
        if valuation > expiry {
            return Err(Error::InvalidInput("valuation is after expiry".into()));
        }
        if !context.calendar().is_trading_day(option.expiry_date()) {
            return Err(Error::InvalidInput("expiry must be a trading day".into()));
        }
        let schedule = ObservationSchedule::fixed_interval(
            option.effective_date(),
            option.expiry_date(),
            1,
            context.calendar(),
        )?;
        let dates = schedule.dates();
        let first = dates.partition_point(|date| date.start_of_day() < valuation);
        if (first != 0 && option.realized_hit_count().is_none())
            || option.realized_hit_count().unwrap_or(0) as usize > first
        {
            return Err(Error::InvalidInput(
                "missing or excessive prior hits".into(),
            ));
        }
        let mut hits = f64::from(option.realized_hit_count().unwrap_or(0));
        let mut future = first;
        if dates
            .get(future)
            .is_some_and(|date| date.start_of_day() == valuation)
        {
            hits += f64::from(
                option.lower_bound() <= context.spot_price()
                    && context.spot_price() <= option.upper_bound(),
            );
            future += 1;
        }
        if option.annual_coupon_rate() == 0.0 {
            return Ok(0.0);
        }
        let term = option.effective_date().start_of_day().years_until(expiry);
        let mut scale = option.notional().ln() + option.annual_coupon_rate().ln() + term.ln()
            - (dates.len() as f64).ln();
        let model = context.model_parameters();
        if future < dates.len() {
            model
                .risk_free_curve()
                .validate_horizon(valuation, expiry)?;
            model.dividend_curve().validate_horizon(valuation, expiry)?;
            let risk = model.risk_free_curve().log_factor(valuation, expiry)?;
            model.dividend_curve().log_factor(valuation, expiry)?;
            scale += risk;
        }
        // Anchor at spot to keep adjacent floating-point price bounds distinct.
        let lower = if option.lower_bound() == 0.0 {
            f64::NEG_INFINITY
        } else {
            log_ratio(option.lower_bound(), context.spot_price())
        };
        let upper = log_ratio(option.upper_bound(), context.spot_price());
        let mut value = Sum::default();
        value.add(scaled_cash(hits, scale));
        for date in &dates[future..] {
            let time = valuation.years_until(date.start_of_day());
            let volatility = model.volatility();
            let drift = -model
                .risk_free_curve()
                .log_factor(valuation, date.start_of_day())?
                + model
                    .dividend_curve()
                    .log_factor(valuation, date.start_of_day())?
                - 0.5 * volatility * volatility * time;
            let root = time.sqrt();
            if !drift.is_finite() || !(volatility * root).is_finite() {
                return Err(Error::ComputationFailure(
                    "non-finite range transition".into(),
                ));
            }
            let probability = log_interval_mass(
                standardize(lower - drift, volatility, root),
                standardize(upper - drift, volatility, root),
            );
            value.add((scale + probability).exp());
        }
        checked(value.total())
    }
}

impl PricingEngine<RangeAccrualOption> for AnalyticRangeAccrualEngine {
    fn price(&self, option: &RangeAccrualOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn volatility_affects_price(
        &self,
        option: &RangeAccrualOption,
        context: &PricingContext,
    ) -> bool {
        option.annual_coupon_rate() != 0.0
            && context.valuation_time() < option.expiry_date().start_of_day()
    }
}
