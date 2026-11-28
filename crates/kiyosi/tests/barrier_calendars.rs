#[path = "support/observation_schedule.rs"]
mod schedule_fixture;
use schedule_fixture::schedule as fixture_schedule;

use kiyosi::engines::{BinomialSettings, FiniteDifferenceEngine, MonteCarloSettings};
use kiyosi::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    terms.calendar = Some(TradingCalendar::all_days());
    terms.observation_schedule = Some(fixture_schedule(
        (0..4096)
            .map(|offset| start.add_micros(offset * 1_000_000))
            .chain([Ok(valuation), Ok(first), Ok(expiry)])
            .collect::<Result<Vec<_>>>()?,
        terms.calendar.as_ref(),
    ));
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
    let calendar = TradingCalendar::all_days();
    let schedule =
        ObservationSchedule::new(vec![start.add_days(1)?, expiry], start, expiry, &calendar)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_schedule = Some(schedule.clone());
    terms.calendar = Some(TradingCalendar::weekdays());
    assert!(matches!(
        BarrierOption::new(terms.clone()),
        Err(Error::InvalidInput(_))
    ));
    terms.calendar = None;
    let option = BarrierOption::new(terms)?;
    assert!(std::ptr::eq(
        option.observation_schedule().unwrap().times(),
        schedule.times()
    ));
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
            assert!(std::ptr::eq(
                option.observation_schedule().unwrap().times(),
                owned.observation_schedule().unwrap().times()
            ));
            let mut clear = $changes;
            clear.calendar = Some(TradingCalendar::weekdays());
            clear.observation_schedule = Some(None);
            let continuous = option.replace(clear)?;
            let mut reuse = $changes;
            reuse.calendar = None;
            reuse.observation_schedule = Some(owned.observation_schedule().cloned());
            assert_eq!(continuous.replace(reuse)?, owned);
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
    double.observation_schedule = option.observation_schedule().cloned();
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
    binary.observation_schedule = option.observation_schedule().cloned();
    check!(
        BinaryBarrierOption::cash(binary, 10.0)?,
        FiniteDifferenceBinaryBarrierEngine::default(),
        BinaryBarrierOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        }
    );
    let mut touch = TouchOptionTerms::new(start, expiry, 120.0, BarrierType::UpAndIn);
    touch.observation_schedule = option.observation_schedule().cloned();
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
    double.observation_schedule = option.observation_schedule().cloned();
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

#[test]
fn reusable_barrier_schedule_only_scans_an_overridden_calendar() -> Result<()> {
    let start = Date::new(2025, 1, 3)?.start_of_day();
    let expiry = start.add_days(1)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let calendar = TradingCalendar::new(
        {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                true
            }
        },
        365,
    )?;
    let schedule = ObservationSchedule::new(
        (0..2048)
            .map(|offset| start.add_micros(offset))
            .collect::<Result<_>>()?,
        start,
        expiry,
        &calendar,
    )?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_schedule = Some(schedule.clone());
    let original = BarrierOption::new(terms)?;
    let changed = original.replace(BarrierOptionChanges {
        strike: Some(105.0),
        calendar: Some(calendar),
        ..Default::default()
    })?;
    assert_ne!(original.strike(), changed.strike());
    assert!(std::ptr::eq(
        changed.observation_schedule().unwrap().times(),
        schedule.times()
    ));
    assert_eq!(calls.load(Ordering::Relaxed), schedule.len());

    let replacement = TradingCalendar::new(
        {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                true
            }
        },
        365,
    )?;
    let overridden = changed.replace(BarrierOptionChanges {
        calendar: Some(replacement.clone()),
        ..Default::default()
    })?;
    assert_eq!(overridden.calendar(), &replacement);
    assert!(std::ptr::eq(
        overridden.observation_schedule().unwrap().times(),
        schedule.times()
    ));
    assert!(matches!(
        overridden.replace(BarrierOptionChanges {
            effective_time: Some(start.add_micros(1)?),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 2 * schedule.len());
    Ok(())
}
