use super::monitoring::Monitoring;
use crate::{
    BarrierTouchState, BarrierType, DigitalOption, Error, ObservationSchedule, OptionContract,
    OptionType, PricingContext, Result, Timestamp, TradingCalendar,
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

/// Supplies binary strike and monitoring inputs.
///
/// Validated by [`BinaryBarrierOption::cash`] or [`BinaryBarrierOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinaryBarrierTerms {
    /// Overrides the schedule calendar, or defaults to weekdays for continuous monitoring.
    pub calendar: Option<TradingCalendar>,
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike; terminal equality pays zero.
    pub strike: f64,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Positive finite trigger; equality touches.
    pub barrier_level: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: BarrierType,
    /// Supplies a nonempty observation schedule, or `None` for continuous monitoring.
    pub observation_schedule: Option<ObservationSchedule>,
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
            calendar: None,
            option_type,
            strike,
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            observation_schedule: None,
            touch_state: None,
        }
    }
    fn monitoring(self, times: Option<Vec<Timestamp>>) -> Result<Monitoring> {
        Monitoring::new(
            self.effective_time,
            self.expiry_time,
            self.barrier_level,
            self.barrier_type,
            super::monitoring::MonitoringInput::new(self.observation_schedule, times),
            self.touch_state,
            self.calendar,
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
    /// Returns [`Error::InvalidInput`] for invalid payout, strike, life,
    /// barrier, fixing times or calendar compatibility, checked in that order.
    /// History relative to valuation is checked during pricing.
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
    /// let terms = BinaryBarrierTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndOut);
    /// let option = BinaryBarrierOption::cash(terms, 10.0)?;
    /// assert_eq!(option.payout(), Some(10.0));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: BinaryBarrierTerms, payout: f64) -> Result<Self> {
        Self::cash_with_observation_times(terms, payout, None)
    }

    pub(crate) fn cash_with_observation_times(
        terms: BinaryBarrierTerms,
        payout: f64,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        let digital = DigitalOption::cash_or_nothing(
            terms.option_type,
            terms.strike,
            payout,
            terms.effective_time,
            terms.expiry_time,
        )?;
        Ok(Self {
            monitoring: terms.monitoring(times)?,
            digital,
        })
    }
    /// Creates an asset binary with the same monitoring and strict payoff as [`Self::cash`].
    ///
    /// # Errors
    ///
    /// Reuses [`Self::cash`] validation without a cash payout.
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
    /// let option = BinaryBarrierOption::asset(BinaryBarrierTerms::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day(), 80.0, BarrierType::DownAndIn))?;
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: BinaryBarrierTerms) -> Result<Self> {
        Self::asset_with_observation_times(terms, None)
    }

    pub(crate) fn asset_with_observation_times(
        terms: BinaryBarrierTerms,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        let digital = DigitalOption::asset_or_nothing(
            terms.option_type,
            terms.strike,
            terms.effective_time,
            terms.expiry_time,
        )?;
        Ok(Self {
            monitoring: terms.monitoring(times)?,
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
}

/// Strike-free touch inputs; validated by [`TouchOption::cash`] or [`TouchOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOptionTerms {
    /// Overrides the schedule calendar, or defaults to weekdays for continuous monitoring.
    pub calendar: Option<TradingCalendar>,
    /// Inclusive effective time.
    pub effective_time: Timestamp,
    /// Inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Positive finite barrier; equality touches.
    pub barrier_level: f64,
    /// In means one-touch, out means no-touch; up/down selects direction.
    pub barrier_type: BarrierType,
    /// Payment timing, default expiry; at-hit requires one-touch.
    pub settlement_timing: SettlementTiming,
    /// Supplies a nonempty observation schedule, or `None` for continuous monitoring.
    pub observation_schedule: Option<ObservationSchedule>,
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
            calendar: None,
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            settlement_timing: SettlementTiming::default(),
            observation_schedule: None,
            touch_state: None,
        }
    }
    fn monitoring(self, times: Option<Vec<Timestamp>>) -> Result<Monitoring> {
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
            super::monitoring::MonitoringInput::new(self.observation_schedule, times),
            self.touch_state,
            self.calendar,
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
    /// Returns [`Error::InvalidInput`] for invalid positive payout or barrier,
    /// reversed life, invalid fixings, or at-hit no-touch settlement.
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
    /// let option = TouchOption::cash(TouchOptionTerms::new(Date::new(2025, 1, 1)?.start_of_day(),
    ///     Date::new(2026, 1, 1)?.start_of_day(), 120.0, BarrierType::UpAndIn), 10.0)?;
    /// assert_eq!(option.barrier_type(), BarrierType::UpAndIn);
    /// assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: TouchOptionTerms, payout: f64) -> Result<Self> {
        Self::cash_with_observation_times(terms, payout, None)
    }

    pub(crate) fn cash_with_observation_times(
        terms: TouchOptionTerms,
        payout: f64,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        if !payout.is_finite() || payout <= 0.0 {
            return Err(Error::InvalidInput(
                "payout must be finite and positive".into(),
            ));
        }
        let settlement_timing = terms.settlement_timing;
        Ok(Self {
            monitoring: terms.monitoring(times)?,
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
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate when checking a calendar override.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::asset(TouchOptionTerms::new(Date::new(2025, 1, 1)?.start_of_day(),
    ///     Date::new(2026, 1, 1)?.start_of_day(), 80.0, BarrierType::DownAndOut))?;
    /// assert_eq!(option.barrier_type(), kiyosi::BarrierType::DownAndOut);
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: TouchOptionTerms) -> Result<Self> {
        Self::asset_with_observation_times(terms, None)
    }

    pub(crate) fn asset_with_observation_times(
        terms: TouchOptionTerms,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        let settlement_timing = terms.settlement_timing;
        Ok(Self {
            monitoring: terms.monitoring(times)?,
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
}

macro_rules! monitoring_accessors {
    ($contract:ty) => {
        impl $contract {
            /// Borrows the contract's observation calendar; see [`Self::replace`].
            pub fn calendar(&self) -> &TradingCalendar {
                self.monitoring.calendar()
            }
            /// Returns the inclusive effective time; see [`Self::cash`].
            pub fn effective_time(&self) -> Timestamp {
                self.monitoring.effective_time()
            }
            /// Returns the inclusive expiry timestamp; see [`Self::cash`].
            pub fn expiry_time(&self) -> Timestamp {
                self.monitoring.expiry_time()
            }
            /// Returns the positive barrier; see [`Self::cash`].
            pub fn barrier_level(&self) -> f64 {
                self.monitoring.barrier_level()
            }
            /// Returns direction and activation behavior; see [`Self::cash`].
            pub fn barrier_type(&self) -> BarrierType {
                self.monitoring.barrier_type()
            }
            /// Borrows scheduled observations, or returns `None` for continuous monitoring.
            ///
            /// See [`Self::cash`] for construction.
            pub fn observation_schedule(&self) -> Option<&ObservationSchedule> {
                (!self.monitoring.continuous()).then(|| self.monitoring.observation_schedule())
            }
            /// Borrows scheduled times, or returns `None` for continuous monitoring.
            ///
            /// See [`Self::observation_schedule`] for access to the schedule with its calendar.
            /// Returns strictly prior touch history; see [`Self::replace`].
            pub fn touch_state(&self) -> Option<BarrierTouchState> {
                self.monitoring.touch_state()
            }
        }
        impl OptionContract for $contract {
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
                    || self
                        .monitoring
                        .live_payoff_discontinuity(context, radius, None, || {
                            self.contract().digital.is_some_and(|digital| {
                                digital.at_spot_discontinuity(context, radius)
                            })
                        })
            }
            fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
                self.monitoring
                    .all_greeks_unavailable(context, None, self.contract().timing)
                    || self
                        .monitoring
                        .live_payoff_discontinuity(context, 0.0, None, || {
                            self.contract()
                                .digital
                                .is_some_and(|digital| digital.all_greeks_unavailable(context))
                        })
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

pub(crate) enum Settlement {
    Pending,
    Value(f64),
    AtExpiry,
}

impl Contract<'_> {
    pub(crate) fn settlement(&self, context: &PricingContext) -> Result<Settlement> {
        Ok(self.settlement_with_state(self.state(context)?, context.spot_price()))
    }

    pub(crate) fn settlement_with_state(
        &self,
        (prior, touched, finished): (bool, bool, bool),
        spot: f64,
    ) -> Settlement {
        let at_hit = self.timing == SettlementTiming::AtHit;
        if prior && at_hit {
            return Settlement::Value(0.0);
        }
        if !touched && !finished {
            return Settlement::Pending;
        }
        if touched != self.monitoring.barrier_type().is_knock_in() {
            Settlement::Value(0.0)
        } else if at_hit {
            Settlement::Value(self.payout.unwrap_or(spot))
        } else {
            Settlement::AtExpiry
        }
    }

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
    pub(crate) fn spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.monitoring
            .spot_discontinuity(context, radius, self.upper_barrier, self.timing)
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

/// Selects replacements for [`BinaryBarrierOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct BinaryBarrierOptionChanges {
    /// Replaces the owned observation calendar; see [`BinaryBarrierOption::replace`].
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
    /// Replaces or clears [`observation_schedule`](BinaryBarrierTerms::observation_schedule).
    pub observation_schedule: Option<Option<ObservationSchedule>>,
    /// Replaces [`touch_state`](BinaryBarrierTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
    /// Replaces settlement explicitly with cash or the asset.
    /// See [`BinaryBarrierOption::replace`].
    pub payoff: Option<BinaryPayoff>,
}

impl BinaryBarrierOption {
    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Set `observation_schedule` to `Some(None)` for continuous monitoring.
    /// A supplied schedule provides its calendar unless `calendar` overrides it.
    /// Clearing or omitting the schedule preserves the calendar unless explicitly overridden.
    /// See [`BinaryBarrierOption::cash`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. [`BinaryPayoff`] selects settlement explicitly.
    /// See [`Self::cash`] for a construction example.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate when checking a calendar override.
    pub fn replace(&self, changes: BinaryBarrierOptionChanges) -> Result<Self> {
        self.replace_with_observation_times(changes, None)
    }

    pub(crate) fn replace_with_observation_times(
        &self,
        changes: BinaryBarrierOptionChanges,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        let replacement = if times.is_some() {
            Some(None)
        } else {
            changes.observation_schedule
        };
        let (observation_schedule, calendar) = super::monitoring::replace_schedule(
            self.observation_schedule(),
            self.calendar(),
            replacement,
            changes.calendar,
        );
        let terms = BinaryBarrierTerms {
            calendar,
            option_type: changes.option_type.unwrap_or(self.option_type()),
            strike: changes.strike.unwrap_or(self.strike()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            barrier_level: changes.barrier_level.unwrap_or(self.barrier_level()),
            barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
            observation_schedule,
            touch_state: changes.touch_state.unwrap_or(self.touch_state()),
        };
        match changes.payoff.unwrap_or_else(|| {
            self.payout()
                .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash)
        }) {
            BinaryPayoff::Cash(payout) => Self::cash_with_observation_times(terms, payout, times),
            BinaryPayoff::Asset => Self::asset_with_observation_times(terms, times),
        }
    }
}

/// Selects replacements for [`TouchOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct TouchOptionChanges {
    /// Replaces the owned observation calendar; see [`TouchOption::replace`].
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
    /// Replaces or clears [`observation_schedule`](TouchOptionTerms::observation_schedule).
    pub observation_schedule: Option<Option<ObservationSchedule>>,
    /// Replaces [`touch_state`](TouchOptionTerms::touch_state).
    pub touch_state: Option<Option<BarrierTouchState>>,
    /// Replaces settlement explicitly with cash or the asset.
    /// See [`TouchOption::replace`].
    pub payoff: Option<BinaryPayoff>,
}

impl TouchOption {
    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Set `observation_schedule` to `Some(None)` for continuous monitoring.
    /// A supplied schedule provides its calendar unless `calendar` overrides it.
    /// Clearing or omitting the schedule preserves the calendar unless explicitly overridden.
    /// See [`TouchOption::cash`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. [`BinaryPayoff`] selects settlement explicitly.
    /// See [`Self::cash`] for a construction example.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate when checking a calendar override.
    pub fn replace(&self, changes: TouchOptionChanges) -> Result<Self> {
        self.replace_with_observation_times(changes, None)
    }

    pub(crate) fn replace_with_observation_times(
        &self,
        changes: TouchOptionChanges,
        times: Option<Vec<Timestamp>>,
    ) -> Result<Self> {
        let replacement = if times.is_some() {
            Some(None)
        } else {
            changes.observation_schedule
        };
        let (observation_schedule, calendar) = super::monitoring::replace_schedule(
            self.observation_schedule(),
            self.calendar(),
            replacement,
            changes.calendar,
        );
        let terms = TouchOptionTerms {
            calendar,
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            barrier_level: changes.barrier_level.unwrap_or(self.barrier_level()),
            barrier_type: changes.barrier_type.unwrap_or(self.barrier_type()),
            settlement_timing: changes
                .settlement_timing
                .unwrap_or(self.settlement_timing()),
            observation_schedule,
            touch_state: changes.touch_state.unwrap_or(self.touch_state()),
        };
        match changes.payoff.unwrap_or_else(|| {
            self.payout()
                .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash)
        }) {
            BinaryPayoff::Cash(payout) => Self::cash_with_observation_times(terms, payout, times),
            BinaryPayoff::Asset => Self::asset_with_observation_times(terms, times),
        }
    }
}
