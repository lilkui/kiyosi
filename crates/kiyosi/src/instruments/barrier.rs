use super::monitoring::Monitoring;
use crate::{
    Error, EuropeanOption, ObservationSchedule, OptionContract, OptionType, PricingContext, Result,
    Timestamp, TradingCalendar,
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
    /// Observes only at the supplied timestamps.
    Scheduled,
}

/// Caller-supplied observations strictly before valuation; see [`BarrierOption::replace_history`].
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
    /// Strictly ordered times within life, empty for continuous monitoring.
    pub observation_times: Vec<Timestamp>,
    /// Prior observations; `None` means unknown, not untouched.
    pub touch_state: Option<BarrierTouchState>,
}
impl BarrierOptionTerms {
    /// Supplies the default zero cash rebate; see [`Self::new`].
    pub const DEFAULT_REBATE: f64 = 0.0;

    /// Supplies zero rebate, expiry payment, continuous monitoring and unknown history.
    ///
    /// Does not validate inputs; see [`BarrierOption::new`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            option_type,
            strike,
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            rebate: Self::DEFAULT_REBATE,
            rebate_timing: RebateTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_times: Vec::new(),
            touch_state: None,
        }
    }
}

/// Owns validated single-barrier vanilla terms and strictly prior history.
///
/// Immutable shared reads and pricing are safe; replacement never mutates the original.
/// See [`Self::new`] and [`Self::replace_history`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct BarrierOption {
    vanilla: EuropeanOption,
    pub(crate) monitoring: Monitoring,
    rebate: f64,
    rebate_timing: RebateTiming,
}
impl BarrierOption {
    /// Replaces the owned observation calendar, leaving other terms unchanged.
    ///
    /// Defaults to weekdays. Construction and replacement check scheduled observations against this
    /// calendar; see [`Self::new`].
    ///
    /// Amends an already validated contract; this is not an initial-construction builder.
    /// Set [`BarrierOptionTerms::calendar`] before calling [`Self::new`] when the
    /// terms require a custom calendar, such as a weekend expiry or observation.
    /// The original contract remains unchanged on success and failure.
    /// See [`crate::RangeAccrualOption::replace_calendar`] for an initial-calendar example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] if observations or required expiry fall on closed dates.
    pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        let mut updated = self.clone();
        updated.monitoring = updated.monitoring.replace_calendar(calendar)?;
        Ok(updated)
    }

    /// Borrows the contract's observation calendar; see [`Self::replace_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        self.monitoring.calendar()
    }

    /// Creates an owning contract after validating monitoring and payoff terms.
    ///
    /// Trading-day compatibility is checked on construction; history relative to valuation is checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid level or rebate, reversed
    /// life, incompatible or invalid fixing times, invalid strike, or an at-hit
    /// knock-in rebate. Diagnostic order is unspecified when several inputs are invalid.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BarrierOption, BarrierOptionTerms, BarrierType, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let mut terms = BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut);
    /// terms.calendar = kiyosi::TradingCalendar::all_days();
    /// let option = BarrierOption::new(terms)?;
    /// assert_eq!(option.calendar(), &kiyosi::TradingCalendar::all_days());
    /// assert_eq!(option.barrier_level(), 120.0);
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: BarrierOptionTerms) -> Result<Self> {
        let monitoring = Monitoring::new(
            terms.effective_time,
            terms.expiry_time,
            terms.barrier_level,
            terms.barrier_type,
            terms.observation_mode,
            terms.observation_times,
            terms.touch_state,
            terms.calendar,
        )?;
        let vanilla = EuropeanOption::new(
            terms.option_type,
            terms.strike,
            terms.effective_time,
            terms.expiry_time,
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
            monitoring,
            rebate: terms.rebate,
            rebate_timing: terms.rebate_timing,
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
    /// use kiyosi::{BarrierOption, BarrierOptionTerms, BarrierType, BarrierTouchState, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut))?;
    /// let known = option.replace_history(Some(BarrierTouchState::Untouched));
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn replace_history(&self, touch_state: Option<BarrierTouchState>) -> Self {
        Self {
            vanilla: self.vanilla.clone(),
            monitoring: self.monitoring.replace_history(touch_state),
            rebate: self.rebate,
            rebate_timing: self.rebate_timing,
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
    /// Returns the inclusive effective time; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.vanilla.effective_time()
    }
    /// Returns the inclusive expiry timestamp; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.vanilla.expiry_time()
    }
    /// Returns the positive trigger level; see [`Self::new`].
    pub fn barrier_level(&self) -> f64 {
        self.monitoring.barrier_level()
    }
    /// Returns direction and activation behavior; see [`Self::new`].
    pub fn barrier_type(&self) -> BarrierType {
        self.monitoring.barrier_type()
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
        self.monitoring.observation_mode()
    }
    /// Borrows immutable schedule storage for the lifetime of the borrow; see [`Self::new`].
    pub fn observation_schedule(&self) -> &ObservationSchedule {
        self.monitoring.observation_schedule()
    }
    /// Borrows ordered times from this owner; see [`Self::observation_schedule`].
    pub fn observation_times(&self) -> &[Timestamp] {
        self.monitoring.observation_times()
    }
    /// Returns prior history; see [`Self::replace_history`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.monitoring.touch_state()
    }
    /// Returns mean spacing in Actual/365 Fixed years, or zero for continuous monitoring.
    ///
    /// Scheduled spacing includes effective time to first fixing; see [`Self::new`].
    pub fn mean_observation_year_fraction(&self) -> f64 {
        self.monitoring.mean_observation_year_fraction()
    }
    pub(crate) fn vanilla(&self) -> &EuropeanOption {
        &self.vanilla
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        self.monitoring.state(context, None)
    }
    pub(crate) fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let Ok((_, touched, finished)) = self.state(context) else {
            return false;
        };
        let knock_in = self.barrier_type().is_knock_in();
        !(touched && !knock_in
            || !touched && finished && knock_in
            || !knock_in && self.payoff_requires_touch() && self.rebate() == 0.0)
    }
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        self.monitoring
            .payoff_requires_touch(self.option_type(), self.strike())
    }
}
impl OptionContract for BarrierOption {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.monitoring.at_spot_discontinuity(context, radius)
            || self.vanilla.at_spot_discontinuity(context, radius)
    }
    fn time_stencil_available(
        &self,
        context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        self.monitoring
            .time_stencil_available(context, before, after)
    }
}

/// Selects replacements for [`BarrierOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct BarrierOptionChanges {
    /// Replaces the owned observation calendar; see [`BarrierOption::replace_calendar`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`option_type`](BarrierOptionTerms::option_type).
    pub option_type: Option<OptionType>,
    /// Replaces [`strike`](BarrierOptionTerms::strike).
    pub strike: Option<f64>,
    /// Replaces [`effective_time`](BarrierOptionTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](BarrierOptionTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`barrier_level`](BarrierOptionTerms::barrier_level).
    pub barrier_level: Option<f64>,
    /// Replaces [`barrier_type`](BarrierOptionTerms::barrier_type).
    pub barrier_type: Option<BarrierType>,
    /// Replaces [`rebate`](BarrierOptionTerms::rebate).
    pub rebate: Option<f64>,
    /// Replaces [`rebate_timing`](BarrierOptionTerms::rebate_timing).
    pub rebate_timing: Option<RebateTiming>,
    /// Replaces [`observation_mode`](BarrierOptionTerms::observation_mode).
    pub observation_mode: Option<ObservationMode>,
    /// Replaces [`observation_times`](BarrierOptionTerms::observation_times).
    pub observation_times: Option<Vec<Timestamp>>,
    /// Replaces [`touch_state`](BarrierOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
}

impl BarrierOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks remain at pricing time.
    /// See [`BarrierOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let original = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(),
    ///     120.0, BarrierType::UpAndOut))?.replace_history(Some(BarrierTouchState::Untouched));
    /// let changed = original.replace(BarrierOptionChanges {
    ///     strike: Some(105.0), ..Default::default()
    /// })?;
    /// assert_eq!(changed.strike(), 105.0);
    /// assert_eq!(changed.touch_state(), original.touch_state());
    /// assert_eq!(original.strike(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, changes: BarrierOptionChanges) -> Result<Self> {
        Self::new(BarrierOptionTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            option_type: changes.option_type.unwrap_or(self.option_type()),
            strike: changes.strike.unwrap_or(self.strike()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            barrier_level: changes.barrier_level.unwrap_or(self.barrier_level()),
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
