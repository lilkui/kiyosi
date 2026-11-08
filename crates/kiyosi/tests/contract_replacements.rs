use kiyosi::*;

macro_rules! check_monitoring_switch {
    ($original:ident, $scheduled:ident, $changes:ident) => {
        assert_eq!($original.observation_times(), None);
        for option in [&$original, &$scheduled] {
            assert_eq!(
                option
                    .replace($changes {
                        observation_times: Some(option.observation_times().map(<[_]>::to_vec)),
                        ..Default::default()
                    })
                    .unwrap(),
                *option
            );
        }
        assert_eq!($scheduled.replace($changes::default()).unwrap(), $scheduled);
        assert!(matches!(
            $scheduled.replace($changes {
                observation_times: Some(Some(vec![])),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            $scheduled.replace($changes {
                observation_times: Some(None),
                ..Default::default()
            }),
            Err(Error::StaleHistory(_))
        ));
        let continuous = $scheduled
            .replace($changes {
                observation_times: Some(None),
                touch_state: Some($scheduled.touch_state()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(continuous, $original);
        let cleared = $scheduled
            .replace($changes {
                observation_times: Some(None),
                touch_state: Some(None),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(cleared.touch_state(), None);
        assert_eq!(cleared.observation_mode(), ObservationMode::Continuous);
        assert_eq!(
            $scheduled
                .replace_history(None, None)
                .unwrap()
                .replace($changes {
                    observation_times: Some(None),
                    ..Default::default()
                })
                .unwrap(),
            cleared
        );
        assert_eq!(
            $scheduled.observation_times(),
            Some(&[$original.expiry_time()][..])
        );
    };
}

#[test]
fn reactivation_with_changed_accumulation_rules_requires_explicit_quantity() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let mut terms = AccumulatorTerms::new(
        100.0,
        120.0,
        1.0,
        2.0,
        start,
        start.add_days(2)?,
        start.time_of_day(),
    );
    terms.accumulated_quantity = Some(3.0);
    terms.history_time = Some(terms.effective_time);
    terms.knock_out_settled = Some(true);
    terms.history_time = Some(terms.effective_time);
    let settled = Accumulator::new(terms)?;
    assert!(matches!(
        settled.replace(AccumulatorChanges {
            knock_out_settled: Some(Some(false)),
            daily_quantity: Some(2.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    let reactivated = settled.replace(AccumulatorChanges {
        history_time: Some(Some(
            settled.history_time().unwrap_or(settled.effective_time()),
        )),
        knock_out_settled: Some(Some(false)),
        daily_quantity: Some(2.0),
        accumulated_quantity: Some(Some(0.0)),
        ..Default::default()
    })?;
    assert_eq!(reactivated.knock_out_settled(), Some(false));
    assert_eq!(reactivated.accumulated_quantity(), Some(0.0));
    assert_eq!(
        settled
            .replace(AccumulatorChanges {
                daily_quantity: Some(2.0),
                ..Default::default()
            })?
            .knock_out_settled(),
        Some(true)
    );
    assert_eq!(settled.knock_out_settled(), Some(true));
    assert_eq!(settled.accumulated_quantity(), Some(3.0));
    Ok(())
}

macro_rules! history_changes {
    ($original:ident, BarrierOptionChanges) => {
        BarrierOptionChanges {
            touch_state: Some($original.touch_state()),
            ..Default::default()
        }
    };
    ($original:ident, DoubleBarrierOptionChanges) => {
        DoubleBarrierOptionChanges {
            touch_state: Some($original.touch_state()),
            ..Default::default()
        }
    };
    ($original:ident, BinaryBarrierOptionChanges) => {
        BinaryBarrierOptionChanges {
            touch_state: Some($original.touch_state()),
            ..Default::default()
        }
    };
    ($original:ident, TouchOptionChanges) => {
        TouchOptionChanges {
            touch_state: Some($original.touch_state()),
            ..Default::default()
        }
    };
    ($original:ident, DoubleTouchOptionChanges) => {
        DoubleTouchOptionChanges {
            touch_state: Some($original.touch_state()),
            ..Default::default()
        }
    };
    ($original:ident, RangeAccrualOptionChanges) => {
        RangeAccrualOptionChanges {
            realized_hit_count: Some($original.realized_hit_count()),
            ..Default::default()
        }
    };
    ($original:ident, AveragePriceOptionChanges) => {
        AveragePriceOptionChanges {
            realized_average: Some($original.realized_average()),
            ..Default::default()
        }
    };
    ($original:ident, SnowballOptionChanges) => {
        SnowballOptionChanges {
            barrier_state: Some($original.barrier_state()),
            ..Default::default()
        }
    };
    ($original:ident, PhoenixOptionChanges) => {
        PhoenixOptionChanges {
            barrier_state: Some($original.barrier_state()),
            ..Default::default()
        }
    };
    ($original:ident, BinarySnowballOptionChanges) => {
        BinarySnowballOptionChanges {
            barrier_state: Some($original.barrier_state()),
            ..Default::default()
        }
    };
    ($original:ident, TernarySnowballOptionChanges) => {
        TernarySnowballOptionChanges {
            barrier_state: Some($original.barrier_state()),
            ..Default::default()
        }
    };
    ($original:ident, AccumulatorChanges) => {
        AccumulatorChanges {
            accumulated_quantity: Some($original.accumulated_quantity()),
            knock_out_settled: Some($original.knock_out_settled()),
            ..Default::default()
        }
    };
}

macro_rules! check_field {
    ($original:ident, AveragePriceOptionChanges, realized_average, $value:expr) => {
        let changed = $original.replace(AveragePriceOptionChanges {
            realized_average: Some($value),
            history_time: Some(Some($original.expiry_time())),
            ..Default::default()
        }).unwrap();
        assert_eq!(changed.realized_average(), $value);
    };
    ($original:ident, $changes:ident, knock_in_observation_mode, $value:expr, $($getter:tt)+) => {
        assert!(matches!($original.replace($changes {
            knock_in_observation_mode: Some($value),
            ..history_changes!($original, $changes)
        }), Err(Error::InvalidInput(_))));
        let changed = $original.replace($changes {
            knock_in_observation_mode: Some($value),
            observation_clock_time: Some(None),
            ..history_changes!($original, $changes)
        }).unwrap();
        assert_eq!(changed.$($getter)+, $value);
    };
    ($original:ident, $changes:ident, $field:ident, $value:expr) => {
        check_field!($original, $changes, $field, $value, $field());
    };
    ($original:ident, $changes:ident, $field:ident, $value:expr, $($getter:tt)+) => {
        let changed = $original.replace($changes {
            $field: Some($value), ..history_changes!($original, $changes)
        }).unwrap();
        assert_eq!(changed.$($getter)+, $value);
    };
}

#[test]
fn barrier_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = BarrierOption::new(BarrierOptionTerms {
        history_time: Some(start.add_days(5).unwrap()),
        calendar: TradingCalendar::default(),
        option_type: OptionType::Call,
        strike: 100.0,
        effective_time: start,
        expiry_time: end,
        barrier_level: 120.0,
        barrier_type: BarrierType::UpAndOut,
        rebate: 0.0,
        rebate_timing: RebateTiming::AtExpiry,
        observation_times: None,
        touch_state: Some(BarrierTouchState::Untouched),
    })
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(BarrierOptionChanges {
            barrier_level: Some(125.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original.replace(BarrierOptionChanges::default()).unwrap(),
        original
    );
    check_field!(original, BarrierOptionChanges, option_type, OptionType::Put);
    check_field!(original, BarrierOptionChanges, strike, 105.0);
    check_field!(
        original,
        BarrierOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        BarrierOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(original, BarrierOptionChanges, barrier_level, 125.0);
    check_field!(
        original,
        BarrierOptionChanges,
        barrier_type,
        BarrierType::DownAndIn
    );
    check_field!(original, BarrierOptionChanges, rebate, 5.0);
    check_field!(
        original,
        BarrierOptionChanges,
        rebate_timing,
        RebateTiming::AtHit
    );
    check_field!(
        original,
        BarrierOptionChanges,
        touch_state,
        Some(BarrierTouchState::Touched)
    );
    let cleared = original
        .replace(BarrierOptionChanges {
            touch_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.touch_state(), None);
    let scheduled = original
        .replace(BarrierOptionChanges {
            observation_times: Some(Some(vec![end])),
            touch_state: Some(original.touch_state()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), Some(&[end][..]));
    check_monitoring_switch!(original, scheduled, BarrierOptionChanges);
    assert!(matches!(
        original.replace(BarrierOptionChanges {
            observation_times: Some(Some(vec![end])),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert!(matches!(
        original.replace(BarrierOptionChanges {
            strike: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn double_barrier_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = DoubleBarrierOption::new(DoubleBarrierOptionTerms {
        history_time: Some(start.add_days(5).unwrap()),
        calendar: TradingCalendar::default(),
        option_type: OptionType::Call,
        strike: 100.0,
        effective_time: start,
        expiry_time: end,
        lower_barrier: 80.0,
        upper_barrier: 120.0,
        barrier_type: DoubleBarrierType::KnockOut,
        rebate: 0.0,
        rebate_timing: RebateTiming::AtExpiry,
        observation_times: None,
        touch_state: Some(BarrierTouchState::Untouched),
    })
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(DoubleBarrierOptionChanges {
            upper_barrier: Some(125.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original
            .replace(DoubleBarrierOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        DoubleBarrierOptionChanges,
        option_type,
        OptionType::Put
    );
    check_field!(original, DoubleBarrierOptionChanges, strike, 105.0);
    check_field!(
        original,
        DoubleBarrierOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        DoubleBarrierOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(original, DoubleBarrierOptionChanges, lower_barrier, 85.0);
    check_field!(original, DoubleBarrierOptionChanges, upper_barrier, 125.0);
    check_field!(
        original,
        DoubleBarrierOptionChanges,
        barrier_type,
        DoubleBarrierType::KnockIn
    );
    check_field!(original, DoubleBarrierOptionChanges, rebate, 5.0);
    check_field!(
        original,
        DoubleBarrierOptionChanges,
        rebate_timing,
        RebateTiming::AtHit
    );
    check_field!(
        original,
        DoubleBarrierOptionChanges,
        touch_state,
        Some(BarrierTouchState::Touched)
    );
    let cleared = original
        .replace(DoubleBarrierOptionChanges {
            touch_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.touch_state(), None);
    let scheduled = original
        .replace(DoubleBarrierOptionChanges {
            observation_times: Some(Some(vec![end])),
            touch_state: Some(original.touch_state()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), Some(&[end][..]));
    check_monitoring_switch!(original, scheduled, DoubleBarrierOptionChanges);
    assert!(matches!(
        original.replace(DoubleBarrierOptionChanges {
            observation_times: Some(Some(vec![end])),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert!(matches!(
        original.replace(DoubleBarrierOptionChanges {
            strike: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn binary_barrier_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = BinaryBarrierOption::cash(
        BinaryBarrierTerms {
            history_time: Some(start.add_days(5).unwrap()),
            calendar: TradingCalendar::default(),
            option_type: OptionType::Call,
            strike: 100.0,
            effective_time: start,
            expiry_time: end,
            barrier_level: 120.0,
            barrier_type: BarrierType::UpAndOut,
            observation_times: None,
            touch_state: Some(BarrierTouchState::Untouched),
        },
        10.0,
    )
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(BinaryBarrierOptionChanges {
            barrier_level: Some(125.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original
            .replace(BinaryBarrierOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        BinaryBarrierOptionChanges,
        option_type,
        OptionType::Put
    );
    check_field!(original, BinaryBarrierOptionChanges, strike, 105.0);
    check_field!(
        original,
        BinaryBarrierOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        BinaryBarrierOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(original, BinaryBarrierOptionChanges, barrier_level, 125.0);
    check_field!(
        original,
        BinaryBarrierOptionChanges,
        barrier_type,
        BarrierType::DownAndIn
    );
    check_field!(
        original,
        BinaryBarrierOptionChanges,
        touch_state,
        Some(BarrierTouchState::Touched)
    );
    let cleared = original
        .replace(BinaryBarrierOptionChanges {
            touch_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.touch_state(), None);
    let updated = original
        .replace(BinaryBarrierOptionChanges {
            payoff: Some(kiyosi::BinaryPayoff::Cash(15.0)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(updated.payout(), Some(15.0));
    let scheduled = original
        .replace(BinaryBarrierOptionChanges {
            observation_times: Some(Some(vec![end])),
            touch_state: Some(original.touch_state()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), Some(&[end][..]));
    check_monitoring_switch!(original, scheduled, BinaryBarrierOptionChanges);
    assert!(matches!(
        original.replace(BinaryBarrierOptionChanges {
            observation_times: Some(Some(vec![end])),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert!(matches!(
        original.replace(BinaryBarrierOptionChanges {
            strike: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn touch_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = TouchOption::cash(
        TouchOptionTerms {
            history_time: Some(start.add_days(5).unwrap()),
            calendar: TradingCalendar::default(),
            effective_time: start,
            expiry_time: end,
            barrier_level: 120.0,
            barrier_type: BarrierType::UpAndIn,
            settlement_timing: SettlementTiming::AtExpiry,
            observation_times: None,
            touch_state: Some(BarrierTouchState::Untouched),
        },
        10.0,
    )
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(TouchOptionChanges {
            barrier_level: Some(125.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original.replace(TouchOptionChanges::default()).unwrap(),
        original
    );
    check_field!(
        original,
        TouchOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        TouchOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(original, TouchOptionChanges, barrier_level, 125.0);
    check_field!(
        original,
        TouchOptionChanges,
        barrier_type,
        BarrierType::DownAndOut
    );
    check_field!(
        original,
        TouchOptionChanges,
        settlement_timing,
        SettlementTiming::AtHit
    );
    check_field!(
        original,
        TouchOptionChanges,
        touch_state,
        Some(BarrierTouchState::Touched)
    );
    let cleared = original
        .replace(TouchOptionChanges {
            touch_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.touch_state(), None);
    let updated = original
        .replace(TouchOptionChanges {
            payoff: Some(kiyosi::BinaryPayoff::Cash(15.0)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(updated.payout(), Some(15.0));
    let scheduled = original
        .replace(TouchOptionChanges {
            observation_times: Some(Some(vec![end])),
            touch_state: Some(original.touch_state()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), Some(&[end][..]));
    check_monitoring_switch!(original, scheduled, TouchOptionChanges);
    assert!(matches!(
        original.replace(TouchOptionChanges {
            observation_times: Some(Some(vec![end])),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert!(matches!(
        original.replace(TouchOptionChanges {
            payoff: Some(kiyosi::BinaryPayoff::Cash(-1.0)),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn double_touch_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = DoubleTouchOption::cash(
        DoubleTouchOptionTerms {
            history_time: Some(start.add_days(5).unwrap()),
            calendar: TradingCalendar::default(),
            effective_time: start,
            expiry_time: end,
            lower_barrier: 80.0,
            upper_barrier: 120.0,
            barrier_type: DoubleBarrierType::KnockIn,
            settlement_timing: SettlementTiming::AtExpiry,
            observation_times: None,
            touch_state: Some(BarrierTouchState::Untouched),
        },
        10.0,
    )
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(DoubleTouchOptionChanges {
            upper_barrier: Some(125.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original
            .replace(DoubleTouchOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        DoubleTouchOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        DoubleTouchOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(original, DoubleTouchOptionChanges, lower_barrier, 85.0);
    check_field!(original, DoubleTouchOptionChanges, upper_barrier, 125.0);
    let updated = original
        .replace(DoubleTouchOptionChanges {
            barrier_type: Some(DoubleBarrierType::KnockOut),
            ..Default::default()
        })
        .unwrap();
    assert!(!updated.is_one_touch());
    check_field!(
        original,
        DoubleTouchOptionChanges,
        settlement_timing,
        SettlementTiming::AtHit
    );
    check_field!(
        original,
        DoubleTouchOptionChanges,
        touch_state,
        Some(BarrierTouchState::Touched)
    );
    let cleared = original
        .replace(DoubleTouchOptionChanges {
            touch_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.touch_state(), None);
    check_field!(original, DoubleTouchOptionChanges, payout, 15.0);
    let scheduled = original
        .replace(DoubleTouchOptionChanges {
            observation_times: Some(Some(vec![end])),
            touch_state: Some(original.touch_state()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), Some(&[end][..]));
    check_monitoring_switch!(original, scheduled, DoubleTouchOptionChanges);
    assert!(matches!(
        original.replace(DoubleTouchOptionChanges {
            observation_times: Some(Some(vec![end])),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert!(matches!(
        original.replace(DoubleTouchOptionChanges {
            payout: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn accumulator_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = Accumulator::new(AccumulatorTerms {
        history_time: Some(start.add_days(5).unwrap()),
        observation_clock_time: (end).time_of_day(),
        calendar: TradingCalendar::default(),
        option_type: OptionType::Call,
        strike: 100.0,
        knock_out_level: 120.0,
        daily_quantity: 1.0,
        acceleration_factor: 2.0,
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        effective_time: start,
        expiry_time: end,
        knock_out_observation_times: Some(vec![end]),
    })
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(AccumulatorChanges {
            daily_quantity: Some(2.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original.replace(AccumulatorChanges::default()).unwrap(),
        original
    );
    check_field!(original, AccumulatorChanges, option_type, OptionType::Put);
    check_field!(original, AccumulatorChanges, strike, 105.0);
    check_field!(original, AccumulatorChanges, knock_out_level, 125.0);
    check_field!(original, AccumulatorChanges, daily_quantity, 2.0);
    check_field!(
        original,
        AccumulatorChanges,
        observation_clock_time,
        TimeOfDay::new(15, 0, 0, 123_456).unwrap()
    );
    check_field!(original, AccumulatorChanges, acceleration_factor, 3.0);
    check_field!(
        original,
        AccumulatorChanges,
        accumulated_quantity,
        Some(4.0)
    );
    let cleared = original
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.accumulated_quantity(), None);
    check_field!(
        original,
        AccumulatorChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AccumulatorChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AccumulatorChanges,
        knock_out_observation_times,
        Some(vec![start]),
        knock_out_observation_times().map(<[Timestamp]>::to_vec)
    );
    let cleared = original
        .replace(AccumulatorChanges {
            knock_out_observation_times: Some(None),
            ..history_changes!(original, AccumulatorChanges)
        })
        .unwrap();
    assert_eq!(
        cleared
            .knock_out_observation_times()
            .map(<[Timestamp]>::to_vec),
        None
    );
    assert!(matches!(
        original.replace(AccumulatorChanges {
            strike: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn range_accrual_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = RangeAccrualOption::new(RangeAccrualTerms {
        history_time: Some(start.add_days(5).unwrap()),
        observation_clock_time: (end).time_of_day(),
        calendar: TradingCalendar::default(),
        notional: 1000.0,
        annual_coupon_rate: 0.12,
        lower_bound: 90.0,
        upper_bound: 110.0,
        effective_time: start,
        expiry_time: end,
        realized_hit_count: Some(1),
    })
    .unwrap();
    let saved = original.clone();
    assert!(matches!(
        original.replace(RangeAccrualOptionChanges {
            lower_bound: Some(85.0),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original
            .replace(RangeAccrualOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(original, RangeAccrualOptionChanges, notional, 2000.0);
    check_field!(
        original,
        RangeAccrualOptionChanges,
        annual_coupon_rate,
        0.15
    );
    check_field!(original, RangeAccrualOptionChanges, lower_bound, 85.0);
    check_field!(original, RangeAccrualOptionChanges, upper_bound, 115.0);
    check_field!(
        original,
        RangeAccrualOptionChanges,
        observation_clock_time,
        TimeOfDay::new(15, 0, 0, 123_456).unwrap()
    );
    check_field!(
        original,
        RangeAccrualOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        RangeAccrualOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        RangeAccrualOptionChanges,
        realized_hit_count,
        Some(2)
    );
    let cleared = original
        .replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.realized_hit_count(), None);
    assert!(matches!(
        original.replace(RangeAccrualOptionChanges {
            notional: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn snowball_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = SnowballOption::new(SnowballTerms {
        history_time: Some(start.add_days(5).unwrap()),
        observation_clock_time: Some((end).time_of_day()),
        calendar: TradingCalendar::default(),
        maturity_coupon_rate: 0.1,
        initial_spot: 100.0,
        knock_in_level: 80.0,
        upper_strike: 100.0,
        lower_strike: 0.0,
        knock_in_observation_mode: KnockInObservationMode::EveryTradingDay,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_redemption_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::snowball(end, 110.0, 0.0).unwrap()],
    })
    .unwrap();
    let saved = original.clone();
    assert_eq!(
        original
            .replace(SnowballOptionChanges {
                observations: Some(vec![
                    AutocallableObservation::snowball(end, 115.0, 0.0).unwrap()
                ]),
                ..Default::default()
            })
            .unwrap()
            .history_time(),
        original.history_time()
    );
    assert_eq!(
        original.replace(SnowballOptionChanges::default()).unwrap(),
        original
    );
    check_field!(
        original,
        SnowballOptionChanges,
        maturity_coupon_rate,
        0.15,
        terms().maturity_coupon_rate
    );
    check_field!(
        original,
        SnowballOptionChanges,
        initial_spot,
        110.0,
        terms().initial_spot
    );
    check_field!(
        original,
        SnowballOptionChanges,
        knock_in_level,
        75.0,
        terms().knock_in_level
    );
    check_field!(
        original,
        SnowballOptionChanges,
        upper_strike,
        105.0,
        terms().upper_strike
    );
    check_field!(
        original,
        SnowballOptionChanges,
        lower_strike,
        5.0,
        terms().lower_strike
    );
    check_field!(
        original,
        SnowballOptionChanges,
        knock_in_observation_mode,
        KnockInObservationMode::AtExpiry,
        terms().knock_in_observation_mode
    );
    check_field!(
        original,
        SnowballOptionChanges,
        observation_clock_time,
        Some(TimeOfDay::new(15, 0, 0, 123_456).unwrap()),
        terms().observation_clock_time
    );
    check_field!(
        original,
        SnowballOptionChanges,
        barrier_state,
        Some(AutocallableBarrierState::KnockedOut),
        terms().barrier_state
    );
    let cleared = original
        .replace(SnowballOptionChanges {
            barrier_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.terms().barrier_state, None);
    check_field!(
        original,
        SnowballOptionChanges,
        principal_redemption_ratio,
        0.9,
        terms().principal_redemption_ratio
    );
    check_field!(
        original,
        SnowballOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        terms().effective_time
    );
    check_field!(
        original,
        SnowballOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day(),
        terms().expiry_time
    );
    check_field!(
        original,
        SnowballOptionChanges,
        observations,
        vec![AutocallableObservation::snowball(end, 115.0, 0.0).unwrap()],
        terms().observations
    );
    assert!(matches!(
        original.replace(SnowballOptionChanges {
            maturity_coupon_rate: Some(f64::NAN),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn phoenix_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = PhoenixOption::new(PhoenixTerms {
        history_time: Some(start.add_days(5).unwrap()),
        observation_clock_time: Some((end).time_of_day()),
        calendar: TradingCalendar::default(),
        coupon: PhoenixCoupon::Annual(0.1),
        initial_spot: 100.0,
        knock_in_level: 80.0,
        upper_strike: 100.0,
        lower_strike: 0.0,
        knock_in_observation_mode: KnockInObservationMode::EveryTradingDay,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_redemption_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::phoenix(end, 110.0, 0.0).unwrap()],
    })
    .unwrap();
    let saved = original.clone();
    assert_eq!(
        original
            .replace(PhoenixOptionChanges {
                observations: Some(vec![
                    AutocallableObservation::phoenix(end, 115.0, 0.0).unwrap()
                ]),
                ..Default::default()
            })
            .unwrap()
            .history_time(),
        original.history_time()
    );
    assert_eq!(
        original.replace(PhoenixOptionChanges::default()).unwrap(),
        original
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        coupon,
        PhoenixCoupon::PerPeriod(0.02),
        terms().coupon
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        initial_spot,
        110.0,
        terms().initial_spot
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        knock_in_level,
        75.0,
        terms().knock_in_level
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        upper_strike,
        105.0,
        terms().upper_strike
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        lower_strike,
        5.0,
        terms().lower_strike
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        knock_in_observation_mode,
        KnockInObservationMode::AtExpiry,
        terms().knock_in_observation_mode
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        observation_clock_time,
        Some(TimeOfDay::new(15, 0, 0, 123_456).unwrap()),
        terms().observation_clock_time
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        barrier_state,
        Some(AutocallableBarrierState::KnockedOut),
        terms().barrier_state
    );
    let cleared = original
        .replace(PhoenixOptionChanges {
            barrier_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.terms().barrier_state, None);
    check_field!(
        original,
        PhoenixOptionChanges,
        principal_redemption_ratio,
        0.9,
        terms().principal_redemption_ratio
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        terms().effective_time
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day(),
        terms().expiry_time
    );
    check_field!(
        original,
        PhoenixOptionChanges,
        observations,
        vec![AutocallableObservation::phoenix(end, 115.0, 0.0).unwrap()],
        terms().observations
    );
    assert!(matches!(
        original.replace(PhoenixOptionChanges {
            initial_spot: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn binary_snowball_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = BinarySnowballOption::new(BinarySnowballTerms {
        history_time: Some(start.add_days(5).unwrap()),
        calendar: TradingCalendar::default(),
        maturity_coupon_rate: 0.1,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_redemption_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::snowball(end, 110.0, 0.0).unwrap()],
    })
    .unwrap();
    let saved = original.clone();
    assert_eq!(
        original
            .replace(BinarySnowballOptionChanges {
                observations: Some(vec![
                    AutocallableObservation::snowball(end, 115.0, 0.0).unwrap()
                ]),
                ..Default::default()
            })
            .unwrap()
            .history_time(),
        original.history_time()
    );
    assert_eq!(
        original
            .replace(BinarySnowballOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        BinarySnowballOptionChanges,
        maturity_coupon_rate,
        0.15,
        terms().maturity_coupon_rate
    );
    check_field!(
        original,
        BinarySnowballOptionChanges,
        barrier_state,
        Some(AutocallableBarrierState::KnockedOut),
        terms().barrier_state
    );
    let cleared = original
        .replace(BinarySnowballOptionChanges {
            barrier_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.terms().barrier_state, None);
    check_field!(
        original,
        BinarySnowballOptionChanges,
        principal_redemption_ratio,
        0.9,
        terms().principal_redemption_ratio
    );
    check_field!(
        original,
        BinarySnowballOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        terms().effective_time
    );
    check_field!(
        original,
        BinarySnowballOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day(),
        terms().expiry_time
    );
    check_field!(
        original,
        BinarySnowballOptionChanges,
        observations,
        vec![AutocallableObservation::snowball(end, 115.0, 0.0).unwrap()],
        terms().observations
    );
    assert!(matches!(
        original.replace(BinarySnowballOptionChanges {
            maturity_coupon_rate: Some(f64::NAN),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn ternary_snowball_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = TernarySnowballOption::new(TernarySnowballTerms {
        history_time: Some(start.add_days(5).unwrap()),
        observation_clock_time: Some((end).time_of_day()),
        calendar: TradingCalendar::default(),
        maturity_coupon_rate: 0.1,
        minimum_coupon_rate: 0.02,
        knock_in_level: 80.0,
        knock_in_observation_mode: KnockInObservationMode::EveryTradingDay,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_redemption_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::snowball(end, 110.0, 0.0).unwrap()],
    })
    .unwrap();
    let saved = original.clone();
    assert_eq!(
        original
            .replace(TernarySnowballOptionChanges {
                observations: Some(vec![
                    AutocallableObservation::snowball(end, 115.0, 0.0).unwrap()
                ]),
                ..Default::default()
            })
            .unwrap()
            .history_time(),
        original.history_time()
    );
    assert_eq!(
        original
            .replace(TernarySnowballOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        maturity_coupon_rate,
        0.15,
        terms().maturity_coupon_rate
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        minimum_coupon_rate,
        0.03,
        terms().minimum_coupon_rate
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        knock_in_level,
        75.0,
        terms().knock_in_level
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        knock_in_observation_mode,
        KnockInObservationMode::AtExpiry,
        terms().knock_in_observation_mode
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        observation_clock_time,
        Some(TimeOfDay::new(15, 0, 0, 123_456).unwrap()),
        terms().observation_clock_time
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        barrier_state,
        Some(AutocallableBarrierState::KnockedOut),
        terms().barrier_state
    );
    let cleared = original
        .replace(TernarySnowballOptionChanges {
            barrier_state: Some(None),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.terms().barrier_state, None);
    check_field!(
        original,
        TernarySnowballOptionChanges,
        principal_redemption_ratio,
        0.9,
        terms().principal_redemption_ratio
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        effective_time,
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        terms().effective_time
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day(),
        terms().expiry_time
    );
    check_field!(
        original,
        TernarySnowballOptionChanges,
        observations,
        vec![AutocallableObservation::snowball(end, 115.0, 0.0).unwrap()],
        terms().observations
    );
    assert!(matches!(
        original.replace(TernarySnowballOptionChanges {
            maturity_coupon_rate: Some(f64::NAN),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn geometric_average_price_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None, None)
            .unwrap();
    let saved = original.clone();
    let known = original
        .replace_history(
            Some(100.0),
            Some(original.history_time().unwrap_or(original.effective_time())),
        )
        .unwrap();
    assert!(matches!(
        known.replace(AveragePriceOptionChanges {
            averaging_start_time: Some(start.add_days(1).unwrap()),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original
            .replace(AveragePriceOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        option_type,
        OptionType::Put
    );
    check_field!(original, AveragePriceOptionChanges, strike, 105.0);
    let updated = original
        .replace(AveragePriceOptionChanges {
            effective_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            averaging_start_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            realized_average: Some(original.realized_average()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        updated.effective_time(),
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        averaging_start_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        realized_average,
        Some(105.0)
    );
    assert!(matches!(
        original.replace(AveragePriceOptionChanges {
            strike: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn arithmetic_average_price_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None, None)
            .unwrap();
    let saved = original.clone();
    let known = original
        .replace_history(
            Some(100.0),
            Some(original.history_time().unwrap_or(original.effective_time())),
        )
        .unwrap();
    assert!(matches!(
        known.replace(AveragePriceOptionChanges {
            averaging_start_time: Some(start.add_days(1).unwrap()),
            ..Default::default()
        }),
        Err(Error::StaleHistory(_))
    ));
    assert_eq!(
        original
            .replace(AveragePriceOptionChanges::default())
            .unwrap(),
        original
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        option_type,
        OptionType::Put
    );
    check_field!(original, AveragePriceOptionChanges, strike, 105.0);
    let updated = original
        .replace(AveragePriceOptionChanges {
            effective_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            averaging_start_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            realized_average: Some(original.realized_average()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        updated.effective_time(),
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        averaging_start_time,
        Date::new(2025, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        expiry_time,
        Date::new(2026, 1, 2).unwrap().start_of_day()
    );
    check_field!(
        original,
        AveragePriceOptionChanges,
        realized_average,
        Some(105.0)
    );
    assert!(matches!(
        original.replace(AveragePriceOptionChanges {
            strike: Some(-1.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original, saved);
}

#[test]
fn settlement_replacements_roundtrip_and_switch_cash_and_asset() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let binary = BinaryBarrierOption::asset(BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))?;
    let touch = TouchOption::asset(TouchOptionTerms::new(
        start,
        end,
        120.0,
        BarrierType::UpAndIn,
    ))?;
    macro_rules! check {
        ($original:ident, $changes:ident) => {{
            assert_eq!(
                $original.replace($changes {
                    payoff: Some(
                        $original
                            .payout()
                            .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash)
                    ),
                    ..Default::default()
                })?,
                $original
            );
            let cash = $original.replace($changes {
                payoff: Some(kiyosi::BinaryPayoff::Cash(10.0)),
                ..Default::default()
            })?;
            assert_eq!(cash.payout(), Some(10.0));
            assert_eq!(cash.replace($changes::default())?, cash);
            assert_eq!(
                cash.replace($changes {
                    payoff: Some(kiyosi::BinaryPayoff::Asset),
                    ..Default::default()
                })?,
                $original
            );
            assert!(matches!(
                $original.replace($changes {
                    payoff: Some(kiyosi::BinaryPayoff::Cash(-1.0)),
                    ..Default::default()
                }),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!($original.payout(), None);
            assert_eq!(cash.payout(), Some(10.0));
        }};
    }
    check!(binary, BinaryBarrierOptionChanges);
    check!(touch, TouchOptionChanges);
    Ok(())
}
