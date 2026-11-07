mod support;
use support::dated;

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
        assert_eq!(range.observation_schedule()?.times(), &[observation]);

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
            Some(observation.time_of_day()),
        );
        terms.calendar = calendar.clone();
        terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
        let note = SnowballOption::new(terms.clone())?;
        assert_eq!(
            note.knock_in_observation_schedule()?.unwrap().times(),
            &[observation]
        );
        terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
        terms.observation_clock_time = None;
        assert!(matches!(
            SnowballOption::new(terms),
            Err(Error::InvalidInput(_))
        ));

        let mut terms = BinarySnowballTerms::new(0.12, start, expiry, records);
        terms.calendar = calendar.clone();
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
                    (AnalyticRangeAccrualEngine.price(&dated(&range, &context), &context)?
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
                                (engine.price(&dated($option, &context), &context)?
                                    - (1.0 + coupon) * discount)
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
                        assert_eq!(
                            engine.price(&dated(&accumulator, &context), &context)?,
                            -60.0
                        );
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
    let parity = AnalyticEuropeanEngine
        .price(&dated(&call, &context), &context)
        .unwrap()
        - AnalyticEuropeanEngine
            .price(&dated(&put, &context), &context)
            .unwrap();
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
            .price(
                &dated(&call, &market(expiry, 110.0)),
                &market(expiry, 110.0)
            )
            .unwrap(),
        10.0
    );
    for at in [time(6, 15, 654_322), time(6, 15, 654_323)] {
        assert!(matches!(
            AnalyticEuropeanEngine.price(&dated(&call, &market(at, 110.0)), &market(at, 110.0)),
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
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_times = vec![time(6, 10, 0), time(6, 15, 0)];
    terms.rebate = 7.0;
    terms.rebate_timing = RebateTiming::AtHit;
    let option = BarrierOption::new(terms).unwrap();
    assert_eq!(
        AnalyticBarrierEngine
            .price(
                &dated(&option, &market(time(6, 10, 0), 110.0)),
                &market(time(6, 10, 0), 110.0)
            )
            .unwrap(),
        7.0
    );
    assert!(matches!(
        AnalyticBarrierEngine.price(
            &dated(&option, &market(time(6, 10, 1), 100.0)),
            &market(time(6, 10, 1), 100.0)
        ),
        Err(Error::InvalidInput(_))
    ));
    let touched = option.replace_history(Some(BarrierTouchState::Touched));
    assert!(matches!(
        AnalyticBarrierEngine.price(
            &dated(&touched, &market(time(6, 9, 0), 110.0)),
            &market(time(6, 9, 0), 110.0)
        ),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        AnalyticBarrierEngine
            .price(
                &dated(&touched, &market(time(6, 10, 1), 100.0)),
                &market(time(6, 10, 1), 100.0)
            )
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
            .price(
                &dated(&option, &market(time(6, 15, 0), 100.0)),
                &market(time(6, 15, 0), 100.0)
            )
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
            .price(
                &dated(&option, &market(time(6, 15, 1), 100.0)),
                &market(time(6, 15, 1), 100.0)
            )
            .unwrap()
            - 120.0 / 365.0)
            .abs()
            < 1e-14
    );
    assert!(matches!(
        AnalyticRangeAccrualEngine.price(
            &dated(
                &option.replace_history(Some(1)).unwrap(),
                &market(time(6, 15, 1), 100.0)
            ),
            &market(time(6, 15, 1), 100.0)
        ),
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
                .price(
                    &dated(&option, &market(time(6, 10, 0), 105.0)),
                    &market(time(6, 10, 0), 105.0)
                )
                .unwrap()
                - 10.0)
                .abs()
                < 1e-4
        );
        assert!(matches!(
            engine.price(
                &dated(&option, &market(time(6, 15, 1), 105.0)),
                &market(time(6, 15, 1), 105.0)
            ),
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
                .price(
                    &dated(&single, &market(time(7, 15, 0), 105.0)),
                    &market(time(7, 15, 0), 105.0)
                )
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
        Some((time(7, 15, 0)).time_of_day()),
    ))
    .unwrap();
    assert!(
        (QuadratureAutocallableEngine::default()
            .price(
                &dated(&note, &market(time(6, 10, 0), 100.0)),
                &market(time(6, 10, 0), 100.0)
            )
            .unwrap()
            - (1.0 + 0.06 * 30.0 / (365.0 * 24.0)))
            .abs()
            < 1e-12
    );
    assert!(matches!(
        QuadratureAutocallableEngine::default().price(
            &dated(&note, &market(time(6, 15, 1), 100.0)),
            &market(time(6, 15, 1), 100.0)
        ),
        Err(Error::InvalidInput(_))
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
        let before = engine.price(
            &dated(&option, &market(time(6, 15, 123_455), 105.0)),
            &market(time(6, 15, 123_455), 105.0),
        )?;
        assert!((before - 5.0).abs() < 1e-4, "before={before}");
        let current = engine.price(
            &dated(&option, &market(time(6, 15, 123_456), 105.0)),
            &market(time(6, 15, 123_456), 105.0),
        )?;
        assert!((current - 5.0).abs() < 1e-4);
        assert_eq!(
            engine.price(
                &dated(&option, &market(time(6, 15, 123_456), 115.0)),
                &market(time(6, 15, 123_456), 115.0)
            )?,
            0.0
        );
        assert!(matches!(
            engine.price(
                &dated(&option, &market(time(6, 15, 123_457), 105.0)),
                &market(time(6, 15, 123_457), 105.0)
            ),
            Err(Error::InvalidInput(_))
        ));
        let seasoned = option.replace_history(AccumulatorHistoryChanges {
            accumulated_quantity: Some(Some(1.0)),
            knock_out_settled: Some(Some(false)),
        })?;
        assert_eq!(
            engine.price(
                &dated(&seasoned, &market(time(6, 18, 0), 105.0)),
                &market(time(6, 18, 0), 105.0)
            )?,
            5.0
        );
        assert_eq!(
            engine.price(
                &dated(&seasoned, &market(time(6, 18, 0), 115.0)),
                &market(time(6, 18, 0), 115.0)
            )?,
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
        assert_eq!(
            engine.price(
                &dated(&option, &market(start, 105.0)),
                &market(start, 105.0)
            )?,
            0.0
        );
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
        Some(clock),
    ))?;
    assert_eq!(
        QuadratureAutocallableEngine::default()
            .price(&dated(&note, &market(start, 70.0)), &market(start, 70.0))?,
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
        let seasoned = option.replace_history(hits)?;
        let actual = AnalyticRangeAccrualEngine.price(&dated(&seasoned, &context), &context)?;
        assert!(
            (actual - expected).abs() < 1e-14,
            "hour={hour}: {actual} != {expected}"
        );
        assert!(matches!(
            implied_volatility(
                &AnalyticRangeAccrualEngine,
                &dated(&seasoned, &context),
                &context,
                actual,
                Default::default(),
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
        Some(clock),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let daily = SnowballOption::new(terms)?;
    let at_expiry = daily.replace(SnowballOptionChanges {
        barrier_state: Some(daily.barrier_state()),
        knock_in_observation_mode: Some(KnockInObservationMode::AtExpiry),
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
                let actual = engine.price(&dated(&daily, &context), &context)?;
                assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
                assert!(matches!(
                    implied_volatility(
                        engine,
                        &dated(&daily, &context),
                        &context,
                        actual,
                        Default::default()
                    ),
                    Err(Error::UnsupportedOperation(_))
                ));
                let observed = engine.price(&dated(&at_expiry, &context), &context)?;
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
        let actual = engine.price(
            &dated(&daily, &before_final_observation),
            &before_final_observation,
        )?;
        assert!(
            (actual - expected_discounted).abs() < 1e-5,
            "daily: {actual} != {expected_discounted}"
        );
        let observed = engine.price(
            &dated(&at_expiry, &before_final_observation),
            &before_final_observation,
        )?;
        assert!((observed - 1.0).abs() < 1e-5, "at_expiry: {observed} != 1");
    }
    let context = market(time(6, 18, 0), 80.0);
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for result in [
        quad.price_with_greeks(&dated(&daily, &context), &context, request)?,
        fd.price_with_greeks(&dated(&daily, &context), &context, request)?,
        mc.price_with_greeks(&dated(&daily, &context), &context, request)?,
    ] {
        assert_eq!(result.require(Greek::Delta)?, 0.0);
        assert_eq!(result.require(Greek::Gamma)?, 0.0);
    }
    assert_eq!(
        quad.price_with_greeks(&dated(&at_expiry, &context), &context, request)?
            .get(Greek::Delta),
        None
    );
    Ok(())
}

#[test]
fn knock_in_mode_replacement_requires_clock_and_preserves_original_on_failure() -> Result<()> {
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
        Some(clock),
    ))?;
    let inactive = original.replace(SnowballOptionChanges {
        knock_in_observation_mode: Some(KnockInObservationMode::AtExpiry),
        ..Default::default()
    })?;
    assert_eq!(inactive.observation_clock_time(), None);
    let mut missing = original.terms().clone();
    missing.observation_clock_time = None;
    assert!(matches!(
        SnowballOption::new(missing),
        Err(Error::InvalidInput(_))
    ));
    let mut redundant = inactive.terms().clone();
    redundant.observation_clock_time = Some(clock);
    assert!(matches!(
        SnowballOption::new(redundant),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        inactive.replace(SnowballOptionChanges {
            knock_in_observation_mode: Some(KnockInObservationMode::EveryTradingDay),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        original.replace(SnowballOptionChanges {
            knock_in_observation_mode: Some(KnockInObservationMode::AtExpiry),
            observation_clock_time: Some(Some(clock)),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    let active = inactive.replace(SnowballOptionChanges {
        knock_in_observation_mode: Some(KnockInObservationMode::EveryTradingDay),
        observation_clock_time: Some(Some(clock)),
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
                .price(
                    &dated(&option, &market(time(6, 9, 0), 105.0)),
                    &market(time(6, 9, 0), 105.0),
                )
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
        Some((expiry).time_of_day()),
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
        let actual = engine.price(&dated(&note, &context), &context).unwrap();
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
        Some((expiry).time_of_day()),
    );
    terms.observations[0] = terms.observations[0]
        .replace(AutocallableObservationChanges {
            coupon_rate: Some(0.12),
            ..Default::default()
        })
        .unwrap();
    let note = SnowballOption::new(terms).unwrap();
    let context = market(start, 120.0);
    let expected = 1.0 + 0.12 / (365.0 * 24.0);
    for engine in [&mc as &dyn PricingEngine<SnowballOption>, &fd, &quad] {
        assert!((engine.price(&dated(&note, &context), &context).unwrap() - expected).abs() < 1e-6);
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
        .price_with_greeks(
            &dated(&option, &context),
            &context,
            GreeksRequest::new([Greek::Vega]),
        )
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
        observation_clock_time: None,
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
                AutocallableObservation::phoenix(first, 1000.0, 0.0).unwrap(),
                AutocallableObservation::phoenix(last, 1000.0, 0.0).unwrap(),
            ],
            Some((last).time_of_day()),
        )
    })
    .unwrap();
    let context = market(start, 100.0);
    assert!(matches!(
        FiniteDifferenceAutocallableEngine::default().price(&dated(&note, &context), &context),
        Err(Error::ComputationFailure(_))
    ));
    assert!(
        (QuadratureAutocallableEngine::default()
            .price(&dated(&note, &context), &context)
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
        .price_with_greeks(&dated(&option, &context), &context, request)?
        .require(Greek::Theta)?;
    let mut errors = Vec::new();
    for seconds in [60, 1] {
        let value = numerical_greeks(
            &AnalyticEuropeanEngine,
            &dated(&option, &context),
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
    let schedule = range.observation_schedule()?;
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
    let mut terms = SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        expiry,
        observations.clone(),
        Some(clock),
    );
    terms.calendar = calendar.clone();
    let note = SnowballOption::new(terms)?;
    assert_eq!(
        note.knock_in_observation_schedule()?
            .map(|s| s.times().to_vec()),
        Some(vec![start, last])
    );
    let expiry_only = note.replace(SnowballOptionChanges {
        knock_in_observation_mode: Some(KnockInObservationMode::AtExpiry),
        ..Default::default()
    })?;
    assert_eq!(
        expiry_only
            .knock_in_observation_schedule()?
            .map(|s| s.times().to_vec()),
        Some(vec![expiry])
    );
    let mut phoenix = PhoenixTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        expiry,
        vec![AutocallableObservation::phoenix(expiry, 110.0, 0.0)?],
        Some(clock),
    );
    phoenix.calendar = calendar.clone();
    assert_eq!(
        PhoenixOption::new(phoenix)?.knock_in_observation_schedule()?,
        note.knock_in_observation_schedule()?
    );
    let mut ternary = TernarySnowballTerms::new(
        0.1,
        0.02,
        80.0,
        start,
        expiry,
        observations.clone(),
        Some(clock),
    );
    ternary.calendar = calendar.clone();
    assert_eq!(
        TernarySnowballOption::new(ternary)?.knock_in_observation_schedule()?,
        note.knock_in_observation_schedule()?
    );
    let mut binary = BinarySnowballTerms::new(0.1, start, expiry, observations);
    binary.calendar = calendar;
    assert_eq!(
        BinarySnowballOption::new(binary)?.knock_in_observation_schedule()?,
        None
    );
    Ok(())
}
