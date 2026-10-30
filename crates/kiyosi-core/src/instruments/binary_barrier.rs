use crate::{
    BarrierOption, BarrierOptionTerms, BarrierTouchState, BarrierType, Date, DigitalOption, Error,
    ObservationMode, ObservationSchedule, OptionContract, OptionType, PricingContext, Result,
    Timestamp,
};

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
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike; terminal equality pays zero.
    pub strike: f64,
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite trigger; equality touches.
    pub barrier_level: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: BarrierType,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing dates, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
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
            observation_mode: ObservationMode::default(),
            observation_dates: Vec::new(),
            touch_state: None,
        }
    }
    fn monitoring(self) -> Result<BarrierOption> {
        let mut terms = BarrierOptionTerms::new(
            self.option_type,
            self.strike,
            self.effective_date,
            self.expiry_date,
            self.barrier_level,
            self.barrier_type,
        );
        terms.observation_mode = self.observation_mode;
        terms.observation_dates = self.observation_dates;
        terms.touch_state = self.touch_state;
        BarrierOption::new(terms)
    }
}

/// Owns a validated strike-based binary barrier paying cash or the asset at expiry.
///
/// Shared reads and pricing are safe; replacement returns an independent owner.
/// See [`Self::cash`] and [`Self::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinaryBarrierOption {
    monitoring: BarrierOption,
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
    /// barrier or fixing dates, checked in that order. History relative to valuation
    /// and calendar compatibility are checked during pricing.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let terms = BinaryBarrierTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut);
    /// let option = BinaryBarrierOption::cash(terms, 10.0)?;
    /// assert_eq!(option.payout(), Some(10.0));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: BinaryBarrierTerms, payout: f64) -> Result<Self> {
        let digital = DigitalOption::cash_or_nothing(
            terms.option_type,
            terms.strike,
            payout,
            terms.effective_date,
            terms.expiry_date,
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
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = BinaryBarrierOption::asset(BinaryBarrierTerms::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 80.0, BarrierType::DownAndIn))?;
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: BinaryBarrierTerms) -> Result<Self> {
        let digital = DigitalOption::asset_or_nothing(
            terms.option_type,
            terms.strike,
            terms.effective_date,
            terms.expiry_date,
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
    /// See [`BarrierOption::replace`] for an example and time consistency rules.
    ///
    /// Preserves the validated monitoring and payoff terms.
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            monitoring: self.monitoring.replace(touch_state)?,
            digital: self.digital.clone(),
        })
    }
}

/// Strike-free touch inputs; validated by [`TouchOption::cash`] or [`TouchOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOptionTerms {
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite barrier; equality touches.
    pub barrier_level: f64,
    /// In means one-touch, out means no-touch; up/down selects direction.
    pub barrier_type: BarrierType,
    /// Payment timing, default expiry; at-hit requires one-touch.
    pub settlement_timing: SettlementTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing dates, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl TouchOptionTerms {
    /// Supplies expiry payment, continuous monitoring and unknown history without validation.
    ///
    /// See [`TouchOption::cash`] for validation and an example.
    pub fn new(
        effective_date: Date,
        expiry_date: Date,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            effective_date,
            expiry_date,
            barrier_level,
            barrier_type,
            settlement_timing: SettlementTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_dates: Vec::new(),
            touch_state: None,
        }
    }
    fn monitoring(self) -> Result<BarrierOption> {
        if self.settlement_timing == SettlementTiming::AtHit && !self.barrier_type.is_knock_in() {
            return Err(Error::InvalidInput(
                "at-hit settlement requires a one-touch".into(),
            ));
        }
        // Reuse the validated barrier's monitoring rules; no strike is exposed or used for touch payoffs.
        let mut terms = BarrierOptionTerms::new(
            OptionType::Call,
            self.barrier_level,
            self.effective_date,
            self.expiry_date,
            self.barrier_level,
            self.barrier_type,
        );
        terms.observation_mode = self.observation_mode;
        terms.observation_dates = self.observation_dates;
        terms.touch_state = self.touch_state;
        BarrierOption::new(terms)
    }
}

/// Owns a validated cash/asset one-touch or no-touch contract, with no strike.
///
/// Prior at-hit touches are already paid; shared reads and pricing are safe.
/// See [`Self::cash`] and [`Self::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOption {
    monitoring: BarrierOption,
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
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::cash(TouchOptionTerms::new(Date::new(2025, 1, 1)?,
    ///     Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndIn), 10.0)?;
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
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::asset(TouchOptionTerms::new(Date::new(2025, 1, 1)?,
    ///     Date::new(2026, 1, 1)?, 80.0, BarrierType::DownAndOut))?;
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
    /// See [`BarrierOption::replace`] for examples and time consistency rules.
    ///
    /// Preserves the validated monitoring and payoff terms.
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            monitoring: self.monitoring.replace(touch_state)?,
            payout: self.payout,
            settlement_timing: self.settlement_timing,
        })
    }
}

macro_rules! monitoring_accessors {
    ($contract:ty) => {
        impl $contract {
            /// Returns the inclusive first date; see the cash constructor.
            pub fn effective_date(&self) -> Date {
                self.monitoring.effective_date()
            }
            /// Returns expiry at midnight UTC; see the cash constructor.
            pub fn expiry_date(&self) -> Date {
                self.monitoring.expiry_date()
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
            /// Borrows ordered dates for this owner's lifetime; see the cash constructor.
            pub fn observation_dates(&self) -> &[Date] {
                self.monitoring.observation_dates()
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
    };
}
monitoring_accessors!(BinaryBarrierOption);
monitoring_accessors!(TouchOption);

pub(crate) struct Contract<'a> {
    pub(crate) monitoring: &'a BarrierOption,
    pub(crate) double: Option<&'a crate::DoubleBarrierOption>,
    pub(crate) digital: Option<&'a DigitalOption>,
    pub(crate) payout: Option<f64>,
    pub(crate) timing: SettlementTiming,
}
impl Contract<'_> {
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        if let Some(double) = self.double {
            double.state(context)
        } else {
            self.monitoring.state(context)
        }
    }
}

impl BinaryBarrierOption {
    pub(crate) fn contract(&self) -> Contract<'_> {
        Contract {
            monitoring: &self.monitoring,
            double: None,
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
            double: None,
            digital: None,
            payout: self.payout,
            timing: self.settlement_timing,
        }
    }
}
