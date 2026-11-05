use crate::{
    BarrierOption, BarrierOptionTerms, BarrierTouchState, BarrierType, EuropeanOption,
    ObservationMode, ObservationSchedule, OptionContract, OptionType, PricingContext, RebateTiming,
    Result, Timestamp, TradingCalendar,
};

/// Activation by either boundary; see [`DoubleBarrierOption::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoubleBarrierType {
    /// Activates the vanilla payoff on the first touch.
    KnockIn,
    /// Terminates the vanilla payoff on the first touch.
    KnockOut,
}

/// Input terms validated by [`DoubleBarrierOption::new`]; see its example.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleBarrierOptionTerms {
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike.
    pub strike: f64,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Positive finite lower boundary; equality touches.
    pub lower_barrier: f64,
    /// Finite upper boundary, strictly above the lower boundary.
    pub upper_barrier: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: DoubleBarrierType,
    /// Nonnegative finite cash rebate, default zero.
    pub rebate: f64,
    /// Rebate payment timing, default expiry.
    pub rebate_timing: RebateTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Strictly ordered times within life, empty for continuous monitoring.
    pub observation_times: Vec<Timestamp>,
    /// Prior observations; `None` means unknown, not untouched.
    pub touch_state: Option<BarrierTouchState>,
}
impl DoubleBarrierOptionTerms {
    /// Supplies the shared single-barrier defaults without validation.
    ///
    /// See [`DoubleBarrierOption::new`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        lower_barrier: f64,
        upper_barrier: f64,
        barrier_type: DoubleBarrierType,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            option_type,
            strike,
            effective_time,
            expiry_time,
            lower_barrier,
            upper_barrier,
            barrier_type,
            rebate: BarrierOptionTerms::DEFAULT_REBATE,
            rebate_timing: RebateTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_times: Vec::new(),
            touch_state: None,
        }
    }
}

/// Owns two ordered boundaries sharing monitoring and strictly prior history.
///
/// Immutable shared pricing and replacement are safe under the crate concurrency contract.
/// See [`Self::new`] and [`Self::replace_history`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleBarrierOption {
    pub(crate) lower: BarrierOption,
    upper: f64,
}
impl DoubleBarrierOption {
    /// Replaces the owned observation calendar; see [`Self::new`] for construction.
    ///
    /// Defaults to weekdays. Construction and replacement validate scheduled observations against
    /// this calendar. Other terms remain unchanged.
    ///
    /// Amends an already validated contract; this is not an initial-construction builder.
    /// Set [`DoubleBarrierOptionTerms::calendar`] before calling [`Self::new`] when the
    /// terms require a custom calendar, such as a weekend expiry or observation.
    /// This method consumes the contract even on failure; [`Self::replace`] preserves it.
    /// See [`crate::RangeAccrualOption::with_calendar`] for an initial-calendar example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] if observations or required expiry fall on closed dates.
    pub fn with_calendar(mut self, calendar: TradingCalendar) -> Result<Self> {
        self.lower = self.lower.with_calendar(calendar)?;
        Ok(self)
    }

    /// Borrows the contract's observation calendar; see [`Self::with_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        self.lower.calendar()
    }

    /// Validates boundaries, life, schedule, strike and rebate before returning an owner.
    ///
    /// Calendar compatibility is checked on construction; history relative to valuation is checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] unless finite `0 < lower < upper`,
    /// then reuses [`BarrierOption::new`] categories and validation order, including
    /// for at-hit knock-in rebates even if zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0,
    ///     DoubleBarrierType::KnockOut))?;
    /// assert_eq!(option.upper_barrier(), 120.0);
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: DoubleBarrierOptionTerms) -> Result<Self> {
        super::monitoring::validate_upper(terms.lower_barrier, terms.upper_barrier)?;
        let mut lower = BarrierOptionTerms::new(
            terms.option_type,
            terms.strike,
            terms.effective_time,
            terms.expiry_time,
            terms.lower_barrier,
            if terms.barrier_type == DoubleBarrierType::KnockIn {
                BarrierType::DownAndIn
            } else {
                BarrierType::DownAndOut
            },
        );
        lower.calendar = terms.calendar;
        lower.rebate = terms.rebate;
        lower.rebate_timing = terms.rebate_timing;
        lower.observation_mode = terms.observation_mode;
        lower.observation_times = terms.observation_times;
        lower.touch_state = terms.touch_state;
        Ok(Self {
            lower: BarrierOption::new(lower)?,
            upper: terms.upper_barrier,
        })
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    ///
    /// Time consistency is checked during pricing. The original is preserved on every outcome.
    ///
    /// Preserves validated monitoring terms through [`BarrierOption::replace_history`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0,
    ///     DoubleBarrierType::KnockOut))?;
    /// let known = option.replace_history(Some(BarrierTouchState::Untouched));
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn replace_history(&self, touch_state: Option<BarrierTouchState>) -> Self {
        Self {
            lower: self.lower.replace_history(touch_state),
            upper: self.upper,
        }
    }
    /// Returns the payoff direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.lower.option_type()
    }
    /// Returns the positive strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.lower.strike()
    }
    /// Returns the inclusive effective time; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.lower.effective_time()
    }
    /// Returns the inclusive expiry timestamp; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.lower.expiry_time()
    }
    /// Returns the positive lower boundary; see [`Self::new`].
    pub fn lower_barrier(&self) -> f64 {
        self.lower.barrier_level()
    }
    /// Returns the upper boundary; see [`Self::new`].
    pub fn upper_barrier(&self) -> f64 {
        self.upper
    }
    /// Returns activation behavior; see [`Self::new`].
    pub fn barrier_type(&self) -> DoubleBarrierType {
        if self.knock_in() {
            DoubleBarrierType::KnockIn
        } else {
            DoubleBarrierType::KnockOut
        }
    }
    /// Returns the nonnegative rebate; see [`Self::new`].
    pub fn rebate(&self) -> f64 {
        self.lower.rebate()
    }
    /// Returns rebate payment timing; see [`Self::new`].
    pub fn rebate_timing(&self) -> RebateTiming {
        self.lower.rebate_timing()
    }
    /// Returns monitoring frequency; see [`Self::new`].
    pub fn observation_mode(&self) -> ObservationMode {
        self.lower.observation_mode()
    }
    /// Borrows immutable schedule storage for the lifetime of the borrow; see [`Self::new`].
    pub fn observation_schedule(&self) -> &ObservationSchedule {
        self.lower.observation_schedule()
    }
    /// Borrows immutable times for the lifetime of the borrow; see [`Self::new`].
    pub fn observation_times(&self) -> &[Timestamp] {
        self.lower.observation_times()
    }
    /// Returns strictly prior history; see [`Self::new`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.lower.touch_state()
    }

    pub(crate) fn knock_in(&self) -> bool {
        self.lower.barrier_type().is_knock_in()
    }
    pub(crate) fn vanilla(&self) -> &EuropeanOption {
        self.lower.vanilla()
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        self.lower.monitoring.state(context, Some(self.upper))
    }
    pub(crate) fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let Ok((_, touched, finished)) = self.state(context) else {
            return false;
        };
        !(touched && !self.knock_in()
            || !touched && finished && self.knock_in()
            || !self.knock_in() && self.payoff_requires_touch() && self.rebate() == 0.0)
    }
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        (self.observation_mode() == ObservationMode::Continuous
            || self.observation_times().last() == Some(&self.expiry_time()))
            && match self.option_type() {
                OptionType::Call => self.strike() >= self.upper,
                OptionType::Put => self.strike() <= self.lower_barrier(),
            }
    }
}
impl OptionContract for DoubleBarrierOption {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.lower.at_spot_discontinuity(context, radius)
            || self
                .lower
                .monitoring
                .spot_discontinuity(context, radius, Some(self.upper))
    }
    fn time_stencil_available(
        &self,
        context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        self.lower.time_stencil_available(context, before, after)
    }
}

/// Selects replacements for [`DoubleBarrierOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct DoubleBarrierOptionChanges {
    /// Replaces the owned observation calendar; see [`DoubleBarrierOption::with_calendar`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`option_type`](DoubleBarrierOptionTerms::option_type).
    pub option_type: Option<OptionType>,
    /// Replaces [`strike`](DoubleBarrierOptionTerms::strike).
    pub strike: Option<f64>,
    /// Replaces [`effective_time`](DoubleBarrierOptionTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](DoubleBarrierOptionTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`lower_barrier`](DoubleBarrierOptionTerms::lower_barrier).
    pub lower_barrier: Option<f64>,
    /// Replaces [`upper_barrier`](DoubleBarrierOptionTerms::upper_barrier).
    pub upper_barrier: Option<f64>,
    /// Replaces [`barrier_type`](DoubleBarrierOptionTerms::barrier_type).
    pub barrier_type: Option<DoubleBarrierType>,
    /// Replaces [`rebate`](DoubleBarrierOptionTerms::rebate).
    pub rebate: Option<f64>,
    /// Replaces [`rebate_timing`](DoubleBarrierOptionTerms::rebate_timing).
    pub rebate_timing: Option<RebateTiming>,
    /// Replaces [`observation_mode`](DoubleBarrierOptionTerms::observation_mode).
    pub observation_mode: Option<ObservationMode>,
    /// Replaces [`observation_times`](DoubleBarrierOptionTerms::observation_times).
    pub observation_times: Option<Vec<Timestamp>>,
    /// Replaces [`touch_state`](DoubleBarrierOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
}

impl DoubleBarrierOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks remain at pricing time.
    /// See [`DoubleBarrierOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: DoubleBarrierOptionChanges) -> Result<Self> {
        Self::new(DoubleBarrierOptionTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            option_type: changes.option_type.unwrap_or(self.option_type()),
            strike: changes.strike.unwrap_or(self.strike()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            lower_barrier: changes.lower_barrier.unwrap_or(self.lower_barrier()),
            upper_barrier: changes.upper_barrier.unwrap_or(self.upper_barrier()),
            barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
            rebate: changes.rebate.unwrap_or(self.rebate()),
            rebate_timing: changes.rebate_timing.unwrap_or(self.rebate_timing()),
            observation_mode: changes.observation_mode.unwrap_or(self.observation_mode()),
            observation_times: changes
                .observation_times
                .unwrap_or_else(|| self.observation_times().to_vec()),
            touch_state: changes.touch_state.unwrap_or(self.touch_state()),
        })
    }
}
