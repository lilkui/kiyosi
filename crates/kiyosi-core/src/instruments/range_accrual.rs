use crate::{Date, Error, ErrorCategory, OptionContract, PricingContext, Result, Timestamp};

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
    /// History relative to valuation is validated by [`crate::AnalyticRangeAccrualEngine::price`].
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
