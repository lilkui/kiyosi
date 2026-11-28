use kiyosi::{Date, ObservationSchedule, Timestamp, TradingCalendar};

#[track_caller]
pub fn schedule(times: Vec<Timestamp>, calendar: Option<&TradingCalendar>) -> ObservationSchedule {
    let anchor = Date::new(2025, 1, 1).unwrap().start_of_day();
    let start = times.iter().copied().min().unwrap_or(anchor);
    let end = times.iter().copied().max().unwrap_or(anchor);
    let default = TradingCalendar::weekdays();
    ObservationSchedule::new(times, start, end, calendar.unwrap_or(&default)).unwrap()
}
