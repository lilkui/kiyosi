use crate::RangeAccrualOption;
use crate::numerics::{Sum, checked, log_interval_mass, log_ratio, scaled_cash, standardize};
use crate::{Error, PricingContext, PricingEngine, Result};

/// Prices range coupons by summing exact BSM marginal probabilities with deterministic curves.
///
/// Owns no mutable state and supports concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticRangeAccrualEngine;

impl AnalyticRangeAccrualEngine {
    /// Prices the coupon, using spot for a current observation and explicit prior hit history.
    ///
    /// Valuation must lie within the inclusive effective and expiry times.
    /// Zero coupons validate life and history,
    /// then return zero without requiring curve coverage. Payment occurs only at expiry.
    ///
    /// # Errors
    ///
    /// Missing required prior history returns [`crate::Error::MissingHistory`].
    /// Supplied history covers events strictly before valuation.
    ///
    /// Checks valuation outside contract life, missing or excessive
    /// prior hits, and required curve coverage, in that order. Invalid life, excessive
    /// hits or missing curve coverage return [`Error::InvalidInput`]. Unresolvable curve knots or non-finite
    /// transitions or prices return [`Error::ComputationFailure`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # use kiyosi::OptionContract as _;
    /// # fn main() -> Result<()> {
    /// let option = RangeAccrualOption::new(RangeAccrualTerms::new(1000.0, 0.12, 95.0, 110.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 1, 3)?.start_of_day(), kiyosi::TimeOfDay::new(0, 0, 0, 0)?))?.replace(RangeAccrualOptionChanges { realized_hit_count: Some(Some(1)), ..Default::default() })?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     95.0, option.expiry_time().into())?;
    /// assert!((AnalyticRangeAccrualEngine.price(&option, &context)? - 240.0 / 365.0).abs() < 1e-12);
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "Hit counts are validated nonnegative; calendar-bounded observation counts fit exactly in f64."
    )]
    ///
    /// # Greeks
    ///
    /// Computes requested numerical Greeks from exact prices with core bump defaults.
    ///
    /// Expiry Greeks follow terminal payoffs; time Greeks are absent. Spot stencils reaching current bounds and time
    /// stencils touching any observation are unavailable; vega/rho remain valid at bounds.
    /// See [`Self::price`] for contract construction and pricing.
    ///
    /// ## Errors
    ///
    /// Propagates [`crate::numerical_greeks`] validation and pricing errors.
    ///
    pub fn price(&self, option: &RangeAccrualOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        crate::instruments::monitoring::validate_life(option, context)?;
        let valuation = context.valuation_time();
        let expiry = option.expiry_time();
        let schedule = option.observation_schedule();
        let dates = schedule.times();
        let first = dates.partition_point(|date| *date < valuation);
        if first != 0 && option.realized_hit_count().is_none() {
            return Err(Error::MissingHistory("prior hit count is required".into()));
        }
        if option.realized_hit_count().unwrap_or(0) as usize > first {
            return Err(Error::InvalidInput("excessive prior hits".into()));
        }
        let mut hits = f64::from(option.realized_hit_count().unwrap_or(0));
        let mut future = first;
        if dates.get(future).is_some_and(|date| *date == valuation) {
            hits += f64::from(
                option.lower_bound() <= context.spot_price()
                    && context.spot_price() <= option.upper_bound(),
            );
            future += 1;
        }
        if option.annual_coupon_rate() == 0.0 || future == dates.len() && hits == 0.0 {
            return Ok(0.0);
        }
        let term = option.effective_time().years_until(expiry);
        let mut scale = option.notional().ln() + option.annual_coupon_rate().ln() + term.ln()
            - (dates.len() as f64).ln();
        let model = context.bsm_parameters()?;
        if valuation < expiry {
            model
                .risk_free_curve()
                .validate_horizon(valuation, expiry)?;
            let risk = model.risk_free_curve().log_factor(valuation, expiry)?;
            scale += risk;
        }
        if future < dates.len() {
            model
                .dividend_curve()
                .validate_horizon(valuation, dates[dates.len() - 1])?;
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
        let mut risk = Sum::default();
        let mut dividend = Sum::default();
        let mut previous = valuation;
        let log_factor = |curve: &crate::RateTermStructure, total: &mut Sum, previous, date| {
            // Direct flat factors preserve exact deterministic boundary comparisons.
            if curve.nodes().is_empty() {
                curve.log_factor(valuation, date)
            } else {
                total.add(curve.log_factor(previous, date)?);
                Ok(total.total())
            }
        };
        for date in &dates[future..] {
            crate::cancellation::check()?;
            let risk_log = log_factor(model.risk_free_curve(), &mut risk, previous, *date)?;
            let dividend_log = log_factor(model.dividend_curve(), &mut dividend, previous, *date)?;
            previous = *date;
            let time = valuation.years_until(*date);
            let volatility = model.volatility();
            let drift = -risk_log + dividend_log - 0.5 * volatility * volatility * time;
            let root = time.sqrt();
            if !drift.is_finite() || !(volatility * root).is_finite() {
                return Err(Error::ComputationFailure(
                    "non-finite range transition".into(),
                ));
            }
            if volatility == 0.0 {
                if lower <= drift && drift <= upper {
                    value.add(scale.exp());
                }
                continue;
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
}
