use kiyosi::*;

fn option(direction: OptionType, strike: f64) -> AmericanOption {
    AmericanOption::new(
        direction,
        strike,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
    .unwrap()
}

fn market(rate: f64, dividend: f64, volatility: f64, spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility).unwrap(),
        spot,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
    )
    .unwrap()
}

#[test]
fn american_path_storage_limit_rejects_large_grids_before_sampling() {
    let put = option(OptionType::Put, 100.0);
    let context = market(0.05, 0.0, 0.2, 100.0);
    for antithetic in [false, true] {
        let engine = MonteCarloVanillaEngine::with_step_count(
            MonteCarloSettings {
                path_count: 15_001,
                seed: Some(42),
                antithetic,
                ..Default::default()
            },
            10_000,
        )
        .unwrap();
        assert!(matches!(
            engine.price(&put, &context),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            engine.price_with_greeks(
                &put,
                &context,
                GreeksRequest::new([Greek::Delta]),
                Default::default()
            ),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            implied_volatility(&engine, &put, &context, 1.0, Default::default()),
            Err(Error::InvalidInput(_))
        ));
        let expiry = context
            .replace(PricingContextChanges {
                valuation_time: Some(put.expiry_time()),
                spot_price: Some(90.0),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(engine.price(&put, &expiry).unwrap(), 10.0);
        let european = EuropeanOption::new(
            OptionType::Call,
            100.0,
            put.effective_time(),
            put.expiry_time(),
        )
        .unwrap();
        let expected = engine.price(&european, &context).unwrap();
        assert!(expected > 0.0);
        assert_eq!(
            engine
                .price(&option(OptionType::Call, 100.0), &context)
                .unwrap(),
            expected
        );
        assert_eq!(engine.settings().seed, Some(42));
    }
}

#[test]
fn nonfinite_continuation_regressions_fail_without_changing_the_engine() {
    let context = market(-0.1, 0.0, 0.2, 100.0);
    let engine = MonteCarloVanillaEngine::with_step_count(
        MonteCarloSettings {
            path_count: 128,
            seed: Some(42),
            ..Default::default()
        },
        4,
    )
    .unwrap();
    let ordinary = option(OptionType::Call, 100.0);
    let before = engine.price(&ordinary, &context).unwrap();
    for strike in [1e-100, 1e-300] {
        let extreme = option(OptionType::Call, strike);
        assert!(matches!(
            engine.price(&extreme, &context),
            Err(Error::ComputationFailure(_))
        ));
        assert!(matches!(
            engine.price_with_greeks(
                &extreme,
                &context,
                GreeksRequest::new([Greek::Delta]),
                Default::default()
            ),
            Err(Error::ComputationFailure(_))
        ));
    }
    assert_eq!(engine.price(&ordinary, &context).unwrap(), before);
    assert_eq!(engine.settings().seed, Some(42));
}

#[test]
fn american_put_includes_immediate_exercise_and_validates_steps_after_expiry() {
    let put = option(OptionType::Put, 100.0);
    let context = market(0.1, 0.0, 0.1, 50.0);
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 4096,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    assert!((engine.price(&put, &context).unwrap() - 50.0).abs() < 1e-10);
    let engine = MonteCarloVanillaEngine::with_step_count(engine.settings(), 1).unwrap();
    assert!(matches!(
        engine.price(&put, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(put.expiry_time()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(engine.price(&put, &expiry).unwrap(), 50.0);
}

#[test]
fn sparse_and_rank_deficient_paths_preserve_exercise_and_currency_scaling() {
    for paths in [1, 3, 4, 127] {
        for antithetic in [false, true] {
            let engine = MonteCarloVanillaEngine::with_step_count(
                MonteCarloSettings {
                    path_count: paths,
                    seed: Some(42),
                    antithetic,
                    ..Default::default()
                },
                4,
            )
            .unwrap();
            // Deterministic decreasing spots leave a constant regression basis.
            let price = engine
                .price(
                    &option(OptionType::Put, 100.0),
                    &market(0.0, 0.1, f64::MIN_POSITIVE, 50.0),
                )
                .unwrap();
            assert!((price - (100.0 - 50.0 * (-0.1_f64).exp())).abs() < 1e-12);
            for scale in [0.01, 100.0] {
                let scaled = engine
                    .price(
                        &option(OptionType::Put, 100.0 * scale),
                        &market(0.0, 0.1, f64::MIN_POSITIVE, 50.0 * scale),
                    )
                    .unwrap();
                assert!((scaled / scale - price).abs() < 1e-11);
            }
        }
    }
    // The constant fit must still exercise when future continuation is lower.
    for direction in [OptionType::Call, OptionType::Put] {
        let (rate, dividend, spot) = if direction == OptionType::Call {
            (-0.1, 0.0, 200.0)
        } else {
            (0.1, 0.0, 50.0)
        };
        let engine = MonteCarloVanillaEngine::with_step_count(
            MonteCarloSettings {
                path_count: 4,
                seed: Some(42),
                ..Default::default()
            },
            4,
        )
        .unwrap();
        assert!(
            (engine
                .price(
                    &option(direction, 100.0),
                    &market(rate, dividend, 1e-12, spot)
                )
                .unwrap()
                - (spot - 100.0_f64).abs())
            .abs()
                < 1e-10
        );
    }
}

#[test]
fn sampling_combinations_preserve_threads_seeds_and_exact_odd_paths() {
    let put = option(OptionType::Put, 100.0);
    let context = market(0.05, 0.02, 0.2, 100.0);
    let reference = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 2000 })
        .unwrap()
        .price(&put, &context)
        .unwrap();
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let engine = MonteCarloVanillaEngine::with_step_count(
                MonteCarloSettings {
                    path_count: 32768,
                    seed: Some(42),
                    thread_count: 1,
                    sampling,
                    antithetic,
                },
                49,
            )
            .unwrap();
            let price = engine.price(&put, &context).unwrap();
            assert!(
                (price - reference).abs() < 0.3,
                "{sampling:?}, {antithetic}: {price} != {reference}"
            );
            for threads in [0, 2, 4] {
                let parallel = MonteCarloVanillaEngine::with_step_count(
                    MonteCarloSettings {
                        thread_count: threads,
                        ..engine.settings()
                    },
                    engine.step_count(),
                )
                .unwrap();
                assert_eq!(parallel.price(&put, &context).unwrap(), price);
            }
            let changed = MonteCarloVanillaEngine::with_step_count(
                MonteCarloSettings {
                    seed: Some(u64::MAX),
                    ..engine.settings()
                },
                engine.step_count(),
            )
            .unwrap();
            assert_ne!(changed.price(&put, &context).unwrap(), price);
        }
    }
    for steps in [3, 7, 50] {
        for paths in [1, 3, 511, 513, 19999] {
            let engine = MonteCarloVanillaEngine::with_step_count(
                MonteCarloSettings {
                    path_count: paths,
                    seed: Some(42),
                    antithetic: true,
                    ..Default::default()
                },
                steps,
            )
            .unwrap();
            let odd = engine.price(&put, &context).unwrap();
            let even = MonteCarloVanillaEngine::with_step_count(
                MonteCarloSettings {
                    path_count: paths + 1,
                    ..engine.settings()
                },
                engine.step_count(),
            )
            .unwrap()
            .price(&put, &context)
            .unwrap();
            assert!(odd.is_finite());
            if paths >= 511 {
                assert_ne!(odd, even);
            }
        }
    }
}

#[test]
fn terminal_only_call_preserves_price_and_immediate_exercise_floor() {
    let call = option(OptionType::Call, 100.0);
    let european = EuropeanOption::new(
        OptionType::Call,
        100.0,
        call.effective_time(),
        call.expiry_time(),
    )
    .unwrap();
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            for (rate, dividend) in [(0.0, 0.0), (0.05, -0.02)] {
                let context = market(rate, dividend, 0.2, 100.0);
                let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                    path_count: 1024,
                    seed: Some(42),
                    sampling,
                    antithetic,
                    ..Default::default()
                })
                .unwrap();
                let price = engine.price(&european, &context).unwrap();
                for steps in [3, 50, 10000] {
                    let other =
                        MonteCarloVanillaEngine::with_step_count(engine.settings(), steps).unwrap();
                    assert_eq!(other.price(&call, &context).unwrap(), price);
                }
                let deep = market(0.0, 0.0, 0.2, 2000.0);
                let one = MonteCarloVanillaEngine::with_step_count(
                    MonteCarloSettings {
                        path_count: 1,
                        ..engine.settings()
                    },
                    engine.step_count(),
                )
                .unwrap();
                assert_eq!(
                    one.price(&call, &deep).unwrap(),
                    one.price(&european, &deep).unwrap().max(1900.0)
                );
            }
        }
    }
}

#[test]
fn shared_sample_greeks_and_implied_volatility_preserve_inputs() {
    let put = option(OptionType::Put, 100.0);
    let context = market(0.05, 0.02, 0.2, 100.0);
    let shifts = NumericalShiftSettings {
        spot_shift: Some(MonteCarloVanillaEngine::default_american_spot_shift(
            &context,
        )),
        volatility_shift: 0.02,
        rate_shift: 0.01,
        time_shift_days: 2,
    };
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let engine = MonteCarloVanillaEngine::with_step_count(
                MonteCarloSettings {
                    path_count: 1024,
                    seed: Some(42),
                    sampling,
                    antithetic,
                    ..Default::default()
                },
                6,
            )
            .unwrap();
            let all = engine
                .price_with_greeks(&put, &context, GreeksRequest::all(), shifts)
                .unwrap();
            let scalar =
                numerical_greeks(&engine, &put, &context, GreeksRequest::all(), shifts).unwrap();
            assert_eq!(all.values(), scalar.values());
            assert_eq!(all.price(), engine.price(&put, &context).unwrap());
            for greek in Greek::ALL {
                let selected = engine
                    .price_with_greeks(&put, &context, GreeksRequest::new([greek]), shifts)
                    .unwrap();
                assert_eq!(selected.get(greek), all.get(greek));
            }
            let solved = implied_volatility(
                &engine,
                &put,
                &context,
                all.price(),
                ImpliedVolatilitySettings {
                    lower_bound: 0.15,
                    upper_bound: 0.25,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!((solved - 0.2).abs() < 1e-7, "{solved}");
            assert!(matches!(
                implied_volatility(&engine, &put, &context, -1.0, Default::default()).unwrap_err(),
                Error::RootNotBracketed(_)
            ));
            let unavailable = engine
                .price_with_greeks(
                    &put,
                    &context,
                    GreeksRequest::new([Greek::Gamma]),
                    NumericalShiftSettings {
                        spot_shift: Some(200.0),
                        ..shifts
                    },
                )
                .unwrap();
            assert_eq!(unavailable.get(Greek::Gamma), None);
            assert_eq!(unavailable.price(), all.price());
            assert_eq!(engine.price(&put, &context).unwrap(), all.price());
            assert_eq!(
                context.model_parameters().as_bsm().unwrap().volatility(),
                0.2
            );
        }
    }
    let unseeded = MonteCarloVanillaEngine::with_step_count(
        MonteCarloSettings {
            path_count: 1024,
            ..Default::default()
        },
        6,
    )
    .unwrap();
    assert!(
        unseeded
            .price_with_greeks(&put, &context, GreeksRequest::all(), shifts)
            .is_ok()
    );
    assert_eq!(unseeded.settings().seed, None);
}

#[test]
fn rate_stencils_crossing_zero_keep_one_exercise_estimator() {
    let call = option(OptionType::Call, 100.0);
    let engine = MonteCarloVanillaEngine::with_step_count(
        MonteCarloSettings {
            path_count: 2048,
            seed: Some(42),
            ..Default::default()
        },
        6,
    )
    .unwrap();
    let shift = 1.0 / 8192.0;
    for rate in [-1.0 / 16384.0, 0.0, 1.0 / 16384.0] {
        let context = market(rate, 0.0, 0.2, 100.0);
        let result = engine
            .price_with_greeks(
                &call,
                &context,
                GreeksRequest::new([Greek::Rho]),
                NumericalShiftSettings {
                    rate_shift: shift,
                    ..Default::default()
                },
            )
            .unwrap();
        // A subnormal positive dividend forces the grid without changing BSM coefficients.
        let up = engine
            .price(&call, &market(rate + shift, f64::from_bits(1), 0.2, 100.0))
            .unwrap();
        let down = engine
            .price(&call, &market(rate - shift, f64::from_bits(1), 0.2, 100.0))
            .unwrap();
        assert!(
            (result.require(Greek::Rho).unwrap() - (up - down) / (200.0 * shift)).abs() < 1e-10
        );
        assert_eq!(result.price(), engine.price(&call, &context).unwrap());
    }
}

#[test]
fn validation_order_expiry_and_failed_bumps_preserve_valid_inputs() {
    let put = option(OptionType::Put, 100.0);
    let context = market(0.05, 0.02, 0.2, 100.0);
    let engine = MonteCarloVanillaEngine::with_step_count(
        MonteCarloSettings {
            path_count: 127,
            seed: Some(42),
            ..Default::default()
        },
        6,
    )
    .unwrap();
    let price = engine.price(&put, &context).unwrap();
    let expiry = put.expiry_time();
    let at_expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(expiry),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&put, &at_expiry, GreeksRequest::all(), Default::default())
        .unwrap();
    assert_eq!(result.price(), 0.0);
    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
        assert_eq!(result.get(greek), None);
    }
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        if result.supported().has(greek) {
            assert_eq!(result.require(greek).unwrap(), 0.0);
        }
    }
    assert!(matches!(
        implied_volatility(&engine, &put, &at_expiry, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    for time in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        Timestamp::new(put.expiry_time().date(), 0, 0, 0, 1).unwrap(),
    ] {
        let shifted = context
            .replace(PricingContextChanges {
                valuation_time: Some(time),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&put, &shifted).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let curve = RateTermStructure::new(put.effective_time(), vec![expiry], vec![0.95]).unwrap();
    let model = context
        .model_parameters()
        .as_bsm()
        .unwrap()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let node = context
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
            valuation_time: Some(expiry),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&put, &node).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(&put, &node, GreeksRequest::new([]), Default::default())
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        implied_volatility(&engine, &put, &node, f64::NAN, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(&put, &context, GreeksRequest::new([]), Default::default())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &put,
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
    assert!(matches!(
        engine
            .price(&put, &market(0.05, 0.02, 1000.0, 100.0))
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(engine.price(&put, &context).unwrap(), price);
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
}

#[test]
fn automatic_spot_bumps_match_explicit_product_defaults() {
    let context = market(0.05, 0.03, 0.2, 100.0);
    let engine = MonteCarloVanillaEngine::with_step_count(
        MonteCarloSettings {
            path_count: 8192,
            seed: Some(42),
            thread_count: 1,
            ..Default::default()
        },
        19,
    )
    .unwrap();
    let put = option(OptionType::Put, 100.0);
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let automatic = engine
        .price_with_greeks(&put, &context, request, Default::default())
        .unwrap();
    let explicit = engine
        .price_with_greeks(
            &put,
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(8.0),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(automatic, explicit);
    let generic = numerical_greeks(&engine, &put, &context, request, Default::default()).unwrap();
    assert_eq!(automatic, generic);
    let european = EuropeanOption::new(
        OptionType::Put,
        100.0,
        put.effective_time(),
        put.expiry_time(),
    )
    .unwrap();
    assert_eq!(
        engine
            .price_with_greeks(&european, &context, request, Default::default())
            .unwrap(),
        engine
            .price_with_greeks(
                &european,
                &context,
                request,
                NumericalShiftSettings {
                    spot_shift: Some(0.01),
                    ..Default::default()
                }
            )
            .unwrap()
    );
}

#[test]
fn monte_carlo_step_counts_are_intervals_with_two_required_for_american_exercise() {
    let context = market(0.05, 0.02, 0.2, 100.0);
    let put = option(OptionType::Put, 100.0);
    let settings = MonteCarloSettings {
        path_count: 64,
        seed: Some(42),
        ..Default::default()
    };
    let two = MonteCarloVanillaEngine::with_step_count(settings, 2).unwrap();
    let price = two.price(&put, &context).unwrap();
    assert!(price > 0.0 && price < 100.0);
    assert_eq!(two.step_count(), 2);
    assert_eq!(
        MonteCarloAsianEngine::with_step_count(settings, 2)
            .unwrap()
            .step_count(),
        2
    );
    let one = MonteCarloVanillaEngine::with_step_count(settings, 1).unwrap();
    assert!(matches!(
        one.price(&put, &context),
        Err(Error::InvalidInput(_))
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(put.expiry_time()),
            spot_price: Some(90.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(one.price(&put, &expiry).unwrap(), 10.0);
}
