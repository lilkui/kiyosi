use kiyosi::*;

#[test]
fn public_day_arithmetic_preserves_clocks_and_checks_bounds() -> Result<()> {
    let leap = Date::new(2024, 2, 28)?;
    assert_eq!(leap.add_days(1)?, Date::new(2024, 2, 29)?);
    assert_eq!(leap.add_days(2)?, Date::new(2024, 3, 1)?);
    assert_eq!(leap.days_until(leap.add_days(-2)?), -2);
    assert_eq!(leap.add_days(0)?, leap);
    for (date, shift) in [
        (Date::new(-32767, 1, 1)?, -1),
        (Date::new(32767, 12, 31)?, 1),
        (leap, i64::MIN),
        (leap, i64::MAX),
    ] {
        assert!(matches!(date.add_days(shift), Err(Error::InvalidInput(_))));
        let instant = Timestamp::new(date, 23, 59, 59, 999_999)?;
        assert!(matches!(
            instant.add_days(shift),
            Err(Error::InvalidInput(_))
        ));
    }
    let instant = Timestamp::new(Date::new(2025, 12, 31)?, 15, 2, 3, 123_456)?;
    let next = instant.add_days(1)?;
    assert_eq!(next.date(), Date::new(2026, 1, 1)?);
    assert_eq!(next.micros_since_midnight(), 54_123_123_456);
    assert_eq!(next.add_days(-1)?, instant);
    Ok(())
}

fn time(day: u32, hour: u32, microsecond: u32) -> Timestamp {
    Timestamp::new(Date::new(2025, 1, day).unwrap(), hour, 0, 0, microsecond).unwrap()
}

fn market(at: Timestamp, spot: f64) -> PricingContext {
    PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0001).unwrap(),
        spot,
        at,
        TradingCalendar::all_days(),
    )
    .unwrap()
}

fn accumulator_engines() -> Vec<Box<dyn PricingEngine<Accumulator>>> {
    vec![
        Box::new(QuadratureAccumulatorEngine::default()),
        Box::new(
            FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
                asset_step_count: 600,
                time_step_count: 100,
                asset_upper_boundary: Some(160.0),
                ..Default::default()
            })
            .unwrap(),
        ),
        Box::new(
            MonteCarloAccumulatorEngine::new(MonteCarloSettings {
                path_count: 128,
                seed: Some(42),
                ..Default::default()
            })
            .unwrap(),
        ),
    ]
}

#[test]
fn intraday_expiry_and_accrual_preserve_microseconds() {
    let start = time(6, 10, 123_456);
    let expiry = time(6, 15, 654_321);
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap();
    let context = PricingContext::new(model, 100.0, start).unwrap();
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, expiry).unwrap();
    let put = EuropeanOption::new(OptionType::Put, 100.0, start, expiry).unwrap();
    let years = (18_000.0 + 0.530_865) / (365.0 * 86_400.0);
    let parity = AnalyticEuropeanEngine.price(&call, &context).unwrap()
        - AnalyticEuropeanEngine.price(&put, &context).unwrap();
    assert!(
        (parity - 100.0 * ((-0.02_f64 * years).exp() - (-0.05_f64 * years).exp())).abs() < 1e-12
    );
    assert!(
        (context
            .model_parameters()
            .risk_free_curve()
            .factor(start, expiry)
            .unwrap()
            - (-0.05_f64 * years).exp())
        .abs()
            < 1e-15
    );
    assert_eq!(call.expiry_time(), expiry);
    assert_eq!(
        AnalyticEuropeanEngine
            .price(&call, &market(expiry, 110.0))
            .unwrap(),
        10.0
    );
    for at in [time(6, 10, 123_455), time(6, 15, 654_322)] {
        assert!(matches!(
            AnalyticEuropeanEngine.price(&call, &market(at, 110.0)),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn observation_schedules_preserve_clock_and_full_timestamp_order() {
    let clock = |month, day, hour| {
        Timestamp::new(Date::new(2025, month, day).unwrap(), hour, 11, 12, 123_456).unwrap()
    };
    let calendar = TradingCalendar::weekdays();
    let schedule =
        ObservationSchedule::monthly(clock(1, 31, 10), clock(3, 31, 15), 1, &calendar).unwrap();
    assert_eq!(schedule.times(), &[clock(2, 28, 10), clock(3, 31, 10)]);
    let fixed =
        ObservationSchedule::fixed_interval(clock(1, 3, 10), clock(1, 7, 9), 1, &calendar).unwrap();
    assert_eq!(fixed.times(), &[clock(1, 6, 10)]);
    let times = vec![time(6, 10, 0), time(6, 10, 1), time(6, 15, 0)];
    assert_eq!(
        ObservationSchedule::new(times.clone(), time(6, 9, 0), time(6, 16, 0), &calendar)
            .unwrap()
            .times(),
        times
    );
    for invalid in [
        vec![time(6, 10, 0); 2],
        vec![time(6, 15, 0), time(6, 10, 0)],
    ] {
        assert!(matches!(
            ObservationSchedule::new(invalid, time(6, 9, 0), time(6, 16, 0), &calendar),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn scheduled_barriers_use_exact_current_observations_and_strict_history() {
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        time(6, 9, 0),
        time(6, 16, 0),
        110.0,
        BarrierType::UpAndOut,
    );
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_times = vec![time(6, 10, 0), time(6, 15, 0)];
    terms.rebate = 7.0;
    terms.rebate_timing = RebateTiming::AtHit;
    let option = BarrierOption::new(terms).unwrap();
    assert_eq!(
        AnalyticBarrierEngine
            .price(&option, &market(time(6, 10, 0), 110.0))
            .unwrap(),
        7.0
    );
    assert!(matches!(
        AnalyticBarrierEngine.price(&option, &market(time(6, 10, 1), 100.0)),
        Err(Error::InvalidInput(_))
    ));
    let touched = option.replace_history(Some(BarrierTouchState::Touched));
    assert!(matches!(
        AnalyticBarrierEngine.price(&touched, &market(time(6, 9, 0), 110.0)),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        AnalyticBarrierEngine
            .price(&touched, &market(time(6, 10, 1), 100.0))
            .unwrap(),
        0.0
    );
}

#[test]
fn range_accrual_observes_same_day_and_excludes_only_the_effective_instant() {
    let option = RangeAccrualOption::new(RangeAccrualTerms::new(
        1000.0,
        0.12,
        1.0,
        1000.0,
        time(6, 9, 0),
        time(6, 15, 0),
    ))
    .unwrap();
    let expected = 1000.0 * 0.12 * 6.0 / (24.0 * 365.0);
    assert!(
        (AnalyticRangeAccrualEngine
            .price(&option, &market(time(6, 15, 0), 100.0))
            .unwrap()
            - expected)
            .abs()
            < 1e-14
    );
    let option = option
        .replace(RangeAccrualOptionChanges {
            effective_time: Some(time(6, 15, 0)),
            expiry_time: Some(time(7, 15, 0)),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (AnalyticRangeAccrualEngine
            .price(&option, &market(time(6, 15, 1), 100.0))
            .unwrap()
            - 120.0 / 365.0)
            .abs()
            < 1e-14
    );
    assert!(matches!(
        AnalyticRangeAccrualEngine.price(
            &option.replace_history(Some(1)).unwrap(),
            &market(time(6, 15, 1), 100.0)
        ),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn daily_history_uses_expiry_clock_and_inclusive_start() {
    let option = Accumulator::new(AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        1.0,
        time(6, 9, 0),
        time(7, 15, 0),
    ))
    .unwrap();
    for engine in accumulator_engines() {
        assert!(
            (engine
                .price(&option, &market(time(6, 10, 0), 105.0))
                .unwrap()
                - 10.0)
                .abs()
                < 1e-4
        );
        assert!(matches!(
            engine.price(&option, &market(time(6, 15, 1), 105.0)),
            Err(Error::InvalidInput(_))
        ));
        let single = option
            .replace(AccumulatorChanges {
                effective_time: Some(time(7, 15, 0)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            engine
                .price(&single, &market(time(7, 15, 0), 105.0))
                .unwrap(),
            5.0
        );
    }
    let note = SnowballOption::new(SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        0.0,
        time(6, 9, 0),
        time(7, 15, 0),
        vec![AutocallableObservation::new(time(7, 15, 0), 110.0).unwrap()],
    ))
    .unwrap();
    assert!(
        (QuadratureAutocallableEngine::default()
            .price(&note, &market(time(6, 10, 0), 100.0))
            .unwrap()
            - (1.0 + 0.06 * 30.0 / (365.0 * 24.0)))
            .abs()
            < 1e-12
    );
    assert!(matches!(
        QuadratureAutocallableEngine::default().price(&note, &market(time(6, 15, 1), 100.0)),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn explicit_accumulator_knock_outs_do_not_add_daily_quantity() {
    for (hour, barrier, expected) in [
        (10, 110.0, 10.0),
        (10, 100.0, 0.0),
        (15, 100.0, 0.0),
        (16, 100.0, 5.0),
    ] {
        let mut terms =
            AccumulatorTerms::new(100.0, barrier, 1.0, 1.0, time(6, 9, 0), time(7, 15, 0));
        terms.knock_out_observation_times = Some(vec![time(6, hour, 0)]);
        let option = Accumulator::new(terms).unwrap();
        for engine in accumulator_engines() {
            let actual = engine
                .price(&option, &market(time(6, 9, 0), 105.0))
                .unwrap();
            assert!(
                (actual - expected).abs() < 1e-4,
                "hour={hour}, barrier={barrier}, {actual} != {expected}"
            );
        }
    }
}

#[test]
fn autocallable_explicit_events_do_not_trigger_daily_knock_in() {
    let start = time(6, 9, 0);
    let expiry = time(6, 15, 0);
    let note = SnowballOption::new(SnowballTerms::new(
        0.12,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        expiry,
        vec![AutocallableObservation::new(time(6, 10, 0), 1000.0).unwrap()],
    ))
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, -1000.0, 0.0001).unwrap(),
        70.0,
        start,
    )
    .unwrap();
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 128,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let fd = FiniteDifferenceAutocallableEngine::default();
    let quad = QuadratureAutocallableEngine::default();
    let expected = 1.0 + 0.12 * 6.0 / (365.0 * 24.0);
    for engine in [&mc as &dyn PricingEngine<SnowballOption>, &fd, &quad] {
        let actual = engine.price(&note, &context).unwrap();
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }
    let mut terms = SnowballTerms::new(
        0.0,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        expiry,
        vec![AutocallableObservation::new(time(6, 10, 0), 110.0).unwrap()],
    );
    terms.observations[0].coupon_rate = 0.12;
    let note = SnowballOption::new(terms).unwrap();
    let context = market(start, 120.0);
    let expected = 1.0 + 0.12 / (365.0 * 24.0);
    for engine in [&mc as &dyn PricingEngine<SnowballOption>, &fd, &quad] {
        assert!((engine.price(&note, &context).unwrap() - expected).abs() < 1e-6);
    }
}

#[test]
fn nontrading_dates_do_not_trigger_daily_knock_out_or_suppress_vega() {
    let option = Accumulator::new(AccumulatorTerms {
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 1.0, time(3, 9, 0), time(6, 15, 0))
    })
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
        110.0,
        time(4, 15, 0),
    )
    .unwrap();
    let result = QuadratureAccumulatorEngine::default()
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Vega]),
            Default::default(),
        )
        .unwrap();
    let root_time = (2.0_f64 / 365.0).sqrt();
    let z = 0.1 * root_time;
    let expected = -1.05 * root_time * (-0.5 * z * z).exp() / std::f64::consts::TAU.sqrt();
    assert!((result.require(Greek::Vega).unwrap() - expected).abs() < 1e-6);
}

#[test]
fn finite_difference_rejects_unresolvable_microsecond_events() {
    let start = Date::new(1, 1, 1).unwrap().start_of_day();
    let first = Timestamp::new(Date::new(9998, 1, 1).unwrap(), 10, 0, 0, 1).unwrap();
    let last = Timestamp::new(first.date(), 10, 0, 0, 2).unwrap();
    let note = PhoenixOption::new(PhoenixTerms {
        coupon: PhoenixCoupon::PerPeriod(0.01),
        knock_in_observation_mode: KnockInObservationMode::AtExpiry,
        ..PhoenixTerms::new(
            0.0,
            100.0,
            1.0,
            1.0,
            1.0,
            start,
            last,
            vec![
                AutocallableObservation::new(first, 1000.0).unwrap(),
                AutocallableObservation::new(last, 1000.0).unwrap(),
            ],
        )
    })
    .unwrap();
    let context = market(start, 100.0);
    assert!(matches!(
        FiniteDifferenceAutocallableEngine::default().price(&note, &context),
        Err(Error::ComputationFailure(_))
    ));
    assert!(
        (QuadratureAutocallableEngine::default()
            .price(&note, &context)
            .unwrap()
            - 1.02)
            .abs()
            < 1e-12
    );
}
