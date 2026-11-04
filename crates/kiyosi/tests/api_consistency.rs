use kiyosi::*;

#[test]
fn terminated_accumulators_have_no_remaining_cashflows() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2025, 1, 10)?.start_of_day();
    let option = Accumulator::new(AccumulatorTerms::new(90.0, 110.0, 1.0, 2.0, start, end))?;
    let settled = option.replace(AccumulatorChanges {
        terminated: Some(true),
        ..Default::default()
    })?;
    assert!(!option.terminated());
    assert!(settled.replace_history(3.0)?.terminated());
    assert!(
        settled
            .replace(AccumulatorChanges {
                strike: Some(95.0),
                ..Default::default()
            })?
            .terminated()
    );
    let context = PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        Date::new(2025, 1, 3)?.start_of_day(),
        TradingCalendar::all_days(),
    )?;
    let engines: Vec<Box<dyn PricingEngine<Accumulator>>> = vec![
        Box::new(QuadratureAccumulatorEngine::default()),
        Box::new(FiniteDifferenceAccumulatorEngine::default()),
        Box::new(MonteCarloAccumulatorEngine::default()),
    ];
    for engine in engines {
        assert_eq!(engine.price(&settled, &context)?, 0.0);
        assert_eq!(engine.price(&settled.replace_history(3.0)?, &context)?, 0.0);
        let active = settled.replace(AccumulatorChanges {
            terminated: Some(false),
            accumulated_quantity: Some(Some(0.0)),
            ..Default::default()
        })?;
        assert!(engine.price(&active, &context)? > 0.0);
        let unknown = settled.replace(AccumulatorChanges {
            terminated: Some(false),
            ..Default::default()
        })?;
        assert!(matches!(
            engine.price(&unknown, &context),
            Err(Error::InvalidInput(_))
        ));
        let result = engine.price_with_greeks(
            &settled,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )?;
        for greek in Greek::ALL {
            assert_eq!(result.require(greek)?, 0.0);
        }
        assert!(matches!(
            implied_volatility(engine.as_ref(), &settled, &context, 0.0, Default::default()),
            Err(Error::UnsupportedOperation(_))
        ));
    }
    Ok(())
}
