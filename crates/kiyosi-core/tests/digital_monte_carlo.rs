use kiyosi_core::*;

fn market(spot: f64, rate: f64, dividend: f64, sigma: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        Date::new(2025, 1, 1).unwrap().into(),
    )
    .unwrap()
}

fn options(direction: OptionType) -> [DigitalOption; 2] {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    [
        DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
        DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
    ]
}

#[test]
fn terminal_prices_and_likelihood_greeks_match_analytic_and_share_draws() {
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let settings = MonteCarloSettings {
                path_count: 65536,
                seed: Some(42),
                sampling,
                antithetic,
                ..Default::default()
            };
            let engine = MonteCarloDigitalEngine::new(settings).unwrap();
            for direction in [OptionType::Call, OptionType::Put] {
                for option in options(direction) {
                    let context = market(100.0, 0.05, 0.02, 0.2);
                    let expected = AnalyticDigitalEngine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap();
                    let actual = engine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap();
                    let scale = option.payout().unwrap_or(100.0);
                    for (got, want, tolerance) in [
                        (actual.price(), expected.price(), 0.015 * scale),
                        (
                            actual.require(Greek::Delta).unwrap(),
                            expected.require(Greek::Delta).unwrap(),
                            0.001 * scale,
                        ),
                        (
                            actual.require(Greek::Gamma).unwrap(),
                            expected.require(Greek::Gamma).unwrap(),
                            0.0001 * scale,
                        ),
                    ] {
                        assert!((got - want).abs() < tolerance, "{got} != {want}");
                    }
                    for greek in Greek::ALL.into_iter().skip(2) {
                        assert!(!actual.has(greek));
                    }
                    assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
                    for thread_count in [0, 1, 4] {
                        let other = MonteCarloDigitalEngine::new(MonteCarloSettings {
                            thread_count,
                            step_count: i32::MIN,
                            ..settings
                        })
                        .unwrap();
                        assert_eq!(
                            actual,
                            other
                                .price_with_greeks(
                                    &option,
                                    &context,
                                    GreeksRequest::all(),
                                    Default::default()
                                )
                                .unwrap()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn tiny_volatility_preserves_exercise_and_only_checks_requested_greeks() {
    let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
        path_count: 2,
        antithetic: true,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for option in options(direction) {
            for sigma in [1e-200, f64::from_bits(1)] {
                let context = market(100.0, 0.0, 0.0, sigma)
                    .replace(PricingContextChanges {
                        valuation_time: Some(
                            Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999)
                                .unwrap(),
                        ),
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(
                    engine.price(&option, &context).unwrap(),
                    option.payout().unwrap_or(100.0) / 2.0
                );
                let result = engine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Rho]),
                        Default::default(),
                    )
                    .unwrap();
                assert!(!result.has(Greek::Rho));
                assert!(matches!(
                    engine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::new([Greek::Gamma]),
                            Default::default()
                        )
                        .unwrap_err(),
                    Error::ComputationFailure(_)
                ));
            }
        }
    }
}

#[test]
fn expiry_is_strict_and_validation_precedes_pricing_without_mutation() {
    let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let base = market(100.0, 0.05, 0.02, 0.2);
    for direction in [OptionType::Call, OptionType::Put] {
        for option in options(direction) {
            for spot in [
                99.0,
                f64::from_bits(100.0_f64.to_bits() - 1),
                100.0,
                f64::from_bits(100.0_f64.to_bits() + 1),
                101.0,
            ] {
                let context = base
                    .replace(PricingContextChanges {
                        spot_price: Some(spot),
                        valuation_time: Some(option.expiry_date().into()),
                        ..Default::default()
                    })
                    .unwrap();
                let pays = if direction == OptionType::Call {
                    spot > 100.0
                } else {
                    spot < 100.0
                };
                let result = engine
                    .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert_eq!(
                    result.price(),
                    if pays {
                        option.payout().unwrap_or(spot)
                    } else {
                        0.0
                    }
                );
                assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
                assert!(matches!(
                    implied_volatility(
                        &engine,
                        &option,
                        &context,
                        result.price(),
                        Default::default()
                    )
                    .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
            }
            let outside = base
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(option.expiry_date(), 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&option, &outside).unwrap_err(),
                Error::InvalidInput(_)
            ));
            assert!(matches!(
                engine
                    .price_with_greeks(
                        &option,
                        &outside,
                        GreeksRequest::default(),
                        Default::default()
                    )
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            let curve = RateTermStructure::new(
                option.effective_date().into(),
                vec![option.expiry_date().into()],
                vec![0.95],
            )
            .unwrap();
            let node = base
                .replace(PricingContextChanges {
                    model_parameters: Some(
                        base.model_parameters()
                            .replace(BlackScholesMertonParameterChanges {
                                risk_free_rate: Some(curve.into()),
                                ..Default::default()
                            })
                            .unwrap(),
                    ),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine
                    .price_with_greeks(&option, &node, GreeksRequest::default(), Default::default())
                    .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(matches!(
                numerical_greeks(
                    &engine,
                    &option,
                    &node,
                    GreeksRequest::default(),
                    Default::default()
                )
                .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(matches!(
                implied_volatility(&engine, &option, &node, f64::NAN, Default::default())
                    .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(engine.price(&option, &base).unwrap().is_finite());
            assert_eq!(engine.settings().seed, Some(42));
        }
    }
}

#[test]
fn discounted_samples_and_greeks_preserve_extreme_scales_and_failure_state() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
        path_count: 65536,
        sampling: MonteCarloSampling::Sobol,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for (spot, payout, rate, sigma) in [
        (1e-300, 1e-300, -710.0, 0.2),
        (1e300, 1e300, 710.0, 0.2),
        (100.0, 1e308, 0.0, 0.2),
        (100.0, 1e-310, 0.0, 0.2),
    ] {
        for option in [
            DigitalOption::cash_or_nothing(OptionType::Call, spot, payout, start, end).unwrap(),
            DigitalOption::asset_or_nothing(OptionType::Call, spot, start, end).unwrap(),
        ] {
            let context = market(spot, rate, rate, sigma);
            let expected = AnalyticDigitalEngine.price(&option, &context).unwrap();
            let got = engine.price(&option, &context).unwrap();
            assert!(
                (got - expected).abs() < expected * 0.002 + 1e-320,
                "{got} != {expected}"
            );
        }
    }
    let option =
        DigitalOption::cash_or_nothing(OptionType::Call, 1e-200, 1e100, start, end).unwrap();
    let context = market(1e-200, 0.0, 0.0, 0.2);
    let delta = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta]),
            Default::default(),
        )
        .unwrap();
    assert!(delta.require(Greek::Delta).unwrap().is_finite());
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Gamma]),
                Default::default()
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(engine.price(&option, &context).unwrap(), delta.price());
    let overflowing =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 1e308, start, end).unwrap();
    assert!(matches!(
        engine
            .price(&overflowing, &market(100.0, -10.0, -10.0, 0.2))
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn odd_paths_and_shared_sample_sensitivities_and_solves_retain_seed() {
    let option = options(OptionType::Call)[0].clone();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
                path_count: 65536,
                seed: Some(42),
                sampling,
                antithetic,
                ..Default::default()
            })
            .unwrap();
            let quote = engine.price(&option, &context).unwrap();
            let bounds = ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.3,
                ..Default::default()
            };
            let sigma = implied_volatility(&engine, &option, &context, quote, bounds).unwrap();
            assert!((sigma - 0.2).abs() < 0.001);
            let shifts = NumericalShiftSettings {
                spot_shift: 2.0,
                volatility_shift: 0.01,
                ..Default::default()
            };
            let numeric =
                numerical_greeks(&engine, &option, &context, GreeksRequest::all(), shifts).unwrap();
            for thread_count in [1, 4] {
                let other = MonteCarloDigitalEngine::new(MonteCarloSettings {
                    thread_count,
                    ..engine.settings()
                })
                .unwrap();
                assert_eq!(
                    numeric,
                    numerical_greeks(&other, &option, &context, GreeksRequest::all(), shifts)
                        .unwrap()
                );
                assert_eq!(
                    sigma,
                    implied_volatility(&other, &option, &context, quote, bounds).unwrap()
                );
            }
            assert!(matches!(
                implied_volatility(&engine, &option, &context, 20.0, bounds).unwrap_err(),
                Error::RootNotBracketed(_)
            ));
        }
    }
    for paths in [1, 2, 3, 511, 512, 513, 1025] {
        let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
            path_count: paths,
            antithetic: true,
            seed: Some(42),
            ..Default::default()
        })
        .unwrap();
        let atm = market(100.0, 0.02, 0.0, 0.2);
        let price = engine.price(&option, &atm).unwrap();
        let expected_pairs = f64::from(paths / 2) * 10.0 * (-0.02_f64).exp() / f64::from(paths);
        let unpaired = if paths % 2 == 1 {
            10.0 * (-0.02_f64).exp() / f64::from(paths)
        } else {
            0.0
        };
        assert!(
            (price - expected_pairs).abs() < 1e-12
                || (price - expected_pairs - unpaired).abs() < 1e-12
        );
    }
    let unseeded = MonteCarloDigitalEngine::new(MonteCarloSettings {
        path_count: 4096,
        ..Default::default()
    })
    .unwrap();
    let solved = implied_volatility(
        &unseeded,
        &option,
        &context,
        4.945810910532236,
        ImpliedVolatilitySettings {
            lower_bound: 0.1,
            upper_bound: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((0.1..0.3).contains(&solved));
    assert_eq!(unseeded.settings().seed, None);
}

#[test]
fn settings_reuse_vanilla_validation_and_cash_sampling_variance() {
    assert_eq!(
        MonteCarloDigitalEngine::default().settings(),
        MonteCarloVanillaEngine::default().settings()
    );
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 10_000_001,
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
            MonteCarloDigitalEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for direction in [OptionType::Call, OptionType::Put] {
        let option = options(direction)[0].clone();
        let context = market(100.0, 0.05, 0.02, 0.2);
        let expected = AnalyticDigitalEngine.price(&option, &context).unwrap();
        let discounted_payout = 10.0 * (-0.05_f64).exp();
        let probability = expected / discounted_payout;
        let variance = discounted_payout.powi(2) * probability * (1.0 - probability);
        for path_count in [2048, 32768] {
            let mut sum = 0.0;
            let mut squared = 0.0;
            for seed in 0..32 {
                let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
                    path_count,
                    seed: Some(seed),
                    thread_count: 1,
                    ..Default::default()
                })
                .unwrap();
                let error = engine.price(&option, &context).unwrap() - expected;
                sum += error;
                squared += error * error;
            }
            let rmse = (squared / 32.0).sqrt();
            let standard_error = (variance / f64::from(path_count)).sqrt();
            assert!(rmse > 0.5 * standard_error && rmse < 1.5 * standard_error);
            assert!((sum / 32.0).abs() < 4.0 * standard_error / 32.0_f64.sqrt());
        }
    }
}
