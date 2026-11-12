use kiyosi::*;

#[test]
fn undated_touch_history_is_validated_at_valuation_and_replaced_atomically() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let first = start.add_days(1)?;
    let end = start.add_days(8)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.calendar = TradingCalendar::all_days();
    terms.observation_times = Some(vec![first, end]);
    terms.touch_state = Some(BarrierTouchState::Touched);
    let original = BarrierOption::new(terms)?;
    let market =
        |at| PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?, 100.0, at);
    assert!(matches!(
        AnalyticBarrierEngine.price(&original, &market(first)?),
        Err(Error::InvalidInput(_))
    ));
    for at in [first.add_micros(1)?, first.add_days(2)?] {
        assert_eq!(AnalyticBarrierEngine.price(&original, &market(at)?)?, 0.0);
    }
    let changed = original.replace(BarrierOptionChanges {
        barrier_level: Some(130.0),
        ..Default::default()
    })?;
    assert_eq!(changed.touch_state(), Some(BarrierTouchState::Touched));
    assert!(matches!(
        original.replace(BarrierOptionChanges {
            barrier_level: Some(-1.0),
            touch_state: Some(None),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(original.barrier_level(), 120.0);
    assert_eq!(original.touch_state(), Some(BarrierTouchState::Touched));
    let cleared = original.replace(BarrierOptionChanges {
        touch_state: Some(None),
        ..Default::default()
    })?;
    assert!(matches!(
        AnalyticBarrierEngine.price(&cleared, &market(first.add_micros(1)?)?),
        Err(Error::MissingHistory(_))
    ));
    Ok(())
}
#[test]
fn undated_history_prices_all_touch_families_at_each_requested_valuation() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(8)?;
    let market =
        |at| PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?, 100.0, at);
    for observations in [None, Some(vec![start.add_days(1)?, end])] {
        let first = observations.as_ref().map_or(start, |times| times[0]);
        macro_rules! check {
            ($terms:expr, $construct:expr, $engine:expr, $changes:ident, $expected:expr) => {{
                let mut terms = $terms;
                terms.calendar = TradingCalendar::all_days();
                terms.observation_times = observations.clone();
                terms.touch_state = Some(BarrierTouchState::Touched);
                let option = ($construct)(terms)?;
                assert!(matches!(
                    $engine.price(&option, &market(first)?),
                    Err(Error::InvalidInput(_))
                ));
                for at in [first.add_micros(1)?, first.add_days(2)?] {
                    assert_eq!($engine.price(&option, &market(at)?)?, $expected);
                }
                let cleared = option.replace($changes {
                    touch_state: Some(None),
                    ..Default::default()
                })?;
                assert!(matches!(
                    $engine.price(&cleared, &market(first.add_micros(1)?)?),
                    Err(Error::MissingHistory(_))
                ));
                assert_eq!(option.touch_state(), Some(BarrierTouchState::Touched));
            }};
        }
        check!(
            BarrierOptionTerms::new(
                OptionType::Call,
                100.0,
                start,
                end,
                120.0,
                BarrierType::UpAndOut
            ),
            BarrierOption::new,
            AnalyticBarrierEngine,
            BarrierOptionChanges,
            0.0
        );
        check!(
            DoubleBarrierOptionTerms::new(
                OptionType::Call,
                100.0,
                start,
                end,
                80.0,
                120.0,
                DoubleBarrierType::KnockOut
            ),
            DoubleBarrierOption::new,
            AnalyticDoubleBarrierEngine,
            DoubleBarrierOptionChanges,
            0.0
        );
        check!(
            BinaryBarrierTerms::new(
                OptionType::Call,
                100.0,
                start,
                end,
                120.0,
                BarrierType::UpAndOut
            ),
            |terms| BinaryBarrierOption::cash(terms, 10.0),
            AnalyticBinaryBarrierEngine,
            BinaryBarrierOptionChanges,
            0.0
        );
        check!(
            BinaryBarrierTerms::new(
                OptionType::Call,
                100.0,
                start,
                end,
                120.0,
                BarrierType::UpAndOut
            ),
            BinaryBarrierOption::asset,
            AnalyticBinaryBarrierEngine,
            BinaryBarrierOptionChanges,
            0.0
        );
        check!(
            TouchOptionTerms::new(start, end, 120.0, BarrierType::UpAndIn),
            |terms| TouchOption::cash(terms, 10.0),
            AnalyticBinaryBarrierEngine,
            TouchOptionChanges,
            10.0
        );
        check!(
            TouchOptionTerms::new(start, end, 120.0, BarrierType::UpAndIn),
            TouchOption::asset,
            AnalyticBinaryBarrierEngine,
            TouchOptionChanges,
            100.0
        );
        check!(
            DoubleTouchOptionTerms::new(start, end, 80.0, 120.0, DoubleBarrierType::KnockIn),
            |terms| DoubleTouchOption::cash(terms, 10.0),
            AnalyticDoubleTouchEngine,
            DoubleTouchOptionChanges,
            10.0
        );
    }
    Ok(())
}

#[test]
fn scheduled_touch_at_valuation_uses_current_spot_once() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let first = start.add_days(1)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        start.add_days(8)?,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_times = Some(vec![first]);
    terms.rebate = 7.0;
    terms.rebate_timing = RebateTiming::AtHit;
    let option = BarrierOption::new(terms)?;
    let market =
        |at, spot| PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, spot, at);
    let engine = QuadratureBarrierEngine::default();
    assert_eq!(
        engine.price(&option, &market(first.add_micros(-1)?, 100.0)?)?,
        0.0
    );
    assert_eq!(engine.price(&option, &market(first, 120.0)?)?, 7.0);
    let touched = option.replace(BarrierOptionChanges {
        touch_state: Some(Some(BarrierTouchState::Touched)),
        ..Default::default()
    })?;
    assert_eq!(
        engine.price(&touched, &market(first.add_micros(1)?, 100.0)?)?,
        0.0
    );
    assert!(matches!(
        engine.price(&option, &market(first.add_micros(1)?, 100.0)?),
        Err(Error::MissingHistory(_))
    ));
    Ok(())
}
