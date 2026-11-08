use kiyosi::{BusinessDayConvention, Date, Error, ObservationSchedule, TradingCalendar};

fn day(year: i32, month: u32, day: u32) -> Date {
    Date::new(year, month, day).unwrap()
}

#[test]
fn schedules_retain_calendars_for_contract_composition() -> kiyosi::Result<()> {
    use kiyosi::{BarrierOption, BarrierOptionTerms, BarrierType, OptionType, TimeOfDay};
    let start = day(2025, 1, 2).start_of_day();
    let end = day(2025, 1, 10).start_of_day();
    let saturday = day(2025, 1, 4).at_time(TimeOfDay::new(15, 0, 0, 0)?);
    let calendar = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![],
        vec![saturday.date()],
        None,
    )?;
    let schedule = ObservationSchedule::new(vec![saturday], start, end, &calendar)?;
    assert_eq!(schedule.calendar(), &calendar);
    assert_eq!(schedule.clone().calendar(), &calendar);
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_times = Some(schedule.times().to_vec());
    terms.calendar = schedule.calendar().clone();
    let option = BarrierOption::new(terms)?;
    let updated = option.replace_calendar(TradingCalendar::all_days())?;
    assert_eq!(
        updated.observation_schedule().calendar(),
        updated.calendar()
    );
    assert_eq!(option.observation_schedule().calendar(), &calendar);
    for generated in [
        ObservationSchedule::fixed_interval(
            start,
            end,
            1,
            &calendar,
            TimeOfDay::new(15, 0, 0, 0)?,
        )?,
        ObservationSchedule::monthly(start, end, 1, &calendar, TimeOfDay::new(15, 0, 0, 0)?)?,
    ] {
        assert_eq!(generated.calendar(), &calendar);
    }
    Ok(())
}

#[test]
fn calendar_equality_preserves_normalized_overrides_and_predicate_identity() {
    let base = TradingCalendar::weekdays();
    let closed = day(2026, 10, 1);
    let open = day(2026, 10, 10);
    let left = TradingCalendar::custom(base.clone(), vec![closed; 2], vec![open], None).unwrap();
    let right = TradingCalendar::custom(base.clone(), vec![closed], vec![open; 2], None).unwrap();
    assert_eq!(left, right);
    assert_eq!(
        TradingCalendar::custom(base.clone(), vec![], vec![], None).unwrap(),
        base
    );
    assert_ne!(
        TradingCalendar::custom(base.clone(), vec![], vec![], Some(365)).unwrap(),
        base
    );
    assert_ne!(
        left,
        TradingCalendar::custom(base.clone(), vec![closed], vec![], None).unwrap()
    );
    assert_ne!(
        left,
        TradingCalendar::custom(base, vec![closed], vec![open], Some(365)).unwrap()
    );
    let nested = TradingCalendar::custom(left, vec![open], vec![closed], None).unwrap();
    assert_eq!(
        nested,
        TradingCalendar::custom(right, vec![open; 2], vec![closed], None).unwrap()
    );
    assert!(!nested.is_trading_day(open));
    assert!(nested.is_trading_day(closed));

    let predicate = TradingCalendar::new(|_| true, 252).unwrap();
    assert_eq!(predicate, predicate.clone());
    assert_eq!(
        TradingCalendar::custom(predicate.clone(), vec![], vec![], None).unwrap(),
        predicate
    );
    assert_ne!(predicate, TradingCalendar::new(|_| true, 252).unwrap());
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
        assert!(matches!(
            TradingCalendar::new(|_| true, basis).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            TradingCalendar::custom(base.clone(), vec![], vec![], Some(basis)).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let conflict = day(2026, 10, 1);
    assert!(matches!(
        TradingCalendar::custom(base, vec![conflict], vec![conflict], None).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn sse_history_forecast_and_fallback_follow_calendar_rules() {
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
    let fixed = ObservationSchedule::fixed_interval(
        time(2025, 1, 3),
        time(2025, 1, 7),
        1,
        &weekdays,
        (time(2025, 1, 3)).time_of_day(),
    )
    .unwrap();
    assert_eq!(fixed.times(), &[time(2025, 1, 6), time(2025, 1, 7)]);
    let monthly = ObservationSchedule::monthly(
        time(2025, 1, 31),
        time(2025, 4, 30),
        1,
        &weekdays,
        (time(2025, 1, 31)).time_of_day(),
    )
    .unwrap();
    assert_eq!(
        monthly.times(),
        &[time(2025, 2, 28), time(2025, 3, 31), time(2025, 4, 30)]
    );
    let monthly = ObservationSchedule::monthly(
        time(2025, 1, 1),
        time(2025, 3, 1),
        1,
        &weekdays,
        (time(2025, 1, 1)).time_of_day(),
    )
    .unwrap();
    assert_eq!(monthly.times(), &[time(2025, 2, 3)]);
    let reopening = time(2025, 4, 1);
    let calendar = TradingCalendar::new(move |date| date >= reopening.date(), 252).unwrap();
    for schedule in [
        ObservationSchedule::monthly(
            time(2025, 1, 1),
            time(2025, 4, 2),
            1,
            &calendar,
            (time(2025, 1, 1)).time_of_day(),
        )
        .unwrap(),
        ObservationSchedule::fixed_interval(
            time(2025, 1, 1),
            time(2025, 4, 2),
            31,
            &calendar,
            (time(2025, 1, 1)).time_of_day(),
        )
        .unwrap(),
    ] {
        assert_eq!(schedule.times(), &[reopening]);
    }
}

#[test]
fn schedule_validation_and_boundaries_preserve_error_timing() {
    let start = time(2025, 1, 1);
    let end = time(2025, 2, 1);
    let calendar = TradingCalendar::default();
    for interval in [0, -1, i64::MIN] {
        assert!(matches!(
            ObservationSchedule::fixed_interval(
                start,
                end,
                interval,
                &calendar,
                (start).time_of_day()
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            ObservationSchedule::fixed_interval(
                end,
                start,
                interval,
                &calendar,
                (end).time_of_day()
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for month in [0, -1, i32::MIN] {
        assert!(matches!(
            ObservationSchedule::monthly(start, end, month, &calendar, (start).time_of_day())
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            ObservationSchedule::monthly(end, start, month, &calendar, (end).time_of_day())
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for dates in [
        vec![end, start],
        vec![start, start],
        vec![time(2024, 12, 31)],
        vec![time(2025, 2, 2)],
    ] {
        assert!(matches!(
            ObservationSchedule::new(dates, start, end, &calendar).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(matches!(
        ObservationSchedule::new(vec![time(2025, 1, 4)], start, end, &calendar).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(
        ObservationSchedule::new(vec![start, end], start, end, &TradingCalendar::all_days())
            .unwrap()
            .times(),
        &[start, end]
    );
    assert!(
        ObservationSchedule::fixed_interval(start, start, 1, &calendar, (start).time_of_day())
            .unwrap()
            .is_empty()
    );
    assert!(
        ObservationSchedule::monthly(start, start, 1, &calendar, (start).time_of_day())
            .unwrap()
            .is_empty()
    );
    for (start, end) in [
        (start, end),
        (time(-32767, 1, 1), time(-32767, 1, 2)),
        (time(32767, 12, 30), time(32767, 12, 31)),
    ] {
        assert!(
            ObservationSchedule::fixed_interval(
                start,
                end,
                i64::MAX,
                &calendar,
                (start).time_of_day()
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            ObservationSchedule::monthly(start, end, i32::MAX, &calendar, (start).time_of_day())
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
        assert!(matches!(
            closed.adjust(boundary, convention).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let start = time(32767, 12, 30);
    let end = time(32767, 12, 31);
    assert!(
        ObservationSchedule::fixed_interval(start, end, 1, &closed, (start).time_of_day())
            .unwrap()
            .is_empty()
    );
    assert!(
        ObservationSchedule::monthly(
            time(32767, 11, 30),
            end,
            1,
            &closed,
            (time(32767, 11, 30)).time_of_day()
        )
        .unwrap()
        .is_empty()
    );
    assert!(matches!(
        closed
            .trading_days_between(end.date(), start.date())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let end_only = TradingCalendar::new(move |date| date == end.date(), 1).unwrap();
    assert_eq!(
        ObservationSchedule::fixed_interval(start, end, 1, &end_only, (start).time_of_day())
            .unwrap()
            .times(),
        &[end]
    );
}

#[test]
fn predicate_calendar_clones_support_concurrent_reads() {
    let open = day(2026, 10, 10);
    let calendar = TradingCalendar::new(move |date| date == open, 1).unwrap();
    let cloned = calendar.clone();
    drop(calendar);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let calendar = &cloned;
            scope.spawn(move || {
                assert!(calendar.is_trading_day(open));
                assert!(!calendar.is_trading_day(day(2026, 10, 9)));
                assert_eq!(calendar.trading_days_per_year(), 1);
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

#[test]
fn calendar_override_components_borrow_normalized_owned_storage() {
    let closed = Date::new(2025, 1, 2).unwrap();
    let opened = Date::new(2025, 1, 4).unwrap();
    let calendar = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![closed, closed],
        vec![opened],
        Some(260),
    )
    .unwrap();
    let (base, closed_dates, open_dates) = calendar.overrides().unwrap();
    assert_eq!(base, &TradingCalendar::weekdays());
    assert_eq!(closed_dates, &[closed]);
    assert_eq!(open_dates, &[opened]);
    assert_eq!(calendar.trading_days_per_year(), 260);
    assert!(TradingCalendar::weekdays().overrides().is_none());
}

fn time(year: i32, month: u32, day_of_month: u32) -> kiyosi::Timestamp {
    day(year, month, day_of_month).start_of_day()
}
