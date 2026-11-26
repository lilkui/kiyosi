use crate::{Date, Error, Result, Timestamp, TradingCalendar};

/// Defines volatility's annualization unit and the clock accumulating variance.
///
/// Rate accrual is governed by the rate curves. Business time advances uniformly
/// over each trading date's 24 hours, at microsecond resolution, and stops on
/// other dates. The calendar has a fixed annualization basis.
/// Instances support shared concurrent reads under [`TradingCalendar`]'s contract.
///
/// ```rust
/// use kiyosi::{Date, VarianceTimeConvention, TradingCalendar};
/// # fn main() -> kiyosi::Result<()> {
/// let friday = Date::new(2026, 11, 13)?.start_of_day();
/// let monday = friday.add_days(3)?;
/// let clock = VarianceTimeConvention::Business(TradingCalendar::weekdays());
/// assert_eq!(clock.year_fraction(friday, monday)?, 1.0 / 261.0);
/// # Ok(()) }
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub enum VarianceTimeConvention {
    /// Measures actual elapsed time over a fixed 365-day year; the default.
    #[default]
    Actual365Fixed,
    /// Measures trading-date time over the owned calendar's fixed year basis.
    Business(TradingCalendar),
}

impl VarianceTimeConvention {
    /// Validates a convention selection and its optional calendar.
    ///
    /// Business time requires a calendar; A/365F has none. Use the enum variants
    /// directly when the selection is already validated.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a missing business calendar or an
    /// unused A/365F calendar.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Error, TradingCalendar, VarianceTimeConvention};
    /// # fn main() -> kiyosi::Result<()> {
    /// let clock = VarianceTimeConvention::from_inputs(true, Some(TradingCalendar::weekdays()))?;
    /// assert_eq!(clock.calendar().map(TradingCalendar::trading_days_per_year), Some(261));
    /// assert!(matches!(VarianceTimeConvention::from_inputs(true, None), Err(Error::InvalidInput(_))));
    /// # Ok(()) }
    /// ```
    pub fn from_inputs(business_time: bool, calendar: Option<TradingCalendar>) -> Result<Self> {
        match (business_time, calendar) {
            (false, None) => Ok(Self::Actual365Fixed),
            (true, Some(calendar)) => Ok(Self::Business(calendar)),
            _ => Err(Error::InvalidInput(
                "business time requires a calendar; actual_365_fixed requires calendar=None".into(),
            )),
        }
    }
    /// Returns the signed variance year fraction between two instants.
    ///
    /// Reversing the endpoints reverses the sign. See the type example for a
    /// weekend interval; no endpoint is rolled to a trading date.
    ///
    /// # Errors
    ///
    /// Propagates date arithmetic errors when traversing the interval, or
    /// [`Error::Io`] with [`std::io::ErrorKind::Interrupted`] on cancellation.
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate.
    pub fn year_fraction(&self, start: Timestamp, end: Timestamp) -> Result<f64> {
        if end < start {
            return self.year_fraction(end, start).map(|v| -v);
        }
        let Self::Business(calendar) = self else {
            return Ok(start.years_until(end));
        };
        if start == end {
            return Ok(0.0);
        }
        crate::cancellation::check()?;
        let first = start.date();
        let last = end.date();
        let fraction = |from: Timestamp, to| from.years_until(to) * 365.0;
        let days = if first == last {
            f64::from(calendar.is_trading_day(first)) * fraction(start, end)
        } else {
            let next = first.add_days(1)?;
            let first_day =
                f64::from(calendar.is_trading_day(first)) * fraction(start, next.start_of_day());
            crate::cancellation::check()?;
            let complete_days = calendar.trading_days_between(next, last)?;
            let midnight = last.start_of_day();
            let last_day = if midnight == end {
                0.0
            } else {
                crate::cancellation::check()?;
                f64::from(calendar.is_trading_day(last)) * fraction(midnight, end)
            };
            first_day + f64::from(complete_days) + last_day
        };
        Ok(days / f64::from(calendar.trading_days_per_year()))
    }

    /// Borrows the business calendar, or returns `None` for A/365F.
    ///
    /// See the type example for construction.
    pub fn calendar(&self) -> Option<&TradingCalendar> {
        match self {
            Self::Actual365Fixed => None,
            Self::Business(calendar) => Some(calendar),
        }
    }

    pub(crate) fn uses_actual365_clock(&self) -> bool {
        self.calendar()
            .is_none_or(|calendar| *calendar == TradingCalendar::all_days())
    }

    pub(crate) fn require_actual365(&self) -> Result<()> {
        if matches!(self, Self::Business(_)) {
            return Err(Error::UnsupportedOperation(
                "operation requires the actual_365_fixed variance time convention".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn variance_start(&self, start: Timestamp, end: Timestamp) -> Result<Timestamp> {
        let Some(calendar) = self.calendar() else {
            return Ok(start.min(end));
        };
        let mut current = start;
        while current < end {
            crate::cancellation::check()?;
            if calendar.is_trading_day(current.date()) {
                return Ok(current);
            }
            if current.date() == end.date() {
                break;
            }
            current = current.date().add_days(1)?.start_of_day();
        }
        Ok(end)
    }

    pub(crate) fn variance_rate(&self, date: Date) -> f64 {
        match self {
            Self::Actual365Fixed => 1.0,
            Self::Business(calendar) => {
                if calendar.is_trading_day(date) {
                    365.0 / f64::from(calendar.trading_days_per_year())
                } else {
                    0.0
                }
            }
        }
    }

    pub(crate) fn transitions(&self, start: Timestamp, end: Timestamp) -> Result<Vec<Timestamp>> {
        let Some(calendar) = self.calendar() else {
            return Ok(Vec::new());
        };
        calendar.transitions(start, end, calendar.is_trading_day(start.date()))
    }
}
