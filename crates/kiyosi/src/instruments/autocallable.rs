mod presets;
pub(crate) mod pricing;
pub use presets::{PhoenixPreset, PhoenixPresetTerms, SnowballPreset, SnowballPresetTerms};

use crate::error::finite;
use crate::{Error, OptionContract, PricingContext, Result, Timestamp, TradingCalendar};

/// Supplies unit principal for autocallable terms; see [`SnowballTerms::new`].
pub const DEFAULT_PRINCIPAL_RATIO: f64 = 1.0;

/// Selects strict downside monitoring; see [`SnowballTerms::new`] for construction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KnockInObservationMode {
    /// Observes at the expiry clock time on trading times within the inclusive contract life.
    #[default]
    EveryTradingDay,
    /// Observes only at expiry.
    AtExpiry,
}

/// Records events strictly before valuation; see [`SnowballOption::replace_history`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutocallableBarrierState {
    /// Records known absence of prior events, distinct from unknown history.
    NoEvent,
    /// Records a settled knock-out; no remaining cash flows are valued.
    KnockedOut,
    /// Records an earlier downside breach.
    KnockedIn,
}

/// Associates one observation time with its contractual levels and coupon.
///
/// Values are validated by the note constructor, not by this input record.
/// See [`autocallable_observations`] for sequence construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutocallableObservation {
    /// Sets the exact observation time.
    pub time: Timestamp,
    /// Sets the positive inclusive knock-out threshold.
    pub knock_out_level: f64,
    /// Sets the finite annual snowball knock-out coupon; must be zero for Phoenix.
    pub coupon_rate: f64,
    /// Sets the inclusive nonnegative Phoenix coupon threshold; must be zero for snowballs.
    pub coupon_barrier_level: f64,
}
impl AutocallableObservation {
    /// Supplies a zero observation coupon; see [`Self::new`].
    pub const DEFAULT_COUPON_RATE: f64 = 0.0;
    /// Supplies a zero coupon barrier; see [`Self::new`].
    pub const DEFAULT_COUPON_BARRIER_LEVEL: f64 = 0.0;

    /// Creates an input record with zero coupon and coupon barrier.
    ///
    /// See [`autocallable_observations`] for an example.
    pub fn new(time: Timestamp, knock_out_level: f64) -> Self {
        Self {
            time,
            knock_out_level,
            coupon_rate: Self::DEFAULT_COUPON_RATE,
            coupon_barrier_level: Self::DEFAULT_COUPON_BARRIER_LEVEL,
        }
    }
}

/// Associates parallel sequences, using core record defaults for omitted coupons.
///
/// Contractual values and ordering are validated when constructing a note.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] for unequal sequence lengths.
///
/// # Examples
///
/// ```rust
/// use kiyosi::{Date, autocallable_observations};
/// # fn main() -> kiyosi::Result<()> {
/// let times = [Date::new(2026, 1, 1)?.start_of_day()];
/// let records = autocallable_observations(&times, &[110.0], Some(&[0.08]), None)?;
/// assert_eq!(records[0].coupon_rate, 0.08);
/// # Ok(()) }
/// ```
pub fn autocallable_observations(
    times: &[Timestamp],
    levels: &[f64],
    coupons: Option<&[f64]>,
    coupon_barriers: Option<&[f64]>,
) -> Result<Vec<AutocallableObservation>> {
    if times.len() != levels.len()
        || coupons.is_some_and(|v| v.len() != times.len())
        || coupon_barriers.is_some_and(|v| v.len() != times.len())
    {
        return Err(Error::InvalidInput(
            "observation sequence lengths differ".into(),
        ));
    }
    Ok(times
        .iter()
        .zip(levels)
        .enumerate()
        .map(|(i, (&time, &level))| {
            let mut record = AutocallableObservation::new(time, level);
            if let Some(values) = coupons {
                record.coupon_rate = values[i];
            }
            if let Some(values) = coupon_barriers {
                record.coupon_barrier_level = values[i];
            }
            record
        })
        .collect())
}

/// Supplies basic snowball terms; see [`Self::new`] and [`SnowballOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct SnowballTerms {
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
    /// Selects knock-in monitoring; defaults to every trading day.
    pub knock_in_observation_mode: KnockInObservationMode,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons and losses.
    pub principal_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry time, which must be a trading day when pricing.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered observations within the note life.
    pub observations: Vec<AutocallableObservation>,
}
impl SnowballTerms {
    /// Creates input terms with daily monitoring, unknown history and unit principal.
    ///
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
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            maturity_coupon_rate,
            initial_spot,
            knock_in_level,
            upper_strike,
            lower_strike,
            knock_in_observation_mode: KnockInObservationMode::default(),
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_time,
            expiry_time,
            observations,
        }
    }
}

/// Selects annual accrual or a fixed payment per qualifying Phoenix observation.
///
/// See [`PhoenixPresetTerms::expand`] for a fixed-payment example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhoenixCoupon {
    /// Accrues the signed annual rate over each observation period on Actual/365 Fixed.
    Annual(f64),
    /// Pays the signed proportion once per qualifying observation, regardless of period length.
    PerPeriod(f64),
}
impl PhoenixCoupon {
    fn value(self) -> f64 {
        match self {
            Self::Annual(value) | Self::PerPeriod(value) => value,
        }
    }
}

/// Supplies Phoenix terms; see [`Self::new`] and [`PhoenixOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct PhoenixTerms {
    /// Sets the finite signed coupon and its contractual payment convention.
    pub coupon: PhoenixCoupon,
    /// Sets the positive reference spot used to normalize downside losses.
    pub initial_spot: f64,
    /// Sets the positive strict downside threshold.
    pub knock_in_level: f64,
    /// Sets the positive upper settlement strike.
    pub upper_strike: f64,
    /// Sets the nonnegative lower settlement strike, at most the upper strike.
    pub lower_strike: f64,
    /// Selects knock-in monitoring; defaults to every trading day.
    pub knock_in_observation_mode: KnockInObservationMode,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons and losses.
    pub principal_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry time, which must be a trading day when pricing.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered observations within the note life.
    pub observations: Vec<AutocallableObservation>,
}
impl PhoenixTerms {
    /// Creates annual-coupon terms with daily monitoring, unknown history and unit principal.
    ///
    /// Validation occurs in [`PhoenixOption::new`]; see its example.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit contractual input terms"
    )]
    pub fn new(
        coupon_rate: f64,
        initial_spot: f64,
        knock_in_level: f64,
        upper_strike: f64,
        lower_strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            coupon: PhoenixCoupon::Annual(coupon_rate),
            initial_spot,
            knock_in_level,
            upper_strike,
            lower_strike,
            knock_in_observation_mode: KnockInObservationMode::default(),
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_time,
            expiry_time,
            observations,
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
    /// Sets the finite annual coupon rate used at maturity if no knock-out occurs.
    pub maturity_coupon_rate: f64,
    /// Records prior events; knock-in is rejected because this product has no knock-in barrier.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons.
    pub principal_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry time, which must be a trading day when pricing.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered observations within the note life.
    pub observations: Vec<AutocallableObservation>,
}
impl BinarySnowballTerms {
    /// Creates input terms with unknown history and unit principal.
    ///
    /// Validation occurs in [`BinarySnowballOption::new`]; see its example.
    pub fn new(
        maturity_coupon_rate: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            maturity_coupon_rate,
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_time,
            expiry_time,
            observations,
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
    /// Sets the finite annual coupon rate used at maturity if no knock-out occurs.
    pub maturity_coupon_rate: f64,
    /// Sets the finite annual coupon rate after knock-in, at most the maturity coupon rate.
    pub minimum_coupon_rate: f64,
    /// Sets the positive strict downside threshold.
    pub knock_in_level: f64,
    /// Selects knock-in monitoring; defaults to every trading day.
    pub knock_in_observation_mode: KnockInObservationMode,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons.
    pub principal_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry time, which must be a trading day when pricing.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly ordered observations within the note life.
    pub observations: Vec<AutocallableObservation>,
}
impl TernarySnowballTerms {
    /// Creates input terms with daily monitoring, unknown history and unit principal.
    ///
    /// Validation occurs in [`TernarySnowballOption::new`]; see its example.
    pub fn new(
        maturity_coupon_rate: f64,
        minimum_coupon_rate: f64,
        knock_in_level: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            maturity_coupon_rate,
            minimum_coupon_rate,
            knock_in_level,
            knock_in_observation_mode: KnockInObservationMode::default(),
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_time,
            expiry_time,
            observations,
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
/// Pays the signed [`PhoenixCoupon`] when spot is at or above its coupon barrier,
/// including on knock-out. Annual coupons use each observation period's Actual/365
/// Fixed length; fixed per-period coupons pay once without day accrual.
/// The first annual period starts at the effective time; later periods start at
/// the preceding contractual observation, even when pricing a seasoned note.
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
            fn with_coupon(&self, coupon: f64, convention: crate::CouponQuoteConvention) -> Self {
                let mut terms = self.clone();
                let shift = coupon - terms.observations.first().map_or(0.0, |record| record.coupon_rate);
                for record in &mut terms.observations {
                    record.coupon_rate += shift;
                }
                if convention == crate::CouponQuoteConvention::ShiftMaturityCoupon {
                    terms.maturity_coupon_rate += shift;
                }
                terms
            }
        }
    )+};
}
snowball_coupon!(SnowballTerms, BinarySnowballTerms, TernarySnowballTerms);

impl<T: SnowballCouponTerms> AutocallableNote<T> {
    /// Returns an owning note with the observation coupon schedule shifted to `coupon`.
    ///
    /// The first observation anchors the shift, including past observations.
    /// `convention` controls the maturity coupon; the ternary minimum stays fixed.
    /// See [`crate::implied_snowball_coupon`] for an example.
    ///
    /// # Errors
    ///
    /// Propagates term validation errors, including a shifted maturity coupon below
    /// the ternary minimum. This note remains unchanged on success and failure.
    pub fn with_coupon(
        &self,
        coupon: f64,
        convention: crate::CouponQuoteConvention,
    ) -> Result<Self> {
        Self::new(self.0.with_coupon(coupon, convention))
    }
}

impl PhoenixOption {
    /// Returns an owning note with its coupon value replaced in the existing units.
    ///
    /// See [`crate::implied_phoenix_coupon`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite coupon.
    /// This note remains unchanged on success and failure.
    pub fn with_coupon(&self, coupon: f64) -> Result<Self> {
        let mut terms = self.0.clone();
        terms.coupon = match terms.coupon {
            PhoenixCoupon::Annual(_) => PhoenixCoupon::Annual(coupon),
            PhoenixCoupon::PerPeriod(_) => PhoenixCoupon::PerPeriod(coupon),
        };
        Self::new(terms)
    }
}

mod sealed {
    use super::{
        AutocallableBarrierState, AutocallableObservation, KnockInObservationMode, PhoenixCoupon,
        Timestamp,
    };
    pub trait Terms {
        fn view(&self) -> View<'_>;
        fn set_history(&mut self, state: Option<AutocallableBarrierState>);
    }
    pub trait SnowballCoupon: Sized {
        fn with_coupon(&self, coupon: f64, convention: crate::CouponQuoteConvention) -> Self;
    }
    #[derive(Clone, Copy)]
    pub struct Downside {
        pub initial_spot: f64,
        pub upper_strike: f64,
        pub lower_strike: f64,
    }
    #[derive(Clone, Copy)]
    pub enum Redemption {
        Downside(Downside),
        FixedCoupon(f64),
    }
    #[derive(Clone, Copy)]
    pub struct KnockIn {
        pub level: f64,
        pub observation_mode: KnockInObservationMode,
        pub redemption: Redemption,
    }
    #[derive(Clone, Copy)]
    pub enum Coupon {
        Snowball(f64),
        Phoenix(PhoenixCoupon),
    }
    pub struct View<'a> {
        pub knock_in: Option<KnockIn>,
        pub coupon: Coupon,
        pub barrier_state: Option<AutocallableBarrierState>,
        pub principal_ratio: f64,
        pub effective_time: Timestamp,
        pub expiry_time: Timestamp,
        pub observations: &'a [AutocallableObservation],
    }
}
pub(crate) use sealed::Coupon;
use sealed::{Downside, KnockIn, Redemption};

macro_rules! terms_view {
    ($terms:ty, $coupon:path, $rate:ident) => {
        impl sealed::Terms for $terms {
            fn set_history(&mut self, state: Option<AutocallableBarrierState>) {
                self.barrier_state = state;
            }
            fn view(&self) -> sealed::View<'_> {
                sealed::View {
                    knock_in: Some(KnockIn {
                        level: self.knock_in_level,
                        observation_mode: self.knock_in_observation_mode,
                        redemption: Redemption::Downside(Downside {
                            initial_spot: self.initial_spot,
                            upper_strike: self.upper_strike,
                            lower_strike: self.lower_strike,
                        }),
                    }),
                    coupon: $coupon(self.$rate),
                    barrier_state: self.barrier_state,
                    principal_ratio: self.principal_ratio,
                    effective_time: self.effective_time,
                    expiry_time: self.expiry_time,
                    observations: &self.observations,
                }
            }
        }
    };
}
terms_view!(SnowballTerms, Coupon::Snowball, maturity_coupon_rate);
terms_view!(PhoenixTerms, Coupon::Phoenix, coupon);

impl sealed::Terms for BinarySnowballTerms {
    fn set_history(&mut self, state: Option<AutocallableBarrierState>) {
        self.barrier_state = state;
    }
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            knock_in: None,
            coupon: Coupon::Snowball(self.maturity_coupon_rate),
            barrier_state: self.barrier_state,
            principal_ratio: self.principal_ratio,
            effective_time: self.effective_time,
            expiry_time: self.expiry_time,
            observations: &self.observations,
        }
    }
}

impl sealed::Terms for TernarySnowballTerms {
    fn set_history(&mut self, state: Option<AutocallableBarrierState>) {
        self.barrier_state = state;
    }
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            knock_in: Some(KnockIn {
                level: self.knock_in_level,
                observation_mode: self.knock_in_observation_mode,
                redemption: Redemption::FixedCoupon(self.minimum_coupon_rate),
            }),
            coupon: Coupon::Snowball(self.maturity_coupon_rate),
            barrier_state: self.barrier_state,
            principal_ratio: self.principal_ratio,
            effective_time: self.effective_time,
            expiry_time: self.expiry_time,
            observations: &self.observations,
        }
    }
}

/// Owns validated terms and immutable observations for a supported autocallable.
///
/// Accessors borrow stable storage for this owner's lifetime. Shared reads and
/// pricing are safe; mutation requires exclusive Rust access. See [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct AutocallableNote<T: AutocallableTerms>(T);
impl<T: AutocallableTerms> AutocallableNote<T> {
    /// Validates all terms before creating an owning note.
    ///
    /// Calendar and history time consistency are checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid numeric terms or
    /// product-incompatible observation fields, reversed life, or empty, unordered
    /// or out-of-life observations. Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let terms = PhoenixTerms::new(0.08, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     autocallable_observations(&[end], &[110.0], None, Some(&[90.0]))?);
    /// let note = PhoenixOption::new(terms)?;
    /// let known = note.replace_history(Some(AutocallableBarrierState::NoEvent))?;
    /// assert_eq!(known.observations(), note.observations());
    /// assert_eq!(note.barrier_state(), None);
    /// let records = autocallable_observations(&[end], &[110.0], Some(&[0.08]), None)?;
    /// let binary = BinarySnowballOption::new(BinarySnowballTerms::new(
    ///     0.06, start, end, records.clone()))?;
    /// let ternary = TernarySnowballOption::new(TernarySnowballTerms::new(
    ///     0.06, 0.02, 80.0, start, end, records))?;
    /// assert_eq!(binary.terms().principal_ratio, 1.0);
    /// assert_eq!(ternary.terms().minimum_coupon_rate, 0.02);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: T) -> Result<Self> {
        let view = terms.view();
        view.validate()?;
        Ok(Self(terms))
    }
    /// Returns a validated owning copy with only history replaced; preserves the original.
    ///
    /// `None` clears history; `Some(AutocallableBarrierState::NoEvent)` records no prior event.
    /// See [`Self::new`] for an example. Time consistency is checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a knock-in state on a binary snowball.
    /// Other terms retain their validated values; this note remains unchanged.
    pub fn replace_history(&self, barrier_state: Option<AutocallableBarrierState>) -> Result<Self> {
        self.view().validate_history(barrier_state)?;
        let mut terms = self.0.clone();
        terms.set_history(barrier_state);
        Ok(Self(terms))
    }
    /// Borrows the complete validated terms; see [`Self::new`].
    pub fn terms(&self) -> &T {
        &self.0
    }
    /// Borrows immutable observation storage for this owner's lifetime; see [`Self::new`].
    pub fn observations(&self) -> &[AutocallableObservation] {
        self.view().observations
    }
    /// Returns prior history, or `None` for unknown; see [`Self::replace_history`].
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

    pub(crate) fn view(&self) -> sealed::View<'_> {
        self.0.view()
    }
    pub(crate) fn observation_coupon(&self, record: &AutocallableObservation, spot: f64) -> f64 {
        if spot < record.coupon_barrier_level {
            0.0
        } else {
            self.period_coupon(record)
        }
    }
    pub(crate) fn period_coupon(&self, record: &AutocallableObservation) -> f64 {
        let t = self.view();
        let rate = match t.coupon {
            Coupon::Snowball(_) => return 0.0,
            Coupon::Phoenix(PhoenixCoupon::PerPeriod(value)) => return value,
            Coupon::Phoenix(PhoenixCoupon::Annual(rate)) => rate,
        };
        let i = t.observations.partition_point(|r| r.time < record.time);
        let previous = if i == 0 {
            t.effective_time
        } else {
            t.observations[i - 1].time
        };
        rate * previous.years_until(record.time)
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
    pub(crate) fn prior(&self, time: Timestamp, calendar: &TradingCalendar) -> Result<bool> {
        let had_out = self.observations()[0].time < time;
        let had_in = self.view().daily_knock_in()
            && crate::ObservationSchedule::daily(
                self.effective_time(),
                self.expiry_time(),
                calendar,
                true,
            )?
            .times()
            .iter()
            .any(|&event| event < time);
        match self.barrier_state() {
            None if had_out || had_in => {
                return Err(Error::InvalidInput(
                    "prior autocallable state is required".into(),
                ));
            }
            Some(AutocallableBarrierState::KnockedOut) if !had_out => {
                return Err(Error::InvalidInput(
                    "knock-out predates observations".into(),
                ));
            }
            Some(AutocallableBarrierState::KnockedIn) if !had_in => {
                return Err(Error::InvalidInput("knock-in predates monitoring".into()));
            }
            _ => {}
        }
        Ok(self.barrier_state() == Some(AutocallableBarrierState::KnockedIn))
    }
    pub(crate) fn daily_at(&self, context: &PricingContext) -> bool {
        self.view().daily_knock_in()
            && self.daily_time(context.valuation_time(), context.calendar())
    }
    pub(crate) fn daily_time(&self, time: Timestamp, calendar: &TradingCalendar) -> bool {
        time >= self.effective_time()
            && time <= self.expiry_time()
            && time.micros_since_midnight() == self.expiry_time().micros_since_midnight()
            && calendar.is_trading_day(time.date())
    }

    pub(crate) fn terminal(&self, spot: f64, knocked_in: bool) -> f64 {
        let t = self.view();
        let coupon = if knocked_in {
            match t.redemption() {
                Redemption::Downside(d) => {
                    return t.principal_ratio
                        + (spot.clamp(d.lower_strike, d.upper_strike) - d.upper_strike)
                            / d.initial_spot;
                }
                Redemption::FixedCoupon(coupon) => coupon,
            }
        } else {
            t.maturity_coupon_rate()
        };
        t.principal_ratio + coupon * t.effective_time.years_until(t.expiry_time)
    }
    pub(crate) fn knock_out_payment(&self, record: &AutocallableObservation) -> f64 {
        let t = self.view();
        t.principal_ratio
            + if matches!(t.coupon, Coupon::Phoenix(_)) {
                0.0
            } else {
                record.coupon_rate
            } * self.effective_time().years_until(record.time)
    }
}
impl<T: AutocallableTerms> OptionContract for AutocallableNote<T> {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn all_greeks_unavailable(&self, _context: &PricingContext) -> bool {
        false
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        if self.barrier_state() == Some(AutocallableBarrierState::KnockedOut) {
            return false;
        }
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        if let Some(record) = self
            .observations()
            .iter()
            .find(|r| r.time == context.valuation_time())
        {
            if matches!(self.view().coupon, Coupon::Phoenix(_))
                && self.period_coupon(record) != 0.0
                && record.coupon_barrier_level > 0.0
                && crosses(record.coupon_barrier_level)
            {
                return true;
            }
            if crosses(record.knock_out_level) {
                return true;
            }
            if spot > record.knock_out_level {
                return false;
            }
        }
        if context.valuation_time() == self.expiry_time()
            && self.view().knock_in.is_some_and(|ki| {
                (self.barrier_state() == Some(AutocallableBarrierState::KnockedIn)
                    || self.view().below_knock_in(spot))
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
            && (self.daily_at(context) || context.valuation_time() == self.expiry_time())
            && self.view().knock_in.is_some_and(|ki| crosses(ki.level))
    }
    fn time_stencil_available(
        &self,
        context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        if self.prior(before, context.calendar()).is_err() {
            return false;
        }
        if self.barrier_state() == Some(AutocallableBarrierState::KnockedOut) {
            return true;
        }
        if self
            .observations()
            .iter()
            .any(|r| before <= r.time && r.time <= after)
        {
            return false;
        }
        if self.view().daily_knock_in()
            && self.barrier_state() != Some(AutocallableBarrierState::KnockedIn)
            && self.knock_in_affects_payoff()
        {
            let mut date = before.date();
            loop {
                let event = self.expiry_time().on_date(date);
                if before <= event && event <= after && self.daily_time(event, context.calendar()) {
                    return false;
                }
                if date == after.date() {
                    break;
                }
                let Ok(next) = date.add_days(1) else {
                    return false;
                };
                date = next;
            }
        }
        true
    }
}

impl sealed::View<'_> {
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
            None => self.observations[0].knock_out_level,
        }
    }

    pub(crate) fn daily_knock_in(&self) -> bool {
        self.knock_in
            .is_some_and(|ki| ki.observation_mode == KnockInObservationMode::EveryTradingDay)
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

    fn validate(&self) -> Result<()> {
        let terms = self;
        if let Some(d) = terms.downside() {
            for value in [d.initial_spot, d.upper_strike] {
                finite(value, true)?;
            }
            finite(d.lower_strike, false)?;
            if d.lower_strike < 0.0 || d.lower_strike > d.upper_strike {
                return Err(Error::InvalidInput("invalid lower strike".into()));
            }
        }
        finite(terms.principal_ratio, false)?;
        if terms.principal_ratio < 0.0 {
            return Err(Error::InvalidInput(
                "negative autocallable principal".into(),
            ));
        }
        if terms.expiry_time < terms.effective_time {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        if terms.observations.is_empty() {
            return Err(Error::InvalidInput("empty autocallable schedule".into()));
        }
        for (i, r) in terms.observations.iter().enumerate() {
            if r.time < terms.effective_time
                || r.time > terms.expiry_time
                || i > 0 && r.time <= terms.observations[i - 1].time
            {
                return Err(Error::InvalidInput(
                    "observations must be ordered within life".into(),
                ));
            }
        }
        for r in terms.observations {
            finite(r.knock_out_level, true)?;
        }
        terms.validate_history(terms.barrier_state)?;
        if let Some(ki) = terms.knock_in {
            finite(ki.level, true)?;
        }
        if let Redemption::FixedCoupon(coupon) = terms.redemption() {
            finite(coupon, false)?;
            if coupon > terms.maturity_coupon_rate() {
                return Err(Error::InvalidInput(
                    "minimum coupon exceeds maturity coupon".into(),
                ));
            }
        }
        if let Coupon::Phoenix(coupon) = terms.coupon {
            finite(coupon.value(), false)?;
            for r in terms.observations {
                finite(r.coupon_barrier_level, false)?;
                if r.coupon_barrier_level < 0.0 || r.coupon_rate != 0.0 {
                    return Err(Error::InvalidInput(
                        "invalid Phoenix observation terms".into(),
                    ));
                }
            }
        } else {
            for r in terms.observations {
                finite(r.coupon_rate, false)?;
            }
            finite(terms.maturity_coupon_rate(), false)?;
            if terms
                .observations
                .iter()
                .any(|r| r.coupon_barrier_level != 0.0)
            {
                return Err(Error::InvalidInput(
                    "snowballs do not support conditional observation coupons".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Selects replacements for [`SnowballOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct SnowballOptionChanges {
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
    /// Replaces [`knock_in_observation_mode`](SnowballTerms::knock_in_observation_mode).
    pub knock_in_observation_mode: Option<KnockInObservationMode>,
    /// Replaces [`barrier_state`](SnowballTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_ratio`](SnowballTerms::principal_ratio).
    pub principal_ratio: Option<f64>,
    /// Replaces [`effective_time`](SnowballTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](SnowballTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`observations`](SnowballTerms::observations).
    pub observations: Option<Vec<AutocallableObservation>>,
}

impl SnowballOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar and valuation-relative history checks remain at pricing time.
    /// See [`SnowballOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: SnowballOptionChanges) -> Result<Self> {
        Self::new(SnowballTerms {
            maturity_coupon_rate: changes
                .maturity_coupon_rate
                .unwrap_or(self.0.maturity_coupon_rate),
            initial_spot: changes.initial_spot.unwrap_or(self.0.initial_spot),
            knock_in_level: changes.knock_in_level.unwrap_or(self.0.knock_in_level),
            upper_strike: changes.upper_strike.unwrap_or(self.0.upper_strike),
            lower_strike: changes.lower_strike.unwrap_or(self.0.lower_strike),
            knock_in_observation_mode: changes
                .knock_in_observation_mode
                .unwrap_or(self.0.knock_in_observation_mode),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_ratio: changes.principal_ratio.unwrap_or(self.0.principal_ratio),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            observations: changes
                .observations
                .unwrap_or_else(|| self.0.observations.clone()),
        })
    }
}

/// Selects replacements for [`PhoenixOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct PhoenixOptionChanges {
    /// Replaces [`coupon`](PhoenixTerms::coupon).
    pub coupon: Option<PhoenixCoupon>,
    /// Replaces [`initial_spot`](PhoenixTerms::initial_spot).
    pub initial_spot: Option<f64>,
    /// Replaces [`knock_in_level`](PhoenixTerms::knock_in_level).
    pub knock_in_level: Option<f64>,
    /// Replaces [`upper_strike`](PhoenixTerms::upper_strike).
    pub upper_strike: Option<f64>,
    /// Replaces [`lower_strike`](PhoenixTerms::lower_strike).
    pub lower_strike: Option<f64>,
    /// Replaces [`knock_in_observation_mode`](PhoenixTerms::knock_in_observation_mode).
    pub knock_in_observation_mode: Option<KnockInObservationMode>,
    /// Replaces [`barrier_state`](PhoenixTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_ratio`](PhoenixTerms::principal_ratio).
    pub principal_ratio: Option<f64>,
    /// Replaces [`effective_time`](PhoenixTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](PhoenixTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`observations`](PhoenixTerms::observations).
    pub observations: Option<Vec<AutocallableObservation>>,
}

impl PhoenixOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar and valuation-relative history checks remain at pricing time.
    /// See [`PhoenixOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: PhoenixOptionChanges) -> Result<Self> {
        Self::new(PhoenixTerms {
            coupon: changes.coupon.unwrap_or(self.0.coupon),
            initial_spot: changes.initial_spot.unwrap_or(self.0.initial_spot),
            knock_in_level: changes.knock_in_level.unwrap_or(self.0.knock_in_level),
            upper_strike: changes.upper_strike.unwrap_or(self.0.upper_strike),
            lower_strike: changes.lower_strike.unwrap_or(self.0.lower_strike),
            knock_in_observation_mode: changes
                .knock_in_observation_mode
                .unwrap_or(self.0.knock_in_observation_mode),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_ratio: changes.principal_ratio.unwrap_or(self.0.principal_ratio),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            observations: changes
                .observations
                .unwrap_or_else(|| self.0.observations.clone()),
        })
    }
}

/// Selects replacements for [`BinarySnowballOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct BinarySnowballOptionChanges {
    /// Replaces [`maturity_coupon_rate`](BinarySnowballTerms::maturity_coupon_rate).
    pub maturity_coupon_rate: Option<f64>,
    /// Replaces [`barrier_state`](BinarySnowballTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_ratio`](BinarySnowballTerms::principal_ratio).
    pub principal_ratio: Option<f64>,
    /// Replaces [`effective_time`](BinarySnowballTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](BinarySnowballTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`observations`](BinarySnowballTerms::observations).
    pub observations: Option<Vec<AutocallableObservation>>,
}

impl BinarySnowballOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar and valuation-relative history checks remain at pricing time.
    /// See [`BinarySnowballOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: BinarySnowballOptionChanges) -> Result<Self> {
        Self::new(BinarySnowballTerms {
            maturity_coupon_rate: changes
                .maturity_coupon_rate
                .unwrap_or(self.0.maturity_coupon_rate),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_ratio: changes.principal_ratio.unwrap_or(self.0.principal_ratio),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            observations: changes
                .observations
                .unwrap_or_else(|| self.0.observations.clone()),
        })
    }
}

/// Selects replacements for [`TernarySnowballOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct TernarySnowballOptionChanges {
    /// Replaces [`maturity_coupon_rate`](TernarySnowballTerms::maturity_coupon_rate).
    pub maturity_coupon_rate: Option<f64>,
    /// Replaces [`minimum_coupon_rate`](TernarySnowballTerms::minimum_coupon_rate).
    pub minimum_coupon_rate: Option<f64>,
    /// Replaces [`knock_in_level`](TernarySnowballTerms::knock_in_level).
    pub knock_in_level: Option<f64>,
    /// Replaces [`knock_in_observation_mode`](TernarySnowballTerms::knock_in_observation_mode).
    pub knock_in_observation_mode: Option<KnockInObservationMode>,
    /// Replaces [`barrier_state`](TernarySnowballTerms::barrier_state).
    pub barrier_state: Option<Option<AutocallableBarrierState>>,
    /// Replaces [`principal_ratio`](TernarySnowballTerms::principal_ratio).
    pub principal_ratio: Option<f64>,
    /// Replaces [`effective_time`](TernarySnowballTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](TernarySnowballTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`observations`](TernarySnowballTerms::observations).
    pub observations: Option<Vec<AutocallableObservation>>,
}

impl TernarySnowballOption {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar and valuation-relative history checks remain at pricing time.
    /// See [`TernarySnowballOption::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: TernarySnowballOptionChanges) -> Result<Self> {
        Self::new(TernarySnowballTerms {
            maturity_coupon_rate: changes
                .maturity_coupon_rate
                .unwrap_or(self.0.maturity_coupon_rate),
            minimum_coupon_rate: changes
                .minimum_coupon_rate
                .unwrap_or(self.0.minimum_coupon_rate),
            knock_in_level: changes.knock_in_level.unwrap_or(self.0.knock_in_level),
            knock_in_observation_mode: changes
                .knock_in_observation_mode
                .unwrap_or(self.0.knock_in_observation_mode),
            barrier_state: changes.barrier_state.unwrap_or(self.0.barrier_state),
            principal_ratio: changes.principal_ratio.unwrap_or(self.0.principal_ratio),
            effective_time: changes.effective_time.unwrap_or(self.0.effective_time),
            expiry_time: changes.expiry_time.unwrap_or(self.0.expiry_time),
            observations: changes
                .observations
                .unwrap_or_else(|| self.0.observations.clone()),
        })
    }
}
