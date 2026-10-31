use kiyosi::*;

fn day(month: u32, date: u32) -> Date {
    Date::new(2025, month, date).unwrap()
}
fn terms(kind: BarrierType, direction: OptionType) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        direction,
        100.0,
        day(1, 1),
        Date::new(2026, 1, 1).unwrap(),
        if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
            120.0
        } else {
            80.0
        },
        kind,
    )
}
fn market(rate: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, 0.01, 0.2).unwrap(),
        100.0,
        day(1, 1).into(),
    )
    .unwrap()
}
const KINDS: [BarrierType; 4] = [
    BarrierType::UpAndIn,
    BarrierType::UpAndOut,
    BarrierType::DownAndIn,
    BarrierType::DownAndOut,
];

#[test]
fn native_greeks_validate_only_the_spot_shift() {
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let context = market(0.04);
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let shifts = NumericalShiftSettings {
        volatility_shift: f64::NAN,
        rate_shift: -1.0,
        time_shift_days: 0,
        ..Default::default()
    };
    let request = GreeksRequest::all();
    let expected = engine
        .price_with_greeks(&option, &context, request, shifts.spot_shift)
        .unwrap();
    let actual =
        PricingEngine::price_with_greeks(&engine, &option, &context, request, shifts).unwrap();
    assert_eq!(actual, expected);
    for spot_shift in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            engine.price_with_greeks(&option, &context, request, spot_shift),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn conditional_survival_prices_and_likelihood_greeks_match_analytic() {
    let context = market(0.04);
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
            path_count: 131072,
            seed: Some(42),
            sampling,
            ..Default::default()
        })
        .unwrap();
        for kind in KINDS {
            for direction in [OptionType::Call, OptionType::Put] {
                let mut t = terms(kind, direction);
                t.rebate = 2.0;
                let option = BarrierOption::new(t).unwrap();
                let actual = engine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::all(),
                        NumericalShiftSettings::default().spot_shift,
                    )
                    .unwrap();
                let expected = AnalyticBarrierEngine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                        Default::default(),
                    )
                    .unwrap();
                for (got, want, budget) in [
                    (actual.price(), expected.price(), 0.12),
                    (
                        actual.require(Greek::Delta).unwrap(),
                        expected.require(Greek::Delta).unwrap(),
                        0.04,
                    ),
                    (
                        actual.require(Greek::Gamma).unwrap(),
                        expected.require(Greek::Gamma).unwrap(),
                        0.012,
                    ),
                ] {
                    assert!(
                        (got - want).abs() < budget,
                        "{sampling:?} {kind:?} {direction:?}: {got} != {want}"
                    );
                }
                assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
                for greek in Greek::ALL.into_iter().skip(2) {
                    assert!(!actual.has(greek));
                }
            }
        }
    }
}

#[test]
fn scheduled_likelihood_greeks_and_early_monitoring_match_quadrature() {
    let context = market(0.04);
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 131072,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    for early in [false, true] {
        for kind in KINDS {
            let mut t = terms(kind, OptionType::Put);
            t.rebate = 2.0;
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![day(4, 1), day(7, 1)];
            if !early {
                t.observation_dates.push(t.expiry_date);
            }
            let option = BarrierOption::new(t).unwrap();
            let actual = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::all(),
                    NumericalShiftSettings::default().spot_shift,
                )
                .unwrap();
            let expected = QuadratureBarrierEngine::default()
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            for (got, want, budget) in [
                (actual.price(), expected.price(), 0.08),
                (
                    actual.require(Greek::Delta).unwrap(),
                    expected.require(Greek::Delta).unwrap(),
                    0.03,
                ),
                (
                    actual.require(Greek::Gamma).unwrap(),
                    expected.require(Greek::Gamma).unwrap(),
                    0.015,
                ),
            ] {
                assert!(
                    (got - want).abs() < budget,
                    "{early} {kind:?}: {got} != {want}"
                );
            }
        }
    }
}

#[test]
fn continuous_hit_rebates_keep_negative_rate_values_and_derivatives() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 65536,
        seed: Some(17),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    for kind in [BarrierType::UpAndOut, BarrierType::DownAndOut] {
        let mut t = terms(kind, OptionType::Call);
        t.rebate = 10.0;
        t.rebate_timing = RebateTiming::AtHit;
        let option = BarrierOption::new(t).unwrap();
        for rate in [-0.02, 0.04] {
            let context = market(rate);
            let actual = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::all(),
                    NumericalShiftSettings::default().spot_shift,
                )
                .unwrap();
            let expected = AnalyticBarrierEngine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            for (got, want, budget) in [
                (actual.price(), expected.price(), 0.015),
                (
                    actual.require(Greek::Delta).unwrap(),
                    expected.require(Greek::Delta).unwrap(),
                    0.006,
                ),
                (
                    actual.require(Greek::Gamma).unwrap(),
                    expected.require(Greek::Gamma).unwrap(),
                    0.003,
                ),
            ] {
                assert!(
                    (got - want).abs() < budget,
                    "{kind:?} {rate}: {got} != {want}"
                );
            }
        }
    }
}

#[test]
fn threads_do_not_change_samples_even_with_early_exit_and_pairing() {
    for scheduled in [false, true] {
        let mut t = terms(BarrierType::UpAndOut, OptionType::Put);
        t.rebate = 3.0;
        t.rebate_timing = RebateTiming::AtHit;
        if scheduled {
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![day(4, 1), day(7, 1)];
        }
        let option = BarrierOption::new(t).unwrap();
        let context = market(0.04);
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            for antithetic in [false, true] {
                let settings = MonteCarloSettings {
                    path_count: 1024,
                    seed: Some(17),
                    thread_count: 1,
                    sampling,
                    antithetic,
                };
                let serial = MonteCarloBarrierEngine::new(settings).unwrap();
                let parallel = MonteCarloBarrierEngine::new(MonteCarloSettings {
                    thread_count: 4,
                    ..settings
                })
                .unwrap();
                let expected = serial
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::all(),
                        NumericalShiftSettings::default().spot_shift,
                    )
                    .unwrap();
                std::thread::scope(|scope| {
                    let call = || {
                        parallel
                            .price_with_greeks(
                                &option,
                                &context,
                                GreeksRequest::all(),
                                NumericalShiftSettings::default().spot_shift,
                            )
                            .unwrap()
                    };
                    let other = scope.spawn(call);
                    assert_eq!(call(), expected);
                    assert_eq!(other.join().unwrap(), expected);
                });
            }
        }
    }
}

#[test]
fn odd_antithetic_counts_keep_in_out_parity_with_the_terminal_sampler() {
    let context = market(0.04);
    let vanilla = EuropeanOption::new(
        OptionType::Call,
        100.0,
        day(1, 1),
        Date::new(2026, 1, 1).unwrap(),
    )
    .unwrap();
    let inside = BarrierOption::new(terms(BarrierType::DownAndIn, OptionType::Call)).unwrap();
    let outside = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    for path_count in [1, 2, 3, 5, 257] {
        let settings = MonteCarloSettings {
            path_count,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        };
        let engine = MonteCarloBarrierEngine::new(settings).unwrap();
        let sum =
            engine.price(&inside, &context).unwrap() + engine.price(&outside, &context).unwrap();
        let expected = MonteCarloVanillaEngine::new(settings)
            .unwrap()
            .price(&vanilla, &context)
            .unwrap();
        assert!(
            (sum - expected).abs() < 1e-12,
            "{path_count}: {sum} != {expected}"
        );
    }
}

#[test]
fn solver_replays_request_samples_and_rejects_fixed_cashflows() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 8192,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    let context = market(0.04);
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
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
    assert!((solved - 0.2).abs() < 1e-5);
    let unseeded = MonteCarloBarrierEngine::default();
    let prepared = unseeded.prepare_request().unwrap().unwrap();
    assert_eq!(
        prepared.price(&option, &context).unwrap(),
        prepared.price(&option, &context).unwrap()
    );
    let later = context
        .replace(PricingContextChanges {
            valuation_time: Some(day(1, 2).into()),
            ..Default::default()
        })
        .unwrap();
    let fixed = option.replace(Some(BarrierTouchState::Touched));
    assert!(matches!(
        implied_volatility(&engine, &fixed, &later, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
}

#[test]
fn history_current_equalities_and_finished_monitoring_resolve_before_sampling() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for kind in KINDS {
        let knock_in = matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn);
        for timing in [RebateTiming::AtExpiry, RebateTiming::AtHit] {
            if knock_in && timing == RebateTiming::AtHit {
                continue;
            }
            let mut t = terms(kind, OptionType::Call);
            t.rebate = 7.0;
            t.rebate_timing = timing;
            let option = BarrierOption::new(t.clone()).unwrap();
            let context = market(0.04)
                .replace(PricingContextChanges {
                    spot_price: Some(t.barrier_level),
                    ..Default::default()
                })
                .unwrap();
            let result = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::all(),
                    NumericalShiftSettings::default().spot_shift,
                )
                .unwrap();
            assert_eq!(
                result.price(),
                AnalyticBarrierEngine.price(&option, &context).unwrap()
            );
            assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
            let later = context
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(day(1, 1), 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&option, &later).unwrap_err(),
                Error::InvalidInput(_)
            ));
            let touched = option.replace(Some(BarrierTouchState::Touched));
            assert!(matches!(
                engine.price(&touched, &context).unwrap_err(),
                Error::InvalidInput(_)
            ));
            assert_eq!(
                engine.price(&touched, &later).unwrap(),
                AnalyticBarrierEngine.price(&touched, &later).unwrap()
            );
            let expired = context
                .replace(PricingContextChanges {
                    valuation_time: Some(t.expiry_date.into()),
                    ..Default::default()
                })
                .unwrap();
            for history in [BarrierTouchState::Touched, BarrierTouchState::Untouched] {
                let known = option.replace(Some(history));
                let result = engine
                    .price_with_greeks(
                        &known,
                        &expired,
                        GreeksRequest::all(),
                        NumericalShiftSettings::default().spot_shift,
                    )
                    .unwrap();
                assert_eq!(
                    result.price(),
                    AnalyticBarrierEngine.price(&known, &expired).unwrap()
                );
                assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
            }
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![day(4, 1)];
            let scheduled = BarrierOption::new(t.clone()).unwrap();
            let on_date = context
                .replace(PricingContextChanges {
                    valuation_time: Some(day(4, 1).into()),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                engine.price(&scheduled, &on_date).unwrap(),
                AnalyticBarrierEngine.price(&scheduled, &on_date).unwrap()
            );
            let after = on_date
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(day(4, 1), 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            let untouched = scheduled.replace(Some(BarrierTouchState::Untouched));
            assert_eq!(
                engine.price(&untouched, &after).unwrap(),
                AnalyticBarrierEngine.price(&untouched, &after).unwrap()
            );
            assert_eq!(option.touch_state(), None);
        }
    }
}

#[test]
fn scheduled_hit_rebates_use_fixing_discount_and_expiry_is_not_an_extra_fixing() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 131072,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let context = market(0.04);
    let mut t = terms(BarrierType::UpAndOut, OptionType::Put);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![day(7, 1)];
    t.rebate = 10.0;
    t.rebate_timing = RebateTiming::AtHit;
    let with_rebate = BarrierOption::new(t.clone()).unwrap();
    t.rebate = 0.0;
    let without_rebate = BarrierOption::new(t).unwrap();
    let time = 181.0 / 365.0;
    let threshold = ((1.2_f64).ln() - 0.01 * time) / (0.2 * time.sqrt());
    let probability = 0.5 * libm::erfc(threshold / 2.0_f64.sqrt());
    let difference = engine.price(&with_rebate, &context).unwrap()
        - engine.price(&without_rebate, &context).unwrap();
    assert!((difference - 10.0 * (-0.04 * time).exp() * probability).abs() < 0.025);
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![day(4, 1)];
    t.strike = 130.0;
    // The payoff would require a touch if expiry were silently added to the schedule.
    let early = BarrierOption::new(t).unwrap();
    let value = engine.price(&early, &context).unwrap();
    assert!(value > 0.01);
    assert!(
        (value
            - QuadratureBarrierEngine::default()
                .price(&early, &context)
                .unwrap())
        .abs()
            < 0.04
    );
}

#[test]
fn settings_applicability_and_failure_categories_preserve_inputs() {
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 10000001,
            ..Default::default()
        },
        MonteCarloSettings {
            thread_count: -1,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 3,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloBarrierEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert_eq!(
        MonteCarloBarrierEngine::default().settings(),
        MonteCarloSettings::default()
    );
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let context = market(0.04);
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let quote = engine.price(&option, &context).unwrap();
    let curve = RateTermStructure::new(
        day(1, 1).into(),
        vec![option.expiry_date().into()],
        vec![(-0.04_f64).exp()],
    )
    .unwrap();
    for (rate, dividend) in [
        (Rate::Curve(curve.clone()), Rate::Flat(0.01)),
        (Rate::Flat(0.04), Rate::Curve(curve)),
    ] {
        let curved = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
                ),
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
                    NumericalShiftSettings::default().spot_shift
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
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(option.expiry_date(), 0, 0, 0, 1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &outside).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms(BarrierType::DownAndOut, OptionType::Call);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![day(1, 4)];
    assert!(matches!(
        engine
            .price(&BarrierOption::new(t).unwrap(), &context)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::default(),
                NumericalShiftSettings::default().spot_shift
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(&option, &context, GreeksRequest::all(), 0.0)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    for (sigma, rate) in [(50.0, 0.04), (0.2, -1000.0)] {
        let extreme = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, 0.01, sigma).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &extreme).unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn rare_hit_samples_retain_scaled_value_and_unrequested_greeks_do_not_fail_price() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let mut t = terms(BarrierType::DownAndIn, OptionType::Call);
    t.strike = 1.0;
    t.barrier_level = 1e300 * (-20.0_f64).exp();
    let option = BarrierOption::new(t).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.5, 0.0, 1.0).unwrap(),
        1e300,
        day(1, 1).into(),
    )
    .unwrap();
    assert!(engine.price(&option, &context).unwrap() > 0.0);
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let tiny = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 1e-200).unwrap(),
        100.0,
        day(1, 1).into(),
    )
    .unwrap();
    let price = engine.price(&option, &tiny).unwrap();
    assert_eq!(
        engine
            .price_with_greeks(
                &option,
                &tiny,
                GreeksRequest::new([Greek::Rho]),
                NumericalShiftSettings::default().spot_shift
            )
            .unwrap()
            .price(),
        price
    );
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &tiny,
                GreeksRequest::new([Greek::Gamma]),
                NumericalShiftSettings::default().spot_shift
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn sobol_dimensions_count_fixings_and_unobserved_expiry_leg() {
    let mut t = terms(BarrierType::UpAndOut, OptionType::Put);
    t.expiry_date = Date::new(2100, 1, 1).unwrap();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = (2025..2100)
        .flat_map(|year| {
            (1..=12)
                .flat_map(move |month| (1..=28).map(move |d| Date::new(year, month, d).unwrap()))
        })
        .filter(|d| *d > day(1, 1))
        .take(21201)
        .collect();
    let option = BarrierOption::new(t).unwrap();
    let context = PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        day(1, 1).into(),
        TradingCalendar::all_days(),
    )
    .unwrap();
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine.price(&option, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
}
