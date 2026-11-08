use kiyosi::*;

#[test]
fn history_free_contracts_reject_timestamps_instead_of_discarding_them() -> Result<()> {
    fn check<I: OptionContract + Clone + PartialEq + std::fmt::Debug>(option: &I) -> Result<()> {
        for time in [option.effective_time(), option.expiry_time().add_days(1)?] {
            assert!(matches!(
                option.clone().with_history_time(Some(time)),
                Err(Error::UnsupportedOperation(_))
            ));
        }
        assert_eq!(option.clone().with_history_time(None)?, *option);
        Ok(())
    }
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(10)?;
    check(&EuropeanOption::new(OptionType::Call, 100.0, start, end)?)?;
    check(&AmericanOption::new(OptionType::Put, 100.0, start, end)?)?;
    check(&DigitalOption::asset_or_nothing(
        OptionType::Call,
        100.0,
        start,
        end,
    )?)?;
    Ok(())
}

#[test]
fn history_refresh_updates_payload_and_timestamp_before_pricing() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = start.add_days(1)?;
    let later = at.add_days(1)?;
    let original = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        start.add_days(8)?,
        120.0,
        BarrierType::UpAndOut,
    ))?
    .replace_history(Some(BarrierTouchState::Untouched), Some(at))?;
    let refreshed = original.replace_history(Some(BarrierTouchState::Touched), Some(later))?;
    assert_eq!(refreshed.touch_state(), Some(BarrierTouchState::Touched));
    assert_eq!(refreshed.history_time(), Some(later));
    assert_eq!(
        AnalyticBarrierEngine.price(&refreshed, &context(later)?)?,
        0.0
    );
    assert_eq!(original.touch_state(), Some(BarrierTouchState::Untouched));
    assert_eq!(original.history_time(), Some(at));
    Ok(())
}

fn context(at: Timestamp) -> Result<PricingContext> {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
        100.0,
        at,
    )
}

fn check<I: OptionContract + Clone>(
    engine: &impl PricingEngine<I>,
    option: I,
    market: &PricingContext,
    clear: impl Fn(&I, Option<Timestamp>) -> Result<I>,
) -> Result<()> {
    assert!(matches!(
        engine.price(&option, market),
        Err(Error::InvalidInput(_))
    ));
    let original = option.clone();
    let dated = option.with_history_time(Some(market.valuation_time()))?;
    assert_eq!(original.history_time(), None);
    assert_eq!(dated.history_time(), Some(market.valuation_time()));
    assert_eq!(clear(&dated, None)?.history_time(), None);
    assert_eq!(dated.history_time(), Some(market.valuation_time()));
    assert_eq!(
        clear(&dated, Some(market.valuation_time()))?.history_time(),
        Some(market.valuation_time())
    );
    assert!(engine.price(&dated, market)?.is_finite());
    let future = dated
        .clone()
        .with_history_time(Some(market.valuation_time().add_days(1)?))?;
    assert!(matches!(
        engine.price(&future, market),
        Err(Error::InvalidInput(_))
    ));
    for invalid in [
        dated.effective_time().add_days(-1)?,
        dated.expiry_time().add_days(1)?,
    ] {
        assert!(matches!(
            clear(&dated, Some(invalid)),
            Err(Error::InvalidInput(_))
        ));
    }
    for boundary in [dated.effective_time(), dated.expiry_time()] {
        assert_eq!(
            dated
                .clone()
                .with_history_time(Some(boundary))?
                .history_time(),
            Some(boundary)
        );
    }
    assert!(matches!(
        engine.price(&dated.with_history_time(None)?, market),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn cleared_average_can_shorten_expiry_before_its_old_snapshot() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = Date::new(2025, 2, 1)?.start_of_day();
    let end = Date::new(2026, 1, 2)?.start_of_day();
    let expiry = Date::new(2025, 1, 15)?.start_of_day();
    let original =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 100.0)?
            .with_history_time(Some(at))?;
    let cleared = original.replace_history(None, None)?;
    assert_eq!(cleared.history_time(), None);
    assert_eq!(cleared.realized_average(), None);
    assert_eq!(
        cleared
            .replace(AveragePriceOptionChanges {
                expiry_time: Some(expiry),
                ..Default::default()
            })?
            .expiry_time(),
        expiry
    );
    let changes = AveragePriceOptionChanges {
        realized_average: Some(None),
        expiry_time: Some(expiry),
        ..Default::default()
    };
    assert_eq!(original.replace(changes)?.history_time(), None);
    assert!(matches!(
        original.replace(AveragePriceOptionChanges {
            history_time: Some(Some(at)),
            ..changes
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original.history_time(), Some(at));
    assert_eq!(original.realized_average(), Some(100.0));
    Ok(())
}

#[test]
fn partial_accumulator_history_clears_retain_the_shared_timestamp() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = start.add_days(4)?;
    let end = start.add_days(8)?;
    let original = Accumulator::new(AccumulatorTerms::new(
        100.0,
        120.0,
        1.0,
        2.0,
        start,
        end,
        TimeOfDay::new(15, 0, 0, 0)?,
    ))?
    .replace_history(
        AccumulatorHistoryChanges {
            accumulated_quantity: Some(Some(0.0)),
            knock_out_settled: Some(Some(false)),
        },
        None,
    )?
    .with_history_time(Some(at))?;
    let quantity = AccumulatorHistoryChanges {
        accumulated_quantity: Some(None),
        ..Default::default()
    };
    let settlement = AccumulatorHistoryChanges {
        knock_out_settled: Some(None),
        ..Default::default()
    };
    for (first, last) in [(quantity, settlement), (settlement, quantity)] {
        let partial = original.replace_history(first, Some(at))?;
        assert_eq!(partial.history_time(), Some(at));
        assert_eq!(partial.replace_history(last, None)?.history_time(), None);
    }
    let explicit = original.replace(AccumulatorChanges {
        accumulated_quantity: Some(None),
        knock_out_settled: Some(None),
        history_time: Some(Some(at)),
        ..Default::default()
    })?;
    assert_eq!(explicit.history_time(), Some(at));
    assert_eq!(
        explicit.replace_history(AccumulatorHistoryChanges::default(), Some(at))?,
        explicit
    );
    assert!(matches!(
        original.replace(AccumulatorChanges {
            accumulated_quantity: Some(None),
            knock_out_settled: Some(None),
            history_time: Some(Some(end.add_days(1)?)),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original.history_time(), Some(at));
    assert_eq!(original.accumulated_quantity(), Some(0.0));
    assert_eq!(original.knock_out_settled(), Some(false));
    Ok(())
}

#[test]
fn dated_history_survives_future_schedule_edits() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let past = start.add_days(1)?;
    let at = start.add_days(4)?;
    let future = start.add_days(6)?;
    let end = start.add_days(8)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_times = vec![past, end];
    terms.touch_state = Some(BarrierTouchState::Untouched);
    let original = BarrierOption::new(terms)?.with_history_time(Some(at))?;
    let updated = original.replace(BarrierOptionChanges {
        observation_times: Some(vec![past, at, future, end]),
        ..Default::default()
    })?;
    assert_eq!(updated.observation_times(), &[past, at, future, end]);
    assert_eq!(updated.touch_state(), original.touch_state());
    assert_eq!(updated.history_time(), Some(at));
    assert_eq!(original.observation_times(), &[past, end]);
    assert!(matches!(
        original.replace(BarrierOptionChanges {
            observation_times: Some(vec![future, end]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        original
            .replace(BarrierOptionChanges {
                expiry_time: Some(end.add_days(3)?),
                ..Default::default()
            })?
            .history_time(),
        Some(at)
    );

    let mut terms = BinarySnowballTerms::new(
        0.1,
        start,
        end,
        vec![
            AutocallableObservation::snowball(past, 110.0, 0.0)?,
            AutocallableObservation::snowball(end, 110.0, 0.0)?,
        ],
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let original = BinarySnowballOption::new(terms)?.with_history_time(Some(at))?;
    let updated = original.replace(BinarySnowballOptionChanges {
        observations: Some(vec![
            original.observations()[0],
            AutocallableObservation::snowball(at, 110.0, 0.0)?,
            AutocallableObservation::snowball(future, 110.0, 0.0)?,
            original.observations()[1],
        ]),
        ..Default::default()
    })?;
    assert_eq!(updated.observations().len(), 4);
    assert_eq!(updated.history_time(), Some(at));
    assert_eq!(updated.barrier_state(), original.barrier_state());
    assert!(matches!(
        original.replace(BinarySnowballOptionChanges {
            observations: Some(vec![original.observations()[1]]),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn dated_daily_history_survives_future_calendar_edits() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = start.add_days(4)?;
    let end = start.add_days(8)?;
    let future = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![start.date().add_days(6)?],
        vec![],
        None,
    )?;
    let past = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![start.date().add_days(1)?],
        vec![],
        None,
    )?;
    let mut range = RangeAccrualTerms::new(
        100.0,
        0.1,
        90.0,
        110.0,
        start,
        end,
        TimeOfDay::new(15, 0, 0, 0)?,
    );
    range.realized_hit_count = Some(1);
    let range = RangeAccrualOption::new(range)?.with_history_time(Some(at))?;
    assert_eq!(
        range.replace_calendar(future.clone())?.realized_hit_count(),
        Some(1)
    );
    assert!(matches!(
        range.replace_calendar(past.clone()),
        Err(Error::InvalidInput(_))
    ));
    let mut terms = AccumulatorTerms::new(
        100.0,
        120.0,
        1.0,
        2.0,
        start,
        end,
        TimeOfDay::new(15, 0, 0, 0)?,
    );
    terms.accumulated_quantity = Some(2.0);
    terms.knock_out_settled = Some(false);
    let accumulator = Accumulator::new(terms)?.with_history_time(Some(at))?;
    assert_eq!(
        accumulator.replace_calendar(future)?.accumulated_quantity(),
        Some(2.0)
    );
    assert!(matches!(
        accumulator.replace_calendar(past),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn history_time_and_contract_life_are_replaced_together() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(8)?;
    let at = start.add_days(4)?;
    let option = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))?
    .replace_history(Some(BarrierTouchState::Untouched), None)
    .unwrap()
    .with_history_time(Some(at))?;
    let expiry = start.add_days(1)?;
    let changes = BarrierOptionChanges {
        expiry_time: Some(expiry),
        touch_state: Some(Some(BarrierTouchState::Untouched)),
        ..Default::default()
    };
    assert!(matches!(
        option.replace(changes.clone()),
        Err(Error::InvalidInput(_))
    ));
    let replaced = option.replace(BarrierOptionChanges {
        history_time: Some(Some(expiry)),
        ..changes
    })?;
    assert_eq!(replaced.history_time(), Some(expiry));
    assert_eq!(replaced.expiry_time(), expiry);
    assert_eq!(option.history_time(), Some(at));
    assert_eq!(option.expiry_time(), end);
    Ok(())
}

#[test]
fn every_path_dependent_family_requires_dated_history() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = Date::new(2025, 1, 10)?.at_time(TimeOfDay::new(16, 0, 0, 0)?);
    let at = Date::new(2025, 1, 6)?.at_time(TimeOfDay::new(12, 0, 0, 0)?);
    let market = context(at)?;
    let clock = TimeOfDay::new(15, 0, 0, 0)?;
    let mut barrier = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    barrier.touch_state = Some(BarrierTouchState::Untouched);
    check(
        &AnalyticBarrierEngine,
        BarrierOption::new(barrier)?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))?
    .replace_history(Some(BarrierTouchState::Untouched), None)
    .unwrap();
    check(
        &AnalyticDoubleBarrierEngine,
        double,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    let binary = BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    check(
        &AnalyticBinaryBarrierEngine,
        BinaryBarrierOption::cash(binary, 10.0)?
            .replace_history(Some(BarrierTouchState::Untouched), None)
            .unwrap(),
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    let touch = TouchOptionTerms::new(start, end, 120.0, BarrierType::UpAndIn);
    check(
        &AnalyticBinaryBarrierEngine,
        TouchOption::cash(touch, 10.0)?
            .replace_history(Some(BarrierTouchState::Untouched), None)
            .unwrap(),
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    let touch = DoubleTouchOptionTerms::new(start, end, 80.0, 120.0, DoubleBarrierType::KnockIn);
    check(
        &AnalyticDoubleTouchEngine,
        DoubleTouchOption::cash(touch, 10.0)?
            .replace_history(Some(BarrierTouchState::Untouched), None)
            .unwrap(),
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    check(
        &AnalyticGeometricAveragePriceEngine,
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, Some(100.0))?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    check(
        &TurnbullWakemanArithmeticAveragePriceEngine,
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, Some(100.0))?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    let mut range = RangeAccrualTerms::new(1000.0, 0.12, 90.0, 110.0, start, end, clock);
    range.realized_hit_count = Some(1);
    check(
        &AnalyticRangeAccrualEngine,
        RangeAccrualOption::new(range)?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    let mut accumulator = AccumulatorTerms::new(100.0, 120.0, 1.0, 2.0, start, end, clock);
    accumulator.accumulated_quantity = Some(3.0);
    accumulator.knock_out_settled = Some(false);
    check(
        &QuadratureAccumulatorEngine::default(),
        Accumulator::new(accumulator)?,
        &market,
        |option, at| {
            option.replace_history(
                AccumulatorHistoryChanges {
                    accumulated_quantity: Some(None),
                    knock_out_settled: Some(None),
                },
                at,
            )
        },
    )?;
    let records = vec![AutocallableObservation::snowball(end, 110.0, 0.0)?];
    let state = Some(AutocallableBarrierState::NoEvent);
    check(
        &QuadratureAutocallableEngine::default(),
        SnowballOption::new(SnowballTerms::new(
            0.1,
            100.0,
            80.0,
            100.0,
            0.0,
            start,
            end,
            records.clone(),
            Some(clock),
        ))?
        .replace_history(state, None)?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    check(
        &QuadratureAutocallableEngine::default(),
        PhoenixOption::new(PhoenixTerms::new(
            kiyosi::PhoenixCoupon::Annual(0.1),
            100.0,
            80.0,
            100.0,
            0.0,
            start,
            end,
            vec![AutocallableObservation::phoenix(end, 110.0, 0.0)?],
            Some(clock),
        ))?
        .replace_history(state, None)?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    check(
        &QuadratureAutocallableEngine::default(),
        BinarySnowballOption::new(BinarySnowballTerms::new(0.1, start, end, records.clone()))?
            .replace_history(state, None)?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    check(
        &QuadratureAutocallableEngine::default(),
        TernarySnowballOption::new(TernarySnowballTerms::new(
            0.1,
            0.02,
            80.0,
            start,
            end,
            records,
            Some(clock),
        ))?
        .replace_history(state, None)?,
        &market,
        |option, at| option.replace_history(None, at),
    )?;
    Ok(())
}

#[test]
fn discrete_history_reuses_between_events_and_requires_refresh_after_them() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = Date::new(2025, 1, 10)?.start_of_day();
    let at = Date::new(2025, 1, 6)?.at_time(TimeOfDay::new(12, 0, 0, 0)?);
    let event = at.date().at_time(TimeOfDay::new(15, 0, 0, 0)?);
    let mut terms =
        RangeAccrualTerms::new(100.0, 0.1, 90.0, 110.0, start, end, event.time_of_day());
    terms.realized_hit_count = Some(1);
    let option = RangeAccrualOption::new(terms)?.with_history_time(Some(at))?;
    let market = PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, 100.0, at)?;
    let engine = AnalyticRangeAccrualEngine;
    let price = engine.price(&option, &market)?;
    for time in [
        at,
        event.date().at_time(TimeOfDay::new(14, 59, 59, 999_999)?),
        event,
    ] {
        let rolled = market.replace(PricingContextChanges {
            valuation_time: Some(time),
            ..Default::default()
        })?;
        assert_eq!(engine.price(&option, &rolled)?, price);
    }
    let rolled = market.replace(PricingContextChanges {
        valuation_time: Some(event.date().at_time(TimeOfDay::new(15, 0, 0, 1)?)),
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &rolled),
        Err(Error::InvalidInput(_))
    ));
    let refreshed = option
        .replace_history(Some(2), None)?
        .with_history_time(Some(rolled.valuation_time()))?;
    assert_eq!(engine.price(&refreshed, &rolled)?, price);
    assert_eq!(option.history_time(), Some(at));
    let backwards = market.replace(PricingContextChanges {
        valuation_time: Some(start),
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &backwards),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn continuous_history_rejects_reuse_but_numerical_time_risk_keeps_it_frozen() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = start.add_days(4)?;
    let end = start.add_days(30)?;
    let market = context(at)?;
    let option =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, Some(100.0))?
            .with_history_time(Some(at))?;
    let engine = AnalyticGeometricAveragePriceEngine;
    let result = engine.price_with_greeks(&option, &market, GreeksRequest::new([Greek::Theta]))?;
    assert!(result.require(Greek::Theta)?.is_finite());
    let rolled = market.replace(PricingContextChanges {
        valuation_time: Some(at.date().at_time(TimeOfDay::new(0, 0, 0, 1)?)),
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &rolled),
        Err(Error::InvalidInput(_))
    ));
    assert!(
        engine
            .price(
                &option
                    .clone()
                    .with_history_time(Some(rolled.valuation_time()))?,
                &rolled
            )?
            .is_finite()
    );
    assert_eq!(option.history_time(), Some(at));
    Ok(())
}

#[test]
fn settled_history_must_already_be_possible_at_its_own_timestamp() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = start.add_days(4)?;
    let end = start.add_days(8)?;
    let market = context(at)?;
    let mut terms = AccumulatorTerms::new(
        100.0,
        120.0,
        1.0,
        2.0,
        start,
        end,
        TimeOfDay::new(15, 0, 0, 0)?,
    );
    terms.knock_out_settled = Some(true);
    let settled = Accumulator::new(terms)?.with_history_time(Some(start))?;
    let engine = QuadratureAccumulatorEngine::default();
    assert!(matches!(
        engine.price(
            &settled.clone().with_history_time(Some(at.add_days(1)?))?,
            &market
        ),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        engine.price(&settled, &market),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        engine.price(&settled.with_history_time(Some(at))?, &market)?,
        0.0
    );
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.1,
        start,
        end,
        vec![AutocallableObservation::snowball(
            start.add_days(1)?,
            110.0,
            0.0,
        )?],
    ))?
    .replace_history(Some(AutocallableBarrierState::KnockedOut), None)?
    .with_history_time(Some(start))?;
    let engine = QuadratureAutocallableEngine::default();
    assert!(matches!(
        engine.price(&note, &market),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        engine.price(&note.with_history_time(Some(at))?, &market)?,
        0.0
    );
    Ok(())
}
