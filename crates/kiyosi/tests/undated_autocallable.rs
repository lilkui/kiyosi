use kiyosi::*;

#[test]
fn supplied_autocallable_state_is_undated_and_retained_on_term_edits() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let expiry = start.add_days(4)?;
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        expiry,
        vec![AutocallableObservation::snowball(expiry, 110.0, 0.08)?],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: start.time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::KnockedIn);
    let note = SnowballOption::new(terms)?;
    let edited = note.replace(SnowballOptionChanges {
        knock_in_level: Some(85.0),
        ..Default::default()
    })?;
    assert_eq!(
        edited.barrier_state(),
        Some(AutocallableBarrierState::KnockedIn)
    );
    let engine = QuadratureAutocallableEngine::default();
    for day in [1, 2, 4] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
            90.0,
            start.add_days(day)?,
        )?;
        assert!((engine.price(&edited, &context)? - 0.9).abs() < 1e-14);
    }
    assert!((note.terms().knock_in_level - 80.0).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn valuation_observations_use_current_spot_and_supplied_prior_state() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let event = start.add_days(2)?;
    let expiry = start.add_days(4)?;
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        expiry,
        vec![
            AutocallableObservation::snowball(event, 110.0, 0.08)?,
            AutocallableObservation::snowball(expiry, 110.0, 0.08)?,
        ],
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: start.time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms)?;
    let engine = QuadratureAutocallableEngine::default();
    let context =
        |spot, at| PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, spot, at);
    for at in [event.add_micros(-1)?, event, event.add_micros(1)?, expiry] {
        assert!(
            (engine.price(&note, &context(90.0, at)?)? - (1.0 + 0.06 * 4.0 / 365.0)).abs() < 1e-14
        );
    }
    assert!(
        (engine.price(&note, &context(110.0, event)?)? - (1.0 + 0.08 * 2.0 / 365.0)).abs() < 1e-14
    );
    let settled = note.replace(SnowballOptionChanges {
        barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&settled, &context(90.0, event)?),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        engine.price(&settled, &context(90.0, event.add_micros(1)?)?)?,
        0.0
    );
    let unknown = note.replace(SnowballOptionChanges {
        barrier_state: Some(None),
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&unknown, &context(90.0, event)?),
        Err(Error::MissingHistory(_))
    ));
    let snapshot = note.clone();
    assert!(matches!(
        note.replace(SnowballOptionChanges {
            barrier_state: Some(None),
            lower_strike: Some(101.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(note, snapshot);
    Ok(())
}
