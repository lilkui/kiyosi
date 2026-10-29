use crate::{
    BarrierOption, BarrierOptionTerms, BarrierTouchState, BarrierType, Date, Error, ErrorCategory,
    EuropeanOption, ObservationMode, ObservationSchedule, OptionContract, OptionType,
    PricingContext, RebateTiming, Result, Timestamp,
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
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike.
    pub strike: f64,
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
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
    /// Strictly ordered dates within life, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Prior observations; `None` means unknown, not untouched.
    pub touch_state: Option<BarrierTouchState>,
}
impl DoubleBarrierOptionTerms {
    /// Supplies the shared single-barrier defaults without validation.
    /// See [`DoubleBarrierOption::new`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
        lower_barrier: f64,
        upper_barrier: f64,
        barrier_type: DoubleBarrierType,
    ) -> Self {
        let defaults = BarrierOptionTerms::new(
            option_type,
            strike,
            effective_date,
            expiry_date,
            lower_barrier,
            BarrierType::DownAndOut,
        );
        Self {
            option_type,
            strike,
            effective_date,
            expiry_date,
            lower_barrier,
            upper_barrier,
            barrier_type,
            rebate: defaults.rebate,
            rebate_timing: defaults.rebate_timing,
            observation_mode: defaults.observation_mode,
            observation_dates: defaults.observation_dates,
            touch_state: defaults.touch_state,
        }
    }
}

/// Owns two ordered boundaries sharing monitoring and strictly prior history.
/// Immutable shared pricing and replacement are safe under the crate concurrency contract.
/// See [`Self::new`] and [`Self::replace`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleBarrierOption {
    pub(crate) lower: BarrierOption,
    upper: f64,
}
impl DoubleBarrierOption {
    /// Validates boundaries, life, schedule, strike and rebate before returning an owner.
    /// Calendar compatibility and history relative to valuation are checked during pricing.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] unless finite `0 < lower < upper`,
    /// then reuses [`BarrierOption::new`] categories and validation order, including
    /// [`ErrorCategory::InvalidOption`] for at-hit knock-in rebates even if zero.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 80.0, 120.0,
    ///     DoubleBarrierType::KnockOut))?;
    /// assert_eq!(option.upper_barrier(), 120.0);
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: DoubleBarrierOptionTerms) -> Result<Self> {
        if !terms.lower_barrier.is_finite()
            || !terms.upper_barrier.is_finite()
            || terms.lower_barrier <= 0.0
            || terms.upper_barrier <= terms.lower_barrier
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "double barriers require finite 0 < lower < upper",
            ));
        }
        let mut lower = BarrierOptionTerms::new(
            terms.option_type,
            terms.strike,
            terms.effective_date,
            terms.expiry_date,
            terms.lower_barrier,
            if terms.barrier_type == DoubleBarrierType::KnockIn {
                BarrierType::DownAndIn
            } else {
                BarrierType::DownAndOut
            },
        );
        lower.rebate = terms.rebate;
        lower.rebate_timing = terms.rebate_timing;
        lower.observation_mode = terms.observation_mode;
        lower.observation_dates = terms.observation_dates;
        lower.touch_state = terms.touch_state;
        Ok(Self {
            lower: BarrierOption::new(lower)?,
            upper: terms.upper_barrier,
        })
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    /// Time consistency is checked during pricing. The original is preserved on every outcome.
    ///
    /// # Errors
    /// Reuses [`BarrierOption::replace`] validation; typed history cannot invalidate construction.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 80.0, 120.0,
    ///     DoubleBarrierType::KnockOut))?;
    /// let known = option.replace(Some(BarrierTouchState::Untouched))?;
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            lower: self.lower.replace(touch_state)?,
            upper: self.upper,
        })
    }
    /// Returns the payoff direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.lower.option_type()
    }
    /// Returns the positive strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.lower.strike()
    }
    /// Returns the inclusive first date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.lower.effective_date()
    }
    /// Returns expiry at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.lower.expiry_date()
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
    /// Borrows immutable schedule storage until this owner is moved or dropped; see [`Self::new`].
    pub fn observation_schedule(&self) -> &ObservationSchedule {
        self.lower.observation_schedule()
    }
    /// Borrows immutable dates until this owner is moved or dropped; see [`Self::new`].
    pub fn observation_dates(&self) -> &[Date] {
        self.lower.observation_dates()
    }
    /// Returns strictly prior history; see [`Self::new`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.lower.touch_state()
    }

    pub(crate) fn knock_in(&self) -> bool {
        self.lower.barrier_type().is_knock_in()
    }
    pub(crate) fn vanilla(&self) -> Result<EuropeanOption> {
        EuropeanOption::new(
            self.option_type(),
            self.strike(),
            self.effective_date(),
            self.expiry_date(),
        )
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        let (prior, touched, finished) = self.lower.state(context)?;
        Ok((
            prior,
            touched
                || self.lower.monitored(context.valuation_time())
                    && context.spot_price() >= self.upper,
            finished,
        ))
    }
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        (self.observation_mode() == ObservationMode::Continuous
            || self.observation_dates().last() == Some(&self.expiry_date()))
            && match self.option_type() {
                OptionType::Call => self.strike() >= self.upper,
                OptionType::Put => self.strike() <= self.lower_barrier(),
            }
    }
}
impl OptionContract for DoubleBarrierOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.lower.at_spot_discontinuity(context, radius)
            || self.touch_state() != Some(BarrierTouchState::Touched)
                && self.lower.monitored(context.valuation_time())
                && context.spot_price() - radius <= self.upper
                && self.upper <= context.spot_price() + radius
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
