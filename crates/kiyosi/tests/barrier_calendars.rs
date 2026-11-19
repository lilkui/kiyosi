use kiyosi::*;

#[test]
fn deterministic_barrier_uses_current_and_future_fixings_after_dense_history() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let valuation = start.add_days(1)?;
    let first = valuation.add_days(30)?;
    let expiry = valuation.add_days(60)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        90.0,
        start,
        expiry,
        100.0,
        BarrierType::UpAndOut,
    );
    terms.rebate = 2.0;
    terms.rebate_timing = RebateTiming::AtHit;
    terms.touch_state = Some(BarrierTouchState::Untouched);
    terms.calendar = TradingCalendar::all_days();
    terms.observation_times = Some(
        (0..4096)
            .map(|offset| start.add_micros(offset * 1_000_000))
            .chain([Ok(valuation), Ok(first), Ok(expiry)])
            .collect::<Result<Vec<_>>>()?,
    );
    let option = BarrierOption::new(terms)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.2, 0.0, 0.0)?,
        99.0,
        valuation,
    )?;
    let expected = 2.0 * (-0.2 * valuation.years_until(first)).exp();
    assert!((AnalyticBarrierEngine.price(&option, &context)? - expected).abs() < 1e-14);
    let saved = FiniteDifferenceBarrierEngine::default().solve(&option, &context, 98.0..=101.0)?;
    assert!((saved.price(99.0)? - expected).abs() < 1e-14);
    assert_eq!(saved.price(100.0)?, 2.0);
    Ok(())
}

#[test]
fn barrier_calendars_are_owned_and_preserved_by_replacement() -> Result<()> {
    let start = Date::new(2025, 1, 3)?.start_of_day();
    let expiry = start.add_days(2)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.02, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    let observations = vec![start.add_days(1)?, expiry];
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_times = Some(observations.clone());
    assert!(matches!(
        BarrierOption::new(terms.clone()),
        Err(Error::InvalidInput(_))
    ));
    terms.calendar = TradingCalendar::all_days();
    let option = BarrierOption::new(terms)?;
    let owned = option.replace(BarrierOptionChanges {
        calendar: Some(TradingCalendar::all_days()),
        ..Default::default()
    })?;
    for engine in [
        &AnalyticBarrierEngine as &dyn PricingEngine<BarrierOption>,
        &CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 2 })?,
        &QuadratureBarrierEngine::default(),
        &FiniteDifferenceBarrierEngine::default(),
        &MonteCarloBarrierEngine::new(MonteCarloSettings {
            path_count: 32,
            seed: Some(42),
            max_active_workers: 1,
            ..Default::default()
        })?,
    ] {
        assert_eq!(
            engine.price(&owned, &context)?,
            engine.price(&option, &context)?
        );
    }
    assert_eq!(owned.replace(Default::default())?, owned);
    assert_eq!(option.calendar(), &TradingCalendar::all_days());

    macro_rules! check {
        ($option:expr, $engine:expr, $changes:expr) => {{
            let option = $option;
            let owned = option.replace($changes)?;
            assert_eq!(owned, option);
            assert_eq!(owned.calendar(), &TradingCalendar::all_days());
            assert_eq!(owned.replace(Default::default())?, owned);
            assert_eq!(option.calendar(), &TradingCalendar::all_days());
            let mut invalid = $changes;
            invalid.calendar = Some(TradingCalendar::weekdays());
            assert!(matches!(
                option.replace(invalid),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(
                $engine.price(&owned, &context)?,
                $engine.price(&option, &context)?
            );
        }};
    }
    let mut double = DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    );
    double.calendar = TradingCalendar::all_days();
    double.observation_times = Some(observations.clone());
    check!(
        DoubleBarrierOption::new(double)?,
        FiniteDifferenceDoubleBarrierEngine::default(),
        DoubleBarrierOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        }
    );
    let mut binary = BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    binary.calendar = TradingCalendar::all_days();
    binary.observation_times = Some(observations.clone());
    check!(
        BinaryBarrierOption::cash(binary, 10.0)?,
        FiniteDifferenceBinaryBarrierEngine::default(),
        BinaryBarrierOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        }
    );
    let mut touch = TouchOptionTerms::new(start, expiry, 120.0, BarrierType::UpAndIn);
    touch.calendar = TradingCalendar::all_days();
    touch.observation_times = Some(observations.clone());
    check!(
        TouchOption::cash(touch, 10.0)?,
        FiniteDifferenceBinaryBarrierEngine::default(),
        TouchOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        }
    );
    let mut double =
        DoubleTouchOptionTerms::new(start, expiry, 80.0, 120.0, DoubleBarrierType::KnockIn);
    double.calendar = TradingCalendar::all_days();
    double.observation_times = Some(observations);
    check!(
        DoubleTouchOption::cash(double, 10.0)?,
        FiniteDifferenceDoubleTouchEngine::default(),
        DoubleTouchOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        }
    );
    Ok(())
}
