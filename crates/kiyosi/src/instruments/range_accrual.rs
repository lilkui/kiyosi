use crate::{Error, OptionContract, PricingContext, Result, Timestamp, TradingCalendar};

/// Supplies equally weighted range-coupon terms; see [`RangeAccrualOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct RangeAccrualTerms {
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Positive finite coupon notional, without principal repayment.
    pub notional: f64,
    /// Nonnegative finite annual coupon rate.
    pub annual_coupon_rate: f64,
    /// Inclusive nonnegative lower spot boundary.
    pub lower_bound: f64,
    /// Inclusive finite upper spot boundary, strictly above the lower boundary.
    pub upper_bound: f64,
    /// Effective time, excluded from observations.
    pub effective_time: Timestamp,
    /// Payment time and final observation.
    pub expiry_time: Timestamp,
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
        effective_time: Timestamp,
        expiry_time: Timestamp,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            notional,
            annual_coupon_rate,
            lower_bound,
            upper_bound,
            effective_time,
            expiry_time,
            realized_hit_count: None,
        }
    }
}

/// Owns a validated range coupon with no principal payment or early termination.
///
/// Observes at the expiry clock time on trading dates in `(effective_time, expiry_time]`.
/// Pays `notional * annual_coupon_rate * ACT/365 * hits / observations` at expiry.
/// Calendar compatibility is checked on construction; history consistency is checked during pricing. Shared reads are safe.
/// See [`Self::new`] for an example.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeAccrualOption {
    terms: RangeAccrualTerms,
}

impl RangeAccrualOption {
    /// Validates finite amounts, ordered bounds, nonnegative history and positive term.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid amounts, bounds or negative
    /// history, then for nonpositive contract life or a nontrading expiry date.
    /// History relative to valuation is validated by [`crate::AnalyticRangeAccrualEngine::price`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, RangeAccrualOption, RangeAccrualTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let terms = RangeAccrualTerms::new(1000.0, 0.12, 95.0, 110.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 1, 17)?.start_of_day());
    /// let option = RangeAccrualOption::new(terms)?;
    /// let seasoned = option.replace_history(Some(3))?;
    /// assert_eq!(seasoned.realized_hit_count(), Some(3));
    /// assert_eq!(seasoned.replace_history(None)?, option);
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
            return Err(Error::InvalidInput(
                "invalid range accrual amounts, bounds or history".into(),
            ));
        }
        if terms.expiry_time <= terms.effective_time {
            return Err(Error::InvalidInput(
                "range accrual requires a positive contract term".into(),
            ));
        }
        let option = Self { terms };
        option.validate_calendar()?;
        Ok(option)
    }

    /// Replaces the owned observation calendar; see [`Self::new`] for construction.
    ///
    /// Defaults to weekdays. Construction and replacement check expiry against this calendar.
    /// Other terms remain unchanged.
    ///
    /// Amends an already validated contract; this is not an initial-construction builder.
    /// Set [`RangeAccrualTerms::calendar`] before calling [`Self::new`] when the
    /// terms require a custom calendar, such as a weekend expiry or observation.
    /// The original contract remains unchanged on success and failure.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] if observations or required expiry fall on closed dates.
    ///
    /// # Examples
    ///
    /// Selects an all-days calendar before validating a Saturday expiry:
    ///
    /// ```rust
    /// use kiyosi::{Date, Error, RangeAccrualOption, RangeAccrualTerms, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 3)?.start_of_day();
    /// let expiry = Date::new(2025, 1, 4)?.start_of_day();
    /// let mut terms = RangeAccrualTerms::new(1000.0, 0.12, 80.0, 120.0, start, expiry);
    /// terms.calendar = TradingCalendar::all_days();
    /// let option = RangeAccrualOption::new(terms)?;
    /// assert!(option.calendar().is_trading_day(expiry.date()));
    /// // An amendment also validates the existing Saturday expiry.
    /// assert!(matches!(option.replace_calendar(TradingCalendar::weekdays()),
    ///     Err(Error::InvalidInput(_))));
    /// # Ok(()) }
    /// ```
    pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        let mut updated = self.clone();
        updated.terms.calendar = calendar;
        updated.validate_calendar()?;
        Ok(updated)
    }

    fn validate_calendar(&self) -> Result<()> {
        if !self.calendar().is_trading_day(self.expiry_time().date()) {
            return Err(Error::InvalidInput("expiry must be a trading day".into()));
        }
        Ok(())
    }

    /// Borrows the contract's observation calendar; see [`Self::replace_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        &self.terms.calendar
    }

    /// Returns an owning copy with replaced history; `None` restores unknown. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation. The original is unchanged on success and failure.
    pub fn replace_history(&self, realized_hit_count: Option<i32>) -> Result<Self> {
        self.replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(realized_hit_count),
            ..Default::default()
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
    /// Returns the effective time, excluded from observations; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.terms.effective_time
    }
    /// Returns payment and final observation time; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.terms.expiry_time
    }
    /// Returns hits strictly before valuation; see [`Self::new`].
    pub fn realized_hit_count(&self) -> Option<i32> {
        self.terms.realized_hit_count
    }
}

impl OptionContract for RangeAccrualOption {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
        context
            .model_parameters()
            .as_bsm()
            .is_ok_and(|model| model.volatility() == 0.0)
            && self.at_spot_discontinuity(context, 0.0)
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        if context
            .model_parameters()
            .as_bsm()
            .is_ok_and(|model| model.volatility() == 0.0)
            && self.annual_coupon_rate() != 0.0
            && crate::ObservationSchedule::daily(
                self.effective_time(),
                self.expiry_time(),
                self.calendar(),
                false,
            )
            .is_ok_and(|schedule| {
                schedule
                    .times()
                    .iter()
                    .filter(|&&time| time > context.valuation_time())
                    .any(|&time| {
                        context.deterministic_crosses(time, self.lower_bound(), radius)
                            || context.deterministic_crosses(time, self.upper_bound(), radius)
                    })
            })
        {
            return true;
        }
        let time = context.valuation_time();
        self.annual_coupon_rate() != 0.0
            && time > self.effective_time()
            && time <= self.expiry_time()
            && time.micros_since_midnight() == self.expiry_time().micros_since_midnight()
            && self.calendar().is_trading_day(time.date())
            && [self.lower_bound(), self.upper_bound()]
                .iter()
                .any(|&level| {
                    context.spot_price() - radius <= level && level <= context.spot_price() + radius
                })
    }
    fn time_stencil_available(
        &self,
        _context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        let Ok(schedule) = crate::ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            false,
        ) else {
            return false;
        };
        !schedule
            .times()
            .iter()
            .any(|&time| before <= time && time <= after)
    }
}

/// Selects replacements for [`RangeAccrualOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct RangeAccrualOptionChanges {
    /// Replaces the owned observation calendar; see [`RangeAccrualOption::replace_calendar`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`notional`](RangeAccrualTerms::notional).
    pub notional: Option<f64>,
    /// Replaces [`annual_coupon_rate`](RangeAccrualTerms::annual_coupon_rate).
    pub annual_coupon_rate: Option<f64>,
    /// Replaces [`lower_bound`](RangeAccrualTerms::lower_bound).
    pub lower_bound: Option<f64>,
    /// Replaces [`upper_bound`](RangeAccrualTerms::upper_bound).
    pub upper_bound: Option<f64>,
    /// Replaces [`effective_time`](RangeAccrualTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](RangeAccrualTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`realized_hit_count`](RangeAccrualTerms::realized_hit_count).
    pub realized_hit_count: Option<Option<i32>>,
}

impl RangeAccrualOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// See [`RangeAccrualOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: RangeAccrualOptionChanges) -> Result<Self> {
        Self::new(RangeAccrualTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            notional: changes.notional.unwrap_or(self.notional()),
            annual_coupon_rate: changes
                .annual_coupon_rate
                .unwrap_or(self.annual_coupon_rate()),
            lower_bound: changes.lower_bound.unwrap_or(self.lower_bound()),
            upper_bound: changes.upper_bound.unwrap_or(self.upper_bound()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            realized_hit_count: changes
                .realized_hit_count
                .unwrap_or(self.realized_hit_count()),
        })
    }
}
