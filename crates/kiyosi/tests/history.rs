use kiyosi::*;

#[test]
fn autocallable_history_respects_the_first_daily_observation_on_sparse_calendars() -> Result<()> {
    let date = Date::new(2025, 1, 2)?;
    let start = date.at_time(TimeOfDay::new(13, 0, 0, 0)?);
    let first = date.add_days(4)?.at_time(TimeOfDay::new(12, 0, 0, 0)?);
    let end = date.add_days(4)?.at_time(TimeOfDay::new(16, 0, 0, 0)?);
    let terms = SnowballTerms::new(
        0.0,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        vec![AutocallableObservation::snowball(end, 110.0, 0.0)?],
        Some(TimeOfDay::new(12, 0, 0, 0)?),
        TradingCalendar::custom(
            TradingCalendar::all_days(),
            vec![date.add_days(1)?, date.add_days(2)?, date.add_days(3)?],
            vec![],
            None,
        )?,
    );
    let note = SnowballOption::new(terms)?;
    let engine = QuadratureAutocallableEngine::default();
    for at in [start, first.add_micros(-1)?, first, first.add_micros(1)?] {
        let market =
            PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, 100.0, at)?;
        if at <= first {
            assert_eq!(engine.price(&note, &market)?, 1.0);
        } else {
            assert!(matches!(
                engine.price(&note, &market),
                Err(Error::MissingHistory(_))
            ));
        }
        let knocked_in = note.replace(SnowballOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedIn)),
            ..Default::default()
        })?;
        if at <= first {
            assert!(matches!(
                engine.price(&knocked_in, &market),
                Err(Error::InvalidInput(_))
            ));
        } else {
            assert_eq!(engine.price(&knocked_in, &market)?, 1.0);
        }
        let known = note.replace(SnowballOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
            ..Default::default()
        })?;
        assert_eq!(engine.price(&known, &market)?, 1.0);
    }
    Ok(())
}

#[test]
fn autocallable_terms_roundtrip_preserves_history() -> Result<()> {
    macro_rules! check {
        ($terms:expr) => {{
            let mut terms = $terms;
            terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
            let note = AutocallableNote::new(terms)?;
            let restored = AutocallableNote::new(note.terms().clone())?;
            assert_eq!(restored, note);
            assert_eq!(
                restored.barrier_state(),
                Some(AutocallableBarrierState::NoEvent)
            );
        }};
    }
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(8)?;
    let clock = Some(TimeOfDay::new(15, 0, 0, 0)?);
    let records = vec![AutocallableObservation::snowball(end, 110.0, 0.0)?];
    check!(SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        records.clone(),
        clock,
        TradingCalendar::weekdays(),
    ));
    check!(PhoenixTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        vec![AutocallableObservation::phoenix(end, 110.0, 0.0)?],
        clock,
        TradingCalendar::weekdays(),
    ));
    check!(BinarySnowballTerms::new(
        0.1,
        start,
        end,
        records.clone(),
        TradingCalendar::weekdays(),
    ));
    check!(TernarySnowballTerms::new(
        0.1,
        0.02,
        80.0,
        start,
        end,
        records,
        clock,
        TradingCalendar::weekdays(),
    ));
    Ok(())
}

fn context(at: Timestamp) -> Result<PricingContext> {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
        100.0,
        at,
    )
}

#[test]
fn supplied_history_survives_schedule_edits() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let past = start.add_days(1)?;
    let at = start.add_days(4)?;
    let future = start.add_days(6)?;
    let end = start.add_days(8)?;
    let mut terms = BinarySnowballTerms::new(
        0.1,
        start,
        end,
        vec![
            AutocallableObservation::snowball(past, 110.0, 0.0)?,
            AutocallableObservation::snowball(end, 110.0, 0.0)?,
        ],
        TradingCalendar::weekdays(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let original = BinarySnowballOption::new(terms)?;
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
    assert_eq!(updated.barrier_state(), original.barrier_state());
    let changed = original.replace(BinarySnowballOptionChanges {
        observations: Some(vec![original.observations()[1]]),
        ..Default::default()
    })?;
    assert_eq!(changed.barrier_state(), original.barrier_state());
    Ok(())
}

#[test]
fn settled_history_must_be_possible_at_valuation() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let at = start.add_days(4)?;
    let end = start.add_days(8)?;
    let market = context(at)?;
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.1,
        start,
        end,
        vec![AutocallableObservation::snowball(
            start.add_days(1)?,
            110.0,
            0.0,
        )?],
        TradingCalendar::weekdays(),
    ))?;
    let engine = QuadratureAutocallableEngine::default();
    let settled = note.replace(BinarySnowballOptionChanges {
        barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&settled, &context(start)?),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(engine.price(&settled, &market)?, 0.0);
    Ok(())
}

#[test]
fn undated_daily_history_survives_calendar_edits() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
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

    let range = RangeAccrualOption::new(range)?;
    assert_eq!(
        range.replace_calendar(future.clone())?.realized_hit_count(),
        Some(1)
    );
    assert_eq!(
        range.replace_calendar(past.clone())?.realized_hit_count(),
        Some(1)
    );
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

    let accumulator = Accumulator::new(terms)?;
    assert_eq!(
        accumulator.replace_calendar(future)?.accumulated_quantity(),
        Some(2.0)
    );
    assert_eq!(
        accumulator.replace_calendar(past)?.accumulated_quantity(),
        Some(2.0)
    );
    Ok(())
}
