use kiyosi_core::*;

fn terms(direction: OptionType, kind: DoubleBarrierType) -> DoubleBarrierOptionTerms {
    DoubleBarrierOptionTerms::new(
        direction,
        100.0,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
        80.0,
        120.0,
        kind,
    )
}

fn market() -> PricingContext {
    PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        Date::new(2025, 1, 1).unwrap().into(),
        TradingCalendar::all_days(),
    )
    .unwrap()
}

#[test]
fn continuous_prices_and_shared_sample_greeks_match_independent_references() {
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
        let context = market();
        for seed in [17, 42, 103] {
            let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
                path_count: 32768,
                seed: Some(seed),
                sampling: MonteCarloSampling::Sobol,
                ..Default::default()
            })
            .unwrap();
            let result = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            assert!((result.price() - price).abs() < 0.003);
            assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 0.003);
            assert!((result.require(Greek::Gamma).unwrap() - gamma).abs() < 0.003);
            assert_eq!(result.price(), engine.price(&option, &context).unwrap());
            assert!(!result.has(Greek::Vega));
        }
    }
}

fn engine() -> MonteCarloDoubleBarrierEngine {
    MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 65536,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap()
}

fn compare(actual: &PricingResult, expected: &PricingResult, price: f64, delta: f64, gamma: f64) {
    for (got, want, budget) in [
        (actual.price(), expected.price(), price),
        (
            actual.require(Greek::Delta).unwrap(),
            expected.require(Greek::Delta).unwrap(),
            delta,
        ),
        (
            actual.require(Greek::Gamma).unwrap(),
            expected.require(Greek::Gamma).unwrap(),
            gamma,
        ),
    ] {
        assert!(
            (got - want).abs() < budget,
            "{got} != {want}, budget {budget}"
        );
    }
}

#[test]
fn pseudo_random_prices_and_likelihood_greeks_match_across_seeds() {
    for direction in [OptionType::Call, OptionType::Put] {
        for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
            let option = DoubleBarrierOption::new(terms(direction, kind)).unwrap();
            let expected = AnalyticDoubleBarrierEngine
                .price_with_greeks(
                    &option,
                    &market(),
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            for seed in [17, 42, 103] {
                let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
                    path_count: 131072,
                    seed: Some(seed),
                    antithetic: true,
                    ..Default::default()
                })
                .unwrap();
                let result = engine
                    .price_with_greeks(&option, &market(), GreeksRequest::all(), Default::default())
                    .unwrap();
                compare(&result, &expected, 0.12, 0.015, 0.003);
                for greek in [
                    Greek::Speed,
                    Greek::Theta,
                    Greek::Charm,
                    Greek::Color,
                    Greek::Vega,
                    Greek::Vanna,
                    Greek::Zomma,
                    Greek::Rho,
                ] {
                    assert!(!result.has(greek));
                }
            }
        }
    }
}

#[test]
fn image_and_spectral_series_and_extreme_bands_match_analytic() {
    let engine = engine();
    for sigma in [0.20273255202675663, 0.20273255608140772, 0.3040988310811233] {
        let context = market()
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(0.04, 0.01, sigma).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        let option =
            DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
        let actual = engine
            .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
            .unwrap();
        let expected = AnalyticDoubleBarrierEngine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                Default::default(),
            )
            .unwrap();
        compare(&actual, &expected, 0.003, 0.003, 0.003);
    }
    for (lower, upper) in [(99.99, 100.01), (0.01, 10000.0)] {
        let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
        t.lower_barrier = lower;
        t.upper_barrier = upper;
        let option = DoubleBarrierOption::new(t).unwrap();
        assert!(
            (engine.price(&option, &market()).unwrap()
                - AnalyticDoubleBarrierEngine
                    .price(&option, &market())
                    .unwrap())
            .abs()
                < 0.025
        );
    }
}

#[test]
fn continuous_rebates_and_tiny_volatility_hits_keep_values_and_derivatives() {
    let engine = engine();
    for rate in [-0.02, 0.04] {
        let context = market()
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, rate - 0.02, 0.2).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        for (kind, timing) in [
            (DoubleBarrierType::KnockOut, RebateTiming::AtExpiry),
            (DoubleBarrierType::KnockOut, RebateTiming::AtHit),
            (DoubleBarrierType::KnockIn, RebateTiming::AtExpiry),
        ] {
            let mut t = terms(OptionType::Call, kind);
            t.upper_barrier = 125.0;
            t.rebate = 3.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t).unwrap();
            let actual = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            let expected = AnalyticDoubleBarrierEngine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            compare(&actual, &expected, 0.01, 0.006, 0.003);
        }
    }
    for up in [false, true] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
            t.lower_barrier = if up { 80.0 } else { 98.0 };
            t.upper_barrier = if up { 102.0 } else { 120.0 };
            t.rebate = 3.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t).unwrap();
            let context = market()
                .replace(PricingContextChanges {
                    model_parameters: Some(
                        BlackScholesMertonParameters::new(
                            0.04,
                            if up { 0.01 } else { 0.07 },
                            1e-12,
                        )
                        .unwrap(),
                    ),
                    ..Default::default()
                })
                .unwrap();
            let time = if timing == RebateTiming::AtHit {
                (if up { 1.02_f64 } else { 100.0 / 98.0 }).ln() / 0.03
            } else {
                1.0
            };
            assert!(
                (engine.price(&option, &context).unwrap() - 3.0 * (-0.04 * time).exp()).abs()
                    < 1e-7
            );
        }
    }
}

#[test]
fn scheduled_dates_and_unobserved_expiry_leg_match_finite_difference() {
    let engine = engine();
    for direction in [OptionType::Call, OptionType::Put] {
        for (kind, timing) in [
            (DoubleBarrierType::KnockOut, RebateTiming::AtExpiry),
            (DoubleBarrierType::KnockOut, RebateTiming::AtHit),
            (DoubleBarrierType::KnockIn, RebateTiming::AtExpiry),
        ] {
            for final_fixing in [false, true] {
                let mut t = terms(direction, kind);
                t.rebate = 2.0;
                t.rebate_timing = timing;
                t.observation_mode = ObservationMode::Scheduled;
                t.observation_dates = vec![Date::new(2025, 6, 30).unwrap()];
                if final_fixing {
                    t.observation_dates.push(t.expiry_date);
                }
                let option = DoubleBarrierOption::new(t).unwrap();
                let actual = engine
                    .price_with_greeks(&option, &market(), GreeksRequest::all(), Default::default())
                    .unwrap();
                let expected = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
                    time_step_count: 1200,
                    asset_step_count: 1200,
                    ..Default::default()
                })
                .unwrap()
                .price_with_greeks(
                    &option,
                    &market(),
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
                compare(&actual, &expected, 0.04, 0.025, 0.01);
            }
        }
    }
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.strike = 130.0;
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![Date::new(2025, 4, 1).unwrap()];
    assert!(
        engine
            .price(&DoubleBarrierOption::new(t).unwrap(), &market())
            .unwrap()
            > 0.01
    );
}

#[test]
fn thread_step_and_sampling_contracts_preserve_exact_samples_and_pairing() {
    assert_eq!(
        MonteCarloDoubleBarrierEngine::default().settings(),
        MonteCarloSettings::default()
    );
    for scheduled in [false, true] {
        let mut t = terms(OptionType::Put, DoubleBarrierType::KnockOut);
        t.rebate = 3.0;
        t.rebate_timing = RebateTiming::AtHit;
        if scheduled {
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![
                Date::new(2025, 4, 1).unwrap(),
                Date::new(2025, 7, 1).unwrap(),
            ];
        }
        let option = DoubleBarrierOption::new(t).unwrap();
        let context = market();
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            for antithetic in [false, true] {
                let settings = MonteCarloSettings {
                    path_count: 1024,
                    seed: Some(17),
                    thread_count: 1,
                    sampling,
                    antithetic,
                    ..Default::default()
                };
                let serial = MonteCarloDoubleBarrierEngine::new(settings).unwrap();
                let parallel = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
                    step_count: 10000,
                    thread_count: 4,
                    ..settings
                })
                .unwrap();
                let expected = serial
                    .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                std::thread::scope(|scope| {
                    let handles = (0..4)
                        .map(|_| {
                            scope.spawn(|| {
                                parallel
                                    .price_with_greeks(
                                        &option,
                                        &context,
                                        GreeksRequest::all(),
                                        Default::default(),
                                    )
                                    .unwrap()
                            })
                        })
                        .collect::<Vec<_>>();
                    for handle in handles {
                        assert_eq!(handle.join().unwrap(), expected);
                    }
                });
            }
        }
    }
    let context = market();
    let vanilla = EuropeanOption::new(
        OptionType::Call,
        100.0,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
    .unwrap();
    for path_count in [1, 2, 3, 5, 257] {
        let settings = MonteCarloSettings {
            path_count,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        };
        let engine = MonteCarloDoubleBarrierEngine::new(settings).unwrap();
        let total = [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut]
            .into_iter()
            .map(|kind| {
                engine
                    .price(
                        &DoubleBarrierOption::new(terms(OptionType::Call, kind)).unwrap(),
                        &context,
                    )
                    .unwrap()
            })
            .sum::<f64>();
        let expected = MonteCarloVanillaEngine::new(settings)
            .unwrap()
            .price(&vanilla, &context)
            .unwrap();
        assert!((total - expected).abs() < 1e-12);
    }
}

fn category<T: std::fmt::Debug>(result: Result<T>, expected: ErrorCategory) {
    assert_eq!(result.unwrap_err().category, expected);
}

#[test]
fn current_and_prior_touches_expiry_and_finished_monitoring_resolve_before_sampling() {
    let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            if kind == DoubleBarrierType::KnockIn && timing == RebateTiming::AtHit {
                continue;
            }
            let mut t = terms(OptionType::Call, kind);
            t.rebate = 7.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t.clone()).unwrap();
            for spot in [79.0, 80.0, 120.0, 121.0] {
                let context = market()
                    .replace(PricingContextChanges {
                        spot_price: Some(spot),
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(
                    engine.price(&option, &context).unwrap(),
                    AnalyticDoubleBarrierEngine
                        .price(&option, &context)
                        .unwrap()
                );
                if spot == 80.0 || spot == 120.0 {
                    let result = engine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap();
                    assert!(!result.has(Greek::Delta) && !result.has(Greek::Gamma));
                }
                let later = context
                    .replace(PricingContextChanges {
                        valuation_time: Some(
                            Timestamp::new(option.effective_date(), 0, 0, 0, 1).unwrap(),
                        ),
                        ..Default::default()
                    })
                    .unwrap();
                category(
                    engine.price(&option, &later),
                    ErrorCategory::InvalidParameter,
                );
                let touched = option.replace(Some(BarrierTouchState::Touched)).unwrap();
                category(
                    engine.price(&touched, &context),
                    ErrorCategory::InvalidOption,
                );
                assert_eq!(
                    engine.price(&touched, &later).unwrap(),
                    AnalyticDoubleBarrierEngine.price(&touched, &later).unwrap()
                );
                for history in [BarrierTouchState::Touched, BarrierTouchState::Untouched] {
                    let known = option.replace(Some(history)).unwrap();
                    let expired = context
                        .replace(PricingContextChanges {
                            valuation_time: Some(option.expiry_date().into()),
                            ..Default::default()
                        })
                        .unwrap();
                    let result = engine
                        .price_with_greeks(
                            &known,
                            &expired,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap();
                    assert_eq!(
                        result.price(),
                        AnalyticDoubleBarrierEngine.price(&known, &expired).unwrap()
                    );
                    assert!(!result.has(Greek::Delta) && !result.has(Greek::Gamma));
                }
            }
            t.observation_mode = ObservationMode::Scheduled;
            let first = Date::new(2025, 7, 1).unwrap();
            t.observation_dates = vec![first];
            let scheduled = DoubleBarrierOption::new(t).unwrap();
            let on_date = market()
                .replace(PricingContextChanges {
                    spot_price: Some(120.0),
                    valuation_time: Some(first.into()),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                engine.price(&scheduled, &on_date).unwrap(),
                AnalyticDoubleBarrierEngine
                    .price(&scheduled, &on_date)
                    .unwrap()
            );
            let after = on_date
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(first, 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            let untouched = scheduled
                .replace(Some(BarrierTouchState::Untouched))
                .unwrap();
            assert_eq!(
                engine.price(&untouched, &after).unwrap(),
                AnalyticDoubleBarrierEngine
                    .price(&untouched, &after)
                    .unwrap()
            );
            assert_eq!(option.touch_state(), None);
        }
    }
    let option =
        DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
    for spot in [80.005, 119.995] {
        let context = market()
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap();
        let result = engine
            .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
            .unwrap();
        assert!(!result.has(Greek::Delta) && !result.has(Greek::Gamma));
    }
}

#[test]
fn solver_pins_request_samples_and_rejects_fixed_cashflows() {
    let engine = engine();
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.lower_barrier = 50.0;
    t.upper_barrier = 200.0;
    let option = DoubleBarrierOption::new(t.clone()).unwrap();
    let context = market();
    let quote = engine.price(&option, &context).unwrap();
    let controls = ImpliedVolatilitySettings {
        lower_bound: 0.15,
        upper_bound: 0.25,
        ..Default::default()
    };
    assert!(
        (implied_volatility(&engine, &option, &context, quote, controls).unwrap() - 0.2).abs()
            < 1e-5
    );
    let unseeded = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 2048,
        ..Default::default()
    })
    .unwrap();
    let prepared = unseeded.prepare_request().unwrap().unwrap();
    assert_eq!(
        prepared.price(&option, &context).unwrap(),
        prepared.price(&option, &context).unwrap()
    );
    for observed in [-1.0, f64::NAN] {
        category(
            implied_volatility(&engine, &option, &context, observed, controls),
            if observed.is_nan() {
                ErrorCategory::InvalidParameter
            } else {
                ErrorCategory::UnbracketedVolatility
            },
        );
    }
    let later = context
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2025, 7, 2).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    t.rebate = 3.0;
    t.touch_state = Some(BarrierTouchState::Touched);
    let touched = DoubleBarrierOption::new(t).unwrap();
    category(
        implied_volatility(
            &engine,
            &touched,
            &later,
            engine.price(&touched, &later).unwrap(),
            controls,
        ),
        ErrorCategory::UnsupportedOperation,
    );
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockIn);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![Date::new(2025, 7, 1).unwrap()];
    t.touch_state = Some(BarrierTouchState::Untouched);
    let finished = DoubleBarrierOption::new(t).unwrap();
    category(
        implied_volatility(&engine, &finished, &later, 0.0, controls),
        ErrorCategory::UnsupportedOperation,
    );
    for direction in [OptionType::Call, OptionType::Put] {
        let mut t = terms(direction, DoubleBarrierType::KnockOut);
        t.strike = if direction == OptionType::Call {
            125.0
        } else {
            75.0
        };
        let excluded = DoubleBarrierOption::new(t).unwrap();
        assert_eq!(engine.price(&excluded, &context).unwrap(), 0.0);
        category(
            implied_volatility(&engine, &excluded, &context, 0.0, controls),
            ErrorCategory::UnsupportedOperation,
        );
    }
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn invalid_settings_applicability_and_failure_categories_preserve_inputs() {
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            step_count: 1,
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
        category(
            MonteCarloDoubleBarrierEngine::new(settings),
            ErrorCategory::InvalidParameter,
        );
    }
    let engine = engine();
    let option =
        DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
    let context = market();
    let quote = engine.price(&option, &context).unwrap();
    let curve = RateTermStructure::new(
        option.effective_date().into(),
        vec![option.expiry_date().into()],
        vec![(-0.03_f64).exp()],
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
        category(
            engine.price(&option, &curved),
            ErrorCategory::UnsupportedOperation,
        );
        category(
            engine.price_with_greeks(
                &option,
                &curved,
                GreeksRequest::default(),
                Default::default(),
            ),
            ErrorCategory::UnsupportedOperation,
        );
        category(
            implied_volatility(&engine, &option, &curved, f64::NAN, Default::default()),
            ErrorCategory::UnsupportedOperation,
        );
    }
    category(
        engine.price_with_greeks(
            &option,
            &context,
            GreeksRequest::default(),
            Default::default(),
        ),
        ErrorCategory::InvalidParameter,
    );
    category(
        engine.price_with_greeks(
            &option,
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings {
                spot_shift: 0.0,
                ..Default::default()
            },
        ),
        ErrorCategory::InvalidParameter,
    );
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(option.expiry_date(), 0, 0, 0, 1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    category(
        engine.price(&option, &outside),
        ErrorCategory::InvalidTimeRange,
    );
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![Date::new(2025, 1, 4).unwrap()];
    let invalid = DoubleBarrierOption::new(t).unwrap();
    let weekdays = context
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::default()),
            ..Default::default()
        })
        .unwrap();
    category(
        engine.price(&invalid, &weekdays),
        ErrorCategory::InvalidDate,
    );
    for (rate, sigma) in [(-1000.0, 0.2), (0.04, 50.0)] {
        let extreme = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, rate, sigma).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        category(
            engine.price(&option, &extreme),
            ErrorCategory::InvalidResult,
        );
    }
    let tiny = context
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.04, 0.01, 1e-200).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let price = engine.price(&option, &tiny).unwrap();
    assert_eq!(
        engine
            .price_with_greeks(
                &option,
                &tiny,
                GreeksRequest::new([Greek::Rho]),
                Default::default()
            )
            .unwrap()
            .price(),
        price
    );
    category(
        engine.price_with_greeks(
            &option,
            &tiny,
            GreeksRequest::new([Greek::Gamma]),
            Default::default(),
        ),
        ErrorCategory::InvalidResult,
    );
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
}

#[test]
fn sobol_dimension_limit_counts_the_unobserved_expiry_leg() {
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.expiry_date = Date::new(2100, 1, 1).unwrap();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = (2025..2100)
        .flat_map(|year| {
            (1..=12).flat_map(move |month| {
                (1..=28).map(move |day| Date::new(year, month, day).unwrap())
            })
        })
        .filter(|date| *date > t.effective_date)
        .take(21201)
        .collect();
    let option = DoubleBarrierOption::new(t).unwrap();
    category(
        engine().price(&option, &market()),
        ErrorCategory::InvalidParameter,
    );
}
