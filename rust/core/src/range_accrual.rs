use crate::{
    Date, Error, ErrorCategory, GreeksRequest, NumericalShiftSettings, ObservationSchedule,
    OptionContract, PricingContext, PricingEngine, PricingResult, Result, Timestamp,
    barrier::{checked, log_ratio, scaled_cash, standardize},
    quadrature::gaussian::{Sum, log_interval_mass},
};

/// Supplies equally weighted range-coupon terms; see [`RangeAccrualOption::new`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeAccrualTerms {
    /// Positive finite coupon notional, without principal repayment.
    pub notional: f64,
    /// Nonnegative finite annual coupon rate.
    pub annual_coupon_rate: f64,
    /// Inclusive nonnegative lower spot boundary.
    pub lower_bound: f64,
    /// Inclusive finite upper spot boundary, strictly above the lower boundary.
    pub upper_bound: f64,
    /// First contract date, excluded from observations.
    pub effective_date: Date,
    /// Payment date and final observation, at midnight UTC.
    pub expiry_date: Date,
    /// Hits strictly before valuation; `None` means unknown history.
    pub realized_hit_count: Option<i32>,
}

impl RangeAccrualTerms {
    /// Creates terms with unknown history; see [`RangeAccrualOption::new`] for validation and usage.
    pub fn new(
        notional: f64,
        annual_coupon_rate: f64,
        lower_bound: f64,
        upper_bound: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Self {
        Self {
            notional,
            annual_coupon_rate,
            lower_bound,
            upper_bound,
            effective_date,
            expiry_date,
            realized_hit_count: None,
        }
    }
}

/// Owns a validated range coupon with no principal payment or early termination.
/// Observes the context calendar's trading dates in `(effective_date, expiry_date]`.
/// Pays `notional * annual_coupon_rate * ACT/365 * hits / observations` at expiry.
/// Calendar and history consistency are checked during pricing. Shared reads are safe.
/// See [`Self::new`] for an example.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeAccrualOption {
    terms: RangeAccrualTerms,
}

impl RangeAccrualOption {
    /// Validates finite amounts, ordered bounds, nonnegative history and positive term.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for invalid amounts, bounds or negative
    /// history, then [`ErrorCategory::InvalidTimeRange`] for nonpositive contract life.
    /// History relative to valuation is validated by [`AnalyticRangeAccrualEngine::price`].
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{Date, RangeAccrualOption, RangeAccrualTerms};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let terms = RangeAccrualTerms::new(1000.0, 0.12, 95.0, 110.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2025, 1, 17)?);
    /// let option = RangeAccrualOption::new(terms)?;
    /// let seasoned = option.replace(Some(3))?;
    /// assert_eq!(seasoned.realized_hit_count(), Some(3));
    /// assert_eq!(seasoned.replace(None)?, option);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: RangeAccrualTerms) -> Result<Self> {
        if !terms.notional.is_finite()
            || terms.notional <= 0.0
            || !terms.annual_coupon_rate.is_finite()
            || terms.annual_coupon_rate < 0.0
            || !terms.lower_bound.is_finite()
            || !terms.upper_bound.is_finite()
            || terms.lower_bound < 0.0
            || terms.lower_bound >= terms.upper_bound
            || terms.realized_hit_count.is_some_and(|n| n < 0)
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "invalid range accrual amounts, bounds or history",
            ));
        }
        if terms.expiry_date <= terms.effective_date {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "range accrual requires a positive contract term",
            ));
        }
        Ok(Self { terms })
    }

    /// Returns an owning copy with replaced history; `None` restores unknown. See [`Self::new`].
    ///
    /// # Errors
    /// Reuses [`Self::new`] validation. The original is unchanged on success and failure.
    pub fn replace(&self, realized_hit_count: Option<i32>) -> Result<Self> {
        Self::new(RangeAccrualTerms {
            realized_hit_count,
            ..self.terms
        })
    }

    /// Returns the positive notional; see [`Self::new`].
    pub fn notional(&self) -> f64 {
        self.terms.notional
    }
    /// Returns the nonnegative annual coupon rate; see [`Self::new`].
    pub fn annual_coupon_rate(&self) -> f64 {
        self.terms.annual_coupon_rate
    }
    /// Returns the inclusive lower spot boundary; see [`Self::new`].
    pub fn lower_bound(&self) -> f64 {
        self.terms.lower_bound
    }
    /// Returns the inclusive upper spot boundary; see [`Self::new`].
    pub fn upper_bound(&self) -> f64 {
        self.terms.upper_bound
    }
    /// Returns the first date, excluded from observations; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.terms.effective_date
    }
    /// Returns payment and final observation at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.terms.expiry_date
    }
    /// Returns hits strictly before valuation; see [`Self::new`].
    pub fn realized_hit_count(&self) -> Option<i32> {
        self.terms.realized_hit_count
    }
}

impl OptionContract for RangeAccrualOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
    fn valuation_start(&self) -> Option<Date> {
        None
    }
    fn all_greeks_unavailable(&self, _context: &PricingContext) -> bool {
        false
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        let time = context.valuation_time();
        self.annual_coupon_rate() != 0.0
            && time > self.effective_date().start_of_day()
            && time <= self.expiry_date().start_of_day()
            && time.micros_since_midnight() == 0
            && context.calendar().is_trading_day(time.date())
            && [self.lower_bound(), self.upper_bound()]
                .iter()
                .any(|&level| {
                    context.spot_price() - radius <= level && level <= context.spot_price() + radius
                })
    }
    fn time_stencil_available(
        &self,
        context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        let mut date = before.date().max(self.effective_date());
        let last = after.date().min(self.expiry_date());
        while date <= last {
            if date > self.effective_date()
                && before <= date.start_of_day()
                && date.start_of_day() <= after
                && context.calendar().is_trading_day(date)
            {
                return false;
            }
            if date == last {
                break;
            }
            let Ok(next) = date.add_days(1) else {
                return false;
            };
            date = next;
        }
        true
    }
}

/// Prices range coupons by summing exact BSM marginal probabilities with deterministic curves.
/// Owns no mutable state and supports concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticRangeAccrualEngine;

impl AnalyticRangeAccrualEngine {
    /// Computes requested numerical Greeks from exact prices with core bump defaults.
    /// All Greeks are absent at expiry. Spot stencils reaching current bounds and time
    /// stencils touching any observation are unavailable; vega/rho remain valid at bounds.
    /// See [`Self::price`] for contract construction and pricing.
    ///
    /// # Errors
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
    /// Valuation before the effective date is valid. Zero coupons still validate calendar/history,
    /// then return zero without requiring curve coverage. Payment occurs only at expiry.
    ///
    /// # Errors
    /// Checks, in order: valuation after expiry ([`ErrorCategory::InvalidTimeRange`]),
    /// non-trading expiry ([`ErrorCategory::InvalidDate`]), missing or excessive prior hits
    /// ([`ErrorCategory::InvalidParameter`]), required curve coverage
    /// ([`ErrorCategory::InvalidTimeRange`]), and unresolvable curve knots or non-finite transitions/prices
    /// ([`ErrorCategory::InvalidResult`]).
    /// Inputs remain unchanged on every outcome.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
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
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "valuation is after expiry",
            ));
        }
        if !context.calendar().is_trading_day(option.expiry_date()) {
            return Err(Error::new(
                ErrorCategory::InvalidDate,
                "expiry must be a trading day",
            ));
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
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "missing or excessive prior hits",
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
                return Err(Error::new(
                    ErrorCategory::InvalidResult,
                    "non-finite range transition",
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
