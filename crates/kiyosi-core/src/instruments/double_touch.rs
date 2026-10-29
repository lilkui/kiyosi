use crate::{
    BarrierTouchState, BarrierType, Date, DoubleBarrierOption, DoubleBarrierOptionTerms,
    DoubleBarrierType, Error, ErrorCategory, ObservationMode, ObservationSchedule, OptionContract,
    OptionType, PricingContext, Result, SettlementTiming, Timestamp, TouchOptionTerms,
};

/// Cash double-touch inputs; see [`DoubleTouchOption::cash`] for validation and usage.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleTouchOptionTerms {
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite lower boundary; equality touches.
    pub lower_barrier: f64,
    /// Finite upper boundary, strictly above the lower boundary.
    pub upper_barrier: f64,
    /// Knock-in means one-touch; knock-out means no-touch.
    pub barrier_type: DoubleBarrierType,
    /// Payment timing, default expiry; at-hit requires one-touch.
    pub settlement_timing: SettlementTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing dates within life, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl DoubleTouchOptionTerms {
    /// Supplies the single-touch defaults without validation; see [`DoubleTouchOption::cash`].
    pub fn new(
        effective_date: Date,
        expiry_date: Date,
        lower_barrier: f64,
        upper_barrier: f64,
        barrier_type: DoubleBarrierType,
    ) -> Self {
        let defaults = TouchOptionTerms::new(
            effective_date,
            expiry_date,
            lower_barrier,
            BarrierType::DownAndIn,
        );
        Self {
            effective_date,
            expiry_date,
            lower_barrier,
            upper_barrier,
            barrier_type,
            settlement_timing: defaults.settlement_timing,
            observation_mode: defaults.observation_mode,
            observation_dates: defaults.observation_dates,
            touch_state: defaults.touch_state,
        }
    }
}

/// Owns a cash payment activated by either inclusive boundary, or by neither being touched.
/// Both boundaries share a schedule and strictly prior history. Prior at-hit payments
/// have no remaining value. Shared reads follow the crate concurrency contract.
/// See [`Self::cash`] for construction and pricing.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleTouchOption {
    monitoring: DoubleBarrierOption,
    payout: f64,
    settlement_timing: SettlementTiming,
}
impl DoubleTouchOption {
    /// Validates a cash double one-touch or no-touch contract before returning an owner.
    /// Calendar compatibility and valuation-relative history are checked during pricing.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for invalid positive finite payout,
    /// then [`ErrorCategory::InvalidOption`] for at-hit no-touch settlement, then reuses
    /// [`DoubleBarrierOption::new`] boundary, life and schedule validation categories.
    /// Failure preserves inputs; no-touch can settle only at expiry.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DoubleTouchOption::cash(DoubleTouchOptionTerms::new(start,
    ///     Date::new(2026, 1, 1)?, 80.0, 120.0, DoubleBarrierType::KnockIn), 10.0)?;
    /// assert!(option.is_one_touch());
    /// assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     120.0, start.into())?;
    /// let engine = FiniteDifferenceDoubleTouchEngine::default();
    /// assert!((engine.price(&option, &context)? - 10.0 * (-0.03_f64).exp()).abs() < 1e-12);
    /// let known = option.replace(Some(BarrierTouchState::Untouched))?;
    /// assert_eq!(option.touch_state(), None);
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: DoubleTouchOptionTerms, payout: f64) -> Result<Self> {
        if !payout.is_finite() || payout <= 0.0 {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "payout must be finite and positive",
            ));
        }
        if terms.settlement_timing == SettlementTiming::AtHit
            && terms.barrier_type == DoubleBarrierType::KnockOut
        {
            return Err(Error::new(
                ErrorCategory::InvalidOption,
                "at-hit settlement requires a one-touch",
            ));
        }
        // Reuse double-barrier monitoring; the internal strike is not a touch payoff.
        let mut monitoring = DoubleBarrierOptionTerms::new(
            OptionType::Call,
            terms.lower_barrier,
            terms.effective_date,
            terms.expiry_date,
            terms.lower_barrier,
            terms.upper_barrier,
            terms.barrier_type,
        );
        monitoring.observation_mode = terms.observation_mode;
        monitoring.observation_dates = terms.observation_dates;
        monitoring.touch_state = terms.touch_state;
        Ok(Self {
            monitoring: DoubleBarrierOption::new(monitoring)?,
            payout,
            settlement_timing: terms.settlement_timing,
        })
    }
    /// Returns the inclusive first date; see [`Self::cash`].
    pub fn effective_date(&self) -> Date {
        self.monitoring.effective_date()
    }
    /// Returns expiry at midnight UTC; see [`Self::cash`].
    pub fn expiry_date(&self) -> Date {
        self.monitoring.expiry_date()
    }
    /// Returns the positive lower boundary; see [`Self::cash`].
    pub fn lower_barrier(&self) -> f64 {
        self.monitoring.lower_barrier()
    }
    /// Returns the upper boundary; see [`Self::cash`].
    pub fn upper_barrier(&self) -> f64 {
        self.monitoring.upper_barrier()
    }
    /// Returns the positive cash payout; see [`Self::cash`].
    pub fn payout(&self) -> f64 {
        self.payout
    }
    /// Reports whether a touch activates payment; see [`Self::cash`].
    pub fn is_one_touch(&self) -> bool {
        self.monitoring.barrier_type() == DoubleBarrierType::KnockIn
    }
    /// Returns payment timing; see [`Self::cash`].
    pub fn settlement_timing(&self) -> SettlementTiming {
        self.settlement_timing
    }
    /// Returns monitoring frequency; see [`Self::cash`].
    pub fn observation_mode(&self) -> ObservationMode {
        self.monitoring.observation_mode()
    }
    /// Borrows immutable schedule storage for this owner's lifetime; see [`Self::cash`].
    pub fn observation_schedule(&self) -> &ObservationSchedule {
        self.monitoring.observation_schedule()
    }
    /// Borrows immutable dates for this owner's lifetime; see [`Self::cash`].
    pub fn observation_dates(&self) -> &[Date] {
        self.monitoring.observation_dates()
    }
    /// Returns strictly prior history; see [`Self::cash`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.monitoring.touch_state()
    }
    /// Returns an owning copy with replaced history; `None` restores unknown. See [`Self::cash`].
    ///
    /// # Errors
    /// Reuses [`DoubleBarrierOption::replace`] validation; typed history cannot invalidate construction.
    /// The original value remains unchanged on every outcome.
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            monitoring: self.monitoring.replace(touch_state)?,
            payout: self.payout,
            settlement_timing: self.settlement_timing,
        })
    }
    pub(crate) fn contract(&self) -> crate::instruments::binary_barrier::Contract<'_> {
        crate::instruments::binary_barrier::Contract {
            monitoring: &self.monitoring.lower,
            double: Some(&self.monitoring),
            digital: None,
            payout: Some(self.payout),
            timing: self.settlement_timing,
        }
    }
}
impl OptionContract for DoubleTouchOption {
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
