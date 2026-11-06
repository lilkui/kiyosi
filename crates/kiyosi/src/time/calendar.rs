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
    Overrides(Arc<CalendarOverrides>),
}

#[derive(PartialEq, Eq)]
struct CalendarOverrides {
    base: TradingCalendar,
    closed: Vec<Date>,
    open: Vec<Date>,
}

/// Owns a trading-day predicate and positive annualization basis.
///
/// Defaults to [`Self::weekdays`] with a 252-day basis.
/// All instances support shared concurrent reads. Custom predicates must return the same
/// answer for each date throughout the lifetime of the calendar and every clone, and be
/// safe for concurrent invocation; query order/count is unspecified. Reading a clock or
/// shared mutable state that changes these answers violates this contract, including
/// changes between operations. Amend calendars with [`Self::custom`] or create a new
/// predicate from an owned, immutable snapshot; contracts retain their original calendar.
/// Clones retain ownership of predicates and overrides. Overrides compare by base,
/// normalized date lists and annualization basis; empty overrides retain the base kind.
/// Predicate calendars compare by shared predicate identity, not by evaluating their
/// behavior; built-in calendars compare by kind. See [`Self::custom`] for overrides.
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
            CalendarKind::Overrides(_) => "overrides",
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
                (CalendarKind::Overrides(left), CalendarKind::Overrides(right)) => left == right,
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

    /// Creates a Shanghai Stock Exchange (SSE) calendar with a 243-day basis.
    ///
    /// Includes weekday closures for 1991 through 2099; uses weekdays outside
    /// that range. Closures beyond the recorded XSHG history
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
    /// The predicate must satisfy [`TradingCalendar`]'s lifetime-stability and concurrency contract.
    /// Predicate panics propagate when the calendar is queried.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a nonpositive basis.
    /// See [`Self::custom`] for date overrides without a custom predicate.
    ///
    /// # Examples
    ///
    /// Captures a fixed holiday and amends a clone without changing the original calendar:
    ///
    /// ```rust
    /// use kiyosi::{Date, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let holiday = Date::new(2025, 1, 1)?;
    /// let calendar = TradingCalendar::new(move |date| date != holiday, 365)?;
    /// let additional_holiday = Date::new(2025, 1, 2)?;
    /// let amended = TradingCalendar::custom(
    ///     calendar.clone(), vec![additional_holiday], vec![], None)?;
    /// assert!(!calendar.is_trading_day(holiday));
    /// assert!(calendar.is_trading_day(additional_holiday));
    /// assert!(!amended.is_trading_day(additional_holiday));
    /// # Ok(()) }
    /// ```
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
    /// Equivalent normalized overrides on equal bases compare equal. Empty overrides
    /// preserve the base's kind, including when changing only its annualization basis.
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
        if basis <= 0 {
            return Err(Error::InvalidInput(
                "annual trading-day count must be positive".into(),
            ));
        }
        if closed.is_empty() && open.is_empty() {
            return Ok(Self { basis, ..base });
        }
        Ok(Self {
            kind: CalendarKind::Overrides(Arc::new(CalendarOverrides { base, closed, open })),
            basis,
        })
    }

    /// Borrows this layer's base calendar and normalized date overrides, if present.
    ///
    /// Built-in and predicate calendars return `None`. References remain valid
    /// for the borrow's lifetime; see [`Self::custom`] for construction.
    pub fn overrides(&self) -> Option<(&Self, &[Date], &[Date])> {
        match &self.kind {
            CalendarKind::Overrides(overrides) => {
                Some((&overrides.base, &overrides.closed, &overrides.open))
            }
            _ => None,
        }
    }

    /// Returns the positive annualization basis; see [`Self::weekdays`].
    pub fn trading_days_per_year(&self) -> i32 {
        self.basis
    }

    /// Tests a validated supported date. Weekend opening overrides are allowed.
    ///
    /// See [`Self::adjust`] for a weekday-calendar example.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom predicate supplied to [`Self::new`].
    #[expect(
        clippy::cast_possible_wrap,
        reason = "Validated calendar months and days fit in i32."
    )]
    pub fn is_trading_day(&self, date: Date) -> bool {
        match &self.kind {
            CalendarKind::AllDays => true,
            CalendarKind::Weekdays => date.is_weekday(),
            CalendarKind::Sse => {
                let encoded = date.year() * 10_000 + date.month() as i32 * 100 + date.day() as i32;
                date.is_weekday() && SSE_HOLIDAYS.binary_search(&encoded).is_err()
            }
            CalendarKind::Predicate(predicate) => predicate(date),
            CalendarKind::Overrides(overrides) => {
                overrides.closed.binary_search(&date).is_err()
                    && (overrides.open.binary_search(&date).is_ok()
                        || overrides.base.is_trading_day(date))
            }
        }
    }

    /// Returns the nearest trading date in the selected direction, including `nominal`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] if no trading date exists before reaching
    /// the supported date-range boundary.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom predicate supplied to [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BusinessDayConvention, Date, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
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
    /// See [`Self::adjust`] for calendar setup.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom predicate supplied to [`Self::new`].
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
    /// Propagates [`Self::trading_days_between`] errors.
    /// See [`Self::adjust`] for calendar setup.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom predicate supplied to [`Self::new`].
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

#[expect(
    clippy::unreadable_literal,
    reason = "generated calendar data uses compact YYYYMMDD integers"
)]
const SSE_HOLIDAYS: &[i32] = include!("sse_holidays.rs");
