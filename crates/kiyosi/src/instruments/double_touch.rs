use super::monitoring::{Monitoring, validate_upper};
use crate::{
    BarrierTouchState, BarrierType, DoubleBarrierType, Error, ObservationMode, ObservationSchedule,
    OptionContract, PricingContext, Result, SettlementTiming, Timestamp, TradingCalendar,
};

/// Cash double-touch inputs; see [`DoubleTouchOption::cash`] for validation and usage.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleTouchOptionTerms {
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
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
    /// Ordered fixing times within life, empty for continuous monitoring.
    pub observation_times: Vec<Timestamp>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl DoubleTouchOptionTerms {
    /// Supplies the single-touch defaults without validation; see [`DoubleTouchOption::cash`].
    pub fn new(
        effective_time: Timestamp,
        expiry_time: Timestamp,
        lower_barrier: f64,
        upper_barrier: f64,
        barrier_type: DoubleBarrierType,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            effective_time,
            expiry_time,
            lower_barrier,
            upper_barrier,
            barrier_type,
            settlement_timing: SettlementTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_times: Vec::new(),
            touch_state: None,
        }
    }
}

/// Owns a cash payment activated by either inclusive boundary, or by neither being touched.
///
/// Both boundaries share a schedule and strictly prior history. Prior at-hit payments
/// have no remaining value. Shared reads follow the crate concurrency contract.
/// See [`Self::cash`] for construction and pricing.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleTouchOption {
    monitoring: Monitoring,
    upper_barrier: f64,
    payout: f64,
    settlement_timing: SettlementTiming,
}
impl DoubleTouchOption {
    /// Replaces the owned observation calendar; see [`Self::cash`] for construction.
    ///
    /// Defaults to weekdays. Construction and replacement validate scheduled observations against
    /// this calendar. Other terms remain unchanged.
    ///
    /// Set [`DoubleTouchOptionTerms::calendar`] before calling [`Self::cash`] for
    /// scheduled observations on dates closed by the default weekday calendar.
    /// Continuous monitoring is independent of trading days; expiry need not be open.
    /// The original contract remains unchanged on success and failure.
    /// See [`crate::RangeAccrualOption::replace_calendar`] for an initial-calendar example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] if scheduled observations fall on closed dates.
    pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        let mut updated = self.clone();
        updated.monitoring = updated.monitoring.replace_calendar(calendar)?;
        updated.with_history_time(self.history_time())
    }

    /// Borrows the contract's observation calendar; see [`Self::replace_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        self.monitoring.calendar()
    }

    /// Validates a cash double one-touch or no-touch contract before returning an owner.
    ///
    /// Calendar compatibility is checked on construction; valuation-relative history is checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid positive finite payout,
    /// then for at-hit no-touch settlement. Shared monitoring validation then
    /// rejects invalid boundaries, reversed life or invalid schedules.
    /// Failure preserves inputs; no-touch can settle only at expiry.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DoubleTouchOption::cash(DoubleTouchOptionTerms::new(start,
    ///     Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0, DoubleBarrierType::KnockIn), 10.0)?;
    /// assert!(option.is_one_touch());
    /// assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     120.0, start.into())?;
    /// let engine = FiniteDifferenceDoubleTouchEngine::default();
    /// assert!((engine.price(&option, &context)? - 10.0 * (-0.03_f64).exp()).abs() < 1e-12);
    /// let known = option.replace_history(Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: DoubleTouchOptionTerms, payout: f64) -> Result<Self> {
        if !payout.is_finite() || payout <= 0.0 {
            return Err(Error::InvalidInput(
                "payout must be finite and positive".into(),
            ));
        }
        if terms.settlement_timing == SettlementTiming::AtHit
            && terms.barrier_type == DoubleBarrierType::KnockOut
        {
            return Err(Error::InvalidInput(
                "at-hit settlement requires a one-touch".into(),
            ));
        }
        validate_upper(terms.lower_barrier, terms.upper_barrier)?;
        let monitoring = Monitoring::new(
            terms.effective_time,
            terms.expiry_time,
            terms.lower_barrier,
            if terms.barrier_type == DoubleBarrierType::KnockIn {
                BarrierType::DownAndIn
            } else {
                BarrierType::DownAndOut
            },
            terms.observation_mode,
            terms.observation_times,
            terms.touch_state,
            terms.calendar,
        )?;
        Ok(Self {
            monitoring,
            upper_barrier: terms.upper_barrier,
            payout,
            settlement_timing: terms.settlement_timing,
        })
    }
    /// Returns the inclusive effective time; see [`Self::cash`].
    pub fn effective_time(&self) -> Timestamp {
        self.monitoring.effective_time()
    }
    /// Returns the inclusive expiry timestamp; see [`Self::cash`].
    pub fn expiry_time(&self) -> Timestamp {
        self.monitoring.expiry_time()
    }
    /// Returns the positive lower boundary; see [`Self::cash`].
    pub fn lower_barrier(&self) -> f64 {
        self.monitoring.barrier_level()
    }
    /// Returns the upper boundary; see [`Self::cash`].
    pub fn upper_barrier(&self) -> f64 {
        self.upper_barrier
    }
    /// Returns the positive cash payout; see [`Self::cash`].
    pub fn payout(&self) -> f64 {
        self.payout
    }
    /// Reports whether a touch activates payment; see [`Self::cash`].
    pub fn is_one_touch(&self) -> bool {
        self.monitoring.barrier_type().is_knock_in()
    }
    /// Returns the knock-in or knock-out payment condition; see [`Self::cash`].
    pub fn barrier_type(&self) -> DoubleBarrierType {
        if self.is_one_touch() {
            DoubleBarrierType::KnockIn
        } else {
            DoubleBarrierType::KnockOut
        }
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
    /// Borrows immutable times for this owner's lifetime; see [`Self::cash`].
    pub fn observation_times(&self) -> &[Timestamp] {
        self.monitoring.observation_times()
    }
    /// Returns strictly prior history; see [`Self::cash`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.monitoring.touch_state()
    }
    /// Returns an owning copy with replaced history; `None` restores unknown. See [`Self::cash`].
    ///
    /// Preserves validated monitoring terms without reconstructing the contract.
    /// The original value remains unchanged.
    #[must_use]
    pub fn replace_history(&self, touch_state: Option<BarrierTouchState>) -> Self {
        Self {
            monitoring: self.monitoring.replace_history(touch_state),
            upper_barrier: self.upper_barrier,
            payout: self.payout,
            settlement_timing: self.settlement_timing,
        }
    }
    pub(crate) fn contract(&self) -> crate::instruments::binary_barrier::Contract<'_> {
        crate::instruments::binary_barrier::Contract {
            monitoring: &self.monitoring,
            upper_barrier: Some(self.upper_barrier),
            digital: None,
            payout: Some(self.payout),
            timing: self.settlement_timing,
        }
    }
}
impl OptionContract for DoubleTouchOption {
    fn history_time(&self) -> Option<Timestamp> {
        self.monitoring.history_time()
    }
    fn with_history_time(mut self, time: Option<Timestamp>) -> Result<Self> {
        self.monitoring = self.monitoring.with_history_time(time)?;
        Ok(self)
    }

    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.monitoring
            .spot_discontinuity(context, radius, Some(self.upper_barrier))
    }
    fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
        self.monitoring
            .all_greeks_unavailable(context, Some(self.upper_barrier))
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

/// Selects replacements for [`DoubleTouchOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct DoubleTouchOptionChanges {
    /// Replaces or clears the history timestamp; see [`OptionContract::with_history_time`].
    pub history_time: Option<Option<Timestamp>>,
    /// Replaces the owned observation calendar; see [`DoubleTouchOption::replace_calendar`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`effective_time`](DoubleTouchOptionTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](DoubleTouchOptionTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`lower_barrier`](DoubleTouchOptionTerms::lower_barrier).
    pub lower_barrier: Option<f64>,
    /// Replaces [`upper_barrier`](DoubleTouchOptionTerms::upper_barrier).
    pub upper_barrier: Option<f64>,
    /// Replaces [`barrier_type`](DoubleTouchOptionTerms::barrier_type).
    pub barrier_type: Option<DoubleBarrierType>,
    /// Replaces [`settlement_timing`](DoubleTouchOptionTerms::settlement_timing).
    pub settlement_timing: Option<SettlementTiming>,
    /// Replaces [`observation_mode`](DoubleTouchOptionTerms::observation_mode).
    pub observation_mode: Option<ObservationMode>,
    /// Replaces [`observation_times`](DoubleTouchOptionTerms::observation_times).
    pub observation_times: Option<Vec<Timestamp>>,
    /// Replaces [`touch_state`](DoubleTouchOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
    /// Replaces [`payout`](DoubleTouchOption::payout).
    pub payout: Option<f64>,
}

impl DoubleTouchOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Switching to continuous monitoring clears omitted observation times; explicit times must match the mode.
    /// See [`DoubleTouchOption::cash`] for construction examples.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: DoubleTouchOptionChanges) -> Result<Self> {
        let history_supplied = changes.touch_state.is_some();
        let updated = Self::cash(
            DoubleTouchOptionTerms {
                calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
                effective_time: changes.effective_time.unwrap_or(self.effective_time()),
                expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
                lower_barrier: changes.lower_barrier.unwrap_or(self.lower_barrier()),
                upper_barrier: changes.upper_barrier.unwrap_or(self.upper_barrier()),
                barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
                settlement_timing: changes
                    .settlement_timing
                    .unwrap_or(self.settlement_timing()),
                observation_mode: changes.observation_mode.unwrap_or(self.observation_mode()),
                observation_times: self
                    .monitoring
                    .replacement_times(changes.observation_mode, changes.observation_times),
                touch_state: changes.touch_state.unwrap_or(self.touch_state()),
            },
            changes.payout.unwrap_or(self.payout()),
        )?;
        self.monitoring.validate_replacement_history(
            &updated.monitoring,
            history_supplied,
            self.upper_barrier().to_bits() != updated.upper_barrier().to_bits(),
        )?;
        updated.with_history_time(changes.history_time.unwrap_or(self.history_time()))
    }
}
