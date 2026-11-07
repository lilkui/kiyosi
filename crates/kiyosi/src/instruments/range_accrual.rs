use crate::{Error, OptionContract, PricingContext, Result, TimeOfDay, Timestamp, TradingCalendar};

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
    /// Maturity payment time.
    pub expiry_time: Timestamp,
    /// Sets the explicit daily observation clock; see [`TimeOfDay::new`].
    pub observation_clock_time: TimeOfDay,
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
        observation_clock_time: TimeOfDay,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            notional,
            annual_coupon_rate,
            lower_bound,
            upper_bound,
            effective_time,
            expiry_time,
            observation_clock_time,
            realized_hit_count: None,
        }
    }
}

/// Owns a validated range coupon with no principal payment or early termination.
///
/// Observes at the specified observation clock time on trading dates in `(effective_time, expiry_time]`.
/// Pays `notional * annual_coupon_rate * ACT/365 * hits / observations` at expiry.
/// Settlement may fall on a closed date without adding an observation.
/// Calendar compatibility is checked on construction; history consistency is checked during pricing. Shared reads are safe.
/// See [`Self::new`] for an example.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeAccrualOption {
    history_time: Option<Timestamp>,
    terms: RangeAccrualTerms,
}

impl RangeAccrualOption {
    /// Validates finite amounts, ordered bounds, nonnegative history and positive term.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid amounts, bounds or negative
    /// history, then for nonpositive contract life or an empty daily observation schedule.
    /// History relative to valuation is validated by [`crate::AnalyticRangeAccrualEngine::price`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, OptionContract, RangeAccrualOption, RangeAccrualTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let terms = RangeAccrualTerms::new(1000.0, 0.12, 95.0, 110.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 1, 17)?.start_of_day(), kiyosi::TimeOfDay::new(0, 0, 0, 0)?);
    /// let option = RangeAccrualOption::new(terms)?;
    /// let seasoned = option.replace_history(Some(3))?
    ///     .with_history_time(Some(option.effective_time().add_days(6)?))?;
    /// assert_eq!(seasoned.realized_hit_count(), Some(3));
    /// assert_eq!(seasoned.replace_history(None)?.with_history_time(None)?, option);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: RangeAccrualTerms) -> Result<Self> {
        crate::error::finite(terms.notional, true, "notional")?;
        for (field, value) in [
            ("annual_coupon_rate", terms.annual_coupon_rate),
            ("lower_bound", terms.lower_bound),
            ("upper_bound", terms.upper_bound),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(Error::InvalidInput(
                    format!("{field} must be finite and nonnegative").into(),
                ));
            }
        }
        if terms.lower_bound >= terms.upper_bound {
            return Err(Error::InvalidInput(
                "upper_bound must exceed lower_bound".into(),
            ));
        }
        if terms.realized_hit_count.is_some_and(|n| n < 0) {
            return Err(Error::InvalidInput(
                "realized_hit_count must be nonnegative".into(),
            ));
        }
        if terms.expiry_time <= terms.effective_time {
            return Err(Error::InvalidInput(
                "range accrual requires a positive contract term".into(),
            ));
        }
        let option = Self {
            terms,
            history_time: None,
        };
        option.validate_calendar()?;
        Ok(option)
    }

    /// Replaces the owned observation calendar; see [`Self::new`] for construction.
    ///
    /// Defaults to weekdays. Construction and replacement require a nonempty observation schedule.
    /// Settlement may fall on a closed date. Other terms remain unchanged.
    ///
    /// Amends an already validated contract; this is not an initial-construction builder.
    /// Set [`RangeAccrualTerms::calendar`] before calling [`Self::new`] when the
    /// terms require a custom calendar, such as weekend observations.
    /// The original contract remains unchanged on success and failure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Returns [`crate::Error::InvalidInput`] if the resulting daily observation schedule is empty.
    ///
    /// # Examples
    ///
    /// Settles on Saturday while observing only on open dates:
    ///
    /// ```rust
    /// use kiyosi::{Date, RangeAccrualOption, RangeAccrualTerms, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 2)?.start_of_day();
    /// let expiry = Date::new(2025, 1, 4)?.start_of_day();
    /// let terms = RangeAccrualTerms::new(1000.0, 0.12, 80.0, 120.0, start, expiry, kiyosi::TimeOfDay::new(0, 0, 0, 0)?);
    /// let option = RangeAccrualOption::new(terms)?;
    /// assert_eq!(option.observation_schedule()?.times(), &[start.add_days(1)?]);
    /// let daily = option.replace_calendar(TradingCalendar::all_days())?;
    /// assert_eq!(daily.observation_schedule()?.len(), 2);
    /// assert_eq!(daily.expiry_time(), expiry);
    /// # Ok(()) }
    /// ```
    pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        self.replace(RangeAccrualOptionChanges {
            calendar: Some(calendar),
            ..Default::default()
        })
    }

    fn validate_calendar(&self) -> Result<()> {
        if self.observation_schedule()?.is_empty() {
            return Err(Error::InvalidInput(
                "range accrual requires at least one observation".into(),
            ));
        }
        Ok(())
    }

    /// Borrows the contract's observation calendar; see [`Self::replace_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        &self.terms.calendar
    }

    /// Returns an owning copy with replaced history; `None` restores unknown. See [`Self::new`].
    ///
    /// Preserves the history timestamp. Refresh both fields with [`Self::replace`]
    /// and [`RangeAccrualOptionChanges::history_time`]; supplied history requires a timestamp at pricing.
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
    /// Returns maturity payment time; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.terms.expiry_time
    }
    /// Returns the daily observation clock; see [`TimeOfDay::new`].
    pub fn observation_clock_time(&self) -> TimeOfDay {
        self.terms.observation_clock_time
    }
    /// Returns an owned schedule of the contract's daily range observations.
    ///
    /// Excludes the effective instant, includes observations through expiry and
    /// applies the contractual clock and calendar. Payment adds no observation.
    /// The returned schedule is independent of this contract and its history.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::ObservationSchedule`] time-range errors.
    ///
    /// # Panics
    ///
    /// Propagates panics from custom calendar predicates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, RangeAccrualOption, RangeAccrualTerms, TimeOfDay};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 6)?.start_of_day();
    /// let clock = TimeOfDay::new(0, 0, 0, 0)?;
    /// let option = RangeAccrualOption::new(RangeAccrualTerms::new(
    ///     1000.0, 0.12, 90.0, 110.0, start, start.add_days(2)?, clock))?;
    /// assert_eq!(option.observation_schedule()?.times(), &[start.add_days(1)?, start.add_days(2)?]);
    /// # Ok(()) }
    /// ```
    pub fn observation_schedule(&self) -> Result<crate::ObservationSchedule> {
        crate::ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            false,
            self.observation_clock_time(),
        )
    }
    /// Returns hits strictly before valuation; see [`Self::new`].
    pub fn realized_hit_count(&self) -> Option<i32> {
        self.terms.realized_hit_count
    }
}

impl OptionContract for RangeAccrualOption {
    fn history_time(&self) -> Option<Timestamp> {
        self.history_time
    }
    fn with_history_time(mut self, time: Option<Timestamp>) -> Result<Self> {
        crate::instruments::validate_history_time_bounds(&self, time)?;
        self.history_time = time;
        Ok(self)
    }

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
            && self.observation_schedule().is_ok_and(|schedule| {
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
            && time.micros_since_midnight() == self.observation_clock_time().micros_since_midnight()
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
        let Ok(schedule) = self.observation_schedule() else {
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
    /// Replaces or clears the history timestamp; see [`OptionContract::with_history_time`].
    pub history_time: Option<Option<Timestamp>>,
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
    /// Replaces the daily observation clock; see [`TimeOfDay::new`].
    pub observation_clock_time: Option<TimeOfDay>,
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
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: RangeAccrualOptionChanges) -> Result<Self> {
        let history_supplied = changes.realized_hit_count.is_some();
        let updated = Self::new(RangeAccrualTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            notional: changes.notional.unwrap_or(self.notional()),
            annual_coupon_rate: changes
                .annual_coupon_rate
                .unwrap_or(self.annual_coupon_rate()),
            lower_bound: changes.lower_bound.unwrap_or(self.lower_bound()),
            upper_bound: changes.upper_bound.unwrap_or(self.upper_bound()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            observation_clock_time: changes
                .observation_clock_time
                .unwrap_or(self.observation_clock_time()),
            realized_hit_count: changes
                .realized_hit_count
                .unwrap_or(self.realized_hit_count()),
        })?;

        super::validate_history_replacement(
            self.realized_hit_count().is_some(),
            history_supplied,
            self.history_time.is_none() && self.expiry_time() != updated.expiry_time()
                || (
                    self.lower_bound(),
                    self.upper_bound(),
                    self.effective_time(),
                    self.observation_clock_time(),
                ) != (
                    updated.lower_bound(),
                    updated.upper_bound(),
                    updated.effective_time(),
                    updated.observation_clock_time(),
                )
                || self.calendar() != updated.calendar()
                    && super::historical_times(
                        self.observation_schedule()?.times(),
                        self.history_time,
                    ) != super::historical_times(
                        updated.observation_schedule()?.times(),
                        self.history_time,
                    ),
            "realized_hit_count",
        )?;
        updated.with_history_time(changes.history_time.unwrap_or(self.history_time))
    }
}
