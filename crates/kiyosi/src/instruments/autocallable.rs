mod presets;
pub(crate) mod pricing;
pub use presets::{PhoenixPreset, PhoenixPresetTerms, SnowballPreset, SnowballPresetTerms};

use crate::error::finite;
use crate::{
    Error, PricingContext, Result, TimeOfDay, Timestamp, TradingCalendar,
    instruments::OptionContract,
};

/// Supplies unit principal redemption for autocallable terms; see [`SnowballTerms::new`].
pub const DEFAULT_PRINCIPAL_REDEMPTION_RATIO: f64 = 1.0;

/// Supplies zero flat and annualized rebates; see [`SnowballTerms::new`].
pub const DEFAULT_AUTOCALLABLE_REBATE_RATE: f64 = 0.0;

/// Supplies no initial knock-out lockout; see [`PhoenixPresetTerms::expand`].
pub const DEFAULT_PHOENIX_LOCKOUT_PERIODS: usize = 0;

/// Specifies when an autocallable's strict downside barrier is observed.
///
/// Daily observation owns its required clock; expiry-only observation has no clock.
///
/// # Examples
///
/// ```rust
/// use kiyosi::{KnockInObservation, TimeOfDay};
/// # fn main() -> kiyosi::Result<()> {
/// let daily = KnockInObservation::EveryTradingDay {
///     observation_clock_time: TimeOfDay::new(15, 0, 0, 0)?,
/// };
/// assert_ne!(daily, KnockInObservation::AtExpiry);
/// # Ok(()) }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnockInObservation {
    /// Observes at the specified clock on trading dates within the inclusive contract life.
    EveryTradingDay {
        /// Sets the timezone-free daily knock-in observation clock; see [`Self`].
        observation_clock_time: TimeOfDay,
    },
    /// Observes only at expiry.
    AtExpiry,
}

/// Records events strictly before valuation; see [`SnowballOption::replace`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutocallableBarrierState {
    /// Records no prior knock-in or knock-out, distinct from unknown history.
    NoEvent,
    /// Records a settled knock-out; no remaining cash flows are valued.
    KnockedOut,
    /// Records a prior downside breach on an outstanding note.
    KnockedIn,
}

/// Associates one snowball autocall time with its knock-out and annual coupon terms.
///
/// Autocall observations cover knock-out and applicable coupon checks, independently
/// of [`KnockInObservation`]. Phoenix uses [`KnockOutObservation`] and [`CouponObservation`].
///
/// Constructors and replacements validate numeric terms. Note construction also
/// validates ordering, life bounds and product-specific coupon constraints.
/// Fields are private so every public record has valid numeric terms. See
/// [`Self::new`] and [`Self::replace`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnowballObservation {
    /// Sets the exact observation time.
    time: Timestamp,
    /// Sets the positive inclusive knock-out threshold.
    knock_out_level: f64,
    coupon_rate: f64,
}

/// Selects observation replacements. Absent fields default to the source values.
///
/// See [`SnowballObservation::replace`] for an example.
#[derive(Clone, Copy, Debug, Default)]
pub struct SnowballObservationChanges {
    /// Replaces the exact observation timestamp.
    pub time: Option<Timestamp>,
    /// Replaces the positive finite knock-out threshold.
    pub knock_out_level: Option<f64>,
    /// Replaces the finite annual coupon rate.
    pub coupon_rate: Option<f64>,
}

impl SnowballObservation {
    /// Creates a snowball observation with an annual knock-out coupon.
    ///
    /// A zero rate explicitly selects no knock-out coupon.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a nonpositive or non-finite knock-out level
    /// or a non-finite coupon rate.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, SnowballObservation};
    /// # fn main() -> kiyosi::Result<()> {
    /// let time = Date::new(2026, 1, 1)?.start_of_day();
    /// let record = SnowballObservation::new(time, 110.0, 0.08)?;
    /// assert_eq!(record.coupon_rate(), 0.08);
    /// # Ok(()) }
    /// ```
    pub fn new(time: Timestamp, knock_out_level: f64, coupon_rate: f64) -> Result<Self> {
        let record = Self {
            time,
            knock_out_level,
            coupon_rate,
        };
        record.validate()?;
        Ok(record)
    }

    /// Associates parallel sequences with explicit snowball knock-out coupons.
    ///
    /// Numeric terms are validated here. Ordering, life bounds and product-specific
    /// constraints are validated when constructing a note.
    /// Supply one annual rate per observation, using zero for observations that pay no coupon.
    ///
    /// Records carry no calendar. When passing [`crate::ObservationSchedule::times`],
    /// also pass [`crate::ObservationSchedule::calendar`] to the note's terms
    /// constructor, such as [`SnowballTerms::new`], to preserve its observation calendar.
    /// Note terms require an explicit calendar, including for daily knock-in monitoring.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for unequal sequence lengths or numeric terms rejected by
    /// [`SnowballObservation::new`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, SnowballObservation};
    /// # fn main() -> kiyosi::Result<()> {
    /// let times = [Date::new(2026, 1, 1)?.start_of_day()];
    /// let records = SnowballObservation::from_columns(&times, &[110.0], &[0.08])?;
    /// assert_eq!(records[0].coupon_rate(), 0.08);
    /// # Ok(()) }
    /// ```
    pub fn from_columns(
        times: &[Timestamp],
        levels: &[f64],
        coupon_rates: &[f64],
    ) -> Result<Vec<Self>> {
        Self::validate_column_length(levels.len(), times.len())?;
        Self::validate_column_length(coupon_rates.len(), times.len())?;
        times
            .iter()
            .zip(levels)
            .zip(coupon_rates)
            .map(|((&time, &level), &rate)| Self::new(time, level, rate))
            .collect()
    }

    /// Checks that a column has one value per timestamp in [`Self::from_columns`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when the lengths differ.
    #[doc(hidden)]
    pub fn validate_column_length(count: usize, timestamps: usize) -> Result<()> {
        if count != timestamps {
            return Err(Error::InvalidInput(
                "observation sequence lengths differ".into(),
            ));
        }
        Ok(())
    }

    /// Returns a validated copy with the supplied fields applied.
    ///
    /// # Errors
    ///
    /// Reuses the numeric validation in [`Self::new`]. The note validates schedule
    /// and product constraints.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, SnowballObservation, SnowballObservationChanges};
    /// # fn main() -> kiyosi::Result<()> {
    /// let record = SnowballObservation::new(Date::new(2026, 1, 1)?.start_of_day(), 110.0, 0.0)?;
    /// let changed = record.replace(SnowballObservationChanges {
    ///     coupon_rate: Some(0.08), ..Default::default()
    /// })?;
    /// assert_eq!(changed.coupon_rate(), 0.08);
    /// assert_eq!(record.coupon_rate(), 0.0);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, changes: SnowballObservationChanges) -> Result<Self> {
        Self::new(
            changes.time.unwrap_or(self.time),
            changes.knock_out_level.unwrap_or(self.knock_out_level),
            changes.coupon_rate.unwrap_or(self.coupon_rate),
        )
    }

    /// Returns the exact timestamp; see [`Self::new`].
    pub fn time(&self) -> Timestamp {
        self.time
    }
    /// Returns the positive finite knock-out threshold; see [`Self::new`].
    pub fn knock_out_level(&self) -> f64 {
        self.knock_out_level
    }
    /// Returns the annual snowball coupon rate.
    ///
    /// See [`Self::new`] for an example.
    pub fn coupon_rate(&self) -> f64 {
        self.coupon_rate
    }

    fn validate(&self) -> Result<()> {
        finite(self.knock_out_level, true, "knock_out_level")?;
        finite(self.coupon_rate, false, "coupon_rate")?;
        Ok(())
    }
}

/// Owns one exact Phoenix observation time and its positive inclusive knock-out threshold.
///
/// Fields are validated at construction; schedule constraints are checked by [`PhoenixOption::new`].
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnockOutObservation {
    time: Timestamp,
    knock_out_level: f64,
}
impl KnockOutObservation {
    /// Creates an observation with a finite positive inclusive knock-out threshold.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for an invalid threshold.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, KnockOutObservation};
    /// # fn main() -> kiyosi::Result<()> {
    /// let time = Date::new(2026, 1, 1)?.start_of_day();
    /// let observation = KnockOutObservation::new(time, 90.0)?;
    /// assert_eq!(observation.time(), time);
    /// # Ok(()) }
    /// ```
    pub fn new(time: Timestamp, knock_out_level: f64) -> Result<Self> {
        finite(knock_out_level, true, "knock_out_level")?;
        Ok(Self {
            time,
            knock_out_level,
        })
    }
    /// Returns the exact observation time; see [`Self::new`].
    pub fn time(&self) -> Timestamp {
        self.time
    }
    /// Returns the positive inclusive knock-out threshold; see [`Self::new`].
    pub fn knock_out_level(&self) -> f64 {
        self.knock_out_level
    }
    /// Returns a validated copy with the supplied fields applied.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] threshold validation; see its example.
    pub fn replace(&self, time: Option<Timestamp>, knock_out_level: Option<f64>) -> Result<Self> {
        Self::new(
            time.unwrap_or(self.time),
            knock_out_level.unwrap_or(self.knock_out_level),
        )
    }
}

/// Owns one exact Phoenix observation time and its nonnegative inclusive coupon threshold.
///
/// Fields are validated at construction; schedule constraints are checked by [`PhoenixOption::new`].
/// See [`Self::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CouponObservation {
    time: Timestamp,
    coupon_barrier_level: f64,
}
impl CouponObservation {
    /// Creates an observation with a finite nonnegative inclusive coupon threshold.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for an invalid threshold.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, CouponObservation};
    /// # fn main() -> kiyosi::Result<()> {
    /// let time = Date::new(2026, 1, 1)?.start_of_day();
    /// let observation = CouponObservation::new(time, 90.0)?;
    /// assert_eq!(observation.time(), time);
    /// # Ok(()) }
    /// ```
    pub fn new(time: Timestamp, coupon_barrier_level: f64) -> Result<Self> {
        finite(coupon_barrier_level, false, "coupon_barrier_level")?;
        if coupon_barrier_level < 0.0 {
            return Err(Error::InvalidInput("negative coupon barrier".into()));
        }
        Ok(Self {
            time,
            coupon_barrier_level,
        })
    }
    /// Returns the exact observation time; see [`Self::new`].
    pub fn time(&self) -> Timestamp {
        self.time
    }
    /// Returns the nonnegative inclusive coupon threshold; see [`Self::new`].
    pub fn coupon_barrier_level(&self) -> f64 {
        self.coupon_barrier_level
    }
    /// Returns a validated copy with the supplied fields applied.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] threshold validation; see its example.
    pub fn replace(
        &self,
        time: Option<Timestamp>,
        coupon_barrier_level: Option<f64>,
    ) -> Result<Self> {
        Self::new(
            time.unwrap_or(self.time),
            coupon_barrier_level.unwrap_or(self.coupon_barrier_level),
        )
    }
}

/// Supplies basic snowball terms; see [`Self::new`] and [`SnowballOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct SnowballTerms {
    /// Owns the explicitly supplied contractual observation calendar.
    pub calendar: TradingCalendar,
    /// Sets the finite annual maturity coupon rate when no knock-in occurs.
    pub maturity_coupon_rate: f64,
    /// Sets the positive reference spot used to normalize downside losses.
    pub initial_spot: f64,
    /// Sets the positive strict downside threshold.
    pub knock_in_level: f64,
    /// Sets the positive upper settlement strike.
    pub upper_strike: f64,
    /// Sets the nonnegative lower settlement strike, at most the upper strike.
    pub lower_strike: f64,
    /// Specifies knock-in monitoring and its daily clock, if applicable; see [`KnockInObservation`].
    pub knock_in_observation: KnockInObservation,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons and losses.
    pub principal_redemption_ratio: f64,
    /// Sets a finite nonnegative flat rebate per unit of notional; see [`AutocallableNote`].
    pub rebate_rate: f64,
    /// Sets a finite nonnegative annualized rebate rate; see [`AutocallableNote`].
    pub annual_rebate_rate: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive settlement time; expiry-only knock-in requires an open date.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered knock-out and coupon observations within life.
    pub autocall_observations: Vec<SnowballObservation>,
}
impl SnowballTerms {
    /// Creates input terms with explicit knock-in observation, unknown history and unit principal.
    ///
    /// The explicit calendar controls contractual observations.
    /// Validation occurs in [`SnowballOption::new`]; see its example.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit contractual input terms"
    )]
    pub fn new(
        maturity_coupon_rate: f64,
        initial_spot: f64,
        knock_in_level: f64,
        upper_strike: f64,
        lower_strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        autocall_observations: Vec<SnowballObservation>,
        knock_in_observation: KnockInObservation,
        calendar: TradingCalendar,
    ) -> Self {
        Self {
            calendar,
            maturity_coupon_rate,
            initial_spot,
            knock_in_level,
            upper_strike,
            lower_strike,
            knock_in_observation,
            barrier_state: None,
            principal_redemption_ratio: DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
            rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            annual_rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            effective_time,
            expiry_time,
            autocall_observations,
        }
    }
}

/// Supplies Phoenix terms; see [`Self::new`] and [`PhoenixOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct PhoenixTerms {
    /// Owns the explicitly supplied contractual observation calendar.
    pub calendar: TradingCalendar,
    /// Sets the finite signed coupon per qualifying observation, without time accrual.
    pub coupon_per_period: f64,
    /// Sets the positive reference spot used to normalize downside losses.
    pub initial_spot: f64,
    /// Sets the positive strict downside threshold.
    pub knock_in_level: f64,
    /// Sets the positive upper settlement strike.
    pub upper_strike: f64,
    /// Sets the nonnegative lower settlement strike, at most the upper strike.
    pub lower_strike: f64,
    /// Specifies knock-in monitoring and its daily clock, if applicable; see [`KnockInObservation`].
    pub knock_in_observation: KnockInObservation,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons and losses.
    pub principal_redemption_ratio: f64,
    /// Sets a finite nonnegative flat rebate per unit of notional; see [`AutocallableNote`].
    pub rebate_rate: f64,
    /// Sets a finite nonnegative annualized rebate rate; see [`AutocallableNote`].
    pub annual_rebate_rate: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive settlement time; expiry-only knock-in requires an open date.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered knock-out observations within life.
    pub knock_out_observations: Vec<KnockOutObservation>,
    /// Supplies nonempty, strictly ordered coupon observations within life.
    pub coupon_observations: Vec<CouponObservation>,
}
impl PhoenixTerms {
    /// Creates periodic coupon terms with explicit knock-in monitoring.
    ///
    /// History defaults to unknown and principal redemption to one.
    /// The supplied calendar controls contractual observations.
    /// Validation occurs in [`PhoenixOption::new`]; see its example.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit contractual input terms"
    )]
    pub fn new(
        coupon_per_period: f64,
        initial_spot: f64,
        knock_in_level: f64,
        upper_strike: f64,
        lower_strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        knock_out_observations: Vec<KnockOutObservation>,
        coupon_observations: Vec<CouponObservation>,
        knock_in_observation: KnockInObservation,
        calendar: TradingCalendar,
    ) -> Self {
        Self {
            calendar,
            coupon_per_period,
            initial_spot,
            knock_in_level,
            upper_strike,
            lower_strike,
            knock_in_observation,
            barrier_state: None,
            principal_redemption_ratio: DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
            rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            annual_rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            effective_time,
            expiry_time,
            knock_out_observations,
            coupon_observations,
        }
    }
}

/// Owns validated basic snowball terms; see [`Self::new`].
///
/// Pays principal plus accrued annual coupon on knock-out, otherwise principal
/// plus maturity coupon if no knock-in occurs, or principal less a capped downside
/// loss after knock-in.
pub type SnowballOption = AutocallableNote<SnowballTerms>;

/// Supplies binary snowball terms; see [`Self::new`] and [`BinarySnowballOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinarySnowballTerms {
    /// Owns the explicitly supplied contractual observation calendar.
    pub calendar: TradingCalendar,
    /// Sets the finite annual coupon rate used at maturity if no knock-out occurs.
    pub maturity_coupon_rate: f64,
    /// Records prior events; knock-in is rejected because this product has no knock-in barrier.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons.
    pub principal_redemption_ratio: f64,
    /// Sets a finite nonnegative flat rebate per unit of notional; see [`AutocallableNote`].
    pub rebate_rate: f64,
    /// Sets a finite nonnegative annualized rebate rate; see [`AutocallableNote`].
    pub annual_rebate_rate: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive settlement time, which may fall on a closed date.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered knock-out and coupon observations within life.
    pub autocall_observations: Vec<SnowballObservation>,
}
impl BinarySnowballTerms {
    /// Creates input terms with unknown history and unit principal.
    ///
    /// The explicit calendar controls contractual observations.
    /// Validation occurs in [`BinarySnowballOption::new`]; see its example.
    pub fn new(
        maturity_coupon_rate: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        autocall_observations: Vec<SnowballObservation>,
        calendar: TradingCalendar,
    ) -> Self {
        Self {
            calendar,
            maturity_coupon_rate,
            barrier_state: None,
            principal_redemption_ratio: DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
            rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            annual_rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            effective_time,
            expiry_time,
            autocall_observations,
        }
    }
}

/// Owns a binary snowball with no knock-in monitoring or downside participation.
///
/// Pays principal plus the accrued observation coupon on knock-out, otherwise
/// principal plus the accrued maturity coupon. See [`Self::new`] for construction.
pub type BinarySnowballOption = AutocallableNote<BinarySnowballTerms>;

/// Supplies ternary snowball terms; see [`Self::new`] and [`TernarySnowballOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct TernarySnowballTerms {
    /// Owns the explicitly supplied contractual observation calendar.
    pub calendar: TradingCalendar,
    /// Sets the finite annual coupon rate used at maturity if no knock-out occurs.
    pub maturity_coupon_rate: f64,
    /// Sets the finite annual coupon rate after knock-in, at most the maturity coupon rate.
    pub minimum_coupon_rate: f64,
    /// Sets the positive strict downside threshold.
    pub knock_in_level: f64,
    /// Specifies knock-in monitoring and its daily clock, if applicable; see [`KnockInObservation`].
    pub knock_in_observation: KnockInObservation,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons.
    pub principal_redemption_ratio: f64,
    /// Sets a finite nonnegative flat rebate per unit of notional; see [`AutocallableNote`].
    pub rebate_rate: f64,
    /// Sets a finite nonnegative annualized rebate rate; see [`AutocallableNote`].
    pub annual_rebate_rate: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive settlement time; expiry-only knock-in requires an open date.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered knock-out and coupon observations within life.
    pub autocall_observations: Vec<SnowballObservation>,
}
impl TernarySnowballTerms {
    /// Creates input terms with explicit knock-in observation, unknown history and unit principal.
    ///
    /// The explicit calendar controls contractual observations.
    /// Validation occurs in [`TernarySnowballOption::new`]; see its example.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit contractual input terms"
    )]
    pub fn new(
        maturity_coupon_rate: f64,
        minimum_coupon_rate: f64,
        knock_in_level: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        autocall_observations: Vec<SnowballObservation>,
        knock_in_observation: KnockInObservation,
        calendar: TradingCalendar,
    ) -> Self {
        Self {
            calendar,
            maturity_coupon_rate,
            minimum_coupon_rate,
            knock_in_level,
            knock_in_observation,
            barrier_state: None,
            principal_redemption_ratio: DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
            rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            annual_rebate_rate: DEFAULT_AUTOCALLABLE_REBATE_RATE,
            effective_time,
            expiry_time,
            autocall_observations,
        }
    }
}

/// Owns a ternary snowball with separate maturity coupon rates before and after knock-in.
///
/// Pays principal plus the accrued observation coupon on knock-out, otherwise
/// principal plus the accrued maturity or minimum coupon according to knock-in. See [`Self::new`] for construction.
pub type TernarySnowballOption = AutocallableNote<TernarySnowballTerms>;

/// Owns validated Phoenix terms; see [`Self::new`].
///
/// Pays the signed periodic coupon when spot is at or above its coupon barrier,
/// including on knock-out. Each qualifying observation pays the same proportion
/// of initial notional, independently of period length, including at the effective time.
/// Otherwise expiry pays principal, less capped downside loss after knock-in.
/// Past coupons are settled; current coupons are included once. Zero coupon
/// barriers pay unconditionally. There is no coupon memory or maturity coupon.
pub type PhoenixOption = AutocallableNote<PhoenixTerms>;

/// Selects the supported autocallable terms; see [`PhoenixOption::new`] for an example.
///
/// This trait is sealed: only the core defines product settlement rules.
pub trait AutocallableTerms: sealed::Terms + Clone + PartialEq + Send + Sync + 'static {}
impl AutocallableTerms for SnowballTerms {}
impl AutocallableTerms for PhoenixTerms {}
impl AutocallableTerms for BinarySnowballTerms {}
impl AutocallableTerms for TernarySnowballTerms {}

/// Selects snowball terms with a quoted observation coupon and explicit maturity convention.
///
/// This trait is sealed; see [`crate::implied_snowball_coupon`] for an example.
pub trait SnowballCouponTerms: AutocallableTerms + sealed::SnowballCoupon {}

macro_rules! snowball_coupon {
    ($($terms:ty),+) => {$(
        impl SnowballCouponTerms for $terms {}
        impl sealed::SnowballCoupon for $terms {
            fn with_coupon(&self, coupon: f64, convention: crate::CouponQuoteConvention) -> Result<Self> {
                let mut terms = self.clone();
                let shift = coupon - terms.autocall_observations.first().map_or(0.0, SnowballObservation::coupon_rate);
                for chunk in terms.autocall_observations.chunks_mut(1024) {
                    crate::cancellation::check()?;
                    for record in chunk {
                        record.coupon_rate += shift;
                    }
                }
                if convention == crate::CouponQuoteConvention::ShiftMaturityCoupon {
                    terms.maturity_coupon_rate += shift;
                }
                Ok(terms)
            }
        }
    )+};
}
snowball_coupon!(SnowballTerms, BinarySnowballTerms, TernarySnowballTerms);

impl<T: SnowballCouponTerms> AutocallableNote<T> {
    pub(crate) fn minimum_quoted_coupon(
        &self,
        convention: crate::CouponQuoteConvention,
    ) -> Option<f64> {
        if convention != crate::CouponQuoteConvention::ShiftMaturityCoupon {
            return None;
        }
        let terms = self.view();
        let Redemption::FixedCoupon(minimum) = terms.knock_in?.redemption else {
            return None;
        };
        let anchor = terms.autocall_observations[0].coupon_rate();
        let maturity = terms.maturity_coupon_rate();
        // Round both inverse operations into the domain used by with_coupon.
        let shift = minimum - maturity;
        let shift = if maturity + shift < minimum {
            libm::nextafter(shift, f64::INFINITY)
        } else {
            shift
        };
        let lower = anchor + shift;
        Some(if lower - anchor < shift {
            libm::nextafter(lower, f64::INFINITY)
        } else {
            lower
        })
    }

    /// Returns a note with the supplied first observation coupon and preserved coupon spreads.
    ///
    /// The quote is an absolute annual rate, not an additive shift.
    /// The first observation anchors the shift, including past observations.
    /// `convention` controls the maturity coupon; the ternary minimum stays fixed.
    /// See [`crate::implied_snowball_coupon`] for an example.
    ///
    /// # Errors
    ///
    /// Propagates term validation errors, including a shifted maturity coupon below
    /// the ternary minimum.
    pub fn with_coupon(
        &self,
        first_observation_coupon: f64,
        convention: crate::CouponQuoteConvention,
    ) -> Result<Self> {
        Self::new(self.0.with_coupon(first_observation_coupon, convention)?)
    }
}

mod sealed {
    use super::{
        AutocallableBarrierState, CouponObservation, KnockInObservation, KnockOutObservation,
        SnowballObservation, Timestamp,
    };
    pub trait Terms {
        fn view(&self) -> View<'_>;
    }
    pub trait SnowballCoupon: Sized {
        fn with_coupon(
            &self,
            coupon: f64,
            convention: crate::CouponQuoteConvention,
        ) -> crate::Result<Self>;
    }
    #[derive(Clone, Copy, PartialEq)]
    pub struct Downside {
        pub initial_spot: f64,
        pub upper_strike: f64,
        pub lower_strike: f64,
    }
    #[derive(Clone, Copy, PartialEq)]
    pub enum Redemption {
        Downside(Downside),
        FixedCoupon(f64),
    }
    #[derive(Clone, Copy, PartialEq)]
    pub struct KnockIn {
        pub level: f64,
        pub observation: KnockInObservation,
        pub redemption: Redemption,
    }
    #[derive(Clone, Copy)]
    pub enum Coupon {
        Snowball(f64),
        Phoenix(f64),
    }
    pub struct View<'a> {
        pub calendar: &'a crate::TradingCalendar,
        pub knock_in: Option<KnockIn>,
        pub coupon: Coupon,
        pub barrier_state: Option<AutocallableBarrierState>,
        pub principal_redemption_ratio: f64,
        pub rebate_rate: f64,
        pub annual_rebate_rate: f64,
        pub effective_time: Timestamp,
        pub expiry_time: Timestamp,
        pub autocall_observations: &'a [SnowballObservation],
        pub knock_out_observations: &'a [KnockOutObservation],
        pub coupon_observations: &'a [CouponObservation],
    }
}
use sealed::Coupon;
use sealed::{Downside, KnockIn, Redemption};

macro_rules! terms_view {
    ($terms:ty, $coupon:path, $rate:ident) => {
        impl sealed::Terms for $terms {
            fn view(&self) -> sealed::View<'_> {
                sealed::View {
                    calendar: &self.calendar,
                    knock_in: Some(KnockIn {
                        level: self.knock_in_level,
                        observation: self.knock_in_observation,
                        redemption: Redemption::Downside(Downside {
                            initial_spot: self.initial_spot,
                            upper_strike: self.upper_strike,
                            lower_strike: self.lower_strike,
                        }),
                    }),
                    coupon: $coupon(self.$rate),
                    barrier_state: self.barrier_state,
                    principal_redemption_ratio: self.principal_redemption_ratio,
                    rebate_rate: self.rebate_rate,
                    annual_rebate_rate: self.annual_rebate_rate,
                    effective_time: self.effective_time,
                    expiry_time: self.expiry_time,
                    autocall_observations: &self.autocall_observations,
                    knock_out_observations: &[],
                    coupon_observations: &[],
                }
            }
        }
    };
}
terms_view!(SnowballTerms, Coupon::Snowball, maturity_coupon_rate);
impl sealed::Terms for PhoenixTerms {
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            calendar: &self.calendar,
            knock_in: Some(KnockIn {
                level: self.knock_in_level,
                observation: self.knock_in_observation,
                redemption: Redemption::Downside(Downside {
                    initial_spot: self.initial_spot,
                    upper_strike: self.upper_strike,
                    lower_strike: self.lower_strike,
                }),
            }),
            coupon: Coupon::Phoenix(self.coupon_per_period),
            barrier_state: self.barrier_state,
            principal_redemption_ratio: self.principal_redemption_ratio,
            rebate_rate: self.rebate_rate,
            annual_rebate_rate: self.annual_rebate_rate,
            effective_time: self.effective_time,
            expiry_time: self.expiry_time,
            autocall_observations: &[],
            knock_out_observations: &self.knock_out_observations,
            coupon_observations: &self.coupon_observations,
        }
    }
}

impl sealed::Terms for BinarySnowballTerms {
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            calendar: &self.calendar,
            knock_in: None,
            coupon: Coupon::Snowball(self.maturity_coupon_rate),
            barrier_state: self.barrier_state,
            principal_redemption_ratio: self.principal_redemption_ratio,
            rebate_rate: self.rebate_rate,
            annual_rebate_rate: self.annual_rebate_rate,
            effective_time: self.effective_time,
            expiry_time: self.expiry_time,
            autocall_observations: &self.autocall_observations,
            knock_out_observations: &[],
            coupon_observations: &[],
        }
    }
}

impl sealed::Terms for TernarySnowballTerms {
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            calendar: &self.calendar,
            knock_in: Some(KnockIn {
                level: self.knock_in_level,
                observation: self.knock_in_observation,
                redemption: Redemption::FixedCoupon(self.minimum_coupon_rate),
            }),
            coupon: Coupon::Snowball(self.maturity_coupon_rate),
            barrier_state: self.barrier_state,
            principal_redemption_ratio: self.principal_redemption_ratio,
            rebate_rate: self.rebate_rate,
            annual_rebate_rate: self.annual_rebate_rate,
            effective_time: self.effective_time,
            expiry_time: self.expiry_time,
            autocall_observations: &self.autocall_observations,
            knock_out_observations: &[],
            coupon_observations: &[],
        }
    }
}

/// Owns validated terms and observations for a supported autocallable.
///
/// At knock-out or maturity, adds a rebate per unit of original notional equal
/// to `rebate_rate + annual_rebate_rate * T`. Both rates default to zero and must
/// be finite and nonnegative. `T` is Actual/365 Fixed from effective time to the
/// settlement event, including intraday time and nontrading days. The rebate
/// is independent of knock-in, coupon eligibility and principal losses. It pays
/// once at termination, including zero-lifetime settlement, and never as a
/// periodic coupon. A note knocked out before valuation is settled and has zero value.
///
/// `principal_redemption_ratio` sets the principal payment per original unit of
/// notional independently of coupons and downside losses. Position sizing multiplies
/// the complete price and sensitivities by position notional; see [`Self::new`].
///
/// Accessors borrow stable storage for this owner's lifetime. Shared reads and
/// pricing are safe; mutation requires exclusive Rust access. See [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct AutocallableNote<T: AutocallableTerms>(T, Vec<ObservationEvent>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ObservationEvent {
    pub(crate) time: Timestamp,
    knock_out: Option<KnockOutObservation>,
    coupon: Option<CouponObservation>,
    coupon_rate: f64,
}
impl ObservationEvent {
    pub(crate) fn has_knock_out(&self) -> bool {
        self.knock_out.is_some()
    }
    pub(crate) fn time(&self) -> Timestamp {
        self.time
    }
    pub(crate) fn knock_out_level(&self) -> f64 {
        self.knock_out
            .expect("knock-out level requires a knock-out event")
            .knock_out_level
    }
    pub(crate) fn coupon_barrier_level(&self) -> Option<f64> {
        self.coupon.map(|record| record.coupon_barrier_level)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortfolioVariation {
    Coupon,
    KnockIn,
    KnockOut,
}

impl<T: AutocallableTerms> AutocallableNote<T> {
    /// Validates all terms before creating an owning note.
    ///
    /// Calendar compatibility and product state choices are checked on construction.
    /// History describes events strictly before valuation; observations at valuation
    /// use the current spot. Callers supply state applicable to each valuation.
    /// History consistency relative to valuation is checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid numeric terms or
    /// product-incompatible observation fields, reversed life, or empty, unordered
    /// or out-of-life observations.
    /// Cancellation returns [`Error::Io`] with [`std::io::ErrorKind::Interrupted`].
    /// Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let knock_in_observation = KnockInObservation::EveryTradingDay {
    ///     observation_clock_time: TimeOfDay::new(0, 0, 0, 0)?,
    /// };
    /// let mut terms = PhoenixTerms::new(0.02, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     vec![KnockOutObservation::new(end, 110.0)?],
    ///     vec![CouponObservation::new(end, 90.0)?], knock_in_observation, kiyosi::TradingCalendar::weekdays());
    /// terms.rebate_rate = 0.02;
    /// terms.annual_rebate_rate = 0.06;
    /// let note = PhoenixOption::new(terms)?;
    /// let known = note.replace(PhoenixOptionChanges {
    ///     barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
    ///     ..Default::default()
    /// })?;
    /// assert_eq!(known.knock_out_observations(), note.knock_out_observations());
    /// assert_eq!(note.barrier_state(), None);
    /// let records = SnowballObservation::from_columns(&[end], &[110.0], &[0.08])?;
    /// let binary = BinarySnowballOption::new(BinarySnowballTerms::new(
    ///     0.06, start, end, records.clone(), kiyosi::TradingCalendar::weekdays()))?;
    /// let ternary = TernarySnowballOption::new(TernarySnowballTerms::new(
    ///     0.06, 0.02, 80.0, start, end, records, knock_in_observation, kiyosi::TradingCalendar::weekdays()))?;
    /// assert_eq!(binary.terms().principal_redemption_ratio, 1.0);
    /// assert_eq!(ternary.terms().minimum_coupon_rate, 0.02);
    /// // Amend an already validated contract to include weekend monitoring.
    /// let daily = ternary.replace(TernarySnowballOptionChanges {
    ///     calendar: Some(kiyosi::TradingCalendar::all_days()), ..Default::default()
    /// })?;
    /// assert_eq!(daily.calendar().trading_days_per_year(), 365);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: T) -> Result<Self> {
        let view = terms.view();
        view.validate()?;
        view.validate_calendar()?;
        let events = view
            .autocall_observations
            .iter()
            .map(|record| ObservationEvent {
                time: record.time,
                knock_out: Some(KnockOutObservation {
                    time: record.time,
                    knock_out_level: record.knock_out_level,
                }),
                coupon: None,
                coupon_rate: record.coupon_rate(),
            })
            .chain(
                view.knock_out_observations
                    .iter()
                    .map(|record| ObservationEvent {
                        time: record.time,
                        knock_out: Some(*record),
                        coupon: None,
                        coupon_rate: 0.0,
                    }),
            )
            .chain(
                view.coupon_observations
                    .iter()
                    .map(|record| ObservationEvent {
                        time: record.time,
                        knock_out: None,
                        coupon: Some(*record),
                        coupon_rate: 0.0,
                    }),
            );
        let mut sorted = Vec::with_capacity(
            view.autocall_observations.len()
                + view.knock_out_observations.len()
                + view.coupon_observations.len(),
        );
        for (index, event) in events.enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            sorted.push(event);
        }
        crate::cancellation::check()?;
        sorted.sort_unstable_by_key(|event| event.time);
        crate::cancellation::check()?;
        sorted.dedup_by(|later, earlier| {
            if later.time != earlier.time {
                return false;
            }
            earlier.knock_out = earlier.knock_out.or(later.knock_out);
            earlier.coupon = earlier.coupon.or(later.coupon);
            true
        });
        crate::cancellation::check()?;
        Ok(Self(terms, sorted))
    }

    /// Borrows the contract's monitoring calendar; see [`Self::new`].
    pub fn calendar(&self) -> &TradingCalendar {
        self.view().calendar
    }
    /// Borrows validated payoff terms, including the contractual calendar. See [`Self::new`].
    pub fn terms(&self) -> &T {
        &self.0
    }
    pub(crate) fn events(&self) -> &[ObservationEvent] {
        &self.1
    }
    pub(crate) fn event_at(&self, time: Timestamp) -> Option<&ObservationEvent> {
        let events = self.events();
        events
            .binary_search_by_key(&time, ObservationEvent::time)
            .ok()
            .map(|index| &events[index])
    }
    /// Returns events strictly before valuation, or `None` for unknown; see [`Self::new`].
    pub fn barrier_state(&self) -> Option<AutocallableBarrierState> {
        self.view().barrier_state
    }
    /// Returns the inclusive effective time; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.view().effective_time
    }
    /// Returns the inclusive expiry timestamp; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.view().expiry_time
    }

    /// Returns the daily clock, or `None` for monitoring without daily events; see [`TimeOfDay::new`].
    pub fn observation_clock_time(&self) -> Option<TimeOfDay> {
        self.view().knock_in.and_then(|ki| match ki.observation {
            KnockInObservation::EveryTradingDay {
                observation_clock_time,
            } => Some(observation_clock_time),
            KnockInObservation::AtExpiry => None,
        })
    }

    pub(crate) fn observes_knock_in(&self, time: Timestamp) -> bool {
        self.view().knock_in.is_some_and(|ki| match ki.observation {
            KnockInObservation::EveryTradingDay { .. } => self.daily_time(time, self.calendar()),
            KnockInObservation::AtExpiry => time == self.expiry_time(),
        })
    }

    /// Returns an owned knock-in schedule, or `None` for products without knock-in.
    ///
    /// Daily monitoring includes the effective instant at the contractual clock
    /// on open dates. Expiry-only monitoring returns exactly the expiry instant.
    /// History does not filter this whole-life schedule. Knock-out and coupon
    /// observations are supplied by the corresponding product terms.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::ObservationSchedule`] validation and time-range errors.
    ///
    /// # Panics
    ///
    /// Propagates panics from custom calendar predicates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{SnowballObservation, Date, KnockInObservation, SnowballOption, SnowballTerms,
    ///     TimeOfDay};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 6)?.start_of_day();
    /// let end = start.add_days(2)?;
    /// let knock_in_observation = KnockInObservation::EveryTradingDay {
    ///     observation_clock_time: TimeOfDay::new(0, 0, 0, 0)?,
    /// };
    /// let note = SnowballOption::new(SnowballTerms::new(
    ///     0.1, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     vec![SnowballObservation::new(end, 110.0, 0.0)?], knock_in_observation, kiyosi::TradingCalendar::weekdays()))?;
    /// assert_eq!(note.knock_in_observation_schedule()?.map(|s| s.len()), Some(3));
    /// # Ok(()) }
    /// ```
    pub fn knock_in_observation_schedule(&self) -> Result<Option<crate::ObservationSchedule>> {
        match self.view().knock_in {
            None => Ok(None),
            Some(ki) if ki.observation == KnockInObservation::AtExpiry => {
                crate::ObservationSchedule::new(
                    vec![self.expiry_time()],
                    self.effective_time(),
                    self.expiry_time(),
                    self.calendar(),
                )
                .map(Some)
            }
            Some(_) => self.daily_observation_schedule().map(Some),
        }
    }

    pub(crate) fn daily_observation_schedule(&self) -> Result<crate::ObservationSchedule> {
        match self.observation_clock_time() {
            Some(clock) => crate::ObservationSchedule::daily(
                self.effective_time(),
                self.expiry_time(),
                self.calendar(),
                true,
                clock,
            ),
            None => crate::ObservationSchedule::new(
                Vec::new(),
                self.effective_time(),
                self.expiry_time(),
                self.calendar(),
            ),
        }
    }

    pub(crate) fn has_daily_observation(&self, start: Timestamp, end: Timestamp) -> Result<bool> {
        let Some(clock) = self.observation_clock_time() else {
            return Ok(false);
        };
        crate::ObservationSchedule::any_daily(
            start.max(self.effective_time()),
            end.min(self.expiry_time()),
            self.calendar(),
            clock,
            |_| true,
        )
    }

    pub(crate) fn view(&self) -> sealed::View<'_> {
        self.0.view()
    }
    pub(crate) fn observation_coupon(
        &self,
        record: &ObservationEvent,
        qualifies: impl FnOnce(f64) -> bool,
    ) -> f64 {
        if record.coupon_barrier_level().is_some_and(qualifies) {
            self.period_coupon()
        } else {
            0.0
        }
    }
    pub(crate) fn period_coupon(&self) -> f64 {
        match self.view().coupon {
            Coupon::Snowball(_) => 0.0,
            Coupon::Phoenix(coupon) => coupon,
        }
    }

    pub(crate) fn portfolio_coupon(&self) -> f64 {
        match self.view().coupon {
            Coupon::Snowball(rate) | Coupon::Phoenix(rate) => rate,
        }
    }

    pub(crate) fn portfolio_family_key(&self, variation: PortfolioVariation) -> Result<u64> {
        use std::hash::{DefaultHasher, Hash, Hasher};

        // Equality accepts both signed zeros.
        let bits = |value: f64| if value == 0.0 { 0 } else { value.to_bits() };
        let terms = self.view();
        let mut key = DefaultHasher::new();
        (
            terms.effective_time,
            terms.expiry_time,
            self.knock_in_affects_payoff(),
        )
            .hash(&mut key);
        terms
            .barrier_state
            .map(|state| std::mem::discriminant(&state))
            .hash(&mut key);
        [
            terms.principal_redemption_ratio,
            terms.rebate_rate,
            terms.annual_rebate_rate,
        ]
        .map(bits)
        .hash(&mut key);
        if variation != PortfolioVariation::Coupon {
            bits(self.portfolio_coupon()).hash(&mut key);
        }
        if let Some(knock_in) = terms.knock_in {
            if variation != PortfolioVariation::KnockIn {
                bits(knock_in.level).hash(&mut key);
            }
            std::mem::discriminant(&knock_in.observation).hash(&mut key);
            match knock_in.redemption {
                Redemption::FixedCoupon(coupon) => bits(coupon).hash(&mut key),
                Redemption::Downside(downside) => {
                    [
                        downside.initial_spot,
                        downside.upper_strike,
                        downside.lower_strike,
                    ]
                    .map(bits)
                    .hash(&mut key);
                }
            }
        }
        for events in self.events().chunks(1024) {
            crate::cancellation::check()?;
            for event in events {
                (
                    event.time,
                    event.knock_out.map(|record| {
                        if variation == PortfolioVariation::KnockOut {
                            0
                        } else {
                            bits(record.knock_out_level)
                        }
                    }),
                    event.coupon_barrier_level().map(bits),
                    bits(event.coupon_rate),
                )
                    .hash(&mut key);
            }
        }
        Ok(key.finish())
    }

    #[expect(
        clippy::float_cmp,
        reason = "Portfolio reductions require exactly matching fixed terms."
    )]
    pub(crate) fn shares_portfolio_terms(
        &self,
        other: &Self,
        variation: PortfolioVariation,
    ) -> Result<bool> {
        let a = self.view();
        let b = other.view();
        let compare_knock_in = |mut terms: KnockIn| {
            if variation == PortfolioVariation::KnockIn {
                terms.level = 0.0;
            }
            terms
        };
        if a.effective_time != b.effective_time
            || a.expiry_time != b.expiry_time
            || a.calendar != b.calendar
            || a.barrier_state != b.barrier_state
            || a.knock_in.map(compare_knock_in) != b.knock_in.map(compare_knock_in)
            || variation != PortfolioVariation::Coupon
                && self.portfolio_coupon() != other.portfolio_coupon()
            || self.knock_in_affects_payoff() != other.knock_in_affects_payoff()
            || a.principal_redemption_ratio != b.principal_redemption_ratio
            || a.rebate_rate != b.rebate_rate
            || a.annual_rebate_rate != b.annual_rebate_rate
            || self.events().len() != other.events().len()
        {
            return Ok(false);
        }
        for (left, right) in self.events().chunks(1024).zip(other.events().chunks(1024)) {
            crate::cancellation::check()?;
            if left.iter().zip(right).any(|(a, b)| {
                if variation == PortfolioVariation::KnockOut {
                    a.time != b.time
                        || a.has_knock_out() != b.has_knock_out()
                        || a.coupon != b.coupon
                        || a.coupon_rate != b.coupon_rate
                } else {
                    a != b
                }
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    #[expect(
        clippy::float_cmp,
        reason = "Exactly matching redemption terms determine whether knock-in changes the payoff."
    )]
    pub(crate) fn knock_in_affects_payoff(&self) -> bool {
        let t = self.view();
        t.knock_in.is_some_and(|ki| match ki.redemption {
            Redemption::FixedCoupon(coupon) => coupon != t.maturity_coupon_rate(),
            Redemption::Downside(d) => {
                d.lower_strike != d.upper_strike || t.maturity_coupon_rate() != 0.0
            }
        })
    }
    pub(crate) fn prior(&self, time: Timestamp) -> Result<bool> {
        self.prior_with_work_check(time, |_| Ok(()))
    }

    pub(crate) fn prior_with_work_check(
        &self,
        time: Timestamp,
        charge: impl Fn(u64) -> Result<()>,
    ) -> Result<bool> {
        let terms = self.view();
        let first_out = terms
            .knock_out_observations
            .first()
            .map(|record| record.time)
            .or_else(|| {
                terms
                    .autocall_observations
                    .first()
                    .map(|record| record.time)
            });
        let had_out = first_out.is_some_and(|first| first < time);
        let had_in = || {
            if !terms.daily_knock_in() || time <= terms.effective_time {
                return Ok(false);
            }
            let end = time.add_micros(-1)?.min(terms.expiry_time);
            charge(
                terms
                    .effective_time
                    .date()
                    .days_until(end.date())
                    .unsigned_abs()
                    + 1,
            )?;
            self.has_daily_observation(terms.effective_time, end)
        };
        match terms.barrier_state {
            None if had_out || had_in()? => {
                return Err(Error::MissingHistory(
                    "prior autocallable state is required".into(),
                ));
            }
            Some(AutocallableBarrierState::KnockedOut) if !had_out => {
                return Err(Error::InvalidInput(
                    "knock-out predates observations".into(),
                ));
            }
            Some(AutocallableBarrierState::KnockedIn) if !had_in()? => {
                return Err(Error::InvalidInput("knock-in predates monitoring".into()));
            }
            _ => {}
        }
        Ok(terms.barrier_state == Some(AutocallableBarrierState::KnockedIn))
    }

    pub(crate) fn future_schedule_visits(&self, start: Timestamp, daily: bool) -> u64 {
        let events = self.events();
        let future = events.len() - events.partition_point(|event| event.time() <= start);
        let days = if daily {
            start
                .date()
                .days_until(self.expiry_time().date())
                .unsigned_abs()
                + 1
        } else {
            0
        };
        days + future as u64 + 1
    }

    pub(crate) fn daily_at(&self, context: &PricingContext) -> bool {
        self.view().daily_knock_in() && self.daily_time(context.valuation_time(), self.calendar())
    }
    pub(crate) fn daily_time(&self, time: Timestamp, calendar: &TradingCalendar) -> bool {
        time >= self.effective_time()
            && time <= self.expiry_time()
            && self.observation_clock_time() == Some(time.time_of_day())
            && calendar.is_trading_day(time.date())
    }

    pub(crate) fn terminal(&self, spot: f64, knocked_in: bool) -> f64 {
        let t = self.view();
        let coupon = if knocked_in {
            match t.redemption() {
                Redemption::Downside(d) => {
                    return t.principal_redemption_ratio
                        + self.rebate_payment(t.expiry_time)
                        + (spot.clamp(d.lower_strike, d.upper_strike) - d.upper_strike)
                            / d.initial_spot;
                }
                Redemption::FixedCoupon(coupon) => coupon,
            }
        } else {
            t.maturity_coupon_rate()
        };
        t.principal_redemption_ratio
            + self.rebate_payment(t.expiry_time)
            + coupon * t.effective_time.years_until(t.expiry_time)
    }
    pub(crate) fn rebate_payment(&self, time: Timestamp) -> f64 {
        let t = self.view();
        t.rebate_rate + t.annual_rebate_rate * t.effective_time.years_until(time)
    }
    pub(crate) fn knock_out_payment(&self, record: &ObservationEvent) -> f64 {
        let t = self.view();
        t.principal_redemption_ratio
            + self.rebate_payment(record.time)
            + record.coupon_rate * self.effective_time().years_until(record.time)
    }
}
impl<T: AutocallableTerms> OptionContract for AutocallableNote<T> {
    fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let state = pricing::initial(self, context);
        state.settlement.is_none()
            && pricing::fixed_value(self, context, state.knocked_in).is_none()
    }
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        if self.barrier_state() == Some(AutocallableBarrierState::KnockedOut) {
            return false;
        }
        if let Ok(until) = context.deterministic_until(self.expiry_time()) {
            let events = self.events();
            for record in events[events.partition_point(|r| r.time <= context.valuation_time())..]
                .iter()
                .take_while(|record| record.time <= until)
            {
                if crate::cancellation::check().is_err() {
                    return false;
                }
                if record.has_knock_out()
                    && context.forward_crosses(record.time, record.knock_out_level(), radius)
                    || self.period_coupon() != 0.0
                        && context.forward_crosses(
                            record.time,
                            record.coupon_barrier_level().unwrap_or_default(),
                            radius,
                        )
                {
                    return true;
                }
            }
            if until > context.valuation_time() && self.knock_in_affects_payoff() {
                if let Some(ki) = self.view().knock_in {
                    let times = match ki.observation {
                        KnockInObservation::EveryTradingDay {
                            observation_clock_time,
                        } => crate::ObservationSchedule::daily_times(
                            context.valuation_time().max(self.effective_time()),
                            until,
                            self.calendar(),
                            true,
                            observation_clock_time,
                        )
                        .unwrap_or_default(),
                        KnockInObservation::AtExpiry if until == self.expiry_time() => {
                            vec![until]
                        }
                        KnockInObservation::AtExpiry => Vec::new(),
                    };
                    let mut knocked_in =
                        self.barrier_state() == Some(AutocallableBarrierState::KnockedIn);
                    for time in times
                        .into_iter()
                        .filter(|&time| time >= context.valuation_time())
                    {
                        if crate::cancellation::check().is_err() {
                            return false;
                        }
                        if knocked_in {
                            break;
                        }
                        if context.forward_crosses(time, ki.level, radius) {
                            return true;
                        }
                        let spot = context.spot_price();
                        knocked_in = context.forward_carry(time).is_ok_and(|carry| {
                            context
                                .forward_spot_cmp(time, spot, ki.level, carry)
                                .is_some_and(std::cmp::Ordering::is_lt)
                        });
                    }
                    if let Redemption::Downside(d) = ki.redemption {
                        if knocked_in
                            && until == self.expiry_time()
                            && (context.forward_crosses(self.expiry_time(), d.lower_strike, radius)
                                || context.forward_crosses(
                                    self.expiry_time(),
                                    d.upper_strike,
                                    radius,
                                ))
                        {
                            return true;
                        }
                    }
                }
            }
        }
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        if let Some(record) = self.event_at(context.valuation_time()) {
            if self.period_coupon() != 0.0
                && record
                    .coupon_barrier_level()
                    .is_some_and(|level| level > 0.0 && crosses(level))
            {
                return true;
            }
            if record.has_knock_out() && crosses(record.knock_out_level()) {
                return true;
            }
            if record.has_knock_out() && spot > record.knock_out_level() {
                return false;
            }
        }
        if context.valuation_time() == self.expiry_time()
            && self.view().knock_in.is_some_and(|ki| {
                (self.barrier_state() == Some(AutocallableBarrierState::KnockedIn)
                    || self.observes_knock_in(context.valuation_time())
                        && self.view().below_knock_in(spot))
                    && match ki.redemption {
                        Redemption::Downside(d) => {
                            crosses(d.upper_strike) || crosses(d.lower_strike)
                        }
                        Redemption::FixedCoupon(_) => false,
                    }
            })
        {
            return true;
        }
        self.barrier_state() != Some(AutocallableBarrierState::KnockedIn)
            && self.knock_in_affects_payoff()
            && self.observes_knock_in(context.valuation_time())
            && self.view().knock_in.is_some_and(|ki| crosses(ki.level))
    }
    fn time_stencil_available(
        &self,
        _context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        if self.prior(before).is_err() {
            return false;
        }
        if self.barrier_state() == Some(AutocallableBarrierState::KnockedOut) {
            return true;
        }
        let observations = self.events();
        if observations
            .get(observations.partition_point(|record| record.time < before))
            .is_some_and(|record| record.time <= after)
        {
            return false;
        }
        if self.view().daily_knock_in()
            && self.barrier_state() != Some(AutocallableBarrierState::KnockedIn)
            && self.knock_in_affects_payoff()
            && self.has_daily_observation(before, after).unwrap_or(true)
        {
            return false;
        }
        true
    }
}

impl sealed::View<'_> {
    fn validate_calendar(&self) -> Result<()> {
        for time in self
            .autocall_observations
            .iter()
            .map(|record| record.time)
            .chain(self.knock_out_observations.iter().map(|record| record.time))
            .chain(self.coupon_observations.iter().map(|record| record.time))
        {
            crate::cancellation::check()?;
            if !self.calendar.is_trading_day(time.date()) {
                return Err(Error::InvalidInput(
                    "observation is not a trading day".into(),
                ));
            }
        }
        crate::cancellation::check()?;
        if self
            .knock_in
            .is_some_and(|ki| ki.observation == KnockInObservation::AtExpiry)
            && !self.calendar.is_trading_day(self.expiry_time.date())
        {
            return Err(Error::InvalidInput(
                "knock-in observation is not a trading day".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn maturity_coupon_rate(&self) -> f64 {
        match self.coupon {
            Coupon::Snowball(rate) => rate,
            Coupon::Phoenix(_) => 0.0,
        }
    }

    fn redemption(&self) -> Redemption {
        self.knock_in
            .map_or(Redemption::FixedCoupon(self.maturity_coupon_rate()), |ki| {
                ki.redemption
            })
    }

    pub(crate) fn downside(&self) -> Option<Downside> {
        match self.redemption() {
            Redemption::Downside(d) => Some(d),
            Redemption::FixedCoupon(_) => None,
        }
    }

    pub(crate) fn reference_spot(&self) -> f64 {
        match self.knock_in {
            Some(KnockIn {
                redemption: Redemption::Downside(d),
                ..
            }) => d.initial_spot,
            Some(ki) => ki.level,
            None => self.autocall_observations[0].knock_out_level,
        }
    }

    pub(crate) fn daily_knock_in(&self) -> bool {
        self.knock_in
            .is_some_and(|ki| matches!(ki.observation, KnockInObservation::EveryTradingDay { .. }))
    }

    pub(crate) fn below_knock_in(&self, spot: f64) -> bool {
        self.knock_in.is_some_and(|ki| spot < ki.level)
    }

    fn validate_history(&self, barrier_state: Option<AutocallableBarrierState>) -> Result<()> {
        if self.knock_in.is_none() && barrier_state == Some(AutocallableBarrierState::KnockedIn) {
            return Err(Error::InvalidInput(
                "knock-in state requires a knock-in barrier".into(),
            ));
        }
        Ok(())
    }

    fn validate_observation_times(
        &self,
        times: impl ExactSizeIterator<Item = Timestamp>,
    ) -> Result<()> {
        if times.len() == 0 {
            return Err(Error::InvalidInput("empty observation schedule".into()));
        }
        let mut previous = None;
        for (index, time) in times.enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            if time < self.effective_time
                || time > self.expiry_time
                || previous.is_some_and(|previous| previous >= time)
            {
                return Err(Error::InvalidInput(
                    "observations must be ordered within life".into(),
                ));
            }
            previous = Some(time);
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let terms = self;
        if let Some(d) = terms.downside() {
            for (field, value) in [
                ("initial_spot", d.initial_spot),
                ("upper_strike", d.upper_strike),
            ] {
                finite(value, true, field)?;
            }
            finite(d.lower_strike, false, "lower_strike")?;
            if d.lower_strike < 0.0 || d.lower_strike > d.upper_strike {
                return Err(Error::InvalidInput("invalid lower strike".into()));
            }
        }
        finite(
            terms.principal_redemption_ratio,
            false,
            "principal_redemption_ratio",
        )?;
        if terms.principal_redemption_ratio < 0.0 {
            return Err(Error::InvalidInput(
                "negative autocallable principal".into(),
            ));
        }
        for (field, rate) in [
            ("rebate_rate", terms.rebate_rate),
            ("annual_rebate_rate", terms.annual_rebate_rate),
        ] {
            finite(rate, false, field)?;
            if rate < 0.0 {
                return Err(Error::InvalidInput(format!("negative {field}").into()));
            }
        }
        if terms.expiry_time < terms.effective_time {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        if matches!(terms.coupon, Coupon::Phoenix(_)) {
            terms.validate_observation_times(
                terms
                    .knock_out_observations
                    .iter()
                    .map(|record| record.time),
            )?;
            terms.validate_observation_times(
                terms.coupon_observations.iter().map(|record| record.time),
            )?;
        } else {
            terms.validate_observation_times(
                terms.autocall_observations.iter().map(|record| record.time),
            )?;
        }
        for chunk in terms.autocall_observations.chunks(1024) {
            crate::cancellation::check()?;
            for record in chunk {
                record.validate()?;
            }
        }
        terms.validate_history(terms.barrier_state)?;
        if let Some(ki) = terms.knock_in {
            finite(ki.level, true, "knock_in_level")?;
        }
        if let Coupon::Snowball(coupon) = terms.coupon {
            finite(coupon, false, "maturity_coupon_rate")?;
        }
        if let Redemption::FixedCoupon(coupon) = terms.redemption() {
            finite(coupon, false, "minimum_coupon_rate")?;
            if coupon > terms.maturity_coupon_rate() {
                return Err(Error::InvalidInput(
                    "minimum coupon exceeds maturity coupon".into(),
                ));
            }
        }
        if let Coupon::Phoenix(coupon) = terms.coupon {
            finite(coupon, false, "coupon_per_period")?;
        }
        Ok(())
    }
}

/// Selects replacements for [`SnowballOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct SnowballOptionChanges {
    /// Replaces the owned monitoring calendar; see [`SnowballOption::replace`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`maturity_coupon_rate`](SnowballTerms::maturity_coupon_rate).
    pub maturity_coupon_rate: Option<f64>,
    /// Replaces [`initial_spot`](SnowballTerms::initial_spot).
    pub initial_spot: Option<f64>,
    /// Replaces [`knock_in_level`](SnowballTerms::knock_in_level).
    pub knock_in_level: Option<f64>,
    /// Replaces [`upper_strike`](SnowballTerms::upper_strike).
    pub upper_strike: Option<f64>,
    /// Replaces [`lower_strike`](SnowballTerms::lower_strike).
    pub lower_strike: Option<f64>,
    /// Replaces [`knock_in_observation`](SnowballTerms::knock_in_observation).
    pub knock_in_observation: Option<KnockInObservation>,
    /// Replaces [`barrier_state`](SnowballTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_redemption_ratio`](SnowballTerms::principal_redemption_ratio).
    pub principal_redemption_ratio: Option<f64>,
    /// Replaces [`rebate_rate`](SnowballTerms::rebate_rate).
    pub rebate_rate: Option<f64>,
    /// Replaces [`annual_rebate_rate`](SnowballTerms::annual_rebate_rate).
    pub annual_rebate_rate: Option<f64>,
    /// Replaces [`effective_time`](SnowballTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](SnowballTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`autocall_observations`](SnowballTerms::autocall_observations).
    pub autocall_observations: Option<Vec<SnowballObservation>>,
}

impl SnowballOption {
    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// See [`SnowballOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract.
    pub fn replace(&self, changes: SnowballOptionChanges) -> Result<Self> {
        Self::new(SnowballTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            maturity_coupon_rate: changes
                .maturity_coupon_rate
                .unwrap_or(self.0.maturity_coupon_rate),
            initial_spot: changes.initial_spot.unwrap_or(self.0.initial_spot),
            knock_in_level: changes.knock_in_level.unwrap_or(self.0.knock_in_level),
            upper_strike: changes.upper_strike.unwrap_or(self.0.upper_strike),
            lower_strike: changes.lower_strike.unwrap_or(self.0.lower_strike),
            knock_in_observation: changes
                .knock_in_observation
                .unwrap_or(self.0.knock_in_observation),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_redemption_ratio: changes
                .principal_redemption_ratio
                .unwrap_or(self.0.principal_redemption_ratio),
            rebate_rate: changes.rebate_rate.unwrap_or(self.0.rebate_rate),
            annual_rebate_rate: changes
                .annual_rebate_rate
                .unwrap_or(self.0.annual_rebate_rate),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            autocall_observations: changes
                .autocall_observations
                .unwrap_or_else(|| self.0.autocall_observations.clone()),
        })
    }
}

/// Selects replacements for [`PhoenixOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct PhoenixOptionChanges {
    /// Replaces the owned monitoring calendar; see [`PhoenixOption::replace`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`coupon_per_period`](PhoenixTerms::coupon_per_period).
    pub coupon_per_period: Option<f64>,
    /// Replaces [`initial_spot`](PhoenixTerms::initial_spot).
    pub initial_spot: Option<f64>,
    /// Replaces [`knock_in_level`](PhoenixTerms::knock_in_level).
    pub knock_in_level: Option<f64>,
    /// Replaces [`upper_strike`](PhoenixTerms::upper_strike).
    pub upper_strike: Option<f64>,
    /// Replaces [`lower_strike`](PhoenixTerms::lower_strike).
    pub lower_strike: Option<f64>,
    /// Replaces [`knock_in_observation`](PhoenixTerms::knock_in_observation).
    pub knock_in_observation: Option<KnockInObservation>,
    /// Replaces [`barrier_state`](PhoenixTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_redemption_ratio`](PhoenixTerms::principal_redemption_ratio).
    pub principal_redemption_ratio: Option<f64>,
    /// Replaces [`rebate_rate`](PhoenixTerms::rebate_rate).
    pub rebate_rate: Option<f64>,
    /// Replaces [`annual_rebate_rate`](PhoenixTerms::annual_rebate_rate).
    pub annual_rebate_rate: Option<f64>,
    /// Replaces [`effective_time`](PhoenixTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](PhoenixTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`knock_out_observations`](PhoenixTerms::knock_out_observations).
    pub knock_out_observations: Option<Vec<KnockOutObservation>>,
    /// Replaces [`coupon_observations`](PhoenixTerms::coupon_observations).
    pub coupon_observations: Option<Vec<CouponObservation>>,
}

impl PhoenixOption {
    /// Borrows the knock-out observations; see [`Self::new`].
    pub fn knock_out_observations(&self) -> &[KnockOutObservation] {
        &self.0.knock_out_observations
    }
    /// Borrows the coupon observations; see [`Self::new`].
    pub fn coupon_observations(&self) -> &[CouponObservation] {
        &self.0.coupon_observations
    }

    /// Returns a note with the supplied finite signed coupon per observation.
    ///
    /// Preserves history and other terms. Apply [`crate::implied_phoenix_coupon`]
    /// results directly as periodic coupon rates.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite coupon.
    ///
    /// # Examples
    ///
    /// See [`crate::implied_phoenix_coupon`] for solving and applying a quote.
    pub fn with_coupon(&self, coupon_per_period: f64) -> Result<Self> {
        self.replace(PhoenixOptionChanges {
            coupon_per_period: Some(coupon_per_period),
            ..Default::default()
        })
    }

    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// See [`PhoenixOption::new`] for construction examples.
    /// Replacing observations does not regenerate preset thresholds.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract.
    pub fn replace(&self, changes: PhoenixOptionChanges) -> Result<Self> {
        Self::new(PhoenixTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            coupon_per_period: changes
                .coupon_per_period
                .unwrap_or(self.0.coupon_per_period),
            initial_spot: changes.initial_spot.unwrap_or(self.0.initial_spot),
            knock_in_level: changes.knock_in_level.unwrap_or(self.0.knock_in_level),
            upper_strike: changes.upper_strike.unwrap_or(self.0.upper_strike),
            lower_strike: changes.lower_strike.unwrap_or(self.0.lower_strike),
            knock_in_observation: changes
                .knock_in_observation
                .unwrap_or(self.0.knock_in_observation),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_redemption_ratio: changes
                .principal_redemption_ratio
                .unwrap_or(self.0.principal_redemption_ratio),
            rebate_rate: changes.rebate_rate.unwrap_or(self.0.rebate_rate),
            annual_rebate_rate: changes
                .annual_rebate_rate
                .unwrap_or(self.0.annual_rebate_rate),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            knock_out_observations: changes
                .knock_out_observations
                .unwrap_or_else(|| self.0.knock_out_observations.clone()),
            coupon_observations: changes
                .coupon_observations
                .unwrap_or_else(|| self.0.coupon_observations.clone()),
        })
    }
}

/// Selects replacements for [`BinarySnowballOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct BinarySnowballOptionChanges {
    /// Replaces the owned monitoring calendar; see [`BinarySnowballOption::replace`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`maturity_coupon_rate`](BinarySnowballTerms::maturity_coupon_rate).
    pub maturity_coupon_rate: Option<f64>,
    /// Replaces [`barrier_state`](BinarySnowballTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_redemption_ratio`](BinarySnowballTerms::principal_redemption_ratio).
    pub principal_redemption_ratio: Option<f64>,
    /// Replaces [`rebate_rate`](BinarySnowballTerms::rebate_rate).
    pub rebate_rate: Option<f64>,
    /// Replaces [`annual_rebate_rate`](BinarySnowballTerms::annual_rebate_rate).
    pub annual_rebate_rate: Option<f64>,
    /// Replaces [`effective_time`](BinarySnowballTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](BinarySnowballTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`autocall_observations`](BinarySnowballTerms::autocall_observations).
    pub autocall_observations: Option<Vec<SnowballObservation>>,
}

impl BinarySnowballOption {
    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// See [`BinarySnowballOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract.
    pub fn replace(&self, changes: BinarySnowballOptionChanges) -> Result<Self> {
        Self::new(BinarySnowballTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            maturity_coupon_rate: changes
                .maturity_coupon_rate
                .unwrap_or(self.0.maturity_coupon_rate),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_redemption_ratio: changes
                .principal_redemption_ratio
                .unwrap_or(self.0.principal_redemption_ratio),
            rebate_rate: changes.rebate_rate.unwrap_or(self.0.rebate_rate),
            annual_rebate_rate: changes
                .annual_rebate_rate
                .unwrap_or(self.0.annual_rebate_rate),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            autocall_observations: changes
                .autocall_observations
                .unwrap_or_else(|| self.0.autocall_observations.clone()),
        })
    }
}

/// Selects replacements for [`TernarySnowballOption::replace`]. Absent fields default to the source values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct TernarySnowballOptionChanges {
    /// Replaces the owned monitoring calendar; see [`TernarySnowballOption::replace`].
    pub calendar: Option<TradingCalendar>,
    /// Replaces [`maturity_coupon_rate`](TernarySnowballTerms::maturity_coupon_rate).
    pub maturity_coupon_rate: Option<f64>,
    /// Replaces [`minimum_coupon_rate`](TernarySnowballTerms::minimum_coupon_rate).
    pub minimum_coupon_rate: Option<f64>,
    /// Replaces [`knock_in_level`](TernarySnowballTerms::knock_in_level).
    pub knock_in_level: Option<f64>,
    /// Replaces [`knock_in_observation`](TernarySnowballTerms::knock_in_observation).
    pub knock_in_observation: Option<KnockInObservation>,
    /// Replaces [`barrier_state`](TernarySnowballTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_redemption_ratio`](TernarySnowballTerms::principal_redemption_ratio).
    pub principal_redemption_ratio: Option<f64>,
    /// Replaces [`rebate_rate`](TernarySnowballTerms::rebate_rate).
    pub rebate_rate: Option<f64>,
    /// Replaces [`annual_rebate_rate`](TernarySnowballTerms::annual_rebate_rate).
    pub annual_rebate_rate: Option<f64>,
    /// Replaces [`effective_time`](TernarySnowballTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](TernarySnowballTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`autocall_observations`](TernarySnowballTerms::autocall_observations).
    pub autocall_observations: Option<Vec<SnowballObservation>>,
}

impl TernarySnowballOption {
    /// Returns a validated copy with the supplied terms applied.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// See [`TernarySnowballOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract.
    pub fn replace(&self, changes: TernarySnowballOptionChanges) -> Result<Self> {
        Self::new(TernarySnowballTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
            maturity_coupon_rate: changes
                .maturity_coupon_rate
                .unwrap_or(self.0.maturity_coupon_rate),
            minimum_coupon_rate: changes
                .minimum_coupon_rate
                .unwrap_or(self.0.minimum_coupon_rate),
            knock_in_level: changes.knock_in_level.unwrap_or(self.0.knock_in_level),
            knock_in_observation: changes
                .knock_in_observation
                .unwrap_or(self.0.knock_in_observation),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_redemption_ratio: changes
                .principal_redemption_ratio
                .unwrap_or(self.0.principal_redemption_ratio),
            rebate_rate: changes.rebate_rate.unwrap_or(self.0.rebate_rate),
            annual_rebate_rate: changes
                .annual_rebate_rate
                .unwrap_or(self.0.annual_rebate_rate),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            autocall_observations: changes
                .autocall_observations
                .unwrap_or_else(|| self.0.autocall_observations.clone()),
        })
    }
}

impl<T: SnowballCouponTerms> AutocallableNote<T> {
    /// Borrows the contractual snowball observations; see [`Self::new`].
    pub fn autocall_observations(&self) -> &[SnowballObservation] {
        self.view().autocall_observations
    }
}
