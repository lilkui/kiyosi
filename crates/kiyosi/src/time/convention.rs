use crate::{Date, Error, Result, Timestamp, TradingCalendar};

/// Defines volatility's annualization unit and the clock accumulating variance.
///
/// Rates and continuous dividend yields always use A/365F. Business time advances
/// uniformly over each trading date's 24 hours, at microsecond resolution;
/// it stops on other dates. The calendar's fixed basis is not recalculated by year.
/// Instances support shared concurrent reads under [`TradingCalendar`]'s contract.
///
/// ```rust
/// use kiyosi::{Date, ModelTimeConvention, TradingCalendar};
/// # fn main() -> kiyosi::Result<()> {
/// let friday = Date::new(2026, 11, 13)?.start_of_day();
/// let monday = friday.add_days(3)?;
/// let clock = ModelTimeConvention::Business(TradingCalendar::weekdays());
/// assert_eq!(clock.year_fraction(friday, monday)?, 1.0 / 252.0);
/// # Ok(()) }
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ModelTimeConvention {
    /// Measures actual elapsed time over a fixed 365-day year; the default.
    #[default]
    Actual365Fixed,
    /// Measures trading-date time over the owned calendar's fixed year basis.
    Business(TradingCalendar),
}

impl ModelTimeConvention {
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
    /// use kiyosi::{ModelTimeConvention, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let clock = ModelTimeConvention::from_inputs(true, Some(TradingCalendar::weekdays()))?;
    /// assert_eq!(clock.calendar().map(TradingCalendar::trading_days_per_year), Some(252));
    /// assert!(ModelTimeConvention::from_inputs(true, None).is_err());
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
        let mut current = start;
        let mut days = 0.0;
        while current < end {
            crate::cancellation::check()?;
            let next = if current.date() == end.date() {
                end
            } else {
                current.date().add_days(1)?.start_of_day()
            };
            if calendar.is_trading_day(current.date()) {
                days += current.years_until(next) * 365.0;
            }
            current = next;
        }
        Ok(days / f64::from(calendar.trading_days_per_year()))
    }

    /// Borrows the business calendar, or returns `None` for A/365F.
    ///
    /// The calendar remains valid for this convention's lifetime; see the type
    /// example for construction.
    pub fn calendar(&self) -> Option<&TradingCalendar> {
        match self {
            Self::Actual365Fixed => None,
            Self::Business(calendar) => Some(calendar),
        }
    }

    pub(crate) fn require_actual365(&self) -> Result<()> {
        if matches!(self, Self::Business(_)) {
            return Err(Error::UnsupportedOperation(
                "operation requires the actual_365_fixed model time convention".into(),
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
        let mut date = start.date();
        let mut open = calendar.is_trading_day(date);
        let mut times = Vec::new();
        while date < end.date() {
            crate::cancellation::check()?;
            date = date.add_days(1)?;
            let next_open = calendar.is_trading_day(date);
            if next_open != open && date.start_of_day() < end {
                times.push(date.start_of_day());
            }
            open = next_open;
        }
        Ok(times)
    }
}
