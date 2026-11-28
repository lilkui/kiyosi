use super::monitoring::{Monitoring, MonitoringInput, replace_schedule, validate_upper};
use crate::{
    BarrierTouchState, BarrierType, DoubleBarrierType, Error, ObservationMode, ObservationSchedule,
    OptionContract, PricingContext, Result, SettlementTiming, Timestamp, TradingCalendar,
};

/// Cash double-touch inputs; see [`DoubleTouchOption::cash`] for validation and usage.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleTouchOptionTerms {
    /// Overrides the schedule calendar, or defaults to weekdays for continuous monitoring.
    pub calendar: Option<TradingCalendar>,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Positive finite lower boundary; equality touches.
    pub lower_barrier: f64,
    /// Finite upper boundary, strictly above the lower boundary.
    pub upper_barrier: f64,
    /// Knock-in means one-touch; knock-out means no-touch.
    pub barrier_type: DoubleBarrierType,
    /// Payment timing, default expiry; at-hit requires one-touch.
    pub settlement_timing: SettlementTiming,
    /// Supplies a nonempty observation schedule, or `None` for continuous monitoring.
    pub observation_schedule: Option<ObservationSchedule>,
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
            calendar: None,
            effective_time,
            expiry_time,
            lower_barrier,
            upper_barrier,
            barrier_type,
            settlement_timing: SettlementTiming::default(),
            observation_schedule: None,
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
    /// Borrows the contract's observation calendar; see [`Self::replace`].
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
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate when checking a calendar override.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DoubleTouchOption::cash(DoubleTouchOptionTerms::new(start,
    ///     Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0, DoubleBarrierType::KnockIn), 10.0)?;
    /// assert_eq!(option.barrier_type(), DoubleBarrierType::KnockIn);
    /// assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.03, 0.01, 0.2)?,
    ///     120.0, start.into())?;
    /// let engine = FiniteDifferenceDoubleTouchEngine::default();
    /// assert!((engine.price(&option, &context)? - 10.0 * (-0.03_f64).exp()).abs() < 1e-12);
    /// let known = option.replace(DoubleTouchOptionChanges { touch_state: Some(Some(BarrierTouchState::Untouched)), ..Default::default() })?;
    /// assert_eq!(option.touch_state(), None);
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: DoubleTouchOptionTerms, payout: f64) -> Result<Self> {
        Self::cash_with_observation_times(terms, payout, None)
    }

    pub(crate) fn cash_with_observation_times(
        terms: DoubleTouchOptionTerms,
        payout: f64,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
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
            MonitoringInput::new(terms.observation_schedule, times),
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
    pub(crate) fn is_one_touch(&self) -> bool {
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
    pub(crate) fn observation_mode(&self) -> ObservationMode {
        self.monitoring.observation_mode()
    }
    /// Borrows the scheduled observations, or returns `None` for continuous monitoring.
    ///
    /// See [`Self::cash`] for construction.
    pub fn observation_schedule(&self) -> Option<&ObservationSchedule> {
        (!self.monitoring.continuous()).then(|| self.monitoring.observation_schedule())
    }
    /// Borrows scheduled times, or returns `None` for continuous monitoring.
    ///
    /// See [`Self::observation_schedule`] for access to the schedule with its calendar.
    /// Returns strictly prior history; see [`Self::cash`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.monitoring.touch_state()
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
    fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        self.contract().volatility_affects_price(context)
    }
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.contract().spot_discontinuity(context, radius)
    }
    fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
        self.monitoring.all_greeks_unavailable(
            context,
            Some(self.upper_barrier),
            self.settlement_timing(),
        )
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

/// Selects replacements for [`DoubleTouchOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct DoubleTouchOptionChanges {
    /// Replaces the owned observation calendar; see [`DoubleTouchOption::replace`].
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
    /// Replaces or clears [`observation_schedule`](DoubleTouchOptionTerms::observation_schedule).
    pub observation_schedule: Option<Option<ObservationSchedule>>,
    /// Replaces [`touch_state`](DoubleTouchOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
    /// Replaces [`payout`](DoubleTouchOption::payout).
    pub payout: Option<f64>,
}

impl DoubleTouchOption {
    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Set `observation_schedule` to `Some(None)` for continuous monitoring.
    /// A supplied schedule provides its calendar unless `calendar` overrides it.
    /// Clearing or omitting the schedule preserves the calendar unless explicitly overridden.
    /// See [`DoubleTouchOption::cash`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate when checking a calendar override.
    pub fn replace(&self, changes: DoubleTouchOptionChanges) -> Result<Self> {
        self.replace_with_observation_times(changes, None)
    }

    pub(crate) fn replace_with_observation_times(
        &self,
        changes: DoubleTouchOptionChanges,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        let replacement = if times.is_some() {
            Some(None)
        } else {
            changes.observation_schedule
        };
        let (observation_schedule, calendar) = replace_schedule(
            self.observation_schedule(),
            self.calendar(),
            replacement,
            changes.calendar,
        );
        let updated = Self::cash_with_observation_times(
            DoubleTouchOptionTerms {
                calendar,
                effective_time: changes.effective_time.unwrap_or(self.effective_time()),
                expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
                lower_barrier: changes.lower_barrier.unwrap_or(self.lower_barrier()),
                upper_barrier: changes.upper_barrier.unwrap_or(self.upper_barrier()),
                barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
                settlement_timing: changes
                    .settlement_timing
                    .unwrap_or(self.settlement_timing()),
                observation_schedule,
                touch_state: changes.touch_state.unwrap_or(self.touch_state()),
            },
            changes.payout.unwrap_or(self.payout()),
            times,
        )?;

        Ok(updated)
    }
}
