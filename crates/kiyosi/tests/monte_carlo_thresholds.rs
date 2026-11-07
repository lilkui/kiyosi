mod support;
use support::dated;

use kiyosi::*;

fn context(volatility: f64, intraday: bool) -> PricingContext {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, volatility).unwrap(),
        100.0,
        Timestamp::new(start.date(), if intraday { 12 } else { 0 }, 0, 0, 0).unwrap(),
    )
    .unwrap()
}

fn settings(antithetic: bool) -> MonteCarloSettings {
    MonteCarloSettings {
        path_count: 65_536,
        seed: Some(42),
        antithetic,
        ..Default::default()
    }
}

#[test]
fn scheduled_barriers_preserve_sub_ulp_threshold_movements() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let single = MonteCarloBarrierEngine::new(settings(true)).unwrap();
    let double = MonteCarloDoubleBarrierEngine::new(settings(true)).unwrap();
    for volatility in [1e-18, 1e-200] {
        let context = context(volatility, false);
        for (direction, strike) in [(OptionType::Call, 90.0), (OptionType::Put, 110.0)] {
            for (kind, lower, upper) in [
                (BarrierType::UpAndIn, 80.0, 100.0),
                (BarrierType::UpAndOut, 80.0, 100.0),
                (BarrierType::DownAndIn, 100.0, 120.0),
                (BarrierType::DownAndOut, 100.0, 120.0),
            ] {
                let mut terms = BarrierOptionTerms::new(direction, strike, start, end, 100.0, kind);
                terms.observation_mode = ObservationMode::Scheduled;
                terms.observation_times = vec![end];
                terms.rebate = 2.0;
                let option = BarrierOption::new(terms).unwrap();
                assert!(
                    (single.price(&dated(&option, &context), &context).unwrap() - 6.0).abs()
                        < 1e-12
                );

                let kind = match kind {
                    BarrierType::UpAndIn | BarrierType::DownAndIn => DoubleBarrierType::KnockIn,
                    _ => DoubleBarrierType::KnockOut,
                };
                let mut terms = DoubleBarrierOptionTerms::new(
                    direction, strike, start, end, lower, upper, kind,
                );
                terms.observation_mode = ObservationMode::Scheduled;
                terms.observation_times = vec![end];
                terms.rebate = 2.0;
                let option = DoubleBarrierOption::new(terms).unwrap();
                assert!(
                    (double.price(&dated(&option, &context), &context).unwrap() - 6.0).abs()
                        < 1e-12
                );
            }
        }
    }
}

#[test]
fn snowball_thresholds_preserve_price_and_sampling_uncertainty() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.02,
        start,
        end,
        autocallable_observations(&[end], &[100.0], Some(&[0.12]), None).unwrap(),
    ))
    .unwrap();
    for volatility in [1e-18, 1e-200] {
        for antithetic in [false, true] {
            let engine = MonteCarloAutocallableEngine::new(settings(antithetic)).unwrap();
            let estimate = engine
                .price_with_statistics(
                    &dated(&note, &context(volatility, false)),
                    &context(volatility, false),
                )
                .unwrap();
            let error = estimate.standard_error().unwrap();
            if antithetic {
                assert!((estimate.price() - 1.07).abs() < 1e-12);
                assert!(error < 1e-14);
            } else {
                assert!((estimate.price() - 1.07).abs() < 4.0 * error);
                assert!((error - 0.05 / 256.0).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn autocallable_knock_in_and_coupon_tests_preserve_sub_ulp_movements() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2025, 1, 2).unwrap().start_of_day();
    let engine = MonteCarloAutocallableEngine::new(settings(true)).unwrap();
    for volatility in [1e-18, 1e-200] {
        let context = context(volatility, true);
        for mode in [
            KnockInObservationMode::EveryTradingDay,
            KnockInObservationMode::AtExpiry,
        ] {
            let mut terms = TernarySnowballTerms::new(
                0.12,
                0.02,
                100.0,
                start,
                end,
                autocallable_observations(&[end], &[120.0], None, None).unwrap(),
                Some((end).time_of_day()),
            );
            terms.knock_in_observation_mode = mode;
            terms.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
                .then_some(terms.expiry_time.time_of_day());
            terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
            let note = TernarySnowballOption::new(terms).unwrap();
            assert!(
                (engine.price(&dated(&note, &context), &context).unwrap() - (1.0 + 0.07 / 365.0))
                    .abs()
                    < 1e-12
            );
        }
        let mut terms = PhoenixTerms::new(
            0.0,
            100.0,
            80.0,
            100.0,
            100.0,
            start,
            end,
            autocallable_observations(&[end], &[120.0], None, Some(&[100.0])).unwrap(),
            Some((end).time_of_day()),
        );
        terms.coupon = PhoenixCoupon::PerPeriod(0.12);
        terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
        terms.observation_clock_time = None;
        let note = PhoenixOption::new(terms).unwrap();
        assert!((engine.price(&dated(&note, &context), &context).unwrap() - 1.06).abs() < 1e-12);
    }
}

#[test]
fn accumulator_thresholds_preserve_knock_out_and_accelerated_settlements() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2025, 1, 2).unwrap().start_of_day();
    let engine = MonteCarloAccumulatorEngine::new(settings(true)).unwrap();
    for volatility in [1e-18, 1e-200] {
        let context = context(volatility, true);
        for (direction, strike, barrier) in [
            (OptionType::Call, 90.0, 120.0),
            (OptionType::Put, 110.0, 80.0),
        ] {
            let mut terms =
                AccumulatorTerms::new(strike, 100.0, 1.0, 1.0, start, end, (end).time_of_day());
            terms.option_type = direction;
            terms.accumulated_quantity = Some(1.0);
            terms.knock_out_settled = Some(false);
            let option = Accumulator::new(terms).unwrap();
            assert!(
                (engine.price(&dated(&option, &context), &context).unwrap() - 15.0).abs() < 1e-12
            );

            let mut terms = AccumulatorTerms::new(
                100.0,
                barrier,
                1.0 / volatility,
                2.0,
                start,
                end,
                (end).time_of_day(),
            );
            terms.accumulated_quantity = Some(0.0);
            terms.knock_out_settled = Some(false);
            terms.option_type = direction;
            let option = Accumulator::new(terms).unwrap();
            let vanilla = EuropeanOption::new(direction, 100.0, start, end).unwrap();
            let expected = -AnalyticEuropeanEngine
                .price(&dated(&vanilla, &context), &context)
                .unwrap()
                / volatility;
            let actual = engine.price(&dated(&option, &context), &context).unwrap();
            assert!(
                (actual - expected).abs() < 0.02 * expected.abs(),
                "{actual} != {expected}"
            );
        }
    }
}
