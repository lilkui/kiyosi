use chrono::{Datelike, Days, NaiveDate};

use crate::{Error, ErrorCategory, Result};

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Validated proleptic Gregorian date, years -32767 through 32767 inclusive.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Date(NaiveDate);

impl Date {
    /// Validates stored civil components without normalizing invalid dates.
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self> {
        if (-32767..=32767).contains(&year) {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                return Ok(Self(date));
            }
        }
        Err(Error::new(
            ErrorCategory::InvalidDate,
            "unsupported calendar date",
        ))
    }

    pub fn year(self) -> i32 {
        self.0.year()
    }
    pub fn month(self) -> u32 {
        self.0.month()
    }
    pub fn day(self) -> u32 {
        self.0.day()
    }

    pub(crate) fn is_weekday(self) -> bool {
        self.0.weekday().number_from_monday() <= 5
    }

    pub(crate) fn days_until(self, end: Self) -> i64 {
        (end.0 - self.0).num_days()
    }

    pub(crate) fn add_days(self, days: i64) -> Result<Self> {
        let shifted = if days >= 0 {
            self.0.checked_add_days(Days::new(days as u64))
        } else {
            self.0.checked_sub_days(Days::new(days.unsigned_abs()))
        };
        match shifted {
            Some(date) => Self::new(date.year(), date.month(), date.day()),
            None => Err(Error::new(
                ErrorCategory::InvalidDate,
                "date arithmetic exceeds supported range",
            )),
        }
    }

    /// Midnight UTC-like timestamp at the start of this date.
    pub fn start_of_day(self) -> Timestamp {
        Timestamp(i64::from(self.0.num_days_from_ce()) * MICROS_PER_DAY)
    }
}

/// Validated UTC-like instant with microsecond resolution.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates an instant on a supported date; leap seconds are not supported.
    pub fn new(date: Date, hour: u32, minute: u32, second: u32, microsecond: u32) -> Result<Self> {
        if hour > 23 || minute > 59 || second > 59 || microsecond > 999_999 {
            return Err(Error::new(
                ErrorCategory::InvalidDate,
                "invalid time components",
            ));
        }
        Ok(Self(
            date.start_of_day().0
                + i64::from((hour * 60 + minute) * 60 + second) * 1_000_000
                + i64::from(microsecond),
        ))
    }

    /// Date containing this instant, including instants before the common era.
    pub fn date(self) -> Date {
        // Construction limits the day count to chrono's supported range.
        Date(
            NaiveDate::from_num_days_from_ce_opt(self.0.div_euclid(MICROS_PER_DAY) as i32)
                .expect("validated timestamp"),
        )
    }

    /// Microseconds elapsed since midnight on `date()`.
    pub fn micros_since_midnight(self) -> u64 {
        self.0.rem_euclid(MICROS_PER_DAY) as u64
    }

    pub(crate) fn years_until(self, end: Self) -> f64 {
        (end.0 - self.0) as f64 / (365.0 * MICROS_PER_DAY as f64)
    }

    pub(crate) fn micros_until(self, end: Self) -> i64 {
        end.0 - self.0
    }

    pub(crate) fn add_micros(self, micros: i64) -> Result<Self> {
        let value = self
            .0
            .checked_add(micros)
            .ok_or(Error::new(ErrorCategory::InvalidDate, "timestamp overflow"))?;
        let first = Date::new(-32767, 1, 1)?.start_of_day().0;
        let last = Date::new(32767, 12, 31)?.start_of_day().0 + MICROS_PER_DAY - 1;
        if value < first || value > last {
            return Err(Error::new(
                ErrorCategory::InvalidDate,
                "timestamp outside supported range",
            ));
        }
        Ok(Self(value))
    }
}

impl From<Date> for Timestamp {
    fn from(value: Date) -> Self {
        value.start_of_day()
    }
}
