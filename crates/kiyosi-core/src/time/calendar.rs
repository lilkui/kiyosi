use std::{fmt, sync::Arc};

use crate::{Date, Error, Result};

/// Selects the direction of inclusive business-day adjustment.
///
/// See [`TradingCalendar::adjust`] for usage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusinessDayConvention {
    /// Keeps a trading date or advances to the next one.
    Following,
    /// Keeps a trading date or retreats to the preceding one.
    Preceding,
}

#[derive(Clone)]
enum CalendarKind {
    AllDays,
    Weekdays,
    Sse,
    Predicate(Arc<dyn Fn(Date) -> bool + Send + Sync>),
}

/// Owns a trading-day predicate and positive annualization basis.
///
/// Defaults to [`Self::weekdays`] with a 252-day basis.
/// All instances support shared concurrent reads. Custom predicates must be deterministic
/// throughout each operation and safe for concurrent invocation; query order/count is unspecified.
/// Clones retain predicate ownership. Predicate calendars compare by shared predicate identity,
/// not by evaluating their behavior; built-in calendars compare by kind.
#[derive(Clone)]
pub struct TradingCalendar {
    kind: CalendarKind,
    basis: i32,
}

impl Default for TradingCalendar {
    fn default() -> Self {
        Self::weekdays()
    }
}

impl fmt::Debug for TradingCalendar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            CalendarKind::AllDays => "all_days",
            CalendarKind::Weekdays => "weekdays",
            CalendarKind::Sse => "sse",
            CalendarKind::Predicate(_) => "custom",
        };
        f.debug_struct("TradingCalendar")
            .field("kind", &kind)
            .field("basis", &self.basis)
            .finish()
    }
}

impl PartialEq for TradingCalendar {
    fn eq(&self, other: &Self) -> bool {
        self.basis == other.basis
            && match (&self.kind, &other.kind) {
                (CalendarKind::Predicate(left), CalendarKind::Predicate(right)) => {
                    Arc::ptr_eq(left, right)
                }
                (left, right) => std::mem::discriminant(left) == std::mem::discriminant(right),
            }
    }
}
impl Eq for TradingCalendar {}

impl TradingCalendar {
    /// Creates a calendar with every date open and a 365-day basis.
    ///
    /// See [`Self::adjust`] for calendar usage.
    pub fn all_days() -> Self {
        Self {
            kind: CalendarKind::AllDays,
            basis: 365,
        }
    }

    /// Creates a Monday-through-Friday calendar with a 252-day basis.
    ///
    /// Ignores holidays; see [`Self::adjust`] for calendar usage.
    pub fn weekdays() -> Self {
        Self {
            kind: CalendarKind::Weekdays,
            basis: 252,
        }
    }

    /// Creates an SSE calendar with weekday closures for 1991–2099 and a 243-day basis.
    ///
    /// Uses weekdays outside that range. Closures beyond the recorded XSHG history
    /// are forecasts that omit ad-hoc closures. Regenerate the bundled table with
    /// `tools/generate_sse_calendar.py` as upstream history grows.
    /// See [`Self::adjust`] for calendar usage.
    pub fn sse() -> Self {
        Self {
            kind: CalendarKind::Sse,
            basis: 243,
        }
    }

    /// Creates a calendar from an owned predicate and positive annualization basis.
    ///
    /// The predicate must satisfy [`TradingCalendar`]'s determinism contract.
    /// Predicate panics propagate when the calendar is queried.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a nonpositive basis.
    /// See [`Self::custom`] for date overrides without a custom predicate.
    pub fn new(
        predicate: impl Fn(Date) -> bool + Send + Sync + 'static,
        basis: i32,
    ) -> Result<Self> {
        if basis <= 0 {
            return Err(Error::InvalidInput(
                "annual trading-day count must be positive".into(),
            ));
        }
        Ok(Self {
            kind: CalendarKind::Predicate(Arc::new(predicate)),
            basis,
        })
    }

    /// Creates a calendar with owned open/closed overrides on a base calendar.
    ///
    /// Override dates may be unsorted or repeated. The basis defaults to the base's
    /// value and is never recalculated from the overrides. The base is consumed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for dates present in both override lists,
    /// then for a nonpositive basis. See [`Self::adjust`] for calendar usage.
    pub fn custom(
        base: Self,
        mut closed: Vec<Date>,
        mut open: Vec<Date>,
        basis: Option<i32>,
    ) -> Result<Self> {
        closed.sort_unstable();
        closed.dedup();
        open.sort_unstable();
        open.dedup();
        if closed.iter().any(|date| open.binary_search(date).is_ok()) {
            return Err(Error::InvalidInput(
                "override date is both open and closed".into(),
            ));
        }
        let basis = basis.unwrap_or(base.basis);
        Self::new(
            move |date| {
                if closed.binary_search(&date).is_ok() {
                    return false;
                }
                open.binary_search(&date).is_ok() || base.is_trading_day(date)
            },
            basis,
        )
    }

    /// Returns the positive annualization basis; see [`Self::weekdays`].
    pub fn trading_days_per_year(&self) -> i32 {
        self.basis
    }

    /// Tests a validated supported date. Weekend opening overrides are allowed.
    pub fn is_trading_day(&self, date: Date) -> bool {
        match &self.kind {
            CalendarKind::AllDays => true,
            CalendarKind::Weekdays => date.is_weekday(),
            CalendarKind::Sse => {
                let encoded = date.year() * 10_000 + date.month() as i32 * 100 + date.day() as i32;
                date.is_weekday() && SSE_HOLIDAYS.binary_search(&encoded).is_err()
            }
            CalendarKind::Predicate(predicate) => predicate(date),
        }
    }

    /// Returns the nearest trading date in the selected direction, including `nominal`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] if no trading date exists before reaching
    /// the supported date-range boundary. Custom predicate panics propagate.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{BusinessDayConvention, Date, TradingCalendar};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let calendar = TradingCalendar::weekdays();
    /// let saturday = Date::new(2025, 1, 4)?;
    /// assert_eq!(calendar.adjust(saturday, BusinessDayConvention::Following)?,
    ///     Date::new(2025, 1, 6)?);
    /// # Ok(()) }
    /// ```
    pub fn adjust(&self, nominal: Date, convention: BusinessDayConvention) -> Result<Date> {
        let direction = match convention {
            BusinessDayConvention::Following => 1,
            BusinessDayConvention::Preceding => -1,
        };
        let mut date = nominal;
        loop {
            if self.is_trading_day(date) {
                return Ok(date);
            }
            date = date.add_days(direction)?;
        }
    }

    /// Counts trading dates in `[start, end)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed ranges.
    /// Custom predicate panics propagate. See [`Self::adjust`] for calendar setup.
    pub fn trading_days_between(&self, start: Date, end: Date) -> Result<i32> {
        validate_range(start, end)?;
        let mut date = start;
        let mut count = 0;
        while date < end {
            count += i32::from(self.is_trading_day(date));
            date = date.add_days(1)?;
        }
        Ok(count)
    }

    /// Returns the trading-day count over `[start, end)` divided by the basis.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::trading_days_between`] errors and predicate panics.
    /// See [`Self::adjust`] for calendar setup.
    pub fn trading_year_fraction(&self, start: Date, end: Date) -> Result<f64> {
        Ok(f64::from(self.trading_days_between(start, end)?) / f64::from(self.basis))
    }
}

pub(crate) fn validate_range(start: Date, end: Date) -> Result<()> {
    if end < start {
        return Err(Error::InvalidInput("range end precedes start".into()));
    }
    Ok(())
}

const SSE_HOLIDAYS: &[i32] = include!("sse_holidays.rs");
