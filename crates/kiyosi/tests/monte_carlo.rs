use kiyosi::*;

fn contract(direction: OptionType) -> EuropeanOption {
    EuropeanOption::new(
        direction,
        100.0,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
    .unwrap()
}

fn market() -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        Date::new(2025, 1, 1).unwrap().into(),
    )
    .unwrap()
}

#[test]
fn prepared_vanilla_request_replays_prices_without_mutating_engine() {
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 256,
        thread_count: 1,
        ..Default::default()
    })
    .unwrap();
    let prepared = <MonteCarloVanillaEngine as PricingEngine>::prepare_request(&engine)
        .unwrap()
        .unwrap();
    let option = contract(OptionType::Call);
    let context = market();
    let price = prepared.price(&option, &context).unwrap();
    assert_eq!(prepared.price(&option, &context).unwrap(), price);
    assert!(prepared.prepare_request().unwrap().is_none());
    assert_eq!(engine.settings().seed, None);
}

#[test]
fn terminal_prices_match_analytic_references_and_ignore_validated_steps() {
    let defaults = MonteCarloVanillaEngine::default().settings();
    assert_eq!(defaults.path_count, 100_000);
    assert_eq!(defaults.step_count, 50);
    assert_eq!(defaults.thread_count, 0);
    assert_eq!(defaults.seed, None);
    for (direction, expected) in [
        (OptionType::Call, 9.227005508154036),
        (OptionType::Put, 6.330080627549918),
    ] {
        let option = contract(direction);
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            seed: Some(42),
            thread_count: 1,
            ..Default::default()
        })
        .unwrap();
        let price = engine.price(&option, &market()).unwrap();
        assert!((price - expected).abs() < 0.2, "{price} != {expected}");
        for steps in [2, 7, 10_000] {
            let other = MonteCarloVanillaEngine::new(MonteCarloSettings {
                step_count: steps,
                ..engine.settings()
            })
            .unwrap();
            assert_eq!(other.price(&option, &market()).unwrap(), price);
        }
        let other = MonteCarloVanillaEngine::new(MonteCarloSettings {
            seed: Some(43),
            ..engine.settings()
        })
        .unwrap();
        assert_ne!(other.price(&option, &market()).unwrap(), price);
    }
}

#[test]
fn terminal_prices_preserve_tiny_positive_volatility() {
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: 65_536,
                seed: Some(42),
                thread_count: 1,
                sampling,
                antithetic,
                ..Default::default()
            })
            .unwrap();
            for amount in [100.0, 1e18] {
                for sigma in [1e-17, 1e-200] {
                    let context = PricingContext::new(
                        BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap(),
                        amount,
                        market().valuation_time(),
                    )
                    .unwrap();
                    for direction in [OptionType::Call, OptionType::Put] {
                        let option = EuropeanOption::new(
                            direction,
                            amount,
                            contract(direction).effective_date(),
                            contract(direction).expiry_date(),
                        )
                        .unwrap();
                        // At the money, the small-volatility price is S * sigma / sqrt(2*pi).
                        let expected = amount * sigma / std::f64::consts::TAU.sqrt();
                        let estimate = engine.price_with_statistics(&option, &context).unwrap();
                        assert_eq!(estimate.price(), engine.price(&option, &context).unwrap());
                        assert!((estimate.price() / expected - 1.0).abs() < 0.02);
                        if sampling == MonteCarloSampling::PseudoRandom {
                            let error = estimate.standard_error().unwrap();
                            assert!(error > 0.0 && error < 0.02 * expected);
                            assert!((estimate.price() - expected).abs() < 6.0 * error);
                            let (lower, upper) = estimate.confidence_interval().unwrap();
                            assert!(lower < estimate.price() && estimate.price() < upper);
                        } else {
                            assert_eq!(estimate.standard_error(), None);
                            assert_eq!(estimate.confidence_interval(), None);
                        }
                        if direction == OptionType::Call {
                            let american = AmericanOption::new(
                                direction,
                                amount,
                                option.effective_date(),
                                option.expiry_date(),
                            )
                            .unwrap();
                            assert_eq!(
                                engine.price(&american, &context).unwrap(),
                                estimate.price()
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn sampling_statistics_report_uncertainty_and_single_path_unavailability() {
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        seed: Some(42),
        thread_count: 4,
        ..Default::default()
    })
    .unwrap();
    let option = contract(OptionType::Call);
    let estimate = engine.price_with_statistics(&option, &market()).unwrap();
    assert_eq!(estimate.price(), engine.price(&option, &market()).unwrap());
    let error = estimate.standard_error().unwrap();
    assert!(error > 0.04 && error < 0.05, "{error}");
    let (lower, upper) = estimate.confidence_interval().unwrap();
    assert!(lower < estimate.price() && upper > estimate.price());
    assert!((upper - lower - 2.0 * 1.959963984540054 * error).abs() < 1e-12);
    let one = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 1,
        ..engine.settings()
    })
    .unwrap();
    let estimate = one.price_with_statistics(&option, &market()).unwrap();
    assert_eq!(estimate.standard_error(), None);
    assert_eq!(estimate.confidence_interval(), None);
    let option = EuropeanOption::new(
        OptionType::Call,
        1.0,
        option.effective_date(),
        option.expiry_date(),
    )
    .unwrap();
    let pair = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 2,
        ..engine.settings()
    })
    .unwrap();
    let pair = pair.price_with_statistics(&option, &market()).unwrap();
    let first = one.price(&option, &market()).unwrap();
    assert!((pair.standard_error().unwrap() - (first - pair.price()).abs()).abs() < 1e-12);
    let mut previous = None;
    for paths in 1..=5 {
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: paths,
            ..engine.settings()
        })
        .unwrap();
        let price = engine.price(&option, &market()).unwrap();
        assert_ne!(
            Some(price),
            previous,
            "adjacent counts must not round to the same sample budget"
        );
        previous = Some(price);
    }
}

#[test]
fn analyses_share_one_seed_and_solvers_recover_seeded_quotes() {
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 4097,
        thread_count: 2,
        ..Default::default()
    })
    .unwrap();
    let option = EuropeanOption::new(
        OptionType::Call,
        1e-100,
        contract(OptionType::Call).effective_date(),
        contract(OptionType::Call).expiry_date(),
    )
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for result in [
        engine
            .price_with_greeks(&option, &market(), request, Default::default())
            .unwrap(),
        numerical_greeks(&engine, &option, &market(), request, Default::default()).unwrap(),
    ] {
        assert!((result.require(Greek::Delta).unwrap() - result.price() / 100.0).abs() < 1e-9);
        assert!(result.require(Greek::Gamma).unwrap().abs() < 1e-7);
    }
    assert_eq!(engine.settings().seed, None);
    let option = contract(OptionType::Call);
    let settings = ImpliedVolatilitySettings {
        lower_bound: 0.1,
        upper_bound: 0.4,
        ..Default::default()
    };
    for seed in [Some(42), Some(u64::MAX), None] {
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            seed,
            ..engine.settings()
        })
        .unwrap();
        let quote = if seed.is_some() {
            engine.price(&option, &market()).unwrap()
        } else {
            9.227005508154036
        };
        let volatility = implied_volatility(&engine, &option, &market(), quote, settings).unwrap();
        if seed.is_some() {
            assert!((volatility - 0.2).abs() < 1e-8);
        } else {
            assert!(volatility > 0.1 && volatility < 0.4);
        }
        assert_eq!(engine.settings().seed, seed);
    }
}

#[test]
fn settings_validate_boundaries_before_committing_an_engine() {
    for paths in [i32::MIN, -1, 0, 10_000_001, i32::MAX] {
        assert!(matches!(
            MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: paths,
                ..Default::default()
            })
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for steps in [i32::MIN, 0, 1, 10_001, i32::MAX] {
        assert!(matches!(
            MonteCarloVanillaEngine::new(MonteCarloSettings {
                step_count: steps,
                ..Default::default()
            })
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(matches!(
        MonteCarloVanillaEngine::new(MonteCarloSettings {
            thread_count: -1,
            ..Default::default()
        })
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    for paths in [1, 10_000_000] {
        for steps in [2, 10_000] {
            let settings = MonteCarloSettings {
                path_count: paths,
                step_count: steps,
                seed: Some(u64::MAX),
                thread_count: i32::MAX,
                ..Default::default()
            };
            assert_eq!(
                MonteCarloVanillaEngine::new(settings).unwrap().settings(),
                settings
            );
        }
    }
}

#[test]
fn sample_block_boundaries_and_shared_instances_are_thread_invariant() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<MonteCarloVanillaEngine>();
    send_sync::<MonteCarloEstimate>();
    for paths in [1, 2, 255, 256, 257, 511, 512, 513, 769, 1025, 3073, 4097] {
        for seed in [0, 42, u64::MAX] {
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: paths,
                seed: Some(seed),
                thread_count: 1,
                ..Default::default()
            })
            .unwrap();
            for direction in [OptionType::Call, OptionType::Put] {
                let option = contract(direction);
                let expected = engine.price_with_statistics(&option, &market()).unwrap();
                for threads in [0, 2, 4, i32::MAX] {
                    let parallel = MonteCarloVanillaEngine::new(MonteCarloSettings {
                        thread_count: threads,
                        ..engine.settings()
                    })
                    .unwrap();
                    assert_eq!(
                        parallel.price_with_statistics(&option, &market()).unwrap(),
                        expected
                    );
                }
            }
        }
    }
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 1025,
        seed: Some(42),
        thread_count: 4,
        ..Default::default()
    })
    .unwrap();
    let option = contract(OptionType::Put);
    let context = market();
    let expected = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    let quote = expected.price();
    let bounds = ImpliedVolatilitySettings {
        lower_bound: 0.1,
        upper_bound: 0.4,
        ..Default::default()
    };
    let volatility = implied_volatility(&engine, &option, &context, quote, bounds).unwrap();
    for threads in [0, 1, 2, 4] {
        let other = MonteCarloVanillaEngine::new(MonteCarloSettings {
            thread_count: threads,
            ..engine.settings()
        })
        .unwrap();
        assert_eq!(
            other
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap(),
            expected
        );
        assert_eq!(
            implied_volatility(&other, &option, &context, quote, bounds).unwrap(),
            volatility
        );
    }
    std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..3)
            .map(|_| {
                scope.spawn(|| {
                    engine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap()
                })
            })
            .collect();
        for task in tasks {
            assert_eq!(task.join().unwrap(), expected);
        }
    });
}

#[test]
fn expiry_life_applicability_and_failed_requests_leave_inputs_unchanged() {
    let option = contract(OptionType::Call);
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 257,
        seed: Some(42),
        thread_count: 4,
        ..Default::default()
    })
    .unwrap();
    let context = market();
    let at = |time| {
        context
            .replace(PricingContextChanges {
                valuation_time: Some(time),
                ..Default::default()
            })
            .unwrap()
    };
    let expiry = option.expiry_date().start_of_day();
    let estimate = engine.price_with_statistics(&option, &at(expiry)).unwrap();
    assert_eq!(estimate.price(), 0.0);
    assert_eq!(estimate.standard_error(), Some(0.0));
    assert_eq!(estimate.confidence_interval(), Some((0.0, 0.0)));
    let result = engine
        .price_with_greeks(
            &option,
            &at(expiry),
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|greek| !result.has(greek)));
    assert!(matches!(
        implied_volatility(&engine, &option, &at(expiry), 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    for time in [
        Timestamp::new(Date::new(2024, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap(),
        Timestamp::new(option.expiry_date(), 0, 0, 0, 1).unwrap(),
    ] {
        assert!(matches!(
            engine.price(&option, &at(time)).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(
        engine
            .price(
                &option,
                &at(Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap())
            )
            .is_ok()
    );
    let curve =
        RateTermStructure::new(option.effective_date().into(), vec![expiry], vec![0.95]).unwrap();
    let model = context
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let node_context = context
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            valuation_time: Some(expiry),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &node_context).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &node_context,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        numerical_greeks(
            &engine,
            &option,
            &node_context,
            GreeksRequest::default(),
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    for quote in [0.0, f64::NAN] {
        assert!(matches!(
            implied_volatility(
                &engine,
                &option,
                &node_context,
                quote,
                ImpliedVolatilitySettings {
                    max_iterations: 0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let baseline = engine.price(&option, &context).unwrap();
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Rho]),
                NumericalShiftSettings {
                    rate_shift: 1000.0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    let missing = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta, Greek::Vega]),
            NumericalShiftSettings {
                spot_shift: 100.0,
                volatility_shift: 0.2,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!missing.has(Greek::Delta) && !missing.has(Greek::Vega));
    assert_eq!(missing.price(), baseline);
    assert_eq!(engine.price(&option, &context).unwrap(), baseline);
    assert_eq!(context, market());
    assert_eq!(engine.settings().seed, Some(42));
}

#[test]
fn confidence_intervals_and_standard_errors_agree_with_independent_distribution() {
    for antithetic in [false, true] {
        let option = contract(OptionType::Call);
        let mut covered = 0;
        let mut estimates = Vec::new();
        for seed in 0..32 {
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: 8192,
                antithetic,
                seed: Some(seed),
                thread_count: 1,
                ..Default::default()
            })
            .unwrap();
            let result = engine.price_with_statistics(&option, &market()).unwrap();
            // Independent BSM moments (plain) and normal-density quadrature (paired).
            let (observations, deviation) = if antithetic {
                (4096.0_f64, 7.286842354)
            } else {
                (8192.0_f64, 13.831466750894657)
            };
            let standard_error = result.standard_error().unwrap();
            assert!((standard_error * observations.sqrt() / deviation - 1.0).abs() < 0.12);
            let (lower, upper) = result.confidence_interval().unwrap();
            covered += usize::from(lower <= 9.227005508154036 && upper >= 9.227005508154036);
            estimates.push(result.price());
        }
        assert!(
            covered >= 26,
            "only {covered} of 32 intervals cover the analytic price"
        );
        let mean = estimates.iter().sum::<f64>() / 32.0;
        assert!((mean - 9.227005508154036).abs() < 0.1);
    }
}

#[test]
fn scaled_statistics_preserve_finite_prices_when_sums_squares_or_discounts_overflow() {
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        seed: Some(42),
        thread_count: 4,
        ..Default::default()
    })
    .unwrap();
    for (amount, rate, sigma) in [
        (1e304, -10.0, 0.5),
        (1e300, 750.0, 0.2),
        (1e-300, -750.0, 0.2),
        (1e-310, 0.0, 0.2),
    ] {
        let option = EuropeanOption::new(
            OptionType::Call,
            amount,
            contract(OptionType::Call).effective_date(),
            contract(OptionType::Call).expiry_date(),
        )
        .unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, rate, sigma).unwrap(),
            amount,
            option.effective_date().into(),
        )
        .unwrap();
        let reference = AnalyticEuropeanEngine.price(&option, &context).unwrap();
        let estimate = engine.price_with_statistics(&option, &context).unwrap();
        assert!(estimate.price() > 0.0 && estimate.standard_error().unwrap() > 0.0);
        assert!(
            (estimate.price() - reference).abs()
                <= 6.0 * estimate.standard_error().unwrap() + 1e-320,
            "{amount} {rate}: {} != {reference}",
            estimate.price()
        );
        assert_eq!(engine.price(&option, &context).unwrap(), estimate.price());
    }
}

#[test]
fn sobol_prices_match_analytic_and_never_claim_sampling_uncertainty() {
    for antithetic in [false, true] {
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: 65_536,
            sampling: MonteCarloSampling::Sobol,
            antithetic,
            seed: Some(42),
            ..Default::default()
        })
        .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let option = contract(direction);
            let reference = AnalyticEuropeanEngine
                .price_with_greeks(&option, &market(), GreeksRequest::all())
                .unwrap();
            let estimate = engine.price_with_statistics(&option, &market()).unwrap();
            assert!((estimate.price() - reference.price()).abs() < 0.005);
            assert_eq!(estimate.standard_error(), None);
            assert_eq!(estimate.confidence_interval(), None);
            let analysis = engine
                .price_with_greeks(
                    &option,
                    &market(),
                    GreeksRequest::all(),
                    NumericalShiftSettings {
                        spot_shift: 1.0,
                        volatility_shift: 0.001,
                        rate_shift: 0.001,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(estimate.price(), analysis.price());
            for (greek, tolerance) in [
                (Greek::Delta, 2e-4),
                (Greek::Gamma, 1e-5),
                (Greek::Speed, 5e-6),
                (Greek::Vega, 5e-5),
                (Greek::Vanna, 1e-5),
                (Greek::Zomma, 5e-6),
                (Greek::Rho, 2e-5),
                (Greek::Theta, 2e-5),
                (Greek::Charm, 5e-7),
                (Greek::Color, 2e-7),
            ] {
                assert!(
                    (analysis.require(greek).unwrap() - reference.require(greek).unwrap()).abs()
                        < tolerance,
                    "{antithetic} {direction:?} {greek:?}"
                );
            }
            let expiry = market()
                .replace(PricingContextChanges {
                    valuation_time: Some(option.expiry_date().into()),
                    ..Default::default()
                })
                .unwrap();
            let known = engine.price_with_statistics(&option, &expiry).unwrap();
            assert_eq!(known.price(), 0.0);
            assert_eq!(known.standard_error(), None);
            assert_eq!(known.confidence_interval(), None);
        }
    }
}

#[test]
fn sobol_path_validation_and_seeded_single_point_preserve_existing_contract() {
    let defaults = MonteCarloSettings::default();
    assert_eq!(defaults.sampling, MonteCarloSampling::PseudoRandom);
    assert!(!defaults.antithetic);
    for antithetic in [false, true] {
        for paths in [1, 2, 1024, 8_388_608] {
            let settings = MonteCarloSettings {
                path_count: paths,
                sampling: MonteCarloSampling::Sobol,
                antithetic,
                seed: Some(u64::MAX),
                ..defaults
            };
            assert_eq!(
                MonteCarloVanillaEngine::new(settings).unwrap().settings(),
                settings
            );
        }
        for paths in [0, 3, 100_000, 8_388_609, 16_777_216] {
            assert!(matches!(
                MonteCarloVanillaEngine::new(MonteCarloSettings {
                    path_count: paths,
                    sampling: MonteCarloSampling::Sobol,
                    antithetic,
                    ..defaults
                })
                .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: 1,
            sampling: MonteCarloSampling::Sobol,
            antithetic,
            seed: Some(42),
            ..defaults
        })
        .unwrap();
        let option = EuropeanOption::new(
            OptionType::Call,
            90.0,
            contract(OptionType::Call).effective_date(),
            contract(OptionType::Call).expiry_date(),
        )
        .unwrap();
        // Independent SciPy ndtri reference.
        let first = -0.10364940758940244_f64;
        let expected = (-0.05_f64).exp() * (100.0 * (0.01 + 0.2 * first).exp() - 90.0);
        assert!((engine.price(&option, &market()).unwrap() - expected).abs() < 1e-11);
        let pair = MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: 2,
            antithetic: true,
            ..engine.settings()
        })
        .unwrap();
        let expected = (-0.05_f64).exp()
            * (50.0 * ((0.01 + 0.2 * first).exp() + (0.01 - 0.2 * first).exp()) - 90.0);
        assert!((pair.price(&option, &market()).unwrap() - expected).abs() < 1e-11);
    }
}

#[test]
fn antithetic_counts_keep_the_unpaired_path_and_use_pair_uncertainty() {
    let option = EuropeanOption::new(
        OptionType::Call,
        1e-100,
        contract(OptionType::Call).effective_date(),
        contract(OptionType::Call).expiry_date(),
    )
    .unwrap();
    let engine = |paths, antithetic| {
        MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: paths,
            antithetic,
            seed: Some(42),
            thread_count: 1,
            ..Default::default()
        })
        .unwrap()
    };
    assert_eq!(
        engine(1, true).price(&option, &market()).unwrap(),
        engine(1, false).price(&option, &market()).unwrap()
    );
    for samples in [1, 2, 3, 128, 129, 256, 257, 513] {
        let positive = f64::from(samples)
            * engine(samples, false).price(&option, &market()).unwrap()
            - if samples == 1 {
                0.0
            } else {
                f64::from(samples - 1)
                    * engine(samples - 1, false)
                        .price(&option, &market())
                        .unwrap()
            };
        // Opposite terminal log-prices sum to twice the drift (including discount).
        let opposite = 10_000.0 * (-0.08_f64).exp() / positive;
        let paired = engine(2 * samples, true)
            .price_with_statistics(&option, &market())
            .unwrap();
        let odd = engine(2 * samples - 1, true)
            .price_with_statistics(&option, &market())
            .unwrap();
        assert!(
            (f64::from(2 * samples) * paired.price()
                - f64::from(2 * samples - 1) * odd.price()
                - opposite)
                .abs()
                < 1e-7
        );
        assert_eq!(odd.standard_error(), None);
        assert_eq!(odd.confidence_interval(), None);
        assert_eq!(paired.standard_error().is_some(), samples > 1);
    }
    let first_pair = engine(2, true).price(&option, &market()).unwrap();
    let two_pairs = engine(4, true)
        .price_with_statistics(&option, &market())
        .unwrap();
    assert!(
        (two_pairs.standard_error().unwrap() - (first_pair - two_pairs.price()).abs()).abs()
            < 1e-12
    );
}

#[test]
fn every_sampling_combination_replays_analytics_across_threads_and_failures() {
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            for paths in [1, 2, 256, 512, 1024] {
                for seed in [0, 42, u64::MAX] {
                    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                        path_count: paths,
                        seed: Some(seed),
                        sampling,
                        antithetic,
                        thread_count: 1,
                        ..Default::default()
                    })
                    .unwrap();
                    for direction in [OptionType::Call, OptionType::Put] {
                        let option = contract(direction);
                        let expected = engine.price_with_statistics(&option, &market()).unwrap();
                        for thread_count in [0, 2, 4, i32::MAX] {
                            let parallel = MonteCarloVanillaEngine::new(MonteCarloSettings {
                                thread_count,
                                ..engine.settings()
                            })
                            .unwrap();
                            assert_eq!(
                                parallel.price_with_statistics(&option, &market()).unwrap(),
                                expected
                            );
                        }
                    }
                }
            }
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: 1024,
                seed: Some(42),
                sampling,
                antithetic,
                thread_count: 1,
                ..Default::default()
            })
            .unwrap();
            let option = contract(OptionType::Call);
            let context = market();
            let expected = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            let bounds = ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.4,
                ..Default::default()
            };
            let sigma =
                implied_volatility(&engine, &option, &context, expected.price(), bounds).unwrap();
            assert!((sigma - 0.2).abs() < 1e-8);
            for thread_count in [0, 2, 4] {
                let parallel = MonteCarloVanillaEngine::new(MonteCarloSettings {
                    thread_count,
                    ..engine.settings()
                })
                .unwrap();
                assert_eq!(
                    parallel
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default()
                        )
                        .unwrap(),
                    expected
                );
                assert_eq!(
                    implied_volatility(&parallel, &option, &context, expected.price(), bounds)
                        .unwrap(),
                    sigma
                );
            }
            let shifted = context
                .replace(PricingContextChanges {
                    spot_price: Some(101.0),
                    ..Default::default()
                })
                .unwrap();
            let down = context
                .replace(PricingContextChanges {
                    spot_price: Some(99.0),
                    ..Default::default()
                })
                .unwrap();
            let analysis = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    NumericalShiftSettings {
                        spot_shift: 1.0,
                        ..Default::default()
                    },
                )
                .unwrap();
            let up_price = engine.price(&option, &shifted).unwrap();
            let down_price = engine.price(&option, &down).unwrap();
            assert!(
                (analysis.require(Greek::Delta).unwrap() - (up_price - down_price) / 2.0).abs()
                    < 1e-10
            );
            assert!(
                (analysis.require(Greek::Gamma).unwrap()
                    - (up_price - 2.0 * analysis.price() + down_price))
                    .abs()
                    < 1e-10
            );
            assert!(matches!(
                engine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Rho]),
                        NumericalShiftSettings {
                            rate_shift: 1000.0,
                            ..Default::default()
                        }
                    )
                    .unwrap_err(),
                Error::ComputationFailure(_)
            ));
            assert_eq!(engine.price(&option, &context).unwrap(), expected.price());
            let unseeded = MonteCarloVanillaEngine::new(MonteCarloSettings {
                seed: None,
                ..engine.settings()
            })
            .unwrap();
            let linear = EuropeanOption::new(
                OptionType::Call,
                1e-100,
                option.effective_date(),
                option.expiry_date(),
            )
            .unwrap();
            let analysis = unseeded
                .price_with_greeks(
                    &linear,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            assert!(
                (analysis.require(Greek::Delta).unwrap() - analysis.price() / 100.0).abs() < 1e-9
            );
            assert!(analysis.require(Greek::Gamma).unwrap().abs() < 1e-7);
            assert_eq!(unseeded.settings().seed, None);
            std::thread::scope(|scope| {
                let tasks: Vec<_> = (0..3)
                    .map(|_| scope.spawn(|| engine.price(&option, &context).unwrap()))
                    .collect();
                for task in tasks {
                    assert_eq!(task.join().unwrap(), expected.price());
                }
            });
        }
    }
}

#[test]
fn antithetic_companion_failures_preserve_engine_and_inputs() {
    let option = contract(OptionType::Call);
    let context = market()
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.5, 0.0, 1.0).unwrap()),
            spot_price: Some(1.7e308),
            ..Default::default()
        })
        .unwrap();
    let one = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        ..Default::default()
    })
    .unwrap();
    assert!(one.price(&option, &context).unwrap().is_finite());
    let pair = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 2,
        ..one.settings()
    })
    .unwrap();
    let baseline = pair.price(&option, &market()).unwrap();
    assert!(matches!(
        pair.price(&option, &context).unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(pair.price(&option, &market()).unwrap(), baseline);
    assert!(pair.settings().antithetic);
}
