use kiyosi::*;

macro_rules! check_field {
    ($original:ident, $changes:ident, $field:ident, $value:expr) => {
        check_field!($original, $changes, $field, $value, $field());
    };
    ($original:ident, $changes:ident, $field:ident, $value:expr, $($getter:tt)+) => {
        let changed = $original.replace($changes {
            $field: Some($value), ..Default::default()
        }).unwrap();
        assert_eq!(changed.$($getter)+, $value);
    };
}

#[test]
fn barrier_option_replacements_preserve_omissions_and_validate_each_field() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = BarrierOption::new(BarrierOptionTerms {
        option_type: OptionType::Call,
        strike: 100.0,
        effective_time: start,
        expiry_time: end,
        barrier_level: 120.0,
        barrier_type: BarrierType::UpAndOut,
        rebate: 0.0,
        rebate_timing: RebateTiming::AtExpiry,
        observation_mode: ObservationMode::Continuous,
        observation_times: vec![],
        touch_state: Some(BarrierTouchState::Untouched),
    })
    .unwrap();
    let saved = original.clone();
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
            observation_mode: Some(ObservationMode::Scheduled),
            observation_times: Some(vec![end]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), &[end]);
    assert!(matches!(
        original.replace(BarrierOptionChanges {
            observation_times: Some(vec![end]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
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
        option_type: OptionType::Call,
        strike: 100.0,
        effective_time: start,
        expiry_time: end,
        lower_barrier: 80.0,
        upper_barrier: 120.0,
        barrier_type: DoubleBarrierType::KnockOut,
        rebate: 0.0,
        rebate_timing: RebateTiming::AtExpiry,
        observation_mode: ObservationMode::Continuous,
        observation_times: vec![],
        touch_state: Some(BarrierTouchState::Untouched),
    })
    .unwrap();
    let saved = original.clone();
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
            observation_mode: Some(ObservationMode::Scheduled),
            observation_times: Some(vec![end]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), &[end]);
    assert!(matches!(
        original.replace(DoubleBarrierOptionChanges {
            observation_times: Some(vec![end]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
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
            option_type: OptionType::Call,
            strike: 100.0,
            effective_time: start,
            expiry_time: end,
            barrier_level: 120.0,
            barrier_type: BarrierType::UpAndOut,
            observation_mode: ObservationMode::Continuous,
            observation_times: vec![],
            touch_state: Some(BarrierTouchState::Untouched),
        },
        10.0,
    )
    .unwrap();
    let saved = original.clone();
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
            payout: Some(15.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(updated.payout(), Some(15.0));
    let scheduled = original
        .replace(BinaryBarrierOptionChanges {
            observation_mode: Some(ObservationMode::Scheduled),
            observation_times: Some(vec![end]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), &[end]);
    assert!(matches!(
        original.replace(BinaryBarrierOptionChanges {
            observation_times: Some(vec![end]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
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
            effective_time: start,
            expiry_time: end,
            barrier_level: 120.0,
            barrier_type: BarrierType::UpAndIn,
            settlement_timing: SettlementTiming::AtExpiry,
            observation_mode: ObservationMode::Continuous,
            observation_times: vec![],
            touch_state: Some(BarrierTouchState::Untouched),
        },
        10.0,
    )
    .unwrap();
    let saved = original.clone();
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
            payout: Some(15.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(updated.payout(), Some(15.0));
    let scheduled = original
        .replace(TouchOptionChanges {
            observation_mode: Some(ObservationMode::Scheduled),
            observation_times: Some(vec![end]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), &[end]);
    assert!(matches!(
        original.replace(TouchOptionChanges {
            observation_times: Some(vec![end]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        original.replace(TouchOptionChanges {
            payout: Some(-1.0),
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
            effective_time: start,
            expiry_time: end,
            lower_barrier: 80.0,
            upper_barrier: 120.0,
            barrier_type: DoubleBarrierType::KnockIn,
            settlement_timing: SettlementTiming::AtExpiry,
            observation_mode: ObservationMode::Continuous,
            observation_times: vec![],
            touch_state: Some(BarrierTouchState::Untouched),
        },
        10.0,
    )
    .unwrap();
    let saved = original.clone();
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
            observation_mode: Some(ObservationMode::Scheduled),
            observation_times: Some(vec![end]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(scheduled.observation_mode(), ObservationMode::Scheduled);
    assert_eq!(scheduled.observation_times(), &[end]);
    assert!(matches!(
        original.replace(DoubleTouchOptionChanges {
            observation_times: Some(vec![end]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
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
        option_type: OptionType::Call,
        strike: 100.0,
        knock_out_level: 120.0,
        daily_quantity: 1.0,
        acceleration_factor: 2.0,
        terminated: false,
        accumulated_quantity: Some(3.0),
        effective_time: start,
        expiry_time: end,
        knock_out_observation_times: Some(vec![end]),
    })
    .unwrap();
    let saved = original.clone();
    assert_eq!(
        original.replace(AccumulatorChanges::default()).unwrap(),
        original
    );
    check_field!(original, AccumulatorChanges, option_type, OptionType::Put);
    check_field!(original, AccumulatorChanges, strike, 105.0);
    check_field!(original, AccumulatorChanges, knock_out_level, 125.0);
    check_field!(original, AccumulatorChanges, daily_quantity, 2.0);
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
            ..Default::default()
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
        maturity_coupon_rate: 0.1,
        initial_spot: 100.0,
        knock_in_level: 80.0,
        upper_strike: 100.0,
        lower_strike: 0.0,
        knock_in_observation_mode: KnockInObservationMode::EveryTradingDay,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::new(end, 110.0)],
    })
    .unwrap();
    let saved = original.clone();
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
        principal_ratio,
        0.9,
        terms().principal_ratio
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
        vec![AutocallableObservation::new(end, 115.0)],
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
        coupon: PhoenixCoupon::Annual(0.1),
        initial_spot: 100.0,
        knock_in_level: 80.0,
        upper_strike: 100.0,
        lower_strike: 0.0,
        knock_in_observation_mode: KnockInObservationMode::EveryTradingDay,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::new(end, 110.0)],
    })
    .unwrap();
    let saved = original.clone();
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
        principal_ratio,
        0.9,
        terms().principal_ratio
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
        vec![AutocallableObservation::new(end, 115.0)],
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
        maturity_coupon_rate: 0.1,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::new(end, 110.0)],
    })
    .unwrap();
    let saved = original.clone();
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
        principal_ratio,
        0.9,
        terms().principal_ratio
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
        vec![AutocallableObservation::new(end, 115.0)],
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
        maturity_coupon_rate: 0.1,
        minimum_coupon_rate: 0.02,
        knock_in_level: 80.0,
        knock_in_observation_mode: KnockInObservationMode::EveryTradingDay,
        barrier_state: Some(AutocallableBarrierState::NoEvent),
        principal_ratio: 1.0,
        effective_time: start,
        expiry_time: end,
        observations: vec![AutocallableObservation::new(end, 110.0)],
    })
    .unwrap();
    let saved = original.clone();
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
        principal_ratio,
        0.9,
        terms().principal_ratio
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
        vec![AutocallableObservation::new(end, 115.0)],
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
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let saved = original.clone();
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
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    let saved = original.clone();
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
fn asset_replacements_reject_cash_payouts_without_changing_settlement() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let binary = BinaryBarrierOption::asset(BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))
    .unwrap();
    assert!(matches!(
        binary.replace(BinaryBarrierOptionChanges {
            payout: Some(10.0),
            ..Default::default()
        }),
        Err(Error::UnsupportedOperation(_))
    ));
    assert_eq!(binary.payout(), None);
    let touch = TouchOption::asset(TouchOptionTerms::new(
        start,
        end,
        120.0,
        BarrierType::UpAndIn,
    ))
    .unwrap();
    assert!(matches!(
        touch.replace(TouchOptionChanges {
            payout: Some(10.0),
            ..Default::default()
        }),
        Err(Error::UnsupportedOperation(_))
    ));
    assert_eq!(touch.payout(), None);
}
