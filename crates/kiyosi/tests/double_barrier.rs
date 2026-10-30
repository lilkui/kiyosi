use kiyosi::*;

fn day(y: i32, m: u32, d: u32) -> Date {
    Date::new(y, m, d).unwrap()
}
fn terms(direction: OptionType, kind: DoubleBarrierType) -> DoubleBarrierOptionTerms {
    DoubleBarrierOptionTerms::new(
        direction,
        100.0,
        day(2025, 1, 1),
        day(2026, 1, 1),
        80.0,
        120.0,
        kind,
    )
}
fn market(spot: f64, time: Date, sigma: f64, rate: f64, dividend: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        time.into(),
    )
    .unwrap()
}

#[test]
fn continuous_prices_and_greeks_match_independent_quantlib_regressions() {
    let engine = AnalyticDoubleBarrierEngine;
    for (direction, price, delta, gamma) in [
        (
            OptionType::Call,
            1.0838814592583166,
            -0.009437716341942348,
            -0.006614552070516311,
        ),
        (
            OptionType::Put,
            1.5760435147766145,
            -0.031725540191196444,
            -0.008683048910995694,
        ),
    ] {
        let option =
            DoubleBarrierOption::new(terms(direction, DoubleBarrierType::KnockOut)).unwrap();
        let context = market(100.0, day(2025, 1, 1), 0.2, 0.04, 0.01);
        let result = engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                Default::default(),
            )
            .unwrap();
        assert!((result.price() - price).abs() < 1e-9);
        assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-7);
        assert!((result.require(Greek::Gamma).unwrap() - gamma).abs() < 1e-6);
        assert!(!result.has(Greek::Vega));
        let inside =
            DoubleBarrierOption::new(terms(direction, DoubleBarrierType::KnockIn)).unwrap();
        let vanilla = EuropeanOption::new(
            direction,
            100.0,
            option.effective_date(),
            option.expiry_date(),
        )
        .unwrap();
        assert!(
            (engine.price(&inside, &context).unwrap() + result.price()
                - AnalyticEuropeanEngine.price(&vanilla, &context).unwrap())
            .abs()
                < 1e-9
        );
    }
}

#[test]
fn construction_validates_boundaries_schedules_and_rebates_in_core() {
    let base = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    let defaults = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        base.effective_date,
        base.expiry_date,
        80.0,
        BarrierType::DownAndOut,
    );
    assert_eq!(base.rebate, defaults.rebate);
    assert_eq!(base.rebate_timing, defaults.rebate_timing);
    assert_eq!(base.observation_mode, defaults.observation_mode);
    assert_eq!(base.observation_dates, defaults.observation_dates);
    assert_eq!(base.touch_state, defaults.touch_state);
    for (lower, upper) in [
        (0.0, 120.0),
        (-1.0, 120.0),
        (120.0, 80.0),
        (80.0, 80.0),
        (f64::NAN, 120.0),
        (f64::NEG_INFINITY, 120.0),
        (f64::INFINITY, 120.0),
        (80.0, f64::NAN),
        (80.0, f64::NEG_INFINITY),
        (80.0, f64::INFINITY),
    ] {
        let mut t = base.clone();
        t.lower_barrier = lower;
        t.upper_barrier = upper;
        assert!(matches!(
            DoubleBarrierOption::new(t).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut t = base.clone();
    t.strike = 0.0;
    assert!(matches!(
        DoubleBarrierOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = base.clone();
    t.expiry_date = day(2024, 1, 1);
    assert!(matches!(
        DoubleBarrierOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    for rebate in [-1.0, f64::NAN, f64::INFINITY] {
        let mut t = base.clone();
        t.rebate = rebate;
        assert!(matches!(
            DoubleBarrierOption::new(t).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut t = base.clone();
    t.barrier_type = DoubleBarrierType::KnockIn;
    t.rebate_timing = RebateTiming::AtHit;
    assert!(matches!(
        DoubleBarrierOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    for (mode, dates) in [
        (ObservationMode::Continuous, vec![base.expiry_date]),
        (ObservationMode::Scheduled, vec![]),
        (
            ObservationMode::Scheduled,
            vec![base.expiry_date, base.expiry_date],
        ),
        (
            ObservationMode::Scheduled,
            vec![base.expiry_date, base.effective_date],
        ),
        (ObservationMode::Scheduled, vec![day(2024, 12, 31)]),
        (ObservationMode::Scheduled, vec![day(2026, 1, 2)]),
    ] {
        let mut t = base.clone();
        t.observation_mode = mode;
        t.observation_dates = dates;
        assert!(matches!(
            DoubleBarrierOption::new(t).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn current_touches_strict_prior_history_and_expiry_preserve_remaining_cashflows() {
    let engine = AnalyticDoubleBarrierEngine;
    let start = day(2025, 1, 1);
    let later = day(2025, 1, 2);
    let end = day(2026, 1, 1);
    for spot in [79.0, 80.0, 120.0, 121.0] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
            t.rebate = 3.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t).unwrap();
            assert_eq!(option.touch_state(), None);
            let expected = if timing == RebateTiming::AtHit {
                3.0
            } else {
                3.0 * (-0.04_f64).exp()
            };
            assert!(
                (engine
                    .price(&option, &market(spot, start, 0.2, 0.04, 0.01))
                    .unwrap()
                    - expected)
                    .abs()
                    < 1e-12
            );
            assert!(matches!(
                engine
                    .price(&option, &market(spot, later, 0.2, 0.04, 0.01))
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            let past = option.replace(Some(BarrierTouchState::Touched));
            assert!(matches!(
                engine
                    .price(&past, &market(spot, start, 0.2, 0.04, 0.01))
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            let expected = if timing == RebateTiming::AtHit {
                0.0
            } else {
                3.0 * (-0.04_f64 * 364.0 / 365.0).exp()
            };
            assert!(
                (engine
                    .price(&past, &market(100.0, later, 0.2, 0.04, 0.01))
                    .unwrap()
                    - expected)
                    .abs()
                    < 1e-12
            );
            assert_eq!(past.replace(None), option);
            let result = engine
                .price_with_greeks(
                    &option,
                    &market(spot, start, 0.2, 0.04, 0.01),
                    GreeksRequest::all(),
                    Default::default(),
                )
                .unwrap();
            if spot == 80.0 || spot == 120.0 {
                assert!(result.values().iter().all(Option::is_none));
            }
        }
    }
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let mut t = terms(OptionType::Call, kind);
        t.rebate = 2.0;
        let option = DoubleBarrierOption::new(t)
            .unwrap()
            .replace(Some(BarrierTouchState::Untouched));
        for spot in [80.0, 110.0, 120.0] {
            let context = market(spot, end, 0.2, 0.04, 0.01);
            let touched = spot != 110.0;
            let expected = if (kind == DoubleBarrierType::KnockIn) == touched {
                (spot - 100.0).max(0.0)
            } else {
                2.0
            };
            let result = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            assert_eq!(result.price(), expected);
            assert!(result.values().iter().all(Option::is_none));
        }
        let touched = option.replace(Some(BarrierTouchState::Touched));
        let context = market(100.0, later, 0.2, 0.04, 0.01);
        if kind == DoubleBarrierType::KnockIn {
            assert_eq!(
                engine.price(&touched, &context).unwrap(),
                AnalyticEuropeanEngine
                    .price(
                        &EuropeanOption::new(
                            option.option_type(),
                            option.strike(),
                            option.effective_date(),
                            option.expiry_date()
                        )
                        .unwrap(),
                        &context
                    )
                    .unwrap()
            );
        }
    }
}

#[test]
fn unresolved_schedules_reject_after_calendar_history_and_settlement_validation() {
    let engine = AnalyticDoubleBarrierEngine;
    let first = day(2025, 6, 30);
    let context = market(100.0, day(2025, 1, 1), 0.2, 0.04, 0.01);
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![first];
    t.rebate = 3.0;
    let option = DoubleBarrierOption::new(t.clone()).unwrap();
    assert_eq!(option.observation_dates(), &[first]);
    assert_eq!(option.observation_schedule().dates(), &[first]);
    assert!(matches!(
        engine.price(&option, &context).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let impossible = option.replace(Some(BarrierTouchState::Touched));
    assert!(matches!(
        engine.price(&impossible, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let at_fixing = market(120.0, first, 0.2, 0.04, 0.01);
    let expected = 3.0 * (-0.04_f64 * 185.0 / 365.0).exp();
    assert!((engine.price(&option, &at_fixing).unwrap() - expected).abs() < 1e-12);
    let intraday = at_fixing
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(first, 12, 0, 0, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &intraday).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let known = option.replace(Some(BarrierTouchState::Untouched));
    assert_eq!(
        engine.price(&known, &intraday).unwrap(),
        AnalyticEuropeanEngine
            .price(
                &EuropeanOption::new(OptionType::Call, 100.0, t.effective_date, t.expiry_date)
                    .unwrap(),
                &intraday
            )
            .unwrap()
    );
    t.barrier_type = DoubleBarrierType::KnockIn;
    let inside = DoubleBarrierOption::new(t.clone())
        .unwrap()
        .replace(Some(BarrierTouchState::Untouched));
    assert!(
        (engine.price(&inside, &intraday).unwrap() - 3.0 * (-0.04_f64 * 184.5 / 365.0).exp()).abs()
            < 1e-12
    );
    t.observation_dates = vec![day(2025, 7, 5)];
    let weekend = DoubleBarrierOption::new(t).unwrap();
    assert!(matches!(
        engine.price(&weekend, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn image_spectral_switch_and_band_limits_preserve_independent_values() {
    let engine = AnalyticDoubleBarrierEngine;
    for (sigma, expected) in [
        (0.20273255202675663, 1.045594059652574),
        (0.20273255608140772, 1.0455940036319245),
        (0.3040988310811233, 0.21271328442609105),
    ] {
        let option =
            DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
        let actual = engine
            .price(
                &option,
                &market(100.0, option.effective_date(), sigma, 0.04, 0.01),
            )
            .unwrap();
        assert!((actual - expected).abs() < 1e-9);
    }
    for (lower, upper) in [(99.99, 100.01), (0.01, 10000.0)] {
        let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
        t.lower_barrier = lower;
        t.upper_barrier = upper;
        let option = DoubleBarrierOption::new(t).unwrap();
        let context = market(100.0, option.effective_date(), 0.2, 0.04, 0.01);
        let expected = if lower > 99.0 {
            0.0
        } else {
            AnalyticEuropeanEngine
                .price(
                    &EuropeanOption::new(
                        OptionType::Call,
                        100.0,
                        option.effective_date(),
                        option.expiry_date(),
                    )
                    .unwrap(),
                    &context,
                )
                .unwrap()
        };
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-9);
    }
    for (direction, strike) in [(OptionType::Call, 125.0), (OptionType::Put, 75.0)] {
        let mut t = terms(direction, DoubleBarrierType::KnockOut);
        t.strike = strike;
        let option = DoubleBarrierOption::new(t.clone()).unwrap();
        let context = market(100.0, option.effective_date(), 0.2, 0.04, 0.01);
        assert_eq!(engine.price(&option, &context).unwrap(), 0.0);
        t.barrier_type = DoubleBarrierType::KnockIn;
        assert_eq!(
            engine
                .price(&DoubleBarrierOption::new(t).unwrap(), &context)
                .unwrap(),
            AnalyticEuropeanEngine
                .price(
                    &EuropeanOption::new(
                        direction,
                        strike,
                        option.effective_date(),
                        option.expiry_date()
                    )
                    .unwrap(),
                    &context
                )
                .unwrap()
        );
    }
}

// Independent driftless killed-Brownian Fourier survival and exit integral.
fn rebate_reference(rate: f64, count: i32) -> [f64; 3] {
    let width = (125.0_f64 / 80.0).ln();
    let (mut survival, mut integrated) = (0.0, 0.0);
    for index in 0..count {
        let frequency = f64::from(2 * index + 1);
        let coefficient =
            4.0 * if index % 2 == 0 { 1.0 } else { -1.0 } / (frequency * std::f64::consts::PI);
        let decay = 0.5 * 0.04 * (frequency * std::f64::consts::PI / width).powi(2);
        survival += coefficient * (-decay).exp();
        integrated += coefficient * -(-(rate + decay)).exp_m1() / (rate + decay);
    }
    let discounted = (-rate).exp() * survival;
    [
        3.0 * ((-rate).exp() - discounted),
        3.0 * (1.0 - discounted - rate * integrated),
        3.0 * discounted,
    ]
}

#[test]
fn rebates_match_independent_exit_series_including_negative_rates() {
    let engine = AnalyticDoubleBarrierEngine;
    for rate in [-0.02, 0.0, 0.04] {
        let reference = rebate_reference(rate, 4097);
        let refined = rebate_reference(rate, 8193);
        for (a, b) in reference.into_iter().zip(refined) {
            assert!((a - b).abs() < 1e-11);
        }
        for (index, kind, timing) in [
            (0, DoubleBarrierType::KnockOut, RebateTiming::AtExpiry),
            (1, DoubleBarrierType::KnockOut, RebateTiming::AtHit),
            (2, DoubleBarrierType::KnockIn, RebateTiming::AtExpiry),
        ] {
            let mut t = terms(OptionType::Call, kind);
            t.upper_barrier = 125.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t.clone()).unwrap();
            let context = market(100.0, option.effective_date(), 0.2, rate, rate - 0.02);
            let base = engine.price(&option, &context).unwrap();
            t.rebate = 3.0;
            let actual = engine
                .price(&DoubleBarrierOption::new(t).unwrap(), &context)
                .unwrap();
            assert!(
                (actual - base - reference[index]).abs() < 1e-9,
                "{rate} {timing:?}: {actual} - {base} != {}",
                reference[index]
            );
        }
    }
}

#[test]
fn tiny_positive_volatility_preserves_future_hit_timing_and_surviving_payoff() {
    let engine = AnalyticDoubleBarrierEngine;
    for up in [false, true] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
            t.lower_barrier = if up { 80.0 } else { 98.0 };
            t.upper_barrier = if up { 102.0 } else { 120.0 };
            t.rebate = 3.0;
            t.rebate_timing = timing;
            let context = market(
                100.0,
                t.effective_date,
                1e-12,
                0.04,
                if up { 0.01 } else { 0.07 },
            );
            let hit = (if up {
                102.0_f64 / 100.0
            } else {
                100.0_f64 / 98.0
            })
            .ln()
                / 0.03;
            let expected = 3.0
                * (-0.04
                    * if timing == RebateTiming::AtHit {
                        hit
                    } else {
                        1.0
                    })
                .exp();
            let actual = engine
                .price(&DoubleBarrierOption::new(t).unwrap(), &context)
                .unwrap();
            assert!(
                (actual - expected).abs() < 1e-7,
                "{up} {timing:?}: {actual} != {expected}"
            );
        }
    }
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.lower_barrier = 99.5;
    t.upper_barrier = 100.5;
    t.strike = 90.0;
    let context = market(100.0, t.effective_date, 1e-12, 0.04, 0.04);
    assert!(
        (engine
            .price(&DoubleBarrierOption::new(t).unwrap(), &context)
            .unwrap()
            - 10.0 * (-0.04_f64).exp())
        .abs()
            < 1e-10
    );
}

#[test]
fn shared_analysis_solver_and_failure_categories_preserve_inputs() {
    let engine = AnalyticDoubleBarrierEngine;
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.lower_barrier = 50.0;
    t.upper_barrier = 200.0;
    let option = DoubleBarrierOption::new(t.clone()).unwrap();
    let context = market(100.0, t.effective_date, 0.2, 0.04, 0.01);
    let original = (option.clone(), context.clone());
    let quote = engine.price(&option, &context).unwrap();
    let solved = implied_volatility(
        &engine,
        &option,
        &context,
        quote,
        ImpliedVolatilitySettings {
            lower_bound: 0.15,
            upper_bound: 0.25,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((solved - 0.2).abs() < 1e-7);
    let known = option.replace(Some(BarrierTouchState::Untouched));
    let later = market(100.0, day(2025, 1, 2), 0.2, 0.04, 0.01);
    let result = engine
        .price_with_greeks(&known, &later, GreeksRequest::all(), Default::default())
        .unwrap();
    for greek in Greek::ALL {
        assert!(result.require(greek).unwrap().is_finite());
    }
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::default(),
                Default::default(),
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    spot_shift: 0.0,
                    ..Default::default()
                },
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    t.rebate = 3.0;
    let fixed = DoubleBarrierOption::new(t)
        .unwrap()
        .replace(Some(BarrierTouchState::Touched));
    assert!(matches!(
        implied_volatility(
            &engine,
            &fixed,
            &later,
            engine.price(&fixed, &later).unwrap(),
            Default::default(),
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let curve = RateTermStructure::new(
        option.effective_date().into(),
        vec![option.expiry_date().into()],
        vec![0.95],
    )
    .unwrap();
    for risk_free in [false, true] {
        let model = context
            .model_parameters()
            .replace(BlackScholesMertonParameterChanges {
                risk_free_rate: risk_free.then(|| curve.clone().into()),
                dividend_yield: (!risk_free).then(|| curve.clone().into()),
                ..Default::default()
            })
            .unwrap();
        let curved = context
            .replace(PricingContextChanges {
                model_parameters: Some(model),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &curved).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
        assert!(matches!(
            engine
                .price_with_greeks(
                    &option,
                    &curved,
                    GreeksRequest::default(),
                    Default::default(),
                )
                .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
        assert!(matches!(
            implied_volatility(&engine, &option, &curved, f64::NAN, Default::default())
                .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let enormous = market(100.0, option.effective_date(), f64::MAX, 0.04, 0.01);
    assert!(matches!(
        engine.price(&option, &enormous).unwrap_err(),
        Error::ComputationFailure(_)
    ));
    for time in [day(2024, 12, 31), day(2026, 1, 2)] {
        assert!(matches!(
            engine
                .price(&known, &market(100.0, time, 0.2, 0.04, 0.01))
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let narrow =
        DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
    for spot in [80.005, 119.995] {
        let context = market(spot, option.effective_date(), 0.2, 0.04, 0.01);
        let result = engine
            .price_with_greeks(
                &narrow,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega]),
                Default::default(),
            )
            .unwrap();
        assert!(!result.has(Greek::Delta));
        assert!(!result.has(Greek::Gamma));
        assert!(result.has(Greek::Vega));
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| engine.price(&option, &context).unwrap()))
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), quote);
        }
    });
    assert_eq!((option, context), original);
}

#[test]
fn settled_zero_rebates_do_not_require_a_representable_discount() {
    let engine = AnalyticDoubleBarrierEngine;
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.expiry_date = day(2027, 1, 1);
    let context = market(120.0, t.effective_date, 0.2, 1e308, 0.0);
    let outside = DoubleBarrierOption::new(t.clone()).unwrap();
    assert_eq!(engine.price(&outside, &context).unwrap(), 0.0);
    t.barrier_type = DoubleBarrierType::KnockIn;
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![t.effective_date];
    t.touch_state = Some(BarrierTouchState::Untouched);
    let inside = DoubleBarrierOption::new(t).unwrap();
    let later = market(100.0, day(2025, 1, 2), 0.2, 1e308, 0.0);
    assert_eq!(engine.price(&inside, &later).unwrap(), 0.0);
}
