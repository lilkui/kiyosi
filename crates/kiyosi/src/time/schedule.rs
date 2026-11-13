use crate::{Error, Result, TimeOfDay, Timestamp, TradingCalendar};

/// Owns strictly ordered observation times and their validation calendar.
///
/// Accessors borrow storage for the lifetime of the borrow; shared reads are safe.
/// See [`Self::fixed_interval`] for construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationSchedule {
    times: Vec<Timestamp>,
    calendar: TradingCalendar,
}

impl ObservationSchedule {
    /// Creates an owning schedule after validating life bounds, order and trading days.
    ///
    /// Empty schedules are valid. The schedule owns its times and a clone of
    /// `calendar`; see [`Self::times`] and [`Self::calendar`] for borrowed access.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed life, unordered or out-of-life
    /// times, or nontrading observation times, checked in that order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, ObservationSchedule, TimeOfDay, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 2)?.at_time(TimeOfDay::new(15, 0, 0, 0)?);
    /// let end = start.add_days(1)?;
    /// let schedule = ObservationSchedule::new(vec![start, end], start, end,
    ///     &TradingCalendar::weekdays())?;
    /// assert_eq!(schedule.times(), &[start, end]);
    /// # Ok(()) }
    /// ```
    pub fn new(
        times: Vec<Timestamp>,
        start_time: Timestamp,
        end_time: Timestamp,
        calendar: &TradingCalendar,
    ) -> Result<Self> {
        Self::validate_times(&times, start_time, end_time, calendar)?;
        Ok(Self {
            times,
            calendar: calendar.clone(),
        })
    }

    pub(crate) fn validate_times(
        times: &[Timestamp],
        start_time: Timestamp,
        end_time: Timestamp,
        calendar: &TradingCalendar,
    ) -> Result<()> {
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
        Ok(())
    }

    pub(crate) fn daily(
        start_time: Timestamp,
        end_time: Timestamp,
        calendar: &TradingCalendar,
        include_start: bool,
        observation_clock_time: TimeOfDay,
    ) -> Result<Self> {
        Ok(Self {
            times: Self::daily_times(
                start_time,
                end_time,
                calendar,
                include_start,
                observation_clock_time,
            )?,
            calendar: calendar.clone(),
        })
    }

    pub(crate) fn daily_times(
        start_time: Timestamp,
        end_time: Timestamp,
        calendar: &TradingCalendar,
        include_start: bool,
        observation_clock_time: TimeOfDay,
    ) -> Result<Vec<Timestamp>> {
        validate_range(start_time, end_time)?;
        let mut times = Vec::new();
        let mut date = start_time.date();
        loop {
            crate::cancellation::check()?;
            let time = date.at_time(observation_clock_time);
            if (time > start_time || include_start && time == start_time)
                && time <= end_time
                && calendar.is_trading_day(date)
            {
                times.push(time);
            }
            if date == end_time.date() {
                break;
            }
            date = date.add_days(1)?;
        }
        Ok(times)
    }

    pub(crate) fn any_daily(
        start: Timestamp,
        end: Timestamp,
        calendar: &TradingCalendar,
        clock: TimeOfDay,
        predicate: impl Fn(Timestamp) -> bool,
    ) -> Result<bool> {
        let mut event = start.date().at_time(clock);
        while event <= end {
            crate::cancellation::check()?;
            if event >= start && calendar.is_trading_day(event.date()) && predicate(event) {
                return Ok(true);
            }
            if event.date() == end.date() {
                break;
            }
            event = event.add_days(1)?;
        }
        Ok(false)
    }

    /// Creates candidates every `interval_days` after the start date at the supplied clock time.
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
    /// use kiyosi::{Date, ObservationSchedule, TimeOfDay, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let schedule = ObservationSchedule::fixed_interval(
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 1, 10)?.start_of_day(), 3,
    ///     &TradingCalendar::all_days(), TimeOfDay::new(0, 0, 0, 0)?)?;
    /// assert_eq!(schedule.len(), 3);
    /// assert_eq!(schedule.calendar(), &TradingCalendar::all_days());
    /// # Ok(()) }
    /// ```
    pub fn fixed_interval(
        start_time: Timestamp,
        end_time: Timestamp,
        interval_days: i64,
        calendar: &TradingCalendar,
        observation_clock_time: TimeOfDay,
    ) -> Result<Self> {
        validate_range(start_time, end_time)?;
        if interval_days <= 0 {
            return Err(Error::InvalidInput("interval must be positive".into()));
        }
        let mut times = Vec::new();
        let mut target = start_time.date().at_time(observation_clock_time);
        while interval_days <= target.date().days_until(end_time.date()) {
            target = target.add_days(interval_days)?;
            if !push_following(&mut times, target, end_time, calendar)? {
                break;
            }
        }
        Ok(Self {
            times,
            calendar: calendar.clone(),
        })
    }

    /// Creates monthly candidates anchored to `start_time`'s day, clamped at month end.
    ///
    /// Candidates use the supplied clock time, including microseconds.
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
        observation_clock_time: TimeOfDay,
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
                    Ok(date) => break date.at_time(observation_clock_time),
                    Err(_) => day -= 1,
                }
            };
            if target > end_time || !push_following(&mut times, target, end_time, calendar)? {
                break;
            }
        }
        Ok(Self {
            times,
            calendar: calendar.clone(),
        })
    }

    /// Borrows the calendar used to validate and generate these observations.
    ///
    /// The calendar remains valid for this schedule's lifetime. Pass it with
    /// [`Self::times`] when constructing a contract; see [`Self::fixed_interval`].
    pub fn calendar(&self) -> &TradingCalendar {
        &self.calendar
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CancellationToken;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn daily_queries_respect_bounds_calendars_and_filters() -> Result<()> {
        let friday = crate::Date::new(2025, 1, 3)?;
        let clock = TimeOfDay::new(15, 0, 0, 123_456)?;
        let first = friday.at_time(clock);
        let sunday = first.add_days(2)?;
        let monday = first.add_days(3)?;
        let calendar = TradingCalendar::custom(
            TradingCalendar::weekdays(),
            vec![monday.date()],
            vec![sunday.date()],
            None,
        )?;
        for (start, end, expected) in [
            (first, first, true),
            (first.add_micros(1)?, sunday.add_micros(-1)?, false),
            (first.add_micros(1)?, sunday, true),
            (sunday.add_micros(1)?, monday, false),
            (monday, monday.add_days(1)?, true),
            (monday, first, false),
        ] {
            assert_eq!(
                ObservationSchedule::any_daily(start, end, &calendar, clock, |_| true)?,
                expected,
                "{start:?} through {end:?}"
            );
        }
        assert!(ObservationSchedule::any_daily(
            first,
            monday,
            &calendar,
            clock,
            |at| at == sunday
        )?);
        assert!(!ObservationSchedule::any_daily(
            first,
            monday,
            &calendar,
            clock,
            |at| at == monday
        )?);
        let last = crate::Date::new(32767, 12, 31)?.at_time(clock);
        assert!(!ObservationSchedule::any_daily(
            last,
            last,
            &calendar,
            clock,
            |_| false
        )?);
        Ok(())
    }

    #[test]
    fn daily_queries_short_circuit_and_preserve_cancellation() -> Result<()> {
        let start = crate::Date::new(2025, 1, 1)?.start_of_day();
        let token = CancellationToken::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let calendar = TradingCalendar::new(
            {
                let calls = Arc::clone(&calls);
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    true
                }
            },
            365,
        )?;
        assert!(ObservationSchedule::any_daily(
            start,
            start.add_days(365)?,
            &calendar,
            start.time_of_day(),
            |_| true,
        )?);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let result = token.run(|| {
            ObservationSchedule::any_daily(
                start,
                start.add_days(365)?,
                &calendar,
                start.time_of_day(),
                |_| {
                    token.cancel();
                    false
                },
            )
        });
        assert!(
            matches!(result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        Ok(())
    }

    #[test]
    fn daily_generation_stops_between_calendar_queries_after_cancellation() -> Result<()> {
        let token = CancellationToken::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let calendar = TradingCalendar::new(
            {
                let token = token.clone();
                let calls = Arc::clone(&calls);
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    token.cancel();
                    true
                }
            },
            365,
        )?;
        let start = crate::Date::new(2025, 1, 1)?.start_of_day();
        let result = token.run(|| {
            ObservationSchedule::daily(
                start,
                start.add_days(365)?,
                &calendar,
                true,
                start.time_of_day(),
            )
        });
        assert!(
            matches!(result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            ObservationSchedule::daily(
                start,
                start.add_days(2)?,
                &TradingCalendar::all_days(),
                true,
                start.time_of_day(),
            )?
            .len(),
            3
        );
        Ok(())
    }
}
