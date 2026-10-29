use crate::{Date, Error, Result, TradingCalendar, time::calendar::validate_range};

/// Immutable, strictly ordered owning observation dates. Accessors borrow stable storage
/// until the schedule is moved, assigned or dropped; shared concurrent reads are safe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationSchedule {
    dates: Vec<Date>,
}

impl ObservationSchedule {
    /// Creates an owning schedule after validating life bounds, order and trading days.
    /// Empty schedules are valid.
    ///
    /// # Errors
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
    /// Start is excluded and end is inclusive. Following adjustment merges collisions
    /// and stops before crossing end; end need not appear.
    ///
    /// # Errors
    /// Returns [`Error::InvalidInput`] for reversed life or nonpositive intervals,
    /// checked in that order. Propagates date-range and schedule validation errors.
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
        Self::new(dates, start, end, calendar)
    }

    /// Monthly candidates anchored to start's day, clamped to each month's last day.
    /// Positive first-month offset; subsequent observations are monthly. Uses the same
    /// adjustment, collision and endpoint rules as `fixed_interval`.
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
        Self::new(dates, start, end, calendar)
    }

    /// Borrows the strictly ordered date storage for this schedule's lifetime.
    pub fn dates(&self) -> &[Date] {
        &self.dates
    }
    pub fn len(&self) -> usize {
        self.dates.len()
    }
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
