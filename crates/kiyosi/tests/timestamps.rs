use kiyosi::*;

#[test]
fn scheduled_time_stencils_exclude_inclusive_events_and_allow_gaps() -> Result<()> {
    let clock = TimeOfDay::new(10, 0, 0, 0)?;
    let start = Date::new(2025, 1, 2)?.at_time(clock);
    let times = [start.add_days(1)?, start.add_days(4)?, start.add_days(5)?];
    let end = times[2];
    let mut barrier_terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    barrier_terms.observation_times = Some(times.to_vec());
    barrier_terms.touch_state = Some(BarrierTouchState::Untouched);
    let barrier = BarrierOption::new(barrier_terms)?;
    let mut note_terms = BinarySnowballTerms::new(
        0.1,
        start,
        end,
        autocallable_observations(&times, &[110.0; 3], Some(&[0.1; 3]), None)?,
        TradingCalendar::weekdays(),
    );
    note_terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = BinarySnowballOption::new(note_terms)?;
    let mut accumulator_terms = AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        2.0,
        start,
        end,
        TimeOfDay::new(15, 0, 0, 0)?,
    );
    accumulator_terms.knock_out_observation_times = Some(times.to_vec());
    let accumulator = Accumulator::new(accumulator_terms)?;
    let accrual = RangeAccrualOption::new(RangeAccrualTerms::new(
        100.0, 0.1, 90.0, 110.0, start, end, clock,
    ))?;
    assert_eq!(accrual.observation_schedule().times(), times);
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let contracts: [&dyn OptionContract; 4] = [&barrier, &note, &accumulator, &accrual];
    for (before, after, available) in [
        (start, start.add_micros(1)?, true),
        (times[0].add_micros(-1)?, times[0], false),
        (times[0], times[0], false),
        (times[0], times[0].add_micros(1)?, false),
        (times[0].add_micros(-1)?, times[0].add_micros(1)?, false),
        (times[0].add_micros(1)?, times[0].add_micros(2)?, true),
        (times[1], times[1], false),
        (times[2], times[2], false),
        (times[2].add_micros(1)?, times[2].add_micros(2)?, true),
        (times[0].add_micros(1)?, times[0].add_micros(-1)?, true),
    ] {
        for (index, contract) in contracts.iter().enumerate() {
            assert_eq!(
                contract.time_stencil_available(&context, before, after),
                available,
                "contract {index}, {before:?} through {after:?}"
            );
        }
    }
    let no_knock_out = accumulator.replace(AccumulatorChanges {
        knock_out_observation_times: Some(Some(Vec::new())),
        ..Default::default()
    })?;
    for time in times {
        assert!(no_knock_out.time_stencil_available(&context, time, time));
    }
    Ok(())
}

#[test]
fn public_intraday_arithmetic_crosses_days_and_checks_supported_bounds() -> Result<()> {
    let at = Timestamp::new(Date::new(2025, 12, 31)?, 23, 58, 0, 123_456)?;
    let later = at.add_micros(300_000_000)?;
    assert_eq!(
        later,
        Timestamp::new(Date::new(2026, 1, 1)?, 0, 3, 0, 123_456)?
    );
    assert_eq!(later.add_micros(-300_000_000)?, at);
    assert_eq!(at.add_micros(0)?, at);
    let leap = Timestamp::new(Date::new(2024, 2, 28)?, 23, 59, 59, 999_999)?;
    assert_eq!(leap.add_micros(1)?, Date::new(2024, 2, 29)?.start_of_day());
    assert_eq!(leap.add_micros(1)?.add_micros(-1)?, leap);
    let first = Date::new(-32767, 1, 1)?.start_of_day();
    let last = Timestamp::new(Date::new(32767, 12, 31)?, 23, 59, 59, 999_999)?;
    assert_eq!(first.add_micros(1)?.add_micros(-1)?, first);
    assert_eq!(last.add_micros(-1)?.add_micros(1)?, last);
    for (at, shift) in [(first, -1), (last, 1), (at, i64::MIN), (at, i64::MAX)] {
        assert!(matches!(at.add_micros(shift), Err(Error::InvalidInput(_))));
    }
    Ok(())
}

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

#[test]
fn public_year_fractions_preserve_intraday_time_and_direction() -> Result<()> {
    let start = Timestamp::new(Date::new(2024, 2, 28)?, 23, 59, 59, 999_999)?;
    let next = Date::new(2024, 2, 29)?.start_of_day();
    let microsecond_years = 1.0 / (365.0 * 86_400.0 * 1_000_000.0);
    assert_eq!(start.years_until(next), microsecond_years);
    assert_eq!(next.years_until(start), -microsecond_years);
    assert_eq!(start.years_until(start), 0.0);
    assert_eq!(start.years_until(start.add_days(365)?), 1.0);
    let last = Timestamp::new(Date::new(32767, 12, 31)?, 23, 59, 59, 999_999)?;
    let first = Date::new(-32767, 1, 1)?.start_of_day();
    assert!(first.years_until(last).is_finite());
    assert_eq!(first.years_until(last), -last.years_until(first));
    Ok(())
}

fn time(day: u32, hour: u32, microsecond: u32) -> Timestamp {
    Timestamp::new(Date::new(2025, 1, day).unwrap(), hour, 0, 0, microsecond).unwrap()
}

fn market(at: Timestamp, spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0001).unwrap(),
        spot,
        at,
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
fn closed_date_settlement_preserves_open_date_observations() -> Result<()> {
    let start = time(2, 16, 0);
    let observation = time(3, 15, 0);
    let valuation = time(3, 16, 0);
    let calendar = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![time(6, 0, 0).date()],
        vec![],
        None,
    )?;
    for expiry in [time(4, 16, 0), time(5, 16, 0), time(6, 16, 0)] {
        let mut terms = RangeAccrualTerms::new(
            1000.0,
            0.12,
            80.0,
            120.0,
            start,
            expiry,
            observation.time_of_day(),
        );
        terms.calendar = calendar.clone();
        terms.realized_hit_count = Some(1);

        let range = RangeAccrualOption::new(terms)?;
        assert_eq!(range.observation_schedule().times(), &[observation]);

        let mut terms = AccumulatorTerms::new(
            90.0,
            110.0,
            1.0,
            2.0,
            start,
            expiry,
            observation.time_of_day(),
        );
        terms.calendar = calendar.clone();
        terms.accumulated_quantity = Some(3.0);

        terms.knock_out_settled = Some(false);

        let accumulator = Accumulator::new(terms)?;
        assert_eq!(accumulator.accumulation_schedule()?.times(), &[observation]);
        assert_eq!(
            accumulator.knock_out_observation_schedule()?.times(),
            &[observation]
        );

        let records = vec![AutocallableObservation::snowball(observation, 110.0, 0.0)?];
        let mut terms = SnowballTerms::new(
            0.12,
            100.0,
            80.0,
            100.0,
            0.0,
            start,
            expiry,
            records.clone(),
            kiyosi::KnockInObservation::EveryTradingDay {
                observation_clock_time: observation.time_of_day(),
            },
            calendar.clone(),
        );
        terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
        let note = SnowballOption::new(terms.clone())?;
        assert_eq!(
            note.knock_in_observation_schedule()?.unwrap().times(),
            &[observation]
        );
        terms.knock_in_observation = KnockInObservation::AtExpiry;
        assert!(matches!(
            SnowballOption::new(terms),
            Err(Error::InvalidInput(_))
        ));

        let mut terms = BinarySnowballTerms::new(0.12, start, expiry, records, calendar.clone());
        terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
        let binary = BinarySnowballOption::new(terms)?;
        let coupon = 0.12 * start.years_until(expiry);
        for volatility in [0.0, 0.2] {
            for at in [valuation, expiry] {
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(0.05, 0.0, volatility)?,
                    70.0,
                    at,
                )?;
                let discount = (-0.05 * at.years_until(expiry)).exp();
                assert!(
                    (AnalyticRangeAccrualEngine.price(&range, &context)?
                        - 1000.0 * coupon * discount)
                        .abs()
                        < 1e-12
                );
                macro_rules! check_note {
                    ($option:expr) => {
                        for engine in [
                            &QuadratureAutocallableEngine::default() as &dyn PricingEngine<_>,
                            &FiniteDifferenceAutocallableEngine::default(),
                            &MonteCarloAutocallableEngine::default(),
                        ] {
                            assert!(
                                (engine.price($option, &context)? - (1.0 + coupon) * discount)
                                    .abs()
                                    < 1e-12
                            );
                        }
                    };
                }
                check_note!(&note);
                check_note!(&binary);
                if at == expiry {
                    for engine in accumulator_engines() {
                        assert_eq!(engine.price(&accumulator, &context)?, -60.0);
                    }
                }
            }
        }
    }
    Ok(())
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
    for at in [time(6, 15, 654_322), time(6, 15, 654_323)] {
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
    let schedule = ObservationSchedule::monthly(
        clock(1, 31, 10),
        clock(3, 31, 15),
        1,
        &calendar,
        (clock(1, 31, 10)).time_of_day(),
    )
    .unwrap();
    assert_eq!(schedule.times(), &[clock(2, 28, 10), clock(3, 31, 10)]);
    let fixed = ObservationSchedule::fixed_interval(
        clock(1, 3, 10),
        clock(1, 7, 9),
        1,
        &calendar,
        (clock(1, 3, 10)).time_of_day(),
    )
    .unwrap();
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
    terms.observation_times = Some(vec![time(6, 10, 0), time(6, 15, 0)]);
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
        Err(Error::MissingHistory(_))
    ));
    let touched = option
        .replace(BarrierOptionChanges {
            touch_state: Some(Some(BarrierTouchState::Touched)),
            ..Default::default()
        })
        .unwrap();
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
        (time(6, 15, 0)).time_of_day(),
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
        option.replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(Some(1)),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn daily_history_uses_specified_clock_and_inclusive_start() {
    let option = Accumulator::new(AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        1.0,
        time(6, 9, 0),
        time(7, 15, 0),
        (time(7, 15, 0)).time_of_day(),
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
            Err(Error::MissingHistory(_))
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
        vec![AutocallableObservation::snowball(time(7, 15, 0), 110.0, 0.0).unwrap()],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (time(7, 15, 0)).time_of_day(),
        },
        TradingCalendar::weekdays(),
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
        Err(Error::MissingHistory(_))
    ));
}

#[test]
fn time_of_day_preserves_microseconds_and_rejects_invalid_components() -> Result<()> {
    let clock = TimeOfDay::new(15, 11, 12, 123_456)?;
    assert_eq!(clock.micros_since_midnight(), 54_672_123_456);
    let date = Date::new(2025, 1, 6)?;
    assert_eq!(
        date.at_time(clock),
        Timestamp::new(date, 15, 11, 12, 123_456)?
    );
    assert_eq!(date.at_time(clock).time_of_day(), clock);
    for (hour, minute, second, microsecond) in [
        (24, 0, 0, 0),
        (0, 60, 0, 0),
        (0, 0, 60, 0),
        (0, 0, 0, 1_000_000),
    ] {
        assert!(matches!(
            TimeOfDay::new(hour, minute, second, microsecond),
            Err(Error::InvalidInput(_))
        ));
    }
    Ok(())
}

#[test]
fn generated_schedules_use_an_independent_clock_and_complete_upper_bound() -> Result<()> {
    let calendar = TradingCalendar::weekdays();
    let clock = TimeOfDay::new(15, 0, 0, 123_456)?;
    let start = time(3, 9, 0);
    let end = time(7, 14, 0);
    let fixed = ObservationSchedule::fixed_interval(start, end, 1, &calendar, clock)?;
    assert_eq!(fixed.times(), &[time(6, 15, 123_456)]);
    let start = Timestamp::new(Date::new(2025, 1, 31)?, 9, 0, 0, 0)?;
    let end = Timestamp::new(Date::new(2025, 3, 31)?, 14, 0, 0, 0)?;
    let monthly = ObservationSchedule::monthly(start, end, 1, &calendar, clock)?;
    assert_eq!(monthly.times(), &[Date::new(2025, 2, 28)?.at_time(clock)]);
    let inclusive = ObservationSchedule::monthly(
        start,
        Date::new(2025, 3, 31)?.at_time(clock),
        1,
        &calendar,
        clock,
    )?;
    assert_eq!(
        inclusive.times(),
        &[
            Date::new(2025, 2, 28)?.at_time(clock),
            Date::new(2025, 3, 31)?.at_time(clock),
        ]
    );
    Ok(())
}

#[test]
fn daily_clock_controls_current_history_and_does_not_add_expiry_units() -> Result<()> {
    let clock = TimeOfDay::new(15, 0, 0, 123_456)?;
    let option = Accumulator::new(AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        1.0,
        time(6, 9, 0),
        time(6, 18, 0),
        clock,
    ))?;
    assert_eq!(option.observation_clock_time(), clock);
    for engine in accumulator_engines() {
        let before = engine.price(&option, &market(time(6, 15, 123_455), 105.0))?;
        assert!((before - 5.0).abs() < 1e-4, "before={before}");
        let current = engine.price(&option, &market(time(6, 15, 123_456), 105.0))?;
        assert!((current - 5.0).abs() < 1e-4);
        assert_eq!(
            engine.price(&option, &market(time(6, 15, 123_456), 115.0))?,
            0.0
        );
        assert!(matches!(
            engine.price(&option, &market(time(6, 15, 123_457), 105.0)),
            Err(Error::MissingHistory(_))
        ));
        let seasoned = option.replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(1.0)),
            knock_out_settled: Some(Some(false)),
            ..Default::default()
        })?;
        assert_eq!(
            engine.price(&seasoned, &market(time(6, 18, 0), 105.0))?,
            5.0
        );
        assert_eq!(
            engine.price(&seasoned, &market(time(6, 18, 0), 115.0))?,
            15.0
        );
    }
    Ok(())
}

#[test]
fn empty_daily_windows_are_valid_except_for_range_accrual() -> Result<()> {
    let start = time(6, 16, 0);
    let end = time(7, 14, 0);
    let clock = TimeOfDay::new(15, 0, 0, 0)?;
    let option = Accumulator::new(AccumulatorTerms::new(
        100.0, 110.0, 1.0, 1.0, start, end, clock,
    ))?;
    for engine in accumulator_engines() {
        assert_eq!(engine.price(&option, &market(start, 105.0))?, 0.0);
    }
    assert!(matches!(
        RangeAccrualOption::new(RangeAccrualTerms::new(
            1000.0, 0.12, 90.0, 110.0, start, end, clock,
        )),
        Err(Error::InvalidInput(_))
    ));
    let note = SnowballOption::new(SnowballTerms::new(
        0.0,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        vec![AutocallableObservation::snowball(end, 1000.0, 0.0)?],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        },
        TradingCalendar::weekdays(),
    ))?;
    assert_eq!(
        QuadratureAutocallableEngine::default().price(&note, &market(start, 70.0))?,
        1.0
    );
    Ok(())
}

#[test]
fn range_accrual_discounts_known_coupon_between_final_observation_and_payment() -> Result<()> {
    let clock = TimeOfDay::new(15, 0, 0, 0)?;
    let option = RangeAccrualOption::new(RangeAccrualTerms::new(
        1000.0,
        0.12,
        90.0,
        110.0,
        time(6, 9, 0),
        time(6, 18, 0),
        clock,
    ))?;
    let coupon = 120.0 * 9.0 / (365.0 * 24.0);
    for (hour, hits) in [(15, None), (16, Some(1)), (18, Some(1))] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.5, 0.0, 0.2)?,
            100.0,
            time(6, hour, 0),
        )?;
        let expected = coupon * (-0.5 * f64::from(18 - hour) / (365.0 * 24.0)).exp();
        let seasoned = option.replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(hits),
            ..Default::default()
        })?;
        let actual = AnalyticRangeAccrualEngine.price(&seasoned, &context)?;
        assert!(
            (actual - expected).abs() < 1e-14,
            "hour={hour}: {actual} != {expected}"
        );
        assert!(matches!(
            implied_volatility(
                &AnalyticRangeAccrualEngine,
                &seasoned,
                &context,
                actual,
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0),
            ),
            Err(Error::UnsupportedOperation(_))
        ));
    }
    assert!(matches!(
        option.replace(RangeAccrualOptionChanges {
            observation_clock_time: Some(TimeOfDay::new(8, 0, 0, 0)?),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(option.observation_clock_time(), clock);
    Ok(())
}

#[test]
fn off_clock_maturity_settles_without_additional_knock_in_in_every_engine() -> Result<()> {
    let clock = TimeOfDay::new(15, 0, 0, 0)?;
    let mut terms = SnowballTerms::new(
        0.12,
        100.0,
        80.0,
        100.0,
        0.0,
        time(6, 9, 0),
        time(6, 18, 0),
        vec![AutocallableObservation::snowball(
            time(6, 10, 0),
            1000.0,
            0.0,
        )?],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        },
        TradingCalendar::weekdays(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let daily = SnowballOption::new(terms)?;
    let at_expiry = daily.replace(SnowballOptionChanges {
        barrier_state: Some(daily.barrier_state()),
        knock_in_observation: Some(KnockInObservation::AtExpiry),
        ..Default::default()
    })?;
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 128,
        seed: Some(42),
        ..Default::default()
    })?;
    let fd = FiniteDifferenceAutocallableEngine::default();
    let quad = QuadratureAutocallableEngine::default();
    let expected = 1.0 + 0.12 * 9.0 / (365.0 * 24.0);
    for hour in [16, 18] {
        for volatility in [0.0, 0.0001] {
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.0, 0.0, volatility)?,
                70.0,
                time(6, hour, 0),
            )?;
            for engine in [&mc as &dyn PricingEngine<SnowballOption>, &fd, &quad] {
                let actual = engine.price(&daily, &context)?;
                assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
                assert!(matches!(
                    implied_volatility(
                        engine,
                        &daily,
                        &context,
                        actual,
                        kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
                    ),
                    Err(Error::UnsupportedOperation(_))
                ));
                let observed = engine.price(&at_expiry, &context)?;
                assert!((observed - 0.7).abs() < 1e-5, "{observed} != 0.7");
            }
        }
    }
    let before_final_observation = PricingContext::new(
        BlackScholesMertonParameters::new(-1000.0, 0.0, 0.0001)?,
        100.0,
        time(6, 14, 0),
    )?;
    let expected_discounted = expected * (1000.0_f64 * 4.0 / (365.0 * 24.0)).exp();
    for engine in [&mc as &dyn PricingEngine<SnowballOption>, &fd, &quad] {
        let actual = engine.price(&daily, &before_final_observation)?;
        assert!(
            (actual - expected_discounted).abs() < 1e-5,
            "daily: {actual} != {expected_discounted}"
        );
        let observed = engine.price(&at_expiry, &before_final_observation)?;
        assert!((observed - 1.0).abs() < 1e-5, "at_expiry: {observed} != 1");
    }
    let context = market(time(6, 18, 0), 80.0);
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for result in [
        quad.price_with_greeks(&daily, &context, request)?,
        fd.price_with_greeks(&daily, &context, request)?,
        mc.price_with_greeks(&daily, &context, request)?,
    ] {
        assert_eq!(result.require(Greek::Delta)?, 0.0);
        assert_eq!(result.require(Greek::Gamma)?, 0.0);
    }
    assert_eq!(
        quad.price_with_greeks(&at_expiry, &context, request)?
            .get(Greek::Delta),
        None
    );
    Ok(())
}

#[test]
fn knock_in_observation_replacement_switches_rule_and_clock_atomically() -> Result<()> {
    let clock = TimeOfDay::new(15, 0, 0, 123_456)?;
    let original = SnowballOption::new(SnowballTerms::new(
        0.12,
        100.0,
        80.0,
        100.0,
        0.0,
        time(6, 9, 0),
        time(7, 18, 0),
        vec![AutocallableObservation::snowball(
            time(7, 15, 0),
            110.0,
            0.0,
        )?],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        },
        TradingCalendar::weekdays(),
    ))?;
    let inactive = original.replace(SnowballOptionChanges {
        knock_in_observation: Some(KnockInObservation::AtExpiry),
        ..Default::default()
    })?;
    assert_eq!(inactive.observation_clock_time(), None);
    let active = inactive.replace(SnowballOptionChanges {
        knock_in_observation: Some(KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        }),
        ..Default::default()
    })?;
    assert_eq!(active, original);
    assert_eq!(original.observation_clock_time(), Some(clock));
    assert_eq!(inactive.observation_clock_time(), None);
    Ok(())
}

#[test]
fn explicit_accumulator_knock_outs_do_not_add_daily_quantity() {
    for (hour, barrier, expected) in [
        (10, 110.0, 10.0),
        (10, 100.0, 0.0),
        (15, 100.0, 0.0),
        (16, 100.0, 5.0),
    ] {
        let mut terms = AccumulatorTerms::new(
            100.0,
            barrier,
            1.0,
            1.0,
            time(6, 9, 0),
            time(7, 15, 0),
            (time(7, 15, 0)).time_of_day(),
        );
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
        vec![AutocallableObservation::snowball(time(6, 10, 0), 1000.0, 0.0).unwrap()],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (expiry).time_of_day(),
        },
        TradingCalendar::weekdays(),
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
        vec![AutocallableObservation::snowball(time(6, 10, 0), 110.0, 0.0).unwrap()],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (expiry).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.autocall_observations[0] = terms.autocall_observations[0]
        .replace(AutocallableObservationChanges {
            coupon_rate: Some(0.12),
            ..Default::default()
        })
        .unwrap();
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
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(
            100.0,
            110.0,
            1.0,
            1.0,
            time(3, 9, 0),
            time(6, 15, 0),
            (time(6, 15, 0)).time_of_day(),
        )
    })
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
        110.0,
        time(4, 15, 0),
    )
    .unwrap();
    let result = QuadratureAccumulatorEngine::default()
        .price_with_greeks(&option, &context, GreeksRequest::new([Greek::Vega]))
        .unwrap();
    let root_time = (2.0_f64 / 365.0).sqrt();
    let z = 0.1 * root_time;
    let expected = -105.0 * root_time * (-0.5 * z * z).exp() / std::f64::consts::TAU.sqrt();
    assert!((result.require(Greek::Vega).unwrap() - expected).abs() < 1e-6);
}

#[test]
fn finite_difference_rejects_unresolvable_microsecond_events() {
    let start = Date::new(1, 1, 1).unwrap().start_of_day();
    let first = Timestamp::new(Date::new(9998, 1, 1).unwrap(), 10, 0, 0, 1).unwrap();
    let last = Timestamp::new(first.date(), 10, 0, 0, 2).unwrap();
    let note = PhoenixOption::new(PhoenixTerms {
        coupon_per_period: 0.01,
        knock_in_observation: KnockInObservation::AtExpiry,
        ..PhoenixTerms::new(
            0.0,
            100.0,
            1.0,
            1.0,
            1.0,
            start,
            last,
            vec![
                AutocallableObservation::phoenix(first, 1000.0, 0.0).unwrap(),
                AutocallableObservation::phoenix(last, 1000.0, 0.0).unwrap(),
            ],
            kiyosi::KnockInObservation::EveryTradingDay {
                observation_clock_time: (last).time_of_day(),
            },
            TradingCalendar::weekdays(),
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
#[test]
fn subday_time_bumps_converge_to_intraday_theta() -> Result<()> {
    use std::time::Duration;
    let date = Date::new(2025, 1, 2)?;
    let start = Timestamp::new(date, 11, 0, 0, 0)?;
    let now = Timestamp::new(date, 12, 0, 0, 0)?;
    let end = Timestamp::new(date, 13, 0, 0, 0)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        now,
    )?;
    let request = GreeksRequest::new([Greek::Theta]);
    let exact = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, request)?
        .require(Greek::Theta)?;
    let mut errors = Vec::new();
    for seconds in [60, 1] {
        let value = numerical_greeks(
            &AnalyticEuropeanEngine,
            &option,
            &context,
            request,
            NumericalShiftSettings {
                time_shift: Some(Duration::from_secs(seconds)),
                ..Default::default()
            },
        )?
        .require(Greek::Theta)?;
        errors.push((value - exact).abs());
    }
    assert!(errors[1] < errors[0]);
    assert!(errors[1] < exact.abs() * 1e-7);
    for shift in [Duration::ZERO, Duration::from_nanos(1), Duration::MAX] {
        assert!(matches!(
            NumericalShiftSettings {
                time_shift: Some(shift),
                ..Default::default()
            }
            .validate(),
            Err(Error::InvalidInput(_))
        ));
    }
    NumericalShiftSettings {
        time_shift: Some(Duration::from_micros(1)),
        ..Default::default()
    }
    .validate()?;
    Ok(())
}
#[test]
fn contract_schedules_expose_effective_observations() -> Result<()> {
    let calendar = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![Date::new(2025, 1, 7)?],
        vec![],
        None,
    )?;
    let clock = TimeOfDay::new(15, 0, 0, 123_456)?;
    let start = Date::new(2025, 1, 6)?.at_time(clock);
    let last = Date::new(2025, 1, 8)?.at_time(clock);
    let expiry = Timestamp::new(last.date(), 17, 0, 0, 0)?;
    let mut range_terms = RangeAccrualTerms::new(1000.0, 0.12, 90.0, 110.0, start, expiry, clock);
    range_terms.calendar = calendar.clone();
    let range = RangeAccrualOption::new(range_terms)?;
    let schedule = range.observation_schedule().clone();
    drop(range);
    assert_eq!(schedule.times(), &[last]);

    let mut accumulation = AccumulatorTerms::new(90.0, 120.0, 1.0, 2.0, start, expiry, clock);
    accumulation.calendar = calendar.clone();
    let accumulator = Accumulator::new(accumulation)?;
    assert_eq!(accumulator.accumulation_schedule()?.times(), &[start, last]);
    assert_eq!(
        accumulator.knock_out_observation_schedule()?,
        accumulator.accumulation_schedule()?
    );
    for explicit in [vec![], vec![Timestamp::new(start.date(), 16, 0, 0, 0)?]] {
        let changed = accumulator.replace(AccumulatorChanges {
            knock_out_observation_times: Some(Some(explicit.clone())),
            ..Default::default()
        })?;
        assert_eq!(changed.knock_out_observation_schedule()?.times(), explicit);
        assert_eq!(changed.accumulation_schedule()?.times(), &[start, last]);
    }

    let observations = vec![AutocallableObservation::snowball(expiry, 110.0, 0.0)?];
    let terms = SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        expiry,
        observations.clone(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        },
        calendar.clone(),
    );
    let note = SnowballOption::new(terms)?;
    assert_eq!(
        note.knock_in_observation_schedule()?
            .map(|s| s.times().to_vec()),
        Some(vec![start, last])
    );
    let expiry_only = note.replace(SnowballOptionChanges {
        knock_in_observation: Some(KnockInObservation::AtExpiry),
        ..Default::default()
    })?;
    assert_eq!(
        expiry_only
            .knock_in_observation_schedule()?
            .map(|s| s.times().to_vec()),
        Some(vec![expiry])
    );
    let phoenix = PhoenixTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        expiry,
        vec![AutocallableObservation::phoenix(expiry, 110.0, 0.0)?],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        },
        calendar.clone(),
    );
    assert_eq!(
        PhoenixOption::new(phoenix)?.knock_in_observation_schedule()?,
        note.knock_in_observation_schedule()?
    );
    let ternary = TernarySnowballTerms::new(
        0.1,
        0.02,
        80.0,
        start,
        expiry,
        observations.clone(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        },
        calendar.clone(),
    );
    assert_eq!(
        TernarySnowballOption::new(ternary)?.knock_in_observation_schedule()?,
        note.knock_in_observation_schedule()?
    );
    let binary = BinarySnowballTerms::new(0.1, start, expiry, observations, calendar);
    assert_eq!(
        BinarySnowballOption::new(binary)?.knock_in_observation_schedule()?,
        None
    );
    Ok(())
}
