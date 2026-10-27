use std::{fmt, sync::Arc};

use crate::{Date, Error, ErrorCategory, Result};

/// Direction of inclusive business-day adjustment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusinessDayConvention {
    Following,
    Preceding,
}

#[derive(Clone)]
enum CalendarKind {
    AllDays,
    Weekdays,
    Sse,
    Predicate(Arc<dyn Fn(Date) -> bool + Send + Sync>),
}

/// Owning calendar with a positive annualization basis. Defaults to weekdays / 252.
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
    /// Every supported Gregorian date is open; annualization basis 365.
    pub fn all_days() -> Self {
        Self {
            kind: CalendarKind::AllDays,
            basis: 365,
        }
    }

    /// Monday through Friday, ignoring holidays; annualization basis 252.
    pub fn weekdays() -> Self {
        Self {
            kind: CalendarKind::Weekdays,
            basis: 252,
        }
    }

    /// SSE weekday closures for 1991–2099, weekdays outside that range; basis 243.
    /// Future closures are estimates; see `docs/sse-calendar.md` for sources and maintenance.
    pub fn sse() -> Self {
        Self {
            kind: CalendarKind::Sse,
            basis: 243,
        }
    }

    /// Validates the positive annualization basis before retaining an owning predicate.
    /// Nonpositive bases return `InvalidCalendar`. The closure must satisfy the type's
    /// determinism contract. A panic in user code remains a native Rust panic.
    pub fn new(
        predicate: impl Fn(Date) -> bool + Send + Sync + 'static,
        basis: i32,
    ) -> Result<Self> {
        if basis <= 0 {
            return Err(Error::new(
                ErrorCategory::InvalidCalendar,
                "annual trading-day count must be positive",
            ));
        }
        Ok(Self {
            kind: CalendarKind::Predicate(Arc::new(predicate)),
            basis,
        })
    }

    /// Owning open/closed overrides. Inputs may be unsorted and repeated; conflicts return
    /// `InvalidCalendar`. The optional positive basis defaults to the base's, never recalculated.
    /// Validation completes before a calendar is returned; the base is never mutated.
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
            return Err(Error::new(
                ErrorCategory::InvalidCalendar,
                "override date is both open and closed",
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

    /// Adjusts inclusively; `InvalidDate` if no trading date exists in the supported direction.
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

    /// Counts trading dates in `[start, end)`. Reversed ranges return `InvalidTimeRange`.
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

    /// Trading-day count over `[start, end)` divided by the positive annualization basis.
    pub fn trading_year_fraction(&self, start: Date, end: Date) -> Result<f64> {
        Ok(f64::from(self.trading_days_between(start, end)?) / f64::from(self.basis))
    }
}

pub(crate) fn validate_range(start: Date, end: Date) -> Result<()> {
    if end < start {
        return Err(Error::new(
            ErrorCategory::InvalidTimeRange,
            "range end precedes start",
        ));
    }
    Ok(())
}

const SSE_HOLIDAYS: &[i32] = include!("sse_holidays.rs");
