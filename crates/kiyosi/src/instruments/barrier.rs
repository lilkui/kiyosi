use super::monitoring::Monitoring;
use crate::{
    Date, Error, EuropeanOption, ObservationSchedule, OptionContract, OptionType, PricingContext,
    Result, Timestamp,
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
    /// Supplies the default zero cash rebate; see [`Self::new`].
    pub const DEFAULT_REBATE: f64 = 0.0;

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
            rebate: Self::DEFAULT_REBATE,
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
    pub(crate) monitoring: Monitoring,
    rebate: f64,
    rebate_timing: RebateTiming,
}
impl BarrierOption {
    /// Creates an owning contract after validating monitoring and payoff terms.
    ///
    /// Trading-day compatibility and history relative to valuation are checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid level or rebate, reversed
    /// life, incompatible or invalid fixing dates, invalid strike, or an at-hit
    /// knock-in rebate. Diagnostic order is unspecified when several inputs are invalid.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BarrierOption, BarrierOptionTerms, BarrierType, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let terms = BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut);
    /// let option = BarrierOption::new(terms)?;
    /// assert_eq!(option.barrier_level(), 120.0);
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: BarrierOptionTerms) -> Result<Self> {
        let monitoring = Monitoring::new(
            terms.effective_date,
            terms.expiry_date,
            terms.barrier_level,
            terms.barrier_type,
            terms.observation_mode,
            terms.observation_dates,
            terms.touch_state,
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
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut))?;
    /// let known = option.replace(Some(BarrierTouchState::Untouched));
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Self {
        Self {
            vanilla: self.vanilla.clone(),
            monitoring: self.monitoring.replace(touch_state),
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
    /// Borrows ordered dates from this owner; see [`Self::observation_schedule`].
    pub fn observation_dates(&self) -> &[Date] {
        self.monitoring.observation_dates()
    }
    /// Returns prior history; see [`Self::replace`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.monitoring.touch_state()
    }
    /// Returns mean spacing in Actual/365 Fixed years, or zero for continuous monitoring.
    ///
    /// Scheduled spacing includes effective date to first fixing; see [`Self::new`].
    pub fn mean_observation_year_fraction(&self) -> f64 {
        self.monitoring.mean_observation_year_fraction()
    }
    pub(crate) fn vanilla(&self) -> &EuropeanOption {
        &self.vanilla
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        self.monitoring.state(context, None)
    }
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        self.monitoring
            .payoff_requires_touch(self.option_type(), self.strike())
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
        self.monitoring.at_spot_discontinuity(context, radius)
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
