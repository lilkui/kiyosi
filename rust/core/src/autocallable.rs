use crate::error::finite;
use crate::{
    Date, Error, ErrorCategory, OptionContract, PricingContext, Result, Timestamp, TradingCalendar,
};

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
/// Values are validated by the note constructor, not by this input record.
/// See [`autocallable_observations`] for sequence construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutocallableObservation {
    /// Sets the observation at midnight UTC.
    pub date: Date,
    /// Sets the positive inclusive knock-out threshold.
    pub knock_out_level: f64,
    /// Sets the finite annual coupon paid on knock-out.
    pub coupon_rate: f64,
    /// Reserves the conditional coupon threshold; must be zero for snowballs.
    pub coupon_barrier_level: f64,
}
impl AutocallableObservation {
    /// Creates an input record with zero coupon and coupon barrier.
    /// See [`autocallable_observations`] for an example.
    pub fn new(date: Date, knock_out_level: f64) -> Self {
        Self {
            date,
            knock_out_level,
            coupon_rate: 0.0,
            coupon_barrier_level: 0.0,
        }
    }
}

/// Associates parallel sequences, using core record defaults for omitted coupons.
/// Contractual values and ordering are validated when constructing a note.
///
/// # Errors
/// Returns [`ErrorCategory::InvalidSchedule`] for unequal sequence lengths.
///
/// # Examples
/// ```
/// use kiyosi_core::{Date, autocallable_observations};
/// # fn main() -> kiyosi_core::Result<()> {
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
        return Err(Error::new(
            ErrorCategory::InvalidSchedule,
            "observation sequence lengths differ",
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
            principal_ratio: 1.0,
            effective_date,
            expiry_date,
            observations,
        }
    }
}

/// Owns validated basic snowball terms and observations.
/// Pays principal plus accrued annual coupon on knock-out, otherwise principal
/// plus maturity coupon if intact, or a capped downside loss after knock-in.
/// Accessors borrow stable immutable data for this owner's lifetime. Shared reads
/// and pricing are safe; mutation requires exclusive Rust access.
/// See [`Self::new`] for an example.
#[derive(Clone, Debug, PartialEq)]
pub struct SnowballOption(SnowballTerms);
impl SnowballOption {
    /// Validates all terms before creating an owning note.
    /// Calendar and history time consistency are checked during pricing.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for invalid numeric terms or unsupported
    /// coupon barriers, [`ErrorCategory::InvalidTimeRange`] for reversed life, and
    /// [`ErrorCategory::InvalidSchedule`] for empty, unordered or out-of-life observations.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let end = Date::new(2026, 1, 1)?;
    /// let terms = SnowballTerms::new(0.08, 100.0, 80.0, 100.0, 0.0, start, end,
    ///     autocallable_observations(&[end], &[110.0], Some(&[0.08]), None)?);
    /// let note = SnowballOption::new(terms)?;
    /// let known = note.replace(Some(AutocallableBarrierState::None))?;
    /// assert_eq!(known.observations(), note.observations());
    /// assert_eq!(note.barrier_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: SnowballTerms) -> Result<Self> {
        let invalid = ErrorCategory::InvalidParameter;
        for value in [terms.initial_spot, terms.upper_strike] {
            finite(value, invalid, true)?;
        }
        for value in [terms.principal_ratio, terms.lower_strike] {
            finite(value, invalid, false)?;
            if value < 0.0 {
                return Err(Error::new(invalid, "negative snowball term"));
            }
        }
        if terms.lower_strike > terms.upper_strike {
            return Err(Error::new(invalid, "lower strike exceeds upper strike"));
        }
        if terms.expiry_date < terms.effective_date {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "expiry precedes effective date",
            ));
        }
        if terms.observations.is_empty() {
            return Err(Error::new(
                ErrorCategory::InvalidSchedule,
                "empty autocallable schedule",
            ));
        }
        for (i, record) in terms.observations.iter().enumerate() {
            if record.date < terms.effective_date
                || record.date > terms.expiry_date
                || i > 0 && record.date <= terms.observations[i - 1].date
            {
                return Err(Error::new(
                    ErrorCategory::InvalidSchedule,
                    "observations must be ordered within life",
                ));
            }
        }
        for record in &terms.observations {
            finite(record.knock_out_level, invalid, true)?;
        }
        finite(terms.knock_in_level, invalid, true)?;
        for record in &terms.observations {
            finite(record.coupon_rate, invalid, false)?;
        }
        finite(terms.maturity_coupon_rate, invalid, false)?;
        if terms
            .observations
            .iter()
            .any(|r| r.coupon_barrier_level != 0.0)
        {
            return Err(Error::new(
                invalid,
                "snowballs do not support conditional observation coupons",
            ));
        }
        Ok(Self(terms))
    }
    /// Returns a validated owning copy with only history replaced; preserves the original.
    /// `None` clears history; `Some(AutocallableBarrierState::None)` records no prior event.
    /// See [`Self::new`] for an example. Time consistency is checked during pricing.
    ///
    /// # Errors
    /// Propagates [`Self::new`] validation errors without changing this note.
    pub fn replace(&self, barrier_state: Option<AutocallableBarrierState>) -> Result<Self> {
        let mut terms = self.0.clone();
        terms.barrier_state = barrier_state;
        Self::new(terms)
    }
    /// Borrows the complete validated terms; see [`Self::new`].
    pub fn terms(&self) -> &SnowballTerms {
        &self.0
    }
    /// Borrows immutable observation storage for this owner's lifetime; see [`Self::new`].
    pub fn observations(&self) -> &[AutocallableObservation] {
        &self.0.observations
    }
    /// Returns prior history, or `None` for unknown; see [`Self::replace`].
    pub fn barrier_state(&self) -> Option<AutocallableBarrierState> {
        self.0.barrier_state
    }
    /// Returns the inclusive first date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.0.effective_date
    }
    /// Returns expiry at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.0.expiry_date
    }

    pub(crate) fn knock_in_affects_payoff(&self) -> bool {
        self.0.lower_strike != self.0.upper_strike || self.0.maturity_coupon_rate != 0.0
    }
    pub(crate) fn prior(&self, time: Timestamp, calendar: &TradingCalendar) -> Result<bool> {
        let had_out = self.observations()[0].date.start_of_day() < time;
        let mut had_in = false;
        if self.0.knock_in_observation_mode == KnockInObservationMode::EveryTradingDay {
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
                return Err(Error::new(
                    ErrorCategory::InvalidParameter,
                    "prior autocallable state is required",
                ));
            }
            Some(AutocallableBarrierState::KnockedOut) if !had_out => {
                return Err(Error::new(
                    ErrorCategory::InvalidOption,
                    "knock-out predates observations",
                ));
            }
            Some(AutocallableBarrierState::KnockedIn) if !had_in => {
                return Err(Error::new(
                    ErrorCategory::InvalidOption,
                    "knock-in predates monitoring",
                ));
            }
            _ => {}
        }
        Ok(self.barrier_state() == Some(AutocallableBarrierState::KnockedIn))
    }
    pub(crate) fn daily_at(&self, context: &PricingContext) -> bool {
        self.0.knock_in_observation_mode == KnockInObservationMode::EveryTradingDay
            && context.valuation_time() == context.valuation_date().start_of_day()
            && context.calendar().is_trading_day(context.valuation_date())
    }
    pub(crate) fn terminal(&self, spot: f64, knocked_in: bool) -> f64 {
        let t = &self.0;
        if knocked_in {
            t.principal_ratio
                + (spot.clamp(t.lower_strike, t.upper_strike) - t.upper_strike) / t.initial_spot
        } else {
            t.principal_ratio
                + t.maturity_coupon_rate
                    * t.effective_date
                        .start_of_day()
                        .years_until(t.expiry_date.into())
        }
    }
    pub(crate) fn knock_out_payment(&self, record: &AutocallableObservation) -> f64 {
        self.0.principal_ratio
            + record.coupon_rate
                * self
                    .effective_date()
                    .start_of_day()
                    .years_until(record.date.into())
    }
}
impl OptionContract for SnowballOption {
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
            && crosses(self.0.knock_in_level)
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
        if self.0.knock_in_observation_mode == KnockInObservationMode::EveryTradingDay
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
