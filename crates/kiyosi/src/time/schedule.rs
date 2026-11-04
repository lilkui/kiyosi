use crate::{Error, Result, Timestamp, TradingCalendar};

/// Owns immutable, strictly ordered observation times.
///
/// Accessors borrow storage for the lifetime of the borrow; shared reads are safe.
/// See [`Self::fixed_interval`] for construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationSchedule {
    times: Vec<Timestamp>,
}

impl ObservationSchedule {
    /// Creates an owning schedule after validating life bounds, order and trading days.
    ///
    /// Empty schedules are valid.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life, unordered or out-of-life
    /// times, or nontrading observation times, checked in that order.
    pub fn new(
        times: Vec<Timestamp>,
        start_time: Timestamp,
        end_time: Timestamp,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        validate_range(start_time, end_time)?;
        for (index, time) in times.iter().enumerate() {
            if (index > 0 && *time <= times[index - 1]) || *time < start_time || *time > end_time {
                return Err(Error::InvalidInput(
                    "observation times must be ordered and within life bounds".into(),
                ));
            }
        }
        if times
            .iter()
            .any(|time| !calendar.is_trading_day(time.date()))
        {
            return Err(Error::InvalidInput(
                "observation time is not a trading day".into(),
            ));
        }
        Ok(Self { times })
    }

    pub(crate) fn daily(
        start_time: Timestamp,
        end_time: Timestamp,
        calendar: &TradingCalendar,
        include_start: bool,
    ) -> Result<Self> {
        validate_range(start_time, end_time)?;
        let mut times = Vec::new();
        let mut date = start_time.date();
        loop {
            let time = end_time.on_date(date);
            if (time > start_time || include_start && time == start_time)
                && calendar.is_trading_day(date)
            {
                times.push(time);
            }
            if date == end_time.date() {
                break;
            }
            date = date.add_days(1)?;
        }
        Ok(Self { times })
    }

    /// Creates candidates at `start_time + n * interval_days` for positive `n`.
    ///
    /// `start_time` is excluded and `end_time` is inclusive. Following adjustment merges collisions
    /// and stops before crossing `end_time`; `end_time` need not appear.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life or nonpositive intervals,
    /// checked in that order. Propagates time-range errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, ObservationSchedule, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let schedule = ObservationSchedule::fixed_interval(
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 1, 10)?.start_of_day(), 3,
    ///     &TradingCalendar::all_days())?;
    /// assert_eq!(schedule.len(), 3);
    /// # Ok(()) }
    /// ```
    pub fn fixed_interval(
        start_time: Timestamp,
        end_time: Timestamp,
        interval_days: i64,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        validate_range(start_time, end_time)?;
        if interval_days <= 0 {
            return Err(Error::InvalidInput("interval must be positive".into()));
        }
        let mut times = Vec::new();
        let mut target = start_time;
        while interval_days <= target.date().days_until(end_time.date()) {
            target = target.add_days(interval_days)?;
            if !push_following(&mut times, target, end_time, calendar)? {
                break;
            }
        }
        Ok(Self { times })
    }

    /// Creates monthly candidates anchored to `start_time`'s day, clamped at month end.
    ///
    /// Day and month shifts preserve the time of day, including microseconds.
    ///
    /// The first offset must be positive; subsequent observations are monthly.
    /// Uses [`Self::fixed_interval`]'s adjustment, collision and endpoint rules.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life or a nonpositive first
    /// offset. Propagates time-range errors.
    /// See [`Self::fixed_interval`] for schedule construction.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "Validated calendar months fit in i32."
    )]
    pub fn monthly(
        start_time: Timestamp,
        end_time: Timestamp,
        first_observation_month: i32,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        validate_range(start_time, end_time)?;
        if first_observation_month <= 0 {
            return Err(Error::InvalidInput("first month must be positive".into()));
        }
        let start_month = start_time.date().year() * 12 + start_time.date().month() as i32 - 1;
        let end_month = end_time.date().year() * 12 + end_time.date().month() as i32 - 1;
        let mut times = Vec::new();
        for offset in first_observation_month..=end_month - start_month {
            let target_month = start_month + offset;
            let year = target_month.div_euclid(12);
            let month = target_month.rem_euclid(12) as u32 + 1;
            let mut day = start_time.date().day();
            // Clamp each candidate from the original day so February does not shift later months.
            let target = loop {
                match crate::Date::new(year, month, day) {
                    Ok(date) => break start_time.on_date(date),
                    Err(_) => day -= 1,
                }
            };
            if target > end_time || !push_following(&mut times, target, end_time, calendar)? {
                break;
            }
        }
        Ok(Self { times })
    }

    /// Borrows the strictly ordered time storage for this schedule's lifetime.
    ///
    /// See [`Self::fixed_interval`] for construction.
    pub fn times(&self) -> &[Timestamp] {
        &self.times
    }
    /// Returns the number of observation times; see [`Self::fixed_interval`].
    pub fn len(&self) -> usize {
        self.times.len()
    }
    /// Reports whether there are no observations; see [`Self::fixed_interval`].
    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }
}

fn push_following(
    times: &mut Vec<Timestamp>,
    mut target: Timestamp,
    end_time: Timestamp,
    calendar: &TradingCalendar,
) -> Result<bool> {
    if times.last().is_some_and(|last| target <= *last) {
        return Ok(true);
    }
    loop {
        if target > end_time {
            return Ok(false);
        }
        if calendar.is_trading_day(target.date()) {
            times.push(target);
            return Ok(true);
        }
        if target == end_time {
            return Ok(false);
        }
        target = target.add_days(1)?;
    }
}

fn validate_range(start_time: Timestamp, end_time: Timestamp) -> Result<()> {
    if end_time < start_time {
        return Err(Error::InvalidInput("end precedes start".into()));
    }
    Ok(())
}
