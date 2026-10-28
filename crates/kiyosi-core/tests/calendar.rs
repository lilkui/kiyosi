use kiyosi_core::{
    BusinessDayConvention, Date, ErrorCategory, ObservationSchedule, TradingCalendar,
};

fn day(year: i32, month: u32, day: u32) -> Date {
    Date::new(year, month, day).unwrap()
}

#[test]
fn custom_calendars_own_overrides_validate_and_retain_basis() {
    let base = TradingCalendar::default();
    let mut closed = vec![day(2026, 10, 2), day(2026, 10, 1), day(2026, 10, 1)];
    let calendar = TradingCalendar::custom(
        base.clone(),
        closed.clone(),
        vec![day(2026, 10, 10); 2],
        None,
    )
    .unwrap();
    closed.clear();
    assert_eq!(
        calendar
            .trading_days_between(day(2026, 10, 1), day(2026, 10, 12))
            .unwrap(),
        6
    );
    assert_eq!(calendar.trading_days_per_year(), 252);
    assert!(base.is_trading_day(day(2026, 10, 1)));
    assert!(!calendar.is_trading_day(day(2026, 10, 1)));
    assert!(calendar.is_trading_day(day(2026, 10, 10)));
    let layered = TradingCalendar::custom(
        calendar,
        vec![day(2026, 10, 10)],
        vec![day(2026, 10, 1)],
        Some(365),
    )
    .unwrap();
    assert!(!layered.is_trading_day(day(2026, 10, 10)));
    assert!(layered.is_trading_day(day(2026, 10, 1)));
    assert_eq!(layered.trading_days_per_year(), 365);
    for basis in [0, -1] {
        assert_eq!(
            TradingCalendar::new(|_| true, basis).unwrap_err().category,
            ErrorCategory::InvalidCalendar
        );
        assert_eq!(
            TradingCalendar::custom(base.clone(), vec![], vec![], Some(basis))
                .unwrap_err()
                .category,
            ErrorCategory::InvalidCalendar
        );
    }
    let conflict = day(2026, 10, 1);
    assert_eq!(
        TradingCalendar::custom(base, vec![conflict], vec![conflict], None)
            .unwrap_err()
            .category,
        ErrorCategory::InvalidCalendar
    );
}

#[test]
fn sse_history_forecast_and_fallback_match_legacy() {
    let sse = TradingCalendar::sse();
    assert_eq!(sse.trading_days_per_year(), 243);
    for date in [
        day(1996, 2, 29),
        day(2026, 2, 23),
        day(2027, 2, 12),
        day(2099, 6, 22),
        day(2031, 1, 4),
    ] {
        assert!(!sse.is_trading_day(date), "{date:?}");
    }
    for date in [day(2026, 2, 24), day(2031, 1, 2)] {
        assert!(sse.is_trading_day(date));
    }
    for date in [day(1990, 12, 25), day(2100, 2, 8)] {
        assert_eq!(
            sse.is_trading_day(date),
            TradingCalendar::default().is_trading_day(date)
        );
    }
}

#[test]
fn schedules_exclude_start_adjust_merge_and_clamp_months() {
    let weekdays = TradingCalendar::default();
    let fixed = ObservationSchedule::fixed_interval(day(2025, 1, 3), day(2025, 1, 7), 1, &weekdays)
        .unwrap();
    assert_eq!(fixed.dates(), &[day(2025, 1, 6), day(2025, 1, 7)]);
    let monthly =
        ObservationSchedule::monthly(day(2025, 1, 31), day(2025, 4, 30), 1, &weekdays).unwrap();
    assert_eq!(
        monthly.dates(),
        &[day(2025, 2, 28), day(2025, 3, 31), day(2025, 4, 30)]
    );
    let monthly =
        ObservationSchedule::monthly(day(2025, 1, 1), day(2025, 3, 1), 1, &weekdays).unwrap();
    assert_eq!(monthly.dates(), &[day(2025, 2, 3)]);
    let reopening = day(2025, 4, 1);
    let calendar = TradingCalendar::new(move |date| date >= reopening, 252).unwrap();
    for schedule in [
        ObservationSchedule::monthly(day(2025, 1, 1), day(2025, 4, 2), 1, &calendar).unwrap(),
        ObservationSchedule::fixed_interval(day(2025, 1, 1), day(2025, 4, 2), 31, &calendar)
            .unwrap(),
    ] {
        assert_eq!(schedule.dates(), &[reopening]);
    }
}

#[test]
fn schedule_validation_and_boundaries_preserve_error_timing() {
    let start = day(2025, 1, 1);
    let end = day(2025, 2, 1);
    let calendar = TradingCalendar::default();
    for interval in [0, -1, i64::MIN] {
        assert_eq!(
            ObservationSchedule::fixed_interval(start, end, interval, &calendar)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidSchedule
        );
        assert_eq!(
            ObservationSchedule::fixed_interval(end, start, interval, &calendar)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidTimeRange
        );
    }
    for month in [0, -1, i32::MIN] {
        assert_eq!(
            ObservationSchedule::monthly(start, end, month, &calendar)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidSchedule
        );
        assert_eq!(
            ObservationSchedule::monthly(end, start, month, &calendar)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidTimeRange
        );
    }
    for dates in [
        vec![end, start],
        vec![start, start],
        vec![day(2024, 12, 31)],
        vec![day(2025, 2, 2)],
    ] {
        assert_eq!(
            ObservationSchedule::new(dates, start, end, &calendar)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidSchedule
        );
    }
    assert_eq!(
        ObservationSchedule::new(vec![day(2025, 1, 4)], start, end, &calendar)
            .unwrap_err()
            .category,
        ErrorCategory::InvalidDate
    );
    assert_eq!(
        ObservationSchedule::new(vec![start, end], start, end, &TradingCalendar::all_days())
            .unwrap()
            .dates(),
        &[start, end]
    );
    assert!(
        ObservationSchedule::fixed_interval(start, start, 1, &calendar)
            .unwrap()
            .is_empty()
    );
    assert!(
        ObservationSchedule::monthly(start, start, 1, &calendar)
            .unwrap()
            .is_empty()
    );
    for (start, end) in [
        (start, end),
        (day(-32767, 1, 1), day(-32767, 1, 2)),
        (day(32767, 12, 30), day(32767, 12, 31)),
    ] {
        assert!(
            ObservationSchedule::fixed_interval(start, end, i64::MAX, &calendar)
                .unwrap()
                .is_empty()
        );
        assert!(
            ObservationSchedule::monthly(start, end, i32::MAX, &calendar)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn closed_calendars_stop_at_domain_and_schedule_bounds() {
    let closed = TradingCalendar::new(|_| false, 252).unwrap();
    for (boundary, convention) in [
        (day(-32767, 1, 1), BusinessDayConvention::Preceding),
        (day(32767, 12, 31), BusinessDayConvention::Following),
    ] {
        assert_eq!(
            closed.adjust(boundary, convention).unwrap_err().category,
            ErrorCategory::InvalidDate
        );
    }
    let start = day(32767, 12, 30);
    let end = day(32767, 12, 31);
    assert!(
        ObservationSchedule::fixed_interval(start, end, 1, &closed)
            .unwrap()
            .is_empty()
    );
    assert!(
        ObservationSchedule::monthly(day(32767, 11, 30), end, 1, &closed)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        closed
            .trading_days_between(end, start)
            .unwrap_err()
            .category,
        ErrorCategory::InvalidTimeRange
    );
    let end_only = TradingCalendar::new(move |date| date == end, 1).unwrap();
    assert_eq!(
        ObservationSchedule::fixed_interval(start, end, 1, &end_only)
            .unwrap()
            .dates(),
        &[end]
    );
}

#[test]
fn predicate_calendars_and_context_clones_support_concurrent_reads() {
    use kiyosi_core::{BlackScholesMertonParameters, PricingContext, PricingContextChanges};
    let open = day(2026, 10, 10);
    let calendar = TradingCalendar::new(move |date| date == open, 1).unwrap();
    let context = PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.03, 0.01, 0.2).unwrap(),
        100.0,
        open.into(),
        calendar,
    )
    .unwrap();
    let replaced = context
        .replace(PricingContextChanges {
            spot_price: Some(101.0),
            ..Default::default()
        })
        .unwrap();
    drop(context);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let context = &replaced;
            scope.spawn(move || {
                assert!(context.calendar().is_trading_day(open));
                assert!(!context.calendar().is_trading_day(day(2026, 10, 9)));
                assert_eq!(context.calendar().trading_days_per_year(), 1);
            });
        }
    });
}

#[test]
fn calendars_count_half_open_ranges_and_adjust_both_directions() {
    let calendar = TradingCalendar::default();
    let start = day(2025, 1, 1);
    let end = day(2025, 1, 6);
    assert_eq!(calendar.trading_days_between(start, end).unwrap(), 3);
    assert_eq!(
        calendar.trading_year_fraction(start, end).unwrap(),
        3.0 / 252.0
    );
    assert_eq!(calendar.trading_days_between(start, start).unwrap(), 0);
    assert_eq!(
        calendar
            .adjust(day(2025, 1, 5), BusinessDayConvention::Following)
            .unwrap(),
        end
    );
    assert_eq!(
        calendar
            .adjust(day(2025, 1, 5), BusinessDayConvention::Preceding)
            .unwrap(),
        day(2025, 1, 3)
    );
    assert_eq!(TradingCalendar::all_days().trading_days_per_year(), 365);
    assert!(TradingCalendar::all_days().is_trading_day(day(2025, 1, 5)));
}
