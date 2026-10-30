use kiyosi_core::*;

fn date(month: u32, day: u32) -> Date {
    Date::new(2025, month, day).unwrap()
}

fn market(spot: f64, sigma: f64, valuation: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, sigma).unwrap(),
        spot,
        valuation,
    )
    .unwrap()
}

#[test]
fn geometric_exact_sampling_matches_analytic_and_ignores_steps() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    for averaging in [start, date(5, 1), end] {
        let option =
            GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, averaging, end, 0.0)
                .unwrap();
        let context = market(100.0, 0.2, start.into());
        let settings = MonteCarloSettings {
            path_count: 16384,
            step_count: 2,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            seed: Some(42),
            ..Default::default()
        };
        let engine = MonteCarloAsianEngine::new(settings).unwrap();
        let price = engine.price(&option, &context).unwrap();
        let reference = AnalyticGeometricAveragePriceEngine
            .price(&option, &context)
            .unwrap();
        assert!((price - reference).abs() < 0.01, "{price} vs {reference}");
        let changed = MonteCarloAsianEngine::new(MonteCarloSettings {
            step_count: 10000,
            thread_count: 1,
            ..settings
        })
        .unwrap();
        assert_eq!(price, changed.price(&option, &context).unwrap());
    }
}

#[test]
fn arithmetic_retains_conditioned_integration_and_converges() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 2.0, start, start, end, 0.0).unwrap();
    let settings = MonteCarloSettings {
        path_count: 65536,
        step_count: 64,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        ..Default::default()
    };
    for (spot, expected) in [(1.9, 0.193174), (2.0, 0.246416), (2.1, 0.306220)] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.05, 0.0, 0.5).unwrap(),
            spot,
            start.into(),
        )
        .unwrap();
        let fine = MonteCarloAsianEngine::new(settings)
            .unwrap()
            .price(&option, &context)
            .unwrap();
        let coarse = MonteCarloAsianEngine::new(MonteCarloSettings {
            step_count: 2,
            ..settings
        })
        .unwrap()
        .price(&option, &context)
        .unwrap();
        assert!((fine - expected).abs() < 0.001, "{fine} vs {expected}");
        assert!((fine - expected).abs() < (coarse - expected).abs());
    }
}

#[test]
fn native_greeks_use_all_future_spots_and_fixed_history() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 32768,
        step_count: 32,
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for (averaging, valuation, history) in [
            (start, start, 0.0),
            (date(5, 1), start, 0.0),
            (start, date(7, 1), 105.0),
        ] {
            let a =
                ArithmeticAveragePriceOption::new(direction, 100.0, start, averaging, end, history)
                    .unwrap();
            let g =
                GeometricAveragePriceOption::new(direction, 100.0, start, averaging, end, history)
                    .unwrap();
            let context = market(100.0, 0.2, valuation.into());
            let result = engine
                .price_with_greeks(&a, &context, GreeksRequest::all())
                .unwrap();
            assert_eq!(result.price(), engine.price(&a, &context).unwrap());
            for h in [0.05, 0.025] {
                let up = engine
                    .price(&a, &market(100.0 + h, 0.2, valuation.into()))
                    .unwrap();
                let down = engine
                    .price(&a, &market(100.0 - h, 0.2, valuation.into()))
                    .unwrap();
                assert!(
                    (result.require(Greek::Delta).unwrap() - (up - down) / (2.0 * h)).abs()
                        < 0.00002
                );
                assert!(
                    (result.require(Greek::Gamma).unwrap()
                        - (up - 2.0 * result.price() + down) / (h * h))
                        .abs()
                        < 0.00002
                );
            }
            for greek in Greek::ALL {
                assert_eq!(
                    result.has(greek),
                    matches!(greek, Greek::Delta | Greek::Gamma)
                );
            }
            let actual = engine
                .price_with_greeks(&g, &context, GreeksRequest::all())
                .unwrap();
            let expected = AnalyticGeometricAveragePriceEngine
                .price_with_greeks(&g, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            for (greek, budget) in [(Greek::Delta, 0.002), (Greek::Gamma, 0.001)] {
                assert!(
                    (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs()
                        < budget
                );
            }
            let scalar = numerical_greeks(
                &engine,
                &a,
                &context,
                GreeksRequest::new([Greek::Vega, Greek::Theta]),
                Default::default(),
            )
            .unwrap();
            assert!(scalar.has(Greek::Vega));
            assert_eq!(scalar.has(Greek::Theta), averaging != valuation);
            let quote = result.price();
            let solved = implied_volatility(
                &engine,
                &a,
                &market(100.0, 0.35, valuation.into()),
                quote,
                ImpliedVolatilitySettings {
                    lower_bound: 0.1,
                    upper_bound: 0.4,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!((solved - 0.2).abs() < 1e-6);
            assert_eq!(a.realized_average(), history);
            assert_eq!(context.model_parameters().volatility(), 0.2);
        }
    }
}

#[test]
fn sampling_exact_counts_threads_and_shared_calls_agree() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let a =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    let g =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    let context = market(100.0, 0.2, start.into());
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let settings = MonteCarloSettings {
                path_count: if sampling == MonteCarloSampling::Sobol {
                    1024
                } else {
                    1025
                },
                step_count: 16,
                seed: Some(42),
                sampling,
                antithetic,
                thread_count: 1,
            };
            let serial = MonteCarloAsianEngine::new(settings).unwrap();
            let parallel = MonteCarloAsianEngine::new(MonteCarloSettings {
                thread_count: 4,
                ..settings
            })
            .unwrap();
            let expected = serial
                .price_with_greeks(&a, &context, GreeksRequest::all())
                .unwrap();
            assert_eq!(
                expected,
                parallel
                    .price_with_greeks(&a, &context, GreeksRequest::all())
                    .unwrap()
            );
            assert_eq!(
                serial.price(&g, &context).unwrap(),
                parallel.price(&g, &context).unwrap()
            );
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..4)
                    .map(|_| {
                        scope.spawn(|| {
                            parallel
                                .price_with_greeks(&a, &context, GreeksRequest::all())
                                .unwrap()
                        })
                    })
                    .collect();
                for handle in handles {
                    assert_eq!(handle.join().unwrap(), expected);
                }
            });
        }
    }
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let settings = MonteCarloSettings {
            path_count: 1,
            seed: Some(42),
            sampling,
            ..Default::default()
        };
        let plain = MonteCarloAsianEngine::new(settings).unwrap();
        let paired = MonteCarloAsianEngine::new(MonteCarloSettings {
            antithetic: true,
            ..settings
        })
        .unwrap();
        assert_eq!(
            plain.price(&a, &context).unwrap(),
            paired.price(&a, &context).unwrap()
        );
        assert_eq!(
            plain.price(&g, &context).unwrap(),
            paired.price(&g, &context).unwrap()
        );
    }
}

#[test]
fn history_life_expiry_and_validation_precedence_preserve_state() {
    let start = date(1, 1);
    let averaging = date(4, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 16,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for (valuation, history, rejects) in [
        (
            Timestamp::new(date(3, 31), 23, 59, 59, 999999).unwrap(),
            100.0,
            true,
        ),
        (averaging.into(), 0.0, false),
        (averaging.into(), 100.0, true),
        (Timestamp::new(averaging, 0, 0, 0, 1).unwrap(), 0.0, true),
        (Timestamp::new(averaging, 0, 0, 0, 1).unwrap(), 100.0, false),
        (Date::new(2024, 12, 31).unwrap().into(), 100.0, true),
        (Timestamp::new(end, 0, 0, 0, 1).unwrap(), 100.0, true),
    ] {
        let a = ArithmeticAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        let g = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        let context = market(100.0, 0.2, valuation);
        for result in [engine.price(&a, &context), engine.price(&g, &context)] {
            if rejects {
                assert!(matches!(result.unwrap_err(), Error::InvalidInput(_)));
            } else {
                assert!(result.unwrap() >= 0.0);
            }
        }
        let invalid = engine.price_with_greeks(&a, &context, GreeksRequest::default());
        assert!(matches!(invalid.unwrap_err(), Error::InvalidInput(_)));
        assert_eq!(a.realized_average(), history);
    }
    for (averaging, history, expected) in [(start, 110.0, 10.0), (end, 0.0, 0.0)] {
        let a = ArithmeticAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        let g = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        let context = market(100.0, 0.2, end.into());
        for result in [
            engine.price_with_greeks(&a, &context, GreeksRequest::all()),
            engine.price_with_greeks(&g, &context, GreeksRequest::all()),
        ] {
            let result = result.unwrap();
            assert_eq!(result.price(), expected);
            assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
        }
        assert!(matches!(
            implied_volatility(&engine, &a, &context, expected, Default::default()).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    assert_eq!(engine.settings().seed, Some(42));
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
            sampling: MonteCarloSampling::Sobol,
            path_count: 3,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloAsianEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert_eq!(
        MonteCarloAsianEngine::default().settings(),
        MonteCarloSettings::default()
    );
}

#[test]
fn tiny_volatility_scaling_and_seasoned_negative_gamma_survive() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 65536,
        step_count: 64,
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for (spot, sigma) in [
        (100.0_f64, 1e-170_f64),
        (1e160, 1e-160),
        (1e300, 1e-200),
        (1e300, f64::from_bits(1)),
    ] {
        let expected = (spot.ln() + sigma.ln()).exp() / (6.0 * std::f64::consts::PI).sqrt();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap(),
            spot,
            start.into(),
        )
        .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let a =
                ArithmeticAveragePriceOption::new(direction, spot, start, start, end, 0.0).unwrap();
            let g =
                GeometricAveragePriceOption::new(direction, spot, start, start, end, 0.0).unwrap();
            for price in [
                engine.price(&a, &context).unwrap(),
                engine.price(&g, &context).unwrap(),
            ] {
                assert!(
                    (price / expected - 1.0).abs() < 0.002,
                    "{price} vs {expected}"
                );
            }
        }
    }
    let g =
        GeometricAveragePriceOption::new(OptionType::Call, 10.0, start, start, end, 100.0).unwrap();
    let context = market(100.0, 0.2, date(7, 1).into());
    let actual = engine
        .price_with_greeks(&g, &context, GreeksRequest::new([Greek::Gamma]))
        .unwrap();
    let expected = AnalyticGeometricAveragePriceEngine
        .price_with_greeks(
            &g,
            &context,
            GreeksRequest::new([Greek::Gamma]),
            Default::default(),
        )
        .unwrap();
    assert!(actual.require(Greek::Gamma).unwrap() < 0.0);
    assert!(
        (actual.require(Greek::Gamma).unwrap() - expected.require(Greek::Gamma).unwrap()).abs()
            < 0.0001
    );
}

#[test]
fn arithmetic_curves_single_fixing_and_affine_history_keep_their_support() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let knot = date(7, 1);
    let curve = RateTermStructure::new(
        start.into(),
        vec![knot.into(), end.into()],
        vec![(-0.04_f64 * 181.0 / 365.0).exp(), (-0.04_f64).exp()],
    )
    .unwrap();
    let model = BlackScholesMertonParameters::new(curve.clone(), 0.01, 0.2).unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 8192,
        step_count: 16,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    for (averaging, valuation, history) in [
        (start, start, 0.0),
        (date(5, 1), start, 0.0),
        (start, knot, 100.0),
        (end, start, 0.0),
    ] {
        let a = ArithmeticAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        let g = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        let context = PricingContext::new(model.clone(), 100.0, valuation.into()).unwrap();
        let actual = engine.price(&a, &context).unwrap();
        let expected = engine
            .price(&a, &market(100.0, 0.2, valuation.into()))
            .unwrap();
        assert!((actual - expected).abs() < 1e-10);
        assert!(matches!(
            engine.price(&g, &context).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let context = PricingContext::new(model, 100.0, end.into()).unwrap();
    let g = GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 110.0)
        .unwrap();
    assert_eq!(engine.price(&g, &context).unwrap(), 10.0);
    let affine =
        ArithmeticAveragePriceOption::new(OptionType::Call, 25.0, start, start, end, 100.0)
            .unwrap();
    let context = market(100.0, 0.2, knot.into());
    let result = engine
        .price_with_greeks(&affine, &context, GreeksRequest::all())
        .unwrap();
    assert_eq!(
        result.price(),
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&affine, &context)
            .unwrap()
    );
    assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
    assert!(matches!(
        implied_volatility(
            &engine,
            &affine,
            &context,
            result.price(),
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let node_context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.01, 0.2).unwrap(),
        100.0,
        knot.into(),
    )
    .unwrap();
    assert!((engine.price(&affine, &node_context).unwrap() - result.price()).abs() < 1e-11);
    let short = RateTermStructure::new(start.into(), vec![knot.into()], vec![0.98]).unwrap();
    let short_context = PricingContext::new(
        BlackScholesMertonParameters::new(short, 0.01, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let a = affine.replace(0.0).unwrap();
    assert!(matches!(
        engine.price(&a, &short_context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine.price(&affine, &short_context).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn conditioned_arithmetic_agrees_with_independent_raw_path_statistics() {
    // Independent raw trapezoids use a different generator and do not condition any interval.
    let mut state = 1977_u64;
    let mut uniform = || {
        state = state
            .wrapping_mul(2862933555777941757)
            .wrapping_add(3037000493);
        ((state >> 12) as f64 + 0.5) / 4503599627370496.0
    };
    let mut normal = || (-2.0 * uniform().ln()).sqrt() * (std::f64::consts::TAU * uniform()).cos();
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let steps = 32;
    let paths = 131072;
    for (averaging, valuation, history) in [
        (start, start, 0.0),
        (date(5, 1), start, 0.0),
        (start, date(7, 1), 105.0),
    ] {
        let lead = if averaging > valuation {
            120.0 / 365.0
        } else {
            0.0
        };
        let future = if valuation > start {
            184.0 / 365.0
        } else {
            1.0 - lead
        };
        let weight = if history > 0.0 { future } else { 1.0 };
        let dt = future / f64::from(steps);
        let discount = (-0.04 * (lead + future)).exp();
        let (mut sums, mut squares) = ([0.0_f64; 3], [0.0_f64; 3]);
        for _ in 0..paths {
            let mut asset = 100.0 * (0.01 * lead + 0.2 * lead.sqrt() * normal()).exp();
            let mut integral = 0.5 * asset;
            for step in 1..=steps {
                asset *= (0.01 * dt + 0.2 * dt.sqrt() * normal()).exp();
                integral += if step == steps { 0.5 * asset } else { asset };
            }
            let average = weight * integral / f64::from(steps);
            let payoff = |spot: f64| {
                discount * ((1.0 - weight) * history + average * (spot / 100.0) - 100.0).max(0.0)
            };
            let price = payoff(100.0);
            let delta = if (1.0 - weight) * history + average > 100.0 {
                discount * average / 100.0
            } else {
                0.0
            };
            let gamma = (payoff(100.5) - 2.0 * price + payoff(99.5)) / 0.25;
            for (index, value) in [price, delta, gamma].into_iter().enumerate() {
                sums[index] += value;
                squares[index] += value * value;
            }
        }
        let option = ArithmeticAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            history,
        )
        .unwrap();
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
                path_count: paths,
                step_count: steps,
                seed: Some(42),
                antithetic: true,
                sampling,
                ..Default::default()
            })
            .unwrap();
            let result = engine
                .price_with_greeks(
                    &option,
                    &market(100.0, 0.2, valuation.into()),
                    GreeksRequest::all(),
                )
                .unwrap();
            for (index, actual) in [
                result.price(),
                result.require(Greek::Delta).unwrap(),
                result.require(Greek::Gamma).unwrap(),
            ]
            .into_iter()
            .enumerate()
            {
                let mean = sums[index] / f64::from(paths);
                let error = ((squares[index] / f64::from(paths) - mean * mean)
                    / f64::from(paths - 1))
                .sqrt();
                assert!(
                    (actual - mean).abs() < 6.0 * error + 0.00005,
                    "{actual} vs {mean}, SE {error}"
                );
            }
        }
    }
}

#[test]
fn unseeded_analyses_pin_request_samples_without_mutation() {
    let start = date(1, 1);
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        100.0,
        start,
        start,
        Date::new(2026, 1, 1).unwrap(),
        0.0,
    )
    .unwrap();
    let context = market(100.0, 0.2, start.into());
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 32768,
        step_count: 16,
        ..Default::default()
    })
    .unwrap();
    let prepared =
        <MonteCarloAsianEngine as PricingEngine<ArithmeticAveragePriceOption>>::prepare_request(
            &engine,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        prepared.price(&option, &context).unwrap(),
        prepared.price(&option, &context).unwrap()
    );
    let result = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]),
        Default::default(),
    )
    .unwrap();
    assert!((0.5..0.65).contains(&result.require(Greek::Delta).unwrap()));
    assert!((0.18..0.28).contains(&result.require(Greek::Vega).unwrap()));
    let sigma = implied_volatility(
        &engine,
        &option,
        &context,
        5.25,
        ImpliedVolatilitySettings {
            lower_bound: 0.1,
            upper_bound: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((0.1..0.4).contains(&sigma));
    assert_eq!(engine.settings().seed, None);
}

#[test]
fn arithmetic_validates_interval_drift_without_rejecting_a_finite_payoff() {
    let start = date(1, 1);
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Put,
        100.0,
        start,
        start,
        Date::new(2035, 1, 1).unwrap(),
        0.0,
    )
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 1e154).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 1,
        step_count: 100,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    // Every sampled future contribution vanishes; the initial trapezoid fixes 0.5.
    assert!((engine.price(&option, &context).unwrap() - 99.5).abs() < 1e-12);
}

#[test]
fn node_curve_affine_history_preserves_the_fused_accrual_residual() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        (90.0 / 365.0) * 1e308,
        start,
        start,
        end,
        1e308,
    )
    .unwrap();
    let curve = RateTermStructure::new(start.into(), vec![end.into()], vec![1.0]).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.2).unwrap(),
        1e-100,
        date(4, 1).into(),
    )
    .unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    // Exact residual of the binary64 elapsed weight times history minus rounded strike.
    let expected = 2.0487666640489847e291;
    assert!((engine.price(&option, &context).unwrap() / expected - 1.0).abs() < 1e-12);
}
