use kiyosi::*;

pub fn multi_event_terms(
    mode: KnockInObservation,
) -> (SnowballTerms, TernarySnowballTerms, PhoenixTerms) {
    let date = |month, day| Date::new(2025, month, day).unwrap().start_of_day();
    let start = date(1, 1);
    let end = date(4, 1);
    let observations = autocallable_observations(
        &[date(2, 3), date(3, 3), end],
        &[110.0, 108.0, 105.0],
        Some(&[0.08; 3]),
        None,
    )
    .unwrap();
    let mut snow = SnowballTerms::new(
        0.06,
        100.0,
        95.0,
        100.0,
        60.0,
        start,
        end,
        observations.clone(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    snow.knock_in_observation = mode;

    snow.barrier_state = Some(AutocallableBarrierState::NoEvent);

    let mut ternary = TernarySnowballTerms::new(
        0.06,
        0.01,
        95.0,
        start,
        end,
        observations.clone(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    ternary.knock_in_observation = mode;

    ternary.barrier_state = Some(AutocallableBarrierState::NoEvent);

    let mut phoenix = PhoenixTerms::new(
        0.08,
        100.0,
        95.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(
            &[date(2, 3), date(3, 3), end],
            &[110.0, 108.0, 105.0],
            None,
            Some(&[95.0; 3]),
        )
        .unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    phoenix.knock_in_observation = mode;

    phoenix.barrier_state = Some(AutocallableBarrierState::NoEvent);

    (snow, ternary, phoenix)
}
