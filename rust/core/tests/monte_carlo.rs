use kiyosi_core::*;

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
        assert_eq!(
            MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: paths,
                ..Default::default()
            })
            .unwrap_err()
            .category,
            ErrorCategory::InvalidParameter
        );
    }
    for steps in [i32::MIN, 0, 1, 10_001, i32::MAX] {
        assert_eq!(
            MonteCarloVanillaEngine::new(MonteCarloSettings {
                step_count: steps,
                ..Default::default()
            })
            .unwrap_err()
            .category,
            ErrorCategory::InvalidParameter
        );
    }
    assert_eq!(
        MonteCarloVanillaEngine::new(MonteCarloSettings {
            thread_count: -1,
            ..Default::default()
        })
        .unwrap_err()
        .category,
        ErrorCategory::InvalidParameter
    );
    for paths in [1, 10_000_000] {
        for steps in [2, 10_000] {
            let settings = MonteCarloSettings {
                path_count: paths,
                step_count: steps,
                seed: Some(u64::MAX),
                thread_count: i32::MAX,
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
    assert_eq!(
        implied_volatility(&engine, &option, &at(expiry), 0.0, Default::default())
            .unwrap_err()
            .category,
        ErrorCategory::UnsupportedOperation
    );
    for time in [
        Timestamp::new(Date::new(2024, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap(),
        Timestamp::new(option.expiry_date(), 0, 0, 0, 1).unwrap(),
    ] {
        assert_eq!(
            engine.price(&option, &at(time)).unwrap_err().category,
            ErrorCategory::InvalidTimeRange
        );
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
    assert_eq!(
        engine.price(&option, &node_context).unwrap_err().category,
        ErrorCategory::UnsupportedOperation
    );
    assert_eq!(
        engine
            .price_with_greeks(
                &option,
                &node_context,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err()
            .category,
        ErrorCategory::UnsupportedOperation
    );
    assert_eq!(
        numerical_greeks(
            &engine,
            &option,
            &node_context,
            GreeksRequest::default(),
            Default::default()
        )
        .unwrap_err()
        .category,
        ErrorCategory::UnsupportedOperation
    );
    for quote in [0.0, f64::NAN] {
        assert_eq!(
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
            .unwrap_err()
            .category,
            ErrorCategory::UnsupportedOperation
        );
    }
    assert_eq!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err()
            .category,
        ErrorCategory::InvalidParameter
    );
    let baseline = engine.price(&option, &context).unwrap();
    assert_eq!(
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
            .unwrap_err()
            .category,
        ErrorCategory::InvalidResult
    );
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
    let option = contract(OptionType::Call);
    let mut covered = 0;
    let mut estimates = Vec::new();
    for seed in 0..32 {
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: 8192,
            seed: Some(seed),
            thread_count: 1,
            ..Default::default()
        })
        .unwrap();
        let result = engine.price_with_statistics(&option, &market()).unwrap();
        // Independent BSM discounted payoff standard deviation is about 13.8315.
        let standard_error = result.standard_error().unwrap();
        assert!((standard_error * 8192.0_f64.sqrt() / 13.831466750894657 - 1.0).abs() < 0.12);
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
        let reference = AnalyticVanillaEngine.price(&option, &context).unwrap();
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
