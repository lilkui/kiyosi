use kiyosi_core::*;

fn option(direction: OptionType, strike: f64) -> AmericanOption {
    AmericanOption::new(
        direction,
        strike,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
    .unwrap()
}

fn market(rate: f64, dividend: f64, volatility: f64, spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility).unwrap(),
        spot,
        Date::new(2025, 1, 1).unwrap().into(),
    )
    .unwrap()
}

#[test]
fn nonfinite_continuation_regressions_fail_without_changing_the_engine() {
    let context = market(-0.1, 0.0, 0.2, 100.0);
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 128,
        step_count: 5,
        seed: Some(42),
        ..Default::default()
    })
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
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        step_count: 2,
        ..engine.settings()
    })
    .unwrap();
    assert!(matches!(
        engine.price(&put, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(put.expiry_date().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(engine.price(&put, &expiry).unwrap(), 50.0);
}

#[test]
fn sparse_and_rank_deficient_paths_preserve_exercise_and_currency_scaling() {
    for paths in [1, 3, 4, 127] {
        for antithetic in [false, true] {
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: paths,
                step_count: 5,
                seed: Some(42),
                antithetic,
                ..Default::default()
            })
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
        let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
            path_count: 4,
            step_count: 5,
            seed: Some(42),
            ..Default::default()
        })
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
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: 32768,
                step_count: 50,
                seed: Some(42),
                thread_count: 1,
                sampling,
                antithetic,
            })
            .unwrap();
            let price = engine.price(&put, &context).unwrap();
            assert!(
                (price - reference).abs() < 0.3,
                "{sampling:?}, {antithetic}: {price} != {reference}"
            );
            for threads in [0, 2, 4] {
                let parallel = MonteCarloVanillaEngine::new(MonteCarloSettings {
                    thread_count: threads,
                    ..engine.settings()
                })
                .unwrap();
                assert_eq!(parallel.price(&put, &context).unwrap(), price);
            }
            let changed = MonteCarloVanillaEngine::new(MonteCarloSettings {
                seed: Some(u64::MAX),
                ..engine.settings()
            })
            .unwrap();
            assert_ne!(changed.price(&put, &context).unwrap(), price);
        }
    }
    for steps in [3, 7, 50] {
        for paths in [1, 3, 511, 513, 19999] {
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: paths,
                step_count: steps,
                seed: Some(42),
                antithetic: true,
                ..Default::default()
            })
            .unwrap();
            let odd = engine.price(&put, &context).unwrap();
            let even = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: paths + 1,
                ..engine.settings()
            })
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
        call.effective_date(),
        call.expiry_date(),
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
                    let other = MonteCarloVanillaEngine::new(MonteCarloSettings {
                        step_count: steps,
                        ..engine.settings()
                    })
                    .unwrap();
                    assert_eq!(other.price(&call, &context).unwrap(), price);
                }
                let deep = market(0.0, 0.0, 0.2, 2000.0);
                let one = MonteCarloVanillaEngine::new(MonteCarloSettings {
                    path_count: 1,
                    ..engine.settings()
                })
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
        spot_shift: MonteCarloVanillaEngine::default_american_spot_shift(&context),
        volatility_shift: 0.02,
        rate_shift: 0.01,
        time_shift_days: 2,
    };
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
                path_count: 1024,
                step_count: 7,
                seed: Some(42),
                sampling,
                antithetic,
                ..Default::default()
            })
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
                        spot_shift: 200.0,
                        ..shifts
                    },
                )
                .unwrap();
            assert_eq!(unavailable.get(Greek::Gamma), None);
            assert_eq!(unavailable.price(), all.price());
            assert_eq!(engine.price(&put, &context).unwrap(), all.price());
            assert_eq!(context.model_parameters().volatility(), 0.2);
        }
    }
    let unseeded = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 1024,
        step_count: 7,
        ..Default::default()
    })
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
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 2048,
        step_count: 7,
        seed: Some(42),
        ..Default::default()
    })
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
    let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 127,
        step_count: 7,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let price = engine.price(&put, &context).unwrap();
    let expiry = put.expiry_date().start_of_day();
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
    assert!(
        Greek::ALL
            .into_iter()
            .all(|greek| result.get(greek).is_none())
    );
    assert!(matches!(
        implied_volatility(&engine, &put, &at_expiry, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    for time in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        Timestamp::new(put.expiry_date(), 0, 0, 0, 1).unwrap(),
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
    let curve =
        RateTermStructure::new(put.effective_date().into(), vec![expiry], vec![0.95]).unwrap();
    let model = context
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let node = context
        .replace(PricingContextChanges {
            model_parameters: Some(model),
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
            .price_with_greeks(&put, &node, GreeksRequest::default(), Default::default())
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        implied_volatility(&engine, &put, &node, f64::NAN, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(&put, &context, GreeksRequest::default(), Default::default())
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
    assert_eq!(context.model_parameters().volatility(), 0.2);
}
