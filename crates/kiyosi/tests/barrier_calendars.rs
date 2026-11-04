use kiyosi::*;

#[test]
fn barrier_calendars_are_owned_and_preserved_by_replacement() -> Result<()> {
    let start = Date::new(2025, 1, 3)?.start_of_day();
    let expiry = start.add_days(2)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.02, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    let daily_market = context.replace(PricingContextChanges {
        calendar: Some(TradingCalendar::all_days()),
        ..Default::default()
    })?;
    let observations = vec![start.add_days(1)?, expiry];
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_times = observations.clone();
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
            thread_count: 1,
            ..Default::default()
        })?,
    ] {
        for market in [&context, &daily_market] {
            assert!(matches!(
                engine.price(&option, market),
                Err(Error::InvalidInput(_))
            ));
        }
        assert_eq!(
            engine.price(&owned, &context)?,
            engine.price(&owned, &daily_market)?
        );
    }
    assert_eq!(owned.replace(Default::default())?, owned);
    assert_eq!(owned.replace_history(None), owned);
    assert_eq!(option.calendar(), &TradingCalendar::weekdays());

    macro_rules! check {
        ($option:expr, $engine:expr, $changes:expr) => {{
            let option = $option;
            let owned = option.replace($changes)?;
            assert_eq!(owned.calendar(), &TradingCalendar::all_days());
            assert_eq!(owned.replace(Default::default())?, owned);
            assert_eq!(owned.replace_history(None), owned);
            assert_eq!(option.calendar(), &TradingCalendar::weekdays());
            assert!(matches!(
                $engine.price(&option, &daily_market),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(
                $engine.price(&owned, &context)?,
                $engine.price(&owned, &daily_market)?
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
    double.observation_mode = ObservationMode::Scheduled;
    double.observation_times = observations.clone();
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
    binary.observation_mode = ObservationMode::Scheduled;
    binary.observation_times = observations.clone();
    check!(
        BinaryBarrierOption::cash(binary, 10.0)?,
        FiniteDifferenceBinaryBarrierEngine::default(),
        BinaryBarrierOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        }
    );
    let mut touch = TouchOptionTerms::new(start, expiry, 120.0, BarrierType::UpAndIn);
    touch.observation_mode = ObservationMode::Scheduled;
    touch.observation_times = observations.clone();
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
    double.observation_mode = ObservationMode::Scheduled;
    double.observation_times = observations;
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
