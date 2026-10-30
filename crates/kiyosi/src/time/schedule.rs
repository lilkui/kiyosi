use crate::{Date, Error, Result, TradingCalendar, time::calendar::validate_range};

/// Owns immutable, strictly ordered observation dates.
///
/// Accessors borrow storage for the lifetime of the borrow; shared reads are safe.
/// See [`Self::fixed_interval`] for construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationSchedule {
    dates: Vec<Date>,
}

impl ObservationSchedule {
    /// Creates an owning schedule after validating life bounds, order and trading days.
    ///
    /// Empty schedules are valid.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life, unordered or out-of-life
    /// dates, or non-trading observation dates, checked in that order.
    pub fn new(
        dates: Vec<Date>,
        start: Date,
        end: Date,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        validate_range(start, end)?;
        for (index, date) in dates.iter().enumerate() {
            if (index > 0 && *date <= dates[index - 1]) || *date < start || *date > end {
                return Err(Error::InvalidInput(
                    "observation dates must be ordered and within life bounds".into(),
                ));
            }
        }
        if dates.iter().any(|date| !calendar.is_trading_day(*date)) {
            return Err(Error::InvalidInput(
                "observation date is not a trading day".into(),
            ));
        }
        Ok(Self { dates })
    }

    /// Creates candidates at `start + n * interval_days` for positive `n`.
    ///
    /// Start is excluded and end is inclusive. Following adjustment merges collisions
    /// and stops before crossing end; end need not appear.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life or nonpositive intervals,
    /// checked in that order. Propagates date-range errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, ObservationSchedule, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let schedule = ObservationSchedule::fixed_interval(
    ///     Date::new(2025, 1, 1)?, Date::new(2025, 1, 10)?, 3,
    ///     &TradingCalendar::all_days())?;
    /// assert_eq!(schedule.len(), 3);
    /// # Ok(()) }
    /// ```
    pub fn fixed_interval(
        start: Date,
        end: Date,
        interval_days: i64,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        validate_range(start, end)?;
        if interval_days <= 0 {
            return Err(Error::InvalidInput("interval must be positive".into()));
        }
        let mut dates = Vec::new();
        let mut target = start;
        while interval_days <= target.days_until(end) {
            target = target.add_days(interval_days)?;
            if !push_following(&mut dates, target, end, calendar)? {
                break;
            }
        }
        Ok(Self { dates })
    }

    /// Creates monthly candidates anchored to `start`'s day, clamped at month end.
    ///
    /// The first offset must be positive; subsequent observations are monthly.
    /// Uses [`Self::fixed_interval`]'s adjustment, collision and endpoint rules.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life or a nonpositive first
    /// offset. Propagates date-range errors.
    /// See [`Self::fixed_interval`] for schedule construction.
    pub fn monthly(
        start: Date,
        end: Date,
        first_observation_month: i32,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        validate_range(start, end)?;
        if first_observation_month <= 0 {
            return Err(Error::InvalidInput("first month must be positive".into()));
        }
        let start_month = start.year() * 12 + start.month() as i32 - 1;
        let end_month = end.year() * 12 + end.month() as i32 - 1;
        let mut dates = Vec::new();
        for offset in first_observation_month..=end_month - start_month {
            let target_month = start_month + offset;
            let year = target_month.div_euclid(12);
            let month = target_month.rem_euclid(12) as u32 + 1;
            let mut day = start.day();
            // Clamp each candidate from the original day so February does not shift later months.
            let target = loop {
                match Date::new(year, month, day) {
                    Ok(date) => break date,
                    Err(_) => day -= 1,
                }
            };
            if target > end || !push_following(&mut dates, target, end, calendar)? {
                break;
            }
        }
        Ok(Self { dates })
    }

    /// Borrows the strictly ordered date storage for this schedule's lifetime.
    ///
    /// See [`Self::fixed_interval`] for construction.
    pub fn dates(&self) -> &[Date] {
        &self.dates
    }
    /// Returns the number of observation dates; see [`Self::fixed_interval`].
    pub fn len(&self) -> usize {
        self.dates.len()
    }
    /// Reports whether there are no observations; see [`Self::fixed_interval`].
    pub fn is_empty(&self) -> bool {
        self.dates.is_empty()
    }
}

fn push_following(
    dates: &mut Vec<Date>,
    mut target: Date,
    end: Date,
    calendar: &TradingCalendar,
) -> Result<bool> {
    if dates.last().is_some_and(|last| target <= *last) {
        return Ok(true);
    }
    loop {
        if calendar.is_trading_day(target) {
            dates.push(target);
            return Ok(true);
        }
        if target == end {
            return Ok(false);
        }
        target = target.add_days(1)?;
    }
}
