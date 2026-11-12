use kiyosi::*;

#[test]
fn supplied_accrual_history_is_validated_at_pricing_and_replaced_partially() -> Result<()> {
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
    terms.knock_out_settled = Some(false);
    let active = Accumulator::new(terms)?;
    let settled = active.replace(AccumulatorChanges {
        knock_out_settled: Some(Some(true)),
        ..Default::default()
    })?;
    assert_eq!(settled.accumulated_quantity(), Some(3.0));
    assert_eq!(active.knock_out_settled(), Some(false));
    let changed = active.replace(AccumulatorChanges {
        accumulated_quantity: Some(Some(4.0)),
        daily_quantity: Some(2.0),
        ..Default::default()
    })?;
    assert_eq!(changed.knock_out_settled(), Some(false));
    assert_eq!(changed.accumulated_quantity(), Some(4.0));
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
        105.0,
        start,
    )?;
    assert!(matches!(
        QuadratureAccumulatorEngine::default().price(&active, &context),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        QuadratureAccumulatorEngine::default().price(&settled, &context),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        QuadratureAccumulatorEngine::default().price(
            &settled,
            &context.replace(PricingContextChanges {
                valuation_time: Some(start.add_days(1)?),
                ..Default::default()
            })?
        )?,
        0.0
    );
    Ok(())
}

#[test]
fn range_accrual_uses_caller_hits_across_the_observation_boundary() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(2)?;
    let option = RangeAccrualOption::new(RangeAccrualTerms::new(
        365.0,
        1.0,
        90.0,
        110.0,
        start,
        end,
        start.time_of_day(),
    ))?;
    let event = start.add_days(1)?;
    let market = |time| {
        PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
            100.0,
            time,
        )
    };
    let engine = AnalyticRangeAccrualEngine;
    assert!((engine.price(&option, &market(event.add_micros(-1)?)?)? - 2.0).abs() < 1e-12);
    assert!((engine.price(&option, &market(event)?)? - 2.0).abs() < 1e-12);
    let known = option.replace(RangeAccrualOptionChanges {
        realized_hit_count: Some(Some(1)),
        ..Default::default()
    })?;
    assert!((engine.price(&known, &market(event.add_micros(1)?)?)? - 2.0).abs() < 1e-12);
    let zero = known.replace(RangeAccrualOptionChanges {
        realized_hit_count: Some(Some(0)),
        ..Default::default()
    })?;
    assert!((engine.price(&zero, &market(event.add_micros(1)?)?)? - 1.0).abs() < 1e-12);
    assert!(matches!(
        engine.price(&known, &market(event)?),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        engine.price(&option, &market(event.add_micros(1)?)?),
        Err(Error::MissingHistory(_))
    ));
    assert_eq!(known.realized_hit_count(), Some(1));
    assert!(
        known
            .replace(RangeAccrualOptionChanges {
                realized_hit_count: Some(Some(-1)),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(known.realized_hit_count(), Some(1));
    assert_eq!(
        known.replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(None),
            ..Default::default()
        })?,
        option
    );
    Ok(())
}

#[test]
fn accumulator_counts_current_observation_once_and_uses_supplied_quantity() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let event = start.add_days(1)?;
    let original = Accumulator::new(AccumulatorTerms::new(
        100.0,
        120.0,
        1.0,
        2.0,
        start,
        start.add_days(2)?,
        start.time_of_day(),
    ))?
    .replace(AccumulatorChanges {
        accumulated_quantity: Some(Some(1.0)),
        knock_out_settled: Some(Some(false)),
        ..Default::default()
    })?;
    let context =
        |at| PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, 105.0, at);
    for engine in [
        &QuadratureAccumulatorEngine::default() as &dyn PricingEngine<Accumulator>,
        &FiniteDifferenceAccumulatorEngine::default(),
        &MonteCarloAccumulatorEngine::new(MonteCarloSettings {
            path_count: 32,
            seed: Some(42),
            ..Default::default()
        })?,
    ] {
        for at in [event.add_micros(-1)?, event] {
            assert!((engine.price(&original, &context(at)?)? - 15.0).abs() < 1e-12);
        }
        let updated = original.replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(2.0)),
            ..Default::default()
        })?;
        let after = context(event.add_micros(1)?)?;
        assert!((engine.price(&updated, &after)? - 15.0).abs() < 1e-12);
        assert!((engine.price(&original, &after)? - 10.0).abs() < 1e-12);
    }
    Ok(())
}
