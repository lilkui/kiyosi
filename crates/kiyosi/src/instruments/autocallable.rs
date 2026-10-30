mod presets;
pub(crate) mod pricing;
pub use presets::{SnowballPreset, SnowballPresetTerms};

use crate::error::finite;
use crate::{Date, Error, OptionContract, PricingContext, Result, Timestamp, TradingCalendar};

/// Supplies unit principal for autocallable terms; see [`SnowballTerms::new`].
pub const DEFAULT_PRINCIPAL_RATIO: f64 = 1.0;

/// Selects strict downside monitoring; see [`SnowballTerms::new`] for construction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KnockInObservationMode {
    /// Observes at midnight on each trading date, including the effective date.
    #[default]
    EveryTradingDay,
    /// Observes only at expiry.
    AtExpiry,
}

/// Records events strictly before valuation; see [`SnowballOption::replace`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutocallableBarrierState {
    /// Records known absence of prior events, distinct from unknown history.
    None,
    /// Records a settled knock-out; no remaining cashflows are valued.
    KnockedOut,
    /// Records an earlier downside breach.
    KnockedIn,
}

/// Associates one observation date with its contractual levels and coupon.
///
/// Values are validated by the note constructor, not by this input record.
/// See [`autocallable_observations`] for sequence construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutocallableObservation {
    /// Sets the observation at midnight UTC.
    pub date: Date,
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
    pub fn new(date: Date, knock_out_level: f64) -> Self {
        Self {
            date,
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
/// let dates = [Date::new(2026, 1, 1)?];
/// let records = autocallable_observations(&dates, &[110.0], Some(&[0.08]), None)?;
/// assert_eq!(records[0].coupon_rate, 0.08);
/// # Ok(()) }
/// ```
pub fn autocallable_observations(
    dates: &[Date],
    levels: &[f64],
    coupons: Option<&[f64]>,
    coupon_barriers: Option<&[f64]>,
) -> Result<Vec<AutocallableObservation>> {
    if dates.len() != levels.len()
        || coupons.is_some_and(|v| v.len() != dates.len())
        || coupon_barriers.is_some_and(|v| v.len() != dates.len())
    {
        return Err(Error::InvalidInput(
            "observation sequence lengths differ".into(),
        ));
    }
    Ok(dates
        .iter()
        .zip(levels)
        .enumerate()
        .map(|(i, (&date, &level))| {
            let mut record = AutocallableObservation::new(date, level);
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
    /// Sets the finite annual maturity coupon when no knock-in occurs.
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
    /// Sets the inclusive first date.
    pub effective_date: Date,
    /// Sets the inclusive final date, which must be a trading day when pricing.
    pub expiry_date: Date,
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
        effective_date: Date,
        expiry_date: Date,
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
            effective_date,
            expiry_date,
            observations,
        }
    }
}

/// Supplies Phoenix terms; see [`Self::new`] and [`PhoenixOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct PhoenixTerms {
    /// Sets the finite annual coupon for each conditional observation period.
    pub coupon_rate: f64,
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
    /// Sets the inclusive first date.
    pub effective_date: Date,
    /// Sets the inclusive final date, which must be a trading day when pricing.
    pub expiry_date: Date,
    /// Supplies nonempty, strictly ordered observations within the note life.
    pub observations: Vec<AutocallableObservation>,
}
impl PhoenixTerms {
    /// Creates input terms with daily monitoring, unknown history and unit principal.
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
        effective_date: Date,
        expiry_date: Date,
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            coupon_rate,
            initial_spot,
            knock_in_level,
            upper_strike,
            lower_strike,
            knock_in_observation_mode: KnockInObservationMode::default(),
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_date,
            expiry_date,
            observations,
        }
    }
}

/// Owns validated basic snowball terms; see [`Self::new`].
///
/// Pays principal plus accrued annual coupon on knock-out, otherwise principal
/// plus maturity coupon if intact, or a capped downside loss after knock-in.
pub type SnowballOption = AutocallableNote<SnowballTerms>;

/// Supplies binary snowball terms; see [`Self::new`] and [`BinarySnowballOption::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinarySnowballTerms {
    /// Sets the finite annual coupon paid at maturity if no knock-out occurs.
    pub maturity_coupon_rate: f64,
    /// Records prior events; knock-in is rejected because this product has no knock-in barrier.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons.
    pub principal_ratio: f64,
    /// Sets the inclusive first date.
    pub effective_date: Date,
    /// Sets the inclusive final date, which must be a trading day when pricing.
    pub expiry_date: Date,
    /// Supplies nonempty, strictly ordered observations within the note life.
    pub observations: Vec<AutocallableObservation>,
}
impl BinarySnowballTerms {
    /// Creates input terms with unknown history and unit principal.
    ///
    /// Validation occurs in [`BinarySnowballOption::new`]; see its example.
    pub fn new(
        maturity_coupon_rate: f64,
        effective_date: Date,
        expiry_date: Date,
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            maturity_coupon_rate,
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_date,
            expiry_date,
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
    /// Sets the finite annual coupon paid at maturity if no knock-out occurs.
    pub maturity_coupon_rate: f64,
    /// Sets the finite annual coupon after knock-in, at most the maturity coupon.
    pub minimum_coupon_rate: f64,
    /// Sets the positive strict downside threshold.
    pub knock_in_level: f64,
    /// Selects knock-in monitoring; defaults to every trading day.
    pub knock_in_observation_mode: KnockInObservationMode,
    /// Records prior events; `None` means unknown history.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets the nonnegative normalized principal, independently of coupons.
    pub principal_ratio: f64,
    /// Sets the inclusive first date.
    pub effective_date: Date,
    /// Sets the inclusive final date, which must be a trading day when pricing.
    pub expiry_date: Date,
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
        effective_date: Date,
        expiry_date: Date,
        observations: Vec<AutocallableObservation>,
    ) -> Self {
        Self {
            maturity_coupon_rate,
            minimum_coupon_rate,
            knock_in_level,
            knock_in_observation_mode: KnockInObservationMode::default(),
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_date,
            expiry_date,
            observations,
        }
    }
}

/// Owns a ternary snowball with fixed intact and knocked-in maturity coupons.
///
/// Pays principal plus the accrued observation coupon on knock-out, otherwise
/// principal plus the accrued maturity or minimum coupon according to knock-in. See [`Self::new`] for construction.
pub type TernarySnowballOption = AutocallableNote<TernarySnowballTerms>;

/// Owns validated Phoenix terms; see [`Self::new`].
///
/// Pays a signed annual coupon times each observation period's Actual/365 Fixed
/// length when spot is at or above its coupon barrier, including on knock-out.
/// The first period starts at the effective date; later periods start at the
/// preceding contractual observation, even when pricing a seasoned note.
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
    /// Returns an owning note with its conditional annual coupon replaced.
    ///
    /// See [`crate::implied_coupon`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite coupon.
    /// This note remains unchanged on success and failure.
    pub fn with_coupon(&self, coupon: f64) -> Result<Self> {
        let mut terms = self.0.clone();
        terms.coupon_rate = coupon;
        Self::new(terms)
    }
}

mod sealed {
    use super::*;
    pub trait Terms {
        fn view(&self) -> View<'_>;
        fn set_history(&mut self, state: Option<AutocallableBarrierState>);
    }
    pub trait SnowballCoupon: Sized {
        fn with_coupon(&self, coupon: f64, convention: crate::CouponQuoteConvention) -> Self;
    }
    pub struct View<'a> {
        pub initial_spot: f64,
        pub knock_in_level: f64,
        pub upper_strike: f64,
        pub lower_strike: f64,
        pub knock_in_observation_mode: KnockInObservationMode,
        pub barrier_state: Option<AutocallableBarrierState>,
        pub principal_ratio: f64,
        pub effective_date: Date,
        pub expiry_date: Date,
        pub observations: &'a [AutocallableObservation],
        pub maturity_coupon_rate: f64,
        pub coupon_rate: f64,
        pub phoenix: bool,
        pub has_knock_in: bool,
        pub fixed_knocked_in_coupon: Option<f64>,
    }
}
macro_rules! terms_view {
    ($terms:ty, $maturity:expr, $coupon:expr, $phoenix:expr) => {
        impl sealed::Terms for $terms {
            fn set_history(&mut self, state: Option<AutocallableBarrierState>) {
                self.barrier_state = state;
            }
            fn view(&self) -> sealed::View<'_> {
                sealed::View {
                    initial_spot: self.initial_spot,
                    knock_in_level: self.knock_in_level,
                    upper_strike: self.upper_strike,
                    lower_strike: self.lower_strike,
                    knock_in_observation_mode: self.knock_in_observation_mode,
                    barrier_state: self.barrier_state,
                    principal_ratio: self.principal_ratio,
                    effective_date: self.effective_date,
                    expiry_date: self.expiry_date,
                    observations: &self.observations,
                    maturity_coupon_rate: $maturity(self),
                    coupon_rate: $coupon(self),
                    phoenix: $phoenix,
                    has_knock_in: true,
                    fixed_knocked_in_coupon: None,
                }
            }
        }
    };
}
terms_view!(
    SnowballTerms,
    |t: &SnowballTerms| t.maturity_coupon_rate,
    |_: &SnowballTerms| 0.0,
    false
);
terms_view!(
    PhoenixTerms,
    |_: &PhoenixTerms| 0.0,
    |t: &PhoenixTerms| t.coupon_rate,
    true
);

impl sealed::Terms for BinarySnowballTerms {
    fn set_history(&mut self, state: Option<AutocallableBarrierState>) {
        self.barrier_state = state;
    }
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            initial_spot: self.observations.first().map_or(1.0, |r| r.knock_out_level),
            knock_in_level: 1.0,
            upper_strike: 1.0,
            lower_strike: 1.0,
            knock_in_observation_mode: KnockInObservationMode::AtExpiry,
            barrier_state: self.barrier_state,
            principal_ratio: self.principal_ratio,
            effective_date: self.effective_date,
            expiry_date: self.expiry_date,
            observations: &self.observations,
            maturity_coupon_rate: self.maturity_coupon_rate,
            coupon_rate: 0.0,
            phoenix: false,
            has_knock_in: false,
            fixed_knocked_in_coupon: Some(self.maturity_coupon_rate),
        }
    }
}

impl sealed::Terms for TernarySnowballTerms {
    fn set_history(&mut self, state: Option<AutocallableBarrierState>) {
        self.barrier_state = state;
    }
    fn view(&self) -> sealed::View<'_> {
        sealed::View {
            initial_spot: self.knock_in_level,
            knock_in_level: self.knock_in_level,
            upper_strike: self.knock_in_level,
            lower_strike: self.knock_in_level,
            knock_in_observation_mode: self.knock_in_observation_mode,
            barrier_state: self.barrier_state,
            principal_ratio: self.principal_ratio,
            effective_date: self.effective_date,
            expiry_date: self.expiry_date,
            observations: &self.observations,
            maturity_coupon_rate: self.maturity_coupon_rate,
            coupon_rate: 0.0,
            phoenix: false,
            has_knock_in: true,
            fixed_knocked_in_coupon: Some(self.minimum_coupon_rate),
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
    /// let start = Date::new(2025, 1, 1)?;
    /// let end = Date::new(2026, 1, 1)?;
    /// let terms = PhoenixTerms::new(0.08, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     autocallable_observations(&[end], &[110.0], None, Some(&[90.0]))?);
    /// let note = PhoenixOption::new(terms)?;
    /// let known = note.replace(Some(AutocallableBarrierState::None))?;
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
    /// `None` clears history; `Some(AutocallableBarrierState::None)` records no prior event.
    /// See [`Self::new`] for an example. Time consistency is checked during pricing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a knock-in state on a binary snowball.
    /// Other terms retain their validated values; this note remains unchanged.
    pub fn replace(&self, barrier_state: Option<AutocallableBarrierState>) -> Result<Self> {
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
    /// Returns prior history, or `None` for unknown; see [`Self::replace`].
    pub fn barrier_state(&self) -> Option<AutocallableBarrierState> {
        self.view().barrier_state
    }
    /// Returns the inclusive first date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.view().effective_date
    }
    /// Returns expiry at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.view().expiry_date
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
        if !t.phoenix {
            return 0.0;
        }
        let i = t.observations.partition_point(|r| r.date < record.date);
        let previous = if i == 0 {
            t.effective_date
        } else {
            t.observations[i - 1].date
        };
        t.coupon_rate * previous.start_of_day().years_until(record.date.into())
    }
    pub(crate) fn knock_in_affects_payoff(&self) -> bool {
        if let Some(coupon) = self.view().fixed_knocked_in_coupon {
            return self.view().has_knock_in && coupon != self.view().maturity_coupon_rate;
        }
        self.view().lower_strike != self.view().upper_strike
            || self.view().maturity_coupon_rate != 0.0
    }
    pub(crate) fn prior(&self, time: Timestamp, calendar: &TradingCalendar) -> Result<bool> {
        let had_out = self.observations()[0].date.start_of_day() < time;
        let mut had_in = false;
        if self.view().knock_in_observation_mode == KnockInObservationMode::EveryTradingDay {
            let mut date = self.effective_date();
            while date.start_of_day() < time {
                if calendar.is_trading_day(date) {
                    had_in = true;
                    break;
                }
                date = date.add_days(1)?;
            }
        }
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
        self.view().knock_in_observation_mode == KnockInObservationMode::EveryTradingDay
            && context.valuation_time() == context.valuation_date().start_of_day()
            && context.calendar().is_trading_day(context.valuation_date())
    }
    pub(crate) fn terminal(&self, spot: f64, knocked_in: bool) -> f64 {
        let t = self.view();
        if knocked_in && t.fixed_knocked_in_coupon.is_none() {
            t.principal_ratio
                + (spot.clamp(t.lower_strike, t.upper_strike) - t.upper_strike) / t.initial_spot
        } else {
            t.principal_ratio
                + if knocked_in {
                    t.fixed_knocked_in_coupon.unwrap_or(t.maturity_coupon_rate)
                } else {
                    t.maturity_coupon_rate
                } * t
                    .effective_date
                    .start_of_day()
                    .years_until(t.expiry_date.into())
        }
    }
    pub(crate) fn knock_out_payment(&self, record: &AutocallableObservation) -> f64 {
        let t = self.view();
        t.principal_ratio
            + if t.phoenix { 0.0 } else { record.coupon_rate }
                * self
                    .effective_date()
                    .start_of_day()
                    .years_until(record.date.into())
    }
}
impl<T: AutocallableTerms> OptionContract for AutocallableNote<T> {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
    fn all_greeks_unavailable(&self, _context: &PricingContext) -> bool {
        false
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        if context.valuation_time() != context.valuation_date().start_of_day()
            || self.barrier_state() == Some(AutocallableBarrierState::KnockedOut)
        {
            return false;
        }
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        if let Some(record) = self
            .observations()
            .iter()
            .find(|r| r.date == context.valuation_date())
        {
            if self.view().phoenix
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
        self.barrier_state() != Some(AutocallableBarrierState::KnockedIn)
            && self.knock_in_affects_payoff()
            && self.daily_at(context)
            && crosses(self.view().knock_in_level)
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
            .any(|r| before <= r.date.start_of_day() && r.date.start_of_day() <= after)
        {
            return false;
        }
        if self.view().knock_in_observation_mode == KnockInObservationMode::EveryTradingDay
            && self.barrier_state() != Some(AutocallableBarrierState::KnockedIn)
            && self.knock_in_affects_payoff()
        {
            let mut date = before.date();
            loop {
                if before <= date.start_of_day() && context.calendar().is_trading_day(date) {
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
    fn validate_history(&self, barrier_state: Option<AutocallableBarrierState>) -> Result<()> {
        if !self.has_knock_in && barrier_state == Some(AutocallableBarrierState::KnockedIn) {
            return Err(Error::InvalidInput(
                "knock-in state requires a knock-in barrier".into(),
            ));
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let terms = self;
        if terms.fixed_knocked_in_coupon.is_none() {
            for value in [terms.initial_spot, terms.upper_strike] {
                finite(value, true)?;
            }
            finite(terms.lower_strike, false)?;
            if terms.lower_strike < 0.0 || terms.lower_strike > terms.upper_strike {
                return Err(Error::InvalidInput("invalid lower strike".into()));
            }
        }
        finite(terms.principal_ratio, false)?;
        if terms.principal_ratio < 0.0 {
            return Err(Error::InvalidInput(
                "negative autocallable principal".into(),
            ));
        }
        if terms.expiry_date < terms.effective_date {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        if terms.observations.is_empty() {
            return Err(Error::InvalidInput("empty autocallable schedule".into()));
        }
        for (i, r) in terms.observations.iter().enumerate() {
            if r.date < terms.effective_date
                || r.date > terms.expiry_date
                || i > 0 && r.date <= terms.observations[i - 1].date
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
        finite(terms.knock_in_level, true)?;
        if let Some(coupon) = terms.fixed_knocked_in_coupon {
            finite(coupon, false)?;
            if coupon > terms.maturity_coupon_rate {
                return Err(Error::InvalidInput(
                    "minimum coupon exceeds maturity coupon".into(),
                ));
            }
        }
        if terms.phoenix {
            finite(terms.coupon_rate, false)?;
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
            finite(terms.maturity_coupon_rate, false)?;
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
