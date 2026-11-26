use kiyosi::*;

#[test]
fn deterministic_arithmetic_weights_large_segment_before_scaling() -> Result<()> {
    let start = date(1, 1);
    let expiry = start.add_days(365)?;
    let dividend =
        RateTermStructure::new(start, vec![start.add_days(361)?, expiry], vec![1.0, 100.0])?;
    let spot = 1e307;
    let market = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, dividend, 0.0)?,
        spot,
        start,
    )?;
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, spot, start, start, expiry, None)?;
    let expected = spot * (4.0 / 365.0) * (99.0 / 100.0_f64.ln() - 1.0);
    let price = MonteCarloAsianEngine::default().price(&option, &market)?;
    assert!(
        (price / expected - 1.0).abs() < 1e-12,
        "{price} vs {expected}"
    );
    Ok(())
}

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
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
fn arithmetic_strike_batches_replay_scalar_prices() -> Result<()> {
    let start = date(1, 1);
    let expiry = start.add_days(365)?;
    let valuation = date(7, 1);
    let market = market(100.0, 0.2, valuation);
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Put,
        40.0,
        start,
        start,
        expiry,
        Some(104.0),
    )?;
    let strikes = [40.0, 105.0]
        .into_iter()
        .chain((85..117).map(f64::from))
        .chain([105.0])
        .collect::<Vec<_>>();
    for (sampling, path_count) in [
        (MonteCarloSampling::PseudoRandom, 1025),
        (MonteCarloSampling::Sobol, 1024),
    ] {
        let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
            sampling: MonteCarloSettings {
                path_count,
                sampling,
                seed: Some(42),
                antithetic: true,
                max_active_workers: 2,
            },
            step_count: 16,
        })?;
        let prices = engine.price_arithmetic_strikes(&option, &market, &strikes)?;
        let reference = strikes
            .iter()
            .map(|&strike| {
                engine.price(
                    &option.replace(AveragePriceOptionChanges {
                        strike: Some(strike),
                        ..Default::default()
                    })?,
                    &market,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(prices, reference);
        assert_eq!(prices[0], 0.0);
        assert!(prices[1] > prices[0]);
        assert_eq!(
            engine.price_arithmetic_strikes(&option, &market, &[])?,
            [] as [f64; 0],
        );
        assert!(matches!(
            engine.price_arithmetic_strikes(&option, &market, &[105.0, f64::NAN]),
            Err(Error::InvalidInput(_))
        ));
        let invalid_history = option.replace(AveragePriceOptionChanges {
            realized_average: Some(None),
            ..Default::default()
        })?;
        assert!(matches!(
            engine.price_arithmetic_strikes(&invalid_history, &market, &[]),
            Err(Error::MissingHistory(_))
        ));
    }
    Ok(())
}

#[test]
fn duplicate_arithmetic_strikes_share_the_path_work_budget() -> Result<()> {
    let start = date(1, 1);
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        100.0,
        start,
        start,
        start.add_days(365)?,
        None,
    )?;
    let context = market(100.0, 0.2, start);
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 131_072,
            seed: Some(7),
            max_active_workers: 1,
            ..Default::default()
        },
        step_count: 2,
    })?;
    let expected = engine.price(&option, &context)?;
    assert!(expected > 0.0);
    let strikes = vec![100.0; 8192];
    let prices = engine.price_arithmetic_strikes(&option, &context, &strikes)?;
    assert_eq!(prices, vec![expected; 8192]);
    Ok(())
}

#[test]
fn single_fixing_preserves_quoted_forward() -> Result<()> {
    let start = date(1, 1);
    let end = start.add_days(365)?;
    let scale = 2.0_f64.powi(50);
    let strike = 100.0 * scale;
    let risk = RateTermStructure::new(start, vec![end], vec![0.4])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(risk, 0.0, 1e-20)?,
        40.0 * scale,
        start,
    )?;
    let engine = MonteCarloAsianEngine::new(
        MonteCarloSettings {
            path_count: 4096,
            seed: Some(42),
            ..Default::default()
        }
        .into(),
    )?;
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Put, strike, start, end, end, None)?;
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Put, strike, start, end, end, None)?;
    for price in [
        engine.price(&arithmetic, &context)?,
        engine.price(&geometric, &context)?,
    ] {
        assert!((price - 2.5).abs() < 1e-4, "{price}");
    }
    Ok(())
}

#[test]
fn geometric_exact_sampling_matches_analytic_and_ignores_steps() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    for averaging in [start, date(5, 1), end] {
        let option =
            GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, averaging, end, None)
                .unwrap();
        let context = market(100.0, 0.2, start);
        let settings = MonteCarloSettings {
            path_count: 16384,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            seed: Some(42),
            ..Default::default()
        };
        let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
            sampling: settings,
            step_count: 2,
        })
        .unwrap();
        let price = engine.price(&option, &context).unwrap();
        let reference = AnalyticGeometricAveragePriceEngine
            .price(&option, &context)
            .unwrap();
        assert!((price - reference).abs() < 0.01, "{price} vs {reference}");
        let changed = MonteCarloAsianEngine::new(MonteCarloGridSettings {
            sampling: MonteCarloSettings {
                max_active_workers: 1,
                ..settings
            },
            step_count: 10000,
        })
        .unwrap();
        assert_eq!(price, changed.price(&option, &context).unwrap());
    }
}

#[test]
fn geometric_flat_node_curves_match_scalar_prices_and_greeks() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let rate_curve = RateTermStructure::new(start, vec![end], vec![(-0.05_f64).exp()]).unwrap();
    let dividend_curve = RateTermStructure::new(start, vec![end], vec![(-0.02_f64).exp()]).unwrap();
    let engine = MonteCarloAsianEngine::new(
        MonteCarloSettings {
            path_count: 32768,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            seed: Some(42),
            ..Default::default()
        }
        .into(),
    )
    .unwrap();
    for (risk, dividend) in [
        (Rate::from(rate_curve.clone()), Rate::from(0.02)),
        (Rate::from(0.05), Rate::from(dividend_curve.clone())),
        (Rate::from(rate_curve), Rate::from(dividend_curve)),
    ] {
        for (averaging, valuation, history) in [
            (start, start, None),
            (date(5, 1), start, None),
            (start, date(7, 1), Some(105.0)),
            (end, start, None),
        ] {
            for direction in [OptionType::Call, OptionType::Put] {
                let option = GeometricAveragePriceOption::new(
                    direction, 100.0, start, averaging, end, history,
                )
                .unwrap();
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(risk.clone(), dividend.clone(), 0.2).unwrap(),
                    100.0,
                    valuation,
                )
                .unwrap();
                let scalar = PricingContext::new(
                    BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
                    100.0,
                    valuation,
                )
                .unwrap();
                let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
                let actual = engine
                    .price_with_greeks(&option, &context, request)
                    .unwrap();
                let equivalent = engine.price_with_greeks(&option, &scalar, request).unwrap();
                let analytic = AnalyticGeometricAveragePriceEngine
                    .price_with_greeks(&option, &context, request)
                    .unwrap();
                let price = engine.price(&option, &context).unwrap();
                let estimate = engine.price_with_statistics(&option, &context).unwrap();
                assert!((price - equivalent.price()).abs() < 1e-10);
                assert!((estimate.price() - equivalent.price()).abs() < 1e-10);
                assert!(
                    (actual.price() - equivalent.price()).abs() < 1e-10,
                    "{direction:?} averaging {averaging:?} valuation {valuation:?}"
                );
                assert!(
                    (actual.price() - analytic.price()).abs() < 0.003,
                    "{} vs {}",
                    actual.price(),
                    analytic.price()
                );
                for (greek, tolerance) in [(Greek::Delta, 0.001), (Greek::Gamma, 0.0005)] {
                    let value = actual.require(greek).unwrap();
                    assert!(
                        (value - equivalent.require(greek).unwrap()).abs() < 1e-10,
                        "{greek:?}: {direction:?} averaging {averaging:?} valuation {valuation:?}"
                    );
                    assert!(
                        (value - analytic.require(greek).unwrap()).abs() < tolerance,
                        "{greek:?}: {value} vs {}",
                        analytic.require(greek).unwrap()
                    );
                }
            }
        }
    }
}

#[test]
fn arithmetic_retains_conditioned_integration_and_converges() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 2.0, start, start, end, None).unwrap();
    let settings = MonteCarloSettings {
        path_count: 65536,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        ..Default::default()
    };
    for (spot, expected) in [(1.9, 0.193_174), (2.0, 0.246_416), (2.1, 0.306_220)] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.05, 0.0, 0.5).unwrap(),
            spot,
            start,
        )
        .unwrap();
        let fine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
            sampling: settings,
            step_count: 64,
        })
        .unwrap()
        .price(&option, &context)
        .unwrap();
        let coarse = MonteCarloAsianEngine::new(MonteCarloGridSettings {
            sampling: settings,
            step_count: 2,
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
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 32768,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            seed: Some(42),
            ..Default::default()
        },
        step_count: 32,
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for (averaging, valuation, history) in [
            (start, start, 0.0),
            (date(5, 1), start, 0.0),
            (start, date(7, 1), 105.0),
        ] {
            let a = ArithmeticAveragePriceOption::new(
                direction,
                100.0,
                start,
                averaging,
                end,
                (history != 0.0).then_some(history),
            )
            .unwrap();
            let g = GeometricAveragePriceOption::new(
                direction,
                100.0,
                start,
                averaging,
                end,
                (history != 0.0).then_some(history),
            )
            .unwrap();
            let context = market(100.0, 0.2, valuation);
            let result = engine
                .price_with_greeks(&a, &context, GreeksRequest::all())
                .unwrap();
            assert_eq!(result.price(), engine.price(&a, &context).unwrap());
            for h in [0.05, 0.025] {
                let up = engine
                    .price(&a, &market(100.0 + h, 0.2, valuation))
                    .unwrap();
                let down = engine
                    .price(&a, &market(100.0 - h, 0.2, valuation))
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
                .price_with_greeks(&g, &context, GreeksRequest::all())
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
                &market(100.0, 0.35, valuation),
                quote,
                ImpliedVolatilitySettings::new(0.1, 0.4),
            )
            .unwrap();
            assert!((solved - 0.2).abs() < 1e-6);
            assert_eq!(a.realized_average(), (history != 0.0).then_some(history));
            assert_eq!(
                context.model_parameters().as_bsm().unwrap().volatility(),
                0.2
            );
        }
    }
}

#[test]
fn sampling_exact_counts_threads_and_shared_calls_agree() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let a = ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
        .unwrap();
    let g =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let context = market(100.0, 0.2, start);
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            let settings = MonteCarloSettings {
                path_count: if sampling == MonteCarloSampling::Sobol {
                    1024
                } else {
                    1025
                },
                seed: Some(42),
                sampling,
                antithetic,
                max_active_workers: 1,
            };
            let serial = MonteCarloAsianEngine::new(MonteCarloGridSettings {
                sampling: settings,
                step_count: 16,
            })
            .unwrap();
            let parallel = MonteCarloAsianEngine::new(MonteCarloGridSettings {
                sampling: MonteCarloSettings {
                    max_active_workers: 4,
                    ..settings
                },
                step_count: 16,
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
        let plain = MonteCarloAsianEngine::new(settings.into()).unwrap();
        let paired = MonteCarloAsianEngine::new(
            MonteCarloSettings {
                antithetic: true,
                ..settings
            }
            .into(),
        )
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
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let engine = MonteCarloAsianEngine::new(
        MonteCarloSettings {
            path_count: 16,
            seed: Some(42),
            ..Default::default()
        }
        .into(),
    )
    .unwrap();
    for (valuation, history, rejects) in [
        (
            Timestamp::new(date(3, 31).date(), 23, 59, 59, 999_999).unwrap(),
            100.0,
            true,
        ),
        (averaging, 0.0, false),
        (averaging, 100.0, true),
        (
            Timestamp::new(averaging.date(), 0, 0, 0, 1).unwrap(),
            0.0,
            true,
        ),
        (
            Timestamp::new(averaging.date(), 0, 0, 0, 1).unwrap(),
            100.0,
            false,
        ),
        (Date::new(2024, 12, 31).unwrap().start_of_day(), 100.0, true),
        (Timestamp::new(end.date(), 0, 0, 0, 1).unwrap(), 100.0, true),
    ] {
        let a = ArithmeticAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            (history != 0.0).then_some(history),
        )
        .unwrap();
        let g = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            (history != 0.0).then_some(history),
        )
        .unwrap();
        let context = market(100.0, 0.2, valuation);
        for result in [engine.price(&a, &context), engine.price(&g, &context)] {
            if rejects {
                if valuation > averaging && valuation <= end && history == 0.0 {
                    assert!(matches!(result.unwrap_err(), Error::MissingHistory(_)));
                } else {
                    assert!(matches!(result.unwrap_err(), Error::InvalidInput(_)));
                }
            } else {
                assert!(result.unwrap() >= 0.0);
            }
        }
        assert_eq!(a.realized_average(), (history != 0.0).then_some(history));
    }
    for (averaging, history, expected) in [(start, 110.0, 10.0), (end, 0.0, 0.0)] {
        let a = ArithmeticAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            (history != 0.0).then_some(history),
        )
        .unwrap();
        let g = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            (history != 0.0).then_some(history),
        )
        .unwrap();
        let context = market(100.0, 0.2, end);
        for result in [
            engine.price_with_greeks(&a, &context, GreeksRequest::all()),
            engine.price_with_greeks(&g, &context, GreeksRequest::all()),
        ] {
            let result = result.unwrap();
            assert_eq!(result.price(), expected);
            for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
                assert_eq!(result.get(greek), None);
            }
            for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
                assert_eq!(
                    result.status(greek),
                    GreekStatus::Unsupported,
                    "{greek:?} at expiry"
                );
            }
        }
        assert!(matches!(
            implied_volatility(
                &engine,
                &a,
                &context,
                expected,
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    assert_eq!(engine.settings().sampling.seed, Some(42));
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            max_active_workers: -1,
            ..Default::default()
        },
        MonteCarloSettings {
            sampling: MonteCarloSampling::Sobol,
            path_count: 3,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloAsianEngine::new(settings.into()).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert_eq!(
        MonteCarloAsianEngine::default().settings().sampling,
        MonteCarloSettings::default()
    );
}

#[test]
fn tiny_volatility_scaling_and_seasoned_negative_gamma_survive() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 65536,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            seed: Some(42),
            ..Default::default()
        },
        step_count: 64,
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
            start,
        )
        .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let a = ArithmeticAveragePriceOption::new(direction, spot, start, start, end, None)
                .unwrap();
            let g =
                GeometricAveragePriceOption::new(direction, spot, start, start, end, None).unwrap();
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
    let context = market(100.0, 0.2, date(7, 1));
    let actual = engine
        .price_with_greeks(&g, &context, GreeksRequest::new([Greek::Gamma]))
        .unwrap();
    let expected = AnalyticGeometricAveragePriceEngine
        .price_with_greeks(&g, &context, GreeksRequest::new([Greek::Gamma]))
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
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let knot = date(7, 1);
    let curve = RateTermStructure::new(
        start,
        vec![knot, end],
        vec![(-0.04_f64 * 181.0 / 365.0).exp(), (-0.04_f64).exp()],
    )
    .unwrap();
    let model = BlackScholesMertonParameters::new(curve.clone(), 0.01, 0.2).unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 8192,
            seed: Some(42),
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
        step_count: 16,
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
            (history != 0.0).then_some(history),
        )
        .unwrap();
        let g = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            (history != 0.0).then_some(history),
        )
        .unwrap();
        let context = PricingContext::new(model.clone(), 100.0, valuation).unwrap();
        let actual = engine.price(&a, &context).unwrap();
        let expected = engine.price(&a, &market(100.0, 0.2, valuation)).unwrap();
        assert!((actual - expected).abs() < 1e-10);
        assert!(matches!(
            engine.price(&g, &context).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let context = PricingContext::new(model, 100.0, end).unwrap();
    let g = GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 110.0)
        .unwrap();
    assert_eq!(engine.price(&g, &context).unwrap(), 10.0);
    let affine =
        ArithmeticAveragePriceOption::new(OptionType::Call, 25.0, start, start, end, 100.0)
            .unwrap();
    let context = market(100.0, 0.2, knot);
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
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let node_context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.01, 0.2).unwrap(),
        100.0,
        knot,
    )
    .unwrap();
    assert!((engine.price(&affine, &node_context).unwrap() - result.price()).abs() < 1e-11);
    let short = RateTermStructure::new(start, vec![knot], vec![0.98]).unwrap();
    let short_context = PricingContext::new(
        BlackScholesMertonParameters::new(short, 0.01, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let a = affine
        .replace(AveragePriceOptionChanges {
            realized_average: Some(None),
            ..Default::default()
        })
        .unwrap();
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
#[expect(
    clippy::cast_precision_loss,
    reason = "the random word contains only 52 bits."
)]
fn conditioned_arithmetic_agrees_with_independent_raw_path_statistics() {
    // Independent raw trapezoids use a different generator and do not condition any interval.
    let mut state = 1977_u64;
    let mut uniform = || {
        state = state
            .wrapping_mul(2_862_933_555_777_941_757)
            .wrapping_add(3_037_000_493);
        ((state >> 12) as f64 + 0.5) / 4_503_599_627_370_496.0
    };
    let mut normal = || (-2.0 * uniform().ln()).sqrt() * (std::f64::consts::TAU * uniform()).cos();
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let steps = 32;
    let paths = 131_072;
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
            (history != 0.0).then_some(history),
        )
        .unwrap();
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
                sampling: MonteCarloSettings {
                    path_count: paths,
                    seed: Some(42),
                    antithetic: true,
                    sampling,
                    ..Default::default()
                },
                step_count: steps,
            })
            .unwrap();
            let result = engine
                .price_with_greeks(
                    &option,
                    &market(100.0, 0.2, valuation),
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
        Date::new(2026, 1, 1).unwrap().start_of_day(),
        None,
    )
    .unwrap();
    let context = market(100.0, 0.2, start);
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 32768,
            ..Default::default()
        },
        step_count: 16,
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
    let request = GreeksRequest::new([Greek::Delta, Greek::Vega]);
    let result = numerical_greeks(&engine, &option, &context, request, Default::default()).unwrap();
    let settings = engine.settings();
    let replay = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            seed: Some(
                result
                    .seed()
                    .expect("stochastic analysis reports the request seed"),
            ),
            ..settings.sampling
        },
        ..settings
    })
    .unwrap();
    assert_eq!(
        result,
        numerical_greeks(&replay, &option, &context, request, Default::default()).unwrap()
    );
    result
        .require(Greek::Delta)
        .expect("the requested delta is available");
    result
        .require(Greek::Vega)
        .expect("the requested vega is available");
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            5.25,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        ),
        Err(Error::InvalidInput(_))
    ));
    let quote = replay.price(&option, &context).unwrap();
    let sigma = implied_volatility(
        &replay,
        &option,
        &context,
        quote,
        ImpliedVolatilitySettings::new(0.1, 0.4),
    )
    .unwrap();
    assert!(
        (sigma - 0.2).abs() < 1e-8,
        "recovered Asian volatility {sigma}"
    );
    let repriced = market(100.0, sigma, start);
    assert!((replay.price(&option, &repriced).unwrap() - quote).abs() <= 1e-8);
    assert_eq!(engine.settings().sampling.seed, None);
}

#[test]
fn arithmetic_validates_interval_drift_without_rejecting_a_finite_payoff() {
    let start = date(1, 1);
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Put,
        100.0,
        start,
        start,
        Date::new(2035, 1, 1).unwrap().start_of_day(),
        None,
    )
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 1e154).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 1,
            seed: Some(42),
            ..Default::default()
        },
        step_count: 100,
    })
    .unwrap();
    // Every sampled future contribution vanishes; the initial trapezoid fixes 0.5.
    assert!((engine.price(&option, &context).unwrap() - 99.5).abs() < 1e-12);
}

#[test]
fn node_curve_affine_history_preserves_the_fused_accrual_residual() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let option = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        (90.0 / 365.0) * 1e308,
        start,
        start,
        end,
        1e308,
    )
    .unwrap();
    let curve = RateTermStructure::new(start, vec![end], vec![1.0]).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.2).unwrap(),
        1e-100,
        date(4, 1),
    )
    .unwrap();
    let engine = MonteCarloAsianEngine::new(
        MonteCarloSettings {
            path_count: 1,
            seed: Some(42),
            ..Default::default()
        }
        .into(),
    )
    .unwrap();
    // Exact residual of the binary64 elapsed weight times history minus rounded strike.
    let expected = 2.048_766_664_048_984_7e291;
    assert!((engine.price(&option, &context).unwrap() / expected - 1.0).abs() < 1e-12);
}

#[test]
fn arithmetic_asian_rejects_excessive_path_steps_before_sampling() -> Result<()> {
    let start = date(1, 1);
    let end = start.add_days(365)?;
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)?;
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 10_000_000,
            seed: Some(42),
            ..Default::default()
        },
        step_count: 10_000,
    })?;
    assert!(matches!(
        engine.price(&option, &market(100.0, 0.2, start)),
        Err(Error::InvalidInput(message)) if message.contains("path steps")
    ));
    assert!(engine.price(&option, &market(100.0, 0.0, start))? > 0.0);
    Ok(())
}

#[test]
fn arithmetic_control_reduces_sampling_error_for_calls_and_puts() -> Result<()> {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.0, 0.5)?,
        2.0,
        start,
    )?;
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 4096,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        },
        step_count: 128,
    })?;
    // Vecer's continuous average call and the corresponding put from Asian parity.
    let call = 0.246_416;
    let parity = (-0.05_f64).exp() * (2.0 * 0.05_f64.exp_m1() / 0.05 - 2.0);
    for (direction, reference) in [(OptionType::Call, call), (OptionType::Put, call - parity)] {
        let option = ArithmeticAveragePriceOption::new(direction, 2.0, start, start, end, None)?;
        let estimate = engine.price_with_statistics(&option, &context)?;
        let error = estimate.standard_error().unwrap();
        assert!(error > 0.0 && error < 0.001, "{direction:?}: {error}");
        assert!((estimate.price() - reference).abs() < 4.0 * error + 0.0001);
        assert_eq!(
            estimate.price().to_bits(),
            engine.price(&option, &context)?.to_bits()
        );
    }
    Ok(())
}
