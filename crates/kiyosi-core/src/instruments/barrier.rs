use crate::{
    Date, Error, EuropeanOption, ObservationSchedule, OptionContract, OptionType, PricingContext,
    Result, Timestamp, TradingCalendar,
};

/// Direction and activation behavior; see [`BarrierOption::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarrierType {
    /// Activates at or above the barrier.
    UpAndIn,
    /// Terminates at or above the barrier.
    UpAndOut,
    /// Activates at or below the barrier.
    DownAndIn,
    /// Terminates at or below the barrier.
    DownAndOut,
}
impl BarrierType {
    pub(crate) fn is_up(self) -> bool {
        matches!(self, Self::UpAndIn | Self::UpAndOut)
    }
    pub(crate) fn is_knock_in(self) -> bool {
        matches!(self, Self::UpAndIn | Self::DownAndIn)
    }
}

/// Monitoring frequency; see [`BarrierOption::new`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObservationMode {
    /// Observes throughout the inclusive contract life (the default).
    #[default]
    Continuous,
    /// Observes only at midnight UTC on the supplied dates.
    Scheduled,
}

/// Caller-supplied observations strictly before valuation; see [`BarrierOption::replace`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarrierTouchState {
    /// No earlier observation touched the barrier.
    Untouched,
    /// An earlier observation touched the barrier; any at-hit payment is already paid.
    Touched,
}

/// Cash rebate payment timing; see [`BarrierOption::new`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RebateTiming {
    /// Pays at the first hit; invalid for knock-ins even with zero rebate.
    AtHit,
    /// Pays at expiry (the default).
    #[default]
    AtExpiry,
}

/// Input terms, validated by [`BarrierOption::new`]. See its example.
#[derive(Clone, Debug, PartialEq)]
pub struct BarrierOptionTerms {
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike.
    pub strike: f64,
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite trigger level; equality touches.
    pub barrier_level: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: BarrierType,
    /// Nonnegative finite cash rebate, default zero.
    pub rebate: f64,
    /// Rebate payment timing, default expiry.
    pub rebate_timing: RebateTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Strictly ordered dates within life, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Prior observations; `None` means unknown, not untouched.
    pub touch_state: Option<BarrierTouchState>,
}
impl BarrierOptionTerms {
    /// Supplies zero rebate, expiry payment, continuous monitoring and unknown history.
    ///
    /// Does not validate inputs; see [`BarrierOption::new`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            option_type,
            strike,
            effective_date,
            expiry_date,
            barrier_level,
            barrier_type,
            rebate: 0.0,
            rebate_timing: RebateTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_dates: Vec::new(),
            touch_state: None,
        }
    }
}

/// Owns validated single-barrier vanilla terms and strictly prior history.
///
/// Immutable shared reads and pricing are safe; replacement never mutates the original.
/// See [`Self::new`] and [`Self::replace`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct BarrierOption {
    vanilla: EuropeanOption,
    barrier_level: f64,
    barrier_type: BarrierType,
    rebate: f64,
    rebate_timing: RebateTiming,
    observation_mode: ObservationMode,
    schedule: ObservationSchedule,
    touch_state: Option<BarrierTouchState>,
}
impl BarrierOption {
    /// Validates barrier, life, schedule, then payoff terms before returning an owning contract.
    ///
    /// Trading-day compatibility and history relative to valuation are checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid level or rebate, reversed
    /// life, incompatible or invalid fixing dates, invalid strike, or an at-hit
    /// knock-in rebate, checked in that order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{BarrierOption, BarrierOptionTerms, BarrierType, Date, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let terms = BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut);
    /// let option = BarrierOption::new(terms)?;
    /// assert_eq!(option.barrier_level(), 120.0);
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: BarrierOptionTerms) -> Result<Self> {
        if !terms.barrier_level.is_finite() || terms.barrier_level <= 0.0 {
            return Err(Error::InvalidInput(
                "barrier level must be finite and positive".into(),
            ));
        }
        crate::time::calendar::validate_range(terms.effective_date, terms.expiry_date)?;
        if (terms.observation_mode == ObservationMode::Continuous)
            != terms.observation_dates.is_empty()
        {
            return Err(Error::InvalidInput(
                "monitoring mode and dates disagree".into(),
            ));
        }
        let schedule = ObservationSchedule::new(
            terms.observation_dates,
            terms.effective_date,
            terms.expiry_date,
            &TradingCalendar::all_days(),
        )?;
        let vanilla = EuropeanOption::new(
            terms.option_type,
            terms.strike,
            terms.effective_date,
            terms.expiry_date,
        )?;
        if !terms.rebate.is_finite() || terms.rebate < 0.0 {
            return Err(Error::InvalidInput(
                "rebate must be finite and nonnegative".into(),
            ));
        }
        if terms.barrier_type.is_knock_in() && terms.rebate_timing == RebateTiming::AtHit {
            return Err(Error::InvalidInput(
                "knock-in rebates cannot pay at hit".into(),
            ));
        }
        Ok(Self {
            vanilla,
            barrier_level: terms.barrier_level,
            barrier_type: terms.barrier_type,
            rebate: terms.rebate,
            rebate_timing: terms.rebate_timing,
            observation_mode: terms.observation_mode,
            schedule,
            touch_state: terms.touch_state,
        })
    }
    /// Returns an owning copy with replaced history, leaving the original unchanged.
    ///
    /// Time consistency is checked during pricing. `None` restores unknown history.
    ///
    /// Preserves the validated terms; typed history cannot invalidate construction.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{BarrierOption, BarrierOptionTerms, BarrierType, BarrierTouchState, Date, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut))?;
    /// let known = option.replace(Some(BarrierTouchState::Untouched));
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Self {
        Self {
            touch_state,
            ..self.clone()
        }
    }
    /// Returns the call-or-put direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.vanilla.option_type()
    }
    /// Returns the positive strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.vanilla.strike()
    }
    /// Returns the inclusive first date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.vanilla.effective_date()
    }
    /// Returns expiry at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.vanilla.expiry_date()
    }
    /// Returns the positive trigger level; see [`Self::new`].
    pub fn barrier_level(&self) -> f64 {
        self.barrier_level
    }
    /// Returns direction and activation behavior; see [`Self::new`].
    pub fn barrier_type(&self) -> BarrierType {
        self.barrier_type
    }
    /// Returns the nonnegative rebate; see [`Self::new`].
    pub fn rebate(&self) -> f64 {
        self.rebate
    }
    /// Returns payment timing; see [`Self::new`].
    pub fn rebate_timing(&self) -> RebateTiming {
        self.rebate_timing
    }
    /// Returns monitoring frequency; see [`Self::new`].
    pub fn observation_mode(&self) -> ObservationMode {
        self.observation_mode
    }
    /// Borrows immutable schedule storage for the lifetime of the borrow; see [`Self::new`].
    pub fn observation_schedule(&self) -> &ObservationSchedule {
        &self.schedule
    }
    /// Borrows ordered dates from this owner; see [`Self::observation_schedule`].
    pub fn observation_dates(&self) -> &[Date] {
        self.schedule.dates()
    }
    /// Returns prior history; see [`Self::replace`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.touch_state
    }
    /// Returns mean spacing in Actual/365 Fixed years, or zero for continuous monitoring.
    ///
    /// Scheduled spacing includes effective date to first fixing; see [`Self::new`].
    pub fn mean_observation_year_fraction(&self) -> f64 {
        self.observation_dates().last().map_or(0.0, |&last| {
            self.effective_date().days_until(last) as f64 / 365.0 / self.schedule.len() as f64
        })
    }
    pub(crate) fn vanilla(&self) -> &EuropeanOption {
        &self.vanilla
    }
    pub(crate) fn continuous(&self) -> bool {
        self.observation_mode() == ObservationMode::Continuous
    }
    pub(crate) fn monitored(&self, time: Timestamp) -> bool {
        time >= self.effective_date().start_of_day()
            && time <= self.expiry_date().start_of_day()
            && (self.continuous()
                || self.observation_dates().binary_search(&time.date()).is_ok()
                    && time == time.date().start_of_day())
    }
    pub(crate) fn prior(&self, time: Timestamp) -> Result<bool> {
        let had = if self.continuous() {
            time > self.effective_date().start_of_day()
        } else {
            self.observation_dates()[0].start_of_day() < time
        };
        if !had && self.touch_state() == Some(BarrierTouchState::Touched) {
            return Err(Error::InvalidInput("touch predates monitoring".into()));
        }
        if had && self.touch_state().is_none() {
            return Err(Error::InvalidInput(
                "prior barrier touch state is required".into(),
            ));
        }
        Ok(self.touch_state() == Some(BarrierTouchState::Touched))
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        let time = context.valuation_time();
        let prior = self.prior(time)?;
        let hit = if self.barrier_type().is_up() {
            context.spot_price() >= self.barrier_level()
        } else {
            context.spot_price() <= self.barrier_level()
        };
        Ok((
            prior,
            prior || self.monitored(time) && hit,
            time == self.expiry_date().start_of_day()
                || !self.continuous()
                    && self
                        .observation_dates()
                        .last()
                        .is_some_and(|d| d.start_of_day() <= time),
        ))
    }
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        (self.continuous() || self.observation_dates().last() == Some(&self.expiry_date()))
            && if self.barrier_type().is_up() {
                self.option_type() == OptionType::Call && self.strike() >= self.barrier_level()
            } else {
                self.option_type() == OptionType::Put && self.strike() <= self.barrier_level()
            }
    }
}
impl OptionContract for BarrierOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.touch_state() != Some(BarrierTouchState::Touched)
            && self.monitored(context.valuation_time())
            && context.spot_price() - radius <= self.barrier_level()
            && self.barrier_level() <= context.spot_price() + radius
    }
    fn time_stencil_available(
        &self,
        _context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        self.prior(before).is_ok()
            && self.prior(after).is_ok()
            && (self.continuous()
                || self.touch_state() == Some(BarrierTouchState::Touched)
                || !self
                    .observation_dates()
                    .iter()
                    .any(|d| before <= d.start_of_day() && d.start_of_day() <= after))
    }
}
