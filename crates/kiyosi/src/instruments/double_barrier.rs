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
    /// Inclusive expiry timestamp.
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
    /// Ordered nonempty scheduled times, or `None` for continuous monitoring.
    pub observation_times: Option<Vec<Timestamp>>,
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
            observation_times: None,
            touch_state: None,
        }
    }
}

/// Owns two ordered boundaries sharing monitoring and strictly prior history.
///
/// Shared pricing and replacement are safe under the crate concurrency contract.
/// See [`Self::new`] and [`Self::replace`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleBarrierOption {
    pub(crate) lower: BarrierOption,
    upper: f64,
}
impl DoubleBarrierOption {
    /// Borrows the contract's observation calendar; see [`Self::replace`].
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
    /// then reuses [`BarrierOption::new`] categories and validation order.
    /// At-hit knock-in rebates are invalid even when zero.
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
        lower.observation_times = terms.observation_times;
        lower.touch_state = terms.touch_state;
        Ok(Self {
            lower: BarrierOption::new(lower)?,
            upper: terms.upper_barrier,
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
    /// Borrows the scheduled observations, or returns `None` for continuous monitoring.
    ///
    /// Storage remains valid for the borrow; see [`Self::new`] for construction.
    pub fn observation_schedule(&self) -> Option<&ObservationSchedule> {
        self.lower.observation_schedule()
    }
    /// Borrows scheduled times, or returns `None` for continuous monitoring.
    ///
    /// Storage remains valid for this borrow; see [`Self::observation_schedule`].
    pub fn observation_times(&self) -> Option<&[Timestamp]> {
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
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        (self.observation_mode() == ObservationMode::Continuous
            || self.observation_times().and_then(|times| times.last()) == Some(&self.expiry_time()))
            && match self.option_type() {
                OptionType::Call => self.strike() >= self.upper,
                OptionType::Put => self.strike() <= self.lower_barrier(),
            }
    }
}
impl OptionContract for DoubleBarrierOption {
    fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let Ok((_, touched, finished)) = self.state(context) else {
            return false;
        };
        !(touched && !self.knock_in()
            || !touched && finished && self.knock_in()
            || !self.knock_in() && self.payoff_requires_touch() && self.rebate() == 0.0)
    }
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.lower
            .monitoring
            .spot_discontinuity(context, radius, Some(self.upper))
            || !self
                .lower
                .monitoring
                .payoff_terminated(context, Some(self.upper))
                && self.vanilla().at_spot_discontinuity(context, radius)
    }
    fn time_stencil_available(
        &self,
        context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        self.lower.time_stencil_available(context, before, after)
    }
    fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
        self.lower
            .monitoring
            .all_greeks_unavailable(context, Some(self.upper))
            || !self
                .lower
                .monitoring
                .payoff_terminated(context, Some(self.upper))
                && self.vanilla().all_greeks_unavailable(context)
    }
}

/// Selects replacements for [`DoubleBarrierOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct DoubleBarrierOptionChanges {
    /// Replaces the owned observation calendar; see [`DoubleBarrierOption::replace`].
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
    /// Replaces or clears [`observation_times`](DoubleBarrierOptionTerms::observation_times).
    pub observation_times: Option<Option<Vec<Timestamp>>>,
    /// Replaces [`touch_state`](DoubleBarrierOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
}

impl DoubleBarrierOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Set `observation_times` to `Some(None)` for continuous monitoring; omitted times are retained.
    /// See [`DoubleBarrierOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: DoubleBarrierOptionChanges) -> Result<Self> {
        let updated = Self::new(DoubleBarrierOptionTerms {
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
            observation_times: changes
                .observation_times
                .unwrap_or_else(|| self.observation_times().map(<[Timestamp]>::to_vec)),
            touch_state: changes.touch_state.unwrap_or(self.touch_state()),
        })?;

        Ok(updated)
    }
}
