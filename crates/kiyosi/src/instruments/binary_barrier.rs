use super::monitoring::Monitoring;
use crate::{
    BarrierTouchState, BarrierType, DigitalOption, Error, ObservationMode, ObservationSchedule,
    OptionContract, OptionType, PricingContext, Result, Timestamp, TradingCalendar,
};

/// Selects explicit cash or asset settlement for binary and touch replacements.
///
/// # Examples
///
/// ```rust
/// use kiyosi::{BinaryBarrierOptionChanges, BinaryPayoff};
/// let changes = BinaryBarrierOptionChanges {
///     payoff: Some(BinaryPayoff::Asset), ..Default::default()
/// };
/// assert_eq!(changes.payoff, Some(BinaryPayoff::Asset));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinaryPayoff {
    /// Pays a fixed positive finite cash amount, validated by the contract.
    Cash(f64),
    /// Pays the asset according to the contract's settlement timing.
    Asset,
}

/// Payment timing for a touch contract; see [`TouchOption::cash`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettlementTiming {
    /// Pays on the first touch; available only for one-touch contracts.
    AtHit,
    /// Pays at expiry (the default).
    #[default]
    AtExpiry,
}

/// Binary strike and monitoring inputs; validated by [`BinaryBarrierOption::cash`] or
///
/// [`BinaryBarrierOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinaryBarrierTerms {
    /// Dates supplied history; required when any history payload is known.
    ///
    /// See the [crate-level history example](crate).
    pub history_time: Option<Timestamp>,
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike; terminal equality pays zero.
    pub strike: f64,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Positive finite trigger; equality touches.
    pub barrier_level: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: BarrierType,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing times, empty for continuous monitoring.
    pub observation_times: Vec<Timestamp>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl BinaryBarrierTerms {
    /// Supplies continuous monitoring and unknown history without validation.
    ///
    /// See [`BinaryBarrierOption::cash`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            history_time: None,
            calendar: TradingCalendar::default(),
            option_type,
            strike,
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            observation_mode: ObservationMode::default(),
            observation_times: Vec::new(),
            touch_state: None,
        }
    }
    fn monitoring(self) -> Result<Monitoring> {
        Monitoring::new(
            self.effective_time,
            self.expiry_time,
            self.barrier_level,
            self.barrier_type,
            self.observation_mode,
            self.observation_times,
            self.touch_state,
            self.calendar,
            self.history_time,
        )
    }
}

/// Owns a validated strike-based binary barrier paying cash or the asset at expiry.
///
/// Shared reads and pricing are safe; replacement returns an independent owner.
/// See [`Self::cash`] and [`Self::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinaryBarrierOption {
    monitoring: Monitoring,
    digital: DigitalOption,
}
impl BinaryBarrierOption {
    /// Creates a positive-cash binary contract, paying strictly in the money if active.
    ///
    /// Validates payout, strike and life before barrier and schedule terms.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for supplied history without a timestamp,
    /// or a timestamp outside the contract life.
    ///
    /// Returns [`Error::InvalidInput`] for invalid payout, strike, life,
    /// barrier, fixing times or calendar compatibility, checked in that order.
    /// Supplied history requires an in-life timestamp on construction.
    /// History relative to valuation is checked during pricing.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let terms = BinaryBarrierTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut);
    /// let option = BinaryBarrierOption::cash(terms, 10.0)?;
    /// assert_eq!(option.payout(), Some(10.0));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: BinaryBarrierTerms, payout: f64) -> Result<Self> {
        let digital = DigitalOption::cash_or_nothing(
            terms.option_type,
            terms.strike,
            payout,
            terms.effective_time,
            terms.expiry_time,
        )?;
        Ok(Self {
            monitoring: terms.monitoring()?,
            digital,
        })
    }
    /// Creates an asset binary with the same monitoring and strict payoff as [`Self::cash`].
    ///
    /// # Errors
    ///
    /// Reuses [`Self::cash`] validation without a cash payout.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = BinaryBarrierOption::asset(BinaryBarrierTerms::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 80.0, BarrierType::DownAndIn))?;
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: BinaryBarrierTerms) -> Result<Self> {
        let digital = DigitalOption::asset_or_nothing(
            terms.option_type,
            terms.strike,
            terms.effective_time,
            terms.expiry_time,
        )?;
        Ok(Self {
            monitoring: terms.monitoring()?,
            digital,
        })
    }
    /// Returns the call-or-put direction; see [`Self::cash`].
    pub fn option_type(&self) -> OptionType {
        self.digital.option_type()
    }
    /// Returns the positive strike; see [`Self::cash`].
    pub fn strike(&self) -> f64 {
        self.digital.strike()
    }
    /// Returns cash payout, or `None` for asset settlement; see [`Self::cash`].
    pub fn payout(&self) -> Option<f64> {
        self.digital.payout()
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    ///
    /// See [`crate::BarrierOption::replace_history`] for an example and time consistency rules.
    ///
    /// Preserves the validated monitoring and payoff terms.
    /// Replaces the payload and snapshot timestamp together; `None` clears the timestamp.
    ///
    /// The supplied timestamp is validated and retained, including with unknown history.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a timestamp outside the contract life.
    pub fn replace_history(
        &self,
        touch_state: Option<BarrierTouchState>,
        history_time: Option<Timestamp>,
    ) -> Result<Self> {
        self.replace(BinaryBarrierOptionChanges {
            touch_state: Some(touch_state),
            history_time: Some(history_time),
            ..Default::default()
        })
    }
}

/// Strike-free touch inputs; validated by [`TouchOption::cash`] or [`TouchOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOptionTerms {
    /// Dates supplied history; required when any history payload is known.
    ///
    /// See the [crate-level history example](crate).
    pub history_time: Option<Timestamp>,
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Positive finite barrier; equality touches.
    pub barrier_level: f64,
    /// In means one-touch, out means no-touch; up/down selects direction.
    pub barrier_type: BarrierType,
    /// Payment timing, default expiry; at-hit requires one-touch.
    pub settlement_timing: SettlementTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing times, empty for continuous monitoring.
    pub observation_times: Vec<Timestamp>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl TouchOptionTerms {
    /// Supplies expiry payment, continuous monitoring and unknown history without validation.
    ///
    /// See [`TouchOption::cash`] for validation and an example.
    pub fn new(
        effective_time: Timestamp,
        expiry_time: Timestamp,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            history_time: None,
            calendar: TradingCalendar::default(),
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            settlement_timing: SettlementTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_times: Vec::new(),
            touch_state: None,
        }
    }
    fn monitoring(self) -> Result<Monitoring> {
        if self.settlement_timing == SettlementTiming::AtHit && !self.barrier_type.is_knock_in() {
            return Err(Error::InvalidInput(
                "at-hit settlement requires a one-touch".into(),
            ));
        }
        Monitoring::new(
            self.effective_time,
            self.expiry_time,
            self.barrier_level,
            self.barrier_type,
            self.observation_mode,
            self.observation_times,
            self.touch_state,
            self.calendar,
            self.history_time,
        )
    }
}

/// Owns a validated cash/asset one-touch or no-touch contract, with no strike.
///
/// Prior at-hit touches are already paid; shared reads and pricing are safe.
/// See [`Self::cash`] and [`Self::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOption {
    monitoring: Monitoring,
    payout: Option<f64>,
    settlement_timing: SettlementTiming,
}
impl TouchOption {
    /// Creates a cash touch contract; one-touch pays on a hit, no-touch on its absence.
    ///
    /// Current at-hit cash pays the payout; prior at-hit settlement has zero remaining value.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for supplied history without a timestamp,
    /// or a timestamp outside the contract life.
    ///
    /// Returns [`Error::InvalidInput`] for invalid positive payout or barrier,
    /// reversed life, invalid fixings, or at-hit no-touch settlement.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::cash(TouchOptionTerms::new(Date::new(2025, 1, 1)?.start_of_day(),
    ///     Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndIn), 10.0)?;
    /// assert!(option.is_one_touch());
    /// assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: TouchOptionTerms, payout: f64) -> Result<Self> {
        if !payout.is_finite() || payout <= 0.0 {
            return Err(Error::InvalidInput(
                "payout must be finite and positive".into(),
            ));
        }
        let settlement_timing = terms.settlement_timing;
        Ok(Self {
            monitoring: terms.monitoring()?,
            payout: Some(payout),
            settlement_timing,
        })
    }
    /// Creates an asset touch contract with [`Self::cash`]'s monitoring rules.
    ///
    /// A current at-hit touch pays spot; future continuous hits transfer the barrier amount.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::cash`] validation without a cash payout.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::asset(TouchOptionTerms::new(Date::new(2025, 1, 1)?.start_of_day(),
    ///     Date::new(2026, 1, 1)?.start_of_day(), 80.0, BarrierType::DownAndOut))?;
    /// assert!(!option.is_one_touch());
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: TouchOptionTerms) -> Result<Self> {
        let settlement_timing = terms.settlement_timing;
        Ok(Self {
            monitoring: terms.monitoring()?,
            payout: None,
            settlement_timing,
        })
    }
    /// Returns cash payout, or `None` for asset settlement; see [`Self::cash`].
    pub fn payout(&self) -> Option<f64> {
        self.payout
    }
    /// Returns payment timing; see [`Self::cash`].
    pub fn settlement_timing(&self) -> SettlementTiming {
        self.settlement_timing
    }
    /// Reports whether a touch activates payment; see [`Self::cash`].
    pub fn is_one_touch(&self) -> bool {
        self.barrier_type().is_knock_in()
    }
    /// Reports whether the barrier triggers upward; see [`Self::cash`].
    pub fn is_up(&self) -> bool {
        self.barrier_type().is_up()
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    ///
    /// See [`crate::BarrierOption::replace_history`] for examples and time consistency rules.
    ///
    /// Preserves the validated monitoring and payoff terms.
    /// Replaces the payload and snapshot timestamp together; `None` clears the timestamp.
    ///
    /// The supplied timestamp is validated and retained, including with unknown history.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a timestamp outside the contract life.
    pub fn replace_history(
        &self,
        touch_state: Option<BarrierTouchState>,
        history_time: Option<Timestamp>,
    ) -> Result<Self> {
        self.replace(TouchOptionChanges {
            touch_state: Some(touch_state),
            history_time: Some(history_time),
            ..Default::default()
        })
    }
}

macro_rules! monitoring_accessors {
    ($contract:ty) => {
        impl $contract {
            /// Replaces the owned observation calendar; see [`crate::BarrierOption::replace_calendar`].
            ///
            /// Amends an already validated contract; this is not an initial-construction builder.
            /// Set [`BinaryBarrierTerms::calendar`] or [`TouchOptionTerms::calendar`]
            /// before calling the corresponding cash or asset constructor. Otherwise,
            /// default weekday validation can reject weekend observations before this method runs.
            /// The original contract remains unchanged on success and failure.
            /// See [`crate::RangeAccrualOption::replace_calendar`] for an initial-calendar example.
            ///
            /// # Errors
            ///
            /// Returns [`crate::Error::InvalidInput`] for scheduled observations on closed dates.
            pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
                let mut updated = self.clone();
                updated.monitoring = updated.monitoring.replace_calendar(calendar)?;
                updated.with_history_time(self.history_time())
            }
            /// Borrows the contract's observation calendar; see [`Self::replace_calendar`].
            pub fn calendar(&self) -> &TradingCalendar {
                self.monitoring.calendar()
            }
            /// Returns the inclusive effective time; see the cash constructor.
            pub fn effective_time(&self) -> Timestamp {
                self.monitoring.effective_time()
            }
            /// Returns the inclusive expiry timestamp; see the cash constructor.
            pub fn expiry_time(&self) -> Timestamp {
                self.monitoring.expiry_time()
            }
            /// Returns the positive barrier; see the cash constructor.
            pub fn barrier_level(&self) -> f64 {
                self.monitoring.barrier_level()
            }
            /// Returns direction and activation behavior; see the cash constructor.
            pub fn barrier_type(&self) -> BarrierType {
                self.monitoring.barrier_type()
            }
            /// Returns monitoring frequency; see the cash constructor.
            pub fn observation_mode(&self) -> ObservationMode {
                self.monitoring.observation_mode()
            }
            /// Borrows immutable schedule storage for this owner's lifetime; see the cash constructor.
            pub fn observation_schedule(&self) -> &ObservationSchedule {
                self.monitoring.observation_schedule()
            }
            /// Borrows ordered times for this owner's lifetime; see the cash constructor.
            pub fn observation_times(&self) -> &[Timestamp] {
                self.monitoring.observation_times()
            }
            /// Returns mean fixing spacing in Actual/365 Fixed years; see the cash constructor.
            pub fn mean_observation_year_fraction(&self) -> f64 {
                self.monitoring.mean_observation_year_fraction()
            }
            /// Returns strictly prior history; see the replace method.
            pub fn touch_state(&self) -> Option<BarrierTouchState> {
                self.monitoring.touch_state()
            }
        }
        impl OptionContract for $contract {
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
                self.monitoring.at_spot_discontinuity(context, radius)
                    || !self.monitoring.payoff_terminated(context, None)
                        && self
                            .contract()
                            .digital
                            .is_some_and(|digital| digital.at_spot_discontinuity(context, radius))
            }
            fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
                self.monitoring.all_greeks_unavailable(context, None)
                    || !self.monitoring.payoff_terminated(context, None)
                        && self
                            .contract()
                            .digital
                            .is_some_and(|digital| digital.all_greeks_unavailable(context))
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
    };
}
monitoring_accessors!(BinaryBarrierOption);
monitoring_accessors!(TouchOption);

pub(crate) struct Contract<'a> {
    pub(crate) monitoring: &'a Monitoring,
    pub(crate) upper_barrier: Option<f64>,
    pub(crate) digital: Option<&'a DigitalOption>,
    pub(crate) payout: Option<f64>,
    pub(crate) timing: SettlementTiming,
}
impl Contract<'_> {
    pub(crate) fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let Ok((_, touched, finished)) = self.state(context) else {
            return false;
        };
        let knock_in = self.monitoring.barrier_type().is_knock_in();
        if touched || finished {
            return self.digital.is_some() && knock_in == touched;
        }
        knock_in || !self.payoff_requires_touch()
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        self.monitoring.state(context, self.upper_barrier)
    }
    pub(crate) fn payoff_requires_touch(&self) -> bool {
        self.digital.is_some_and(|digital| {
            self.monitoring
                .payoff_requires_touch(digital.option_type(), digital.strike())
        })
    }
}

impl BinaryBarrierOption {
    pub(crate) fn contract(&self) -> Contract<'_> {
        Contract {
            monitoring: &self.monitoring,
            upper_barrier: None,
            digital: Some(&self.digital),
            payout: self.payout(),
            timing: SettlementTiming::AtExpiry,
        }
    }
}
impl TouchOption {
    pub(crate) fn contract(&self) -> Contract<'_> {
        Contract {
            monitoring: &self.monitoring,
            upper_barrier: None,
            digital: None,
            payout: self.payout,
            timing: self.settlement_timing,
        }
    }
}

/// Selects replacements for [`BinaryBarrierOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct BinaryBarrierOptionChanges {
    /// Replaces or clears the history timestamp; see [`OptionContract::with_history_time`].
    pub history_time: Option<Option<Timestamp>>,
    /// Replaces the owned observation calendar; see [`BinaryBarrierOption::replace_calendar`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`option_type`](BinaryBarrierTerms::option_type).
    pub option_type: Option<OptionType>,
    /// Replaces [`strike`](BinaryBarrierTerms::strike).
    pub strike: Option<f64>,
    /// Replaces [`effective_time`](BinaryBarrierTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](BinaryBarrierTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`barrier_level`](BinaryBarrierTerms::barrier_level).
    pub barrier_level: Option<f64>,
    /// Replaces [`barrier_type`](BinaryBarrierTerms::barrier_type).
    pub barrier_type: Option<BarrierType>,
    /// Replaces [`observation_mode`](BinaryBarrierTerms::observation_mode).
    pub observation_mode: Option<ObservationMode>,
    /// Replaces [`observation_times`](BinaryBarrierTerms::observation_times).
    pub observation_times: Option<Vec<Timestamp>>,
    /// Replaces [`touch_state`](BinaryBarrierTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
    /// Replaces settlement explicitly with cash or the asset.
    /// See [`BinaryBarrierOption::replace`].
    pub payoff: Option<BinaryPayoff>,
}

impl BinaryBarrierOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Switching to continuous monitoring requires explicitly empty observation times; omitted times are retained.
    /// See [`BinaryBarrierOption::cash`] for construction examples.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged. [`BinaryPayoff`] selects settlement explicitly.
    /// See [`Self::cash`] for a construction example.
    pub fn replace(&self, changes: BinaryBarrierOptionChanges) -> Result<Self> {
        let history_supplied = changes.touch_state.is_some();
        let terms = BinaryBarrierTerms {
            history_time: super::replacement_history_time(
                self.history_time(),
                changes.history_time,
                changes.touch_state.is_some()
                    && changes.touch_state.unwrap_or(self.touch_state()).is_none(),
            ),
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            option_type: changes.option_type.unwrap_or(self.option_type()),
            strike: changes.strike.unwrap_or(self.strike()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            barrier_level: changes.barrier_level.unwrap_or(self.barrier_level()),
            barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
            observation_mode: changes.observation_mode.unwrap_or(self.observation_mode()),
            observation_times: changes
                .observation_times
                .unwrap_or_else(|| self.observation_times().to_vec()),
            touch_state: changes.touch_state.unwrap_or(self.touch_state()),
        };
        let updated = match changes.payoff.unwrap_or_else(|| {
            self.payout()
                .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash)
        }) {
            BinaryPayoff::Cash(payout) => Self::cash(terms, payout),
            BinaryPayoff::Asset => Self::asset(terms),
        }?;
        self.monitoring.validate_replacement_history(
            &updated.monitoring,
            history_supplied,
            false,
        )?;
        Ok(updated)
    }
}

/// Selects replacements for [`TouchOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct TouchOptionChanges {
    /// Replaces or clears the history timestamp; see [`OptionContract::with_history_time`].
    pub history_time: Option<Option<Timestamp>>,
    /// Replaces the owned observation calendar; see [`TouchOption::replace_calendar`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`effective_time`](TouchOptionTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](TouchOptionTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`barrier_level`](TouchOptionTerms::barrier_level).
    pub barrier_level: Option<f64>,
    /// Replaces [`barrier_type`](TouchOptionTerms::barrier_type).
    pub barrier_type: Option<BarrierType>,
    /// Replaces [`settlement_timing`](TouchOptionTerms::settlement_timing).
    pub settlement_timing: Option<SettlementTiming>,
    /// Replaces [`observation_mode`](TouchOptionTerms::observation_mode).
    pub observation_mode: Option<ObservationMode>,
    /// Replaces [`observation_times`](TouchOptionTerms::observation_times).
    pub observation_times: Option<Vec<Timestamp>>,
    /// Replaces [`touch_state`](TouchOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
    /// Replaces settlement explicitly with cash or the asset.
    /// See [`TouchOption::replace`].
    pub payoff: Option<BinaryPayoff>,
}

impl TouchOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Switching to continuous monitoring requires explicitly empty observation times; omitted times are retained.
    /// See [`TouchOption::cash`] for construction examples.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged. [`BinaryPayoff`] selects settlement explicitly.
    /// See [`Self::cash`] for a construction example.
    pub fn replace(&self, changes: TouchOptionChanges) -> Result<Self> {
        let history_supplied = changes.touch_state.is_some();
        let terms = TouchOptionTerms {
            history_time: super::replacement_history_time(
                self.history_time(),
                changes.history_time,
                changes.touch_state.is_some()
                    && changes.touch_state.unwrap_or(self.touch_state()).is_none(),
            ),
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            barrier_level: changes.barrier_level.unwrap_or(self.barrier_level()),
            barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
            settlement_timing: changes
                .settlement_timing
                .unwrap_or(self.settlement_timing()),
            observation_mode: changes.observation_mode.unwrap_or(self.observation_mode()),
            observation_times: changes
                .observation_times
                .unwrap_or_else(|| self.observation_times().to_vec()),
            touch_state: changes.touch_state.unwrap_or(self.touch_state()),
        };
        let updated = match changes.payoff.unwrap_or_else(|| {
            self.payout()
                .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash)
        }) {
            BinaryPayoff::Cash(payout) => Self::cash(terms, payout),
            BinaryPayoff::Asset => Self::asset(terms),
        }?;
        self.monitoring.validate_replacement_history(
            &updated.monitoring,
            history_supplied,
            false,
        )?;
        Ok(updated)
    }
}
