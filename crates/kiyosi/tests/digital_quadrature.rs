use kiyosi::*;

fn dates() -> (Date, Date) {
    (
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
}

fn market(spot: f64, rate: f64, dividend: f64, sigma: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        dates().0.into(),
    )
    .unwrap()
}

fn options(direction: OptionType, strike: f64, payout: f64, end: Date) -> [DigitalOption; 2] {
    [
        DigitalOption::cash_or_nothing(direction, strike, payout, dates().0, end).unwrap(),
        DigitalOption::asset_or_nothing(direction, strike, dates().0, end).unwrap(),
    ]
}

#[test]
fn prices_match_analytic_across_payoffs_moneyness_maturity_and_carry() {
    let engine = QuadratureDigitalEngine::new(QuadratureSettings {
        asset_step_count: 2048,
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for (month, day, year) in [(1, 2, 2025), (1, 31, 2025), (1, 1, 2026), (1, 1, 2027)] {
            for spot in [80.0, 100.0, 120.0] {
                for (rate, dividend) in [(0.04, 0.01), (-0.02, -0.03)] {
                    let context = market(spot, rate, dividend, 0.3);
                    for option in
                        options(direction, 100.0, 10.0, Date::new(year, month, day).unwrap())
                    {
                        let expected = AnalyticDigitalEngine.price(&option, &context).unwrap();
                        let actual = engine.price(&option, &context).unwrap();
                        assert!(
                            (actual - expected).abs() < 1e-8,
                            "{option:?}: {actual} != {expected}"
                        );
                        let default = QuadratureDigitalEngine::default()
                            .price(&option, &context)
                            .unwrap();
                        assert!((default - expected).abs() < 1e-7);
                    }
                }
            }
        }
    }
}

#[test]
fn greeks_and_implied_volatility_use_shared_contracts() {
    let engine = QuadratureDigitalEngine::new(QuadratureSettings {
        asset_step_count: 2048,
        ..Default::default()
    })
    .unwrap();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for direction in [OptionType::Call, OptionType::Put] {
        for option in options(direction, 100.0, 10.0, dates().1) {
            let actual = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            let expected = AnalyticDigitalEngine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            for greek in Greek::ALL {
                assert!(
                    (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs()
                        < 1e-6,
                    "{option:?} {greek:?}: {actual:?} != {expected:?}"
                );
            }
            let selected = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(selected.get(Greek::Gamma), None);
            assert_eq!(selected.get(Greek::Delta), actual.get(Greek::Delta));
            let sigma = implied_volatility(
                &engine,
                &option,
                &context,
                expected.price(),
                ImpliedVolatilitySettings {
                    lower_bound: 0.1,
                    upper_bound: 0.22,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!((sigma - 0.2).abs() < 1e-7);
            assert!(matches!(
                implied_volatility(&engine, &option, &context, 1e4, Default::default()),
                Err(Error::RootNotBracketed(_))
            ));
        }
    }
}

#[test]
fn expiry_is_strict_and_life_validation_preserves_state() {
    let engine = QuadratureDigitalEngine::default();
    let (start, end) = dates();
    for direction in [OptionType::Call, OptionType::Put] {
        for option in options(direction, 100.0, 10.0, end) {
            let original = market(100.0, 0.05, 0.02, 0.2);
            let baseline = engine.price(&option, &original).unwrap();
            for spot in [80.0, 100.0, 120.0] {
                let context = market(spot, 0.05, 0.02, 0.2)
                    .replace(PricingContextChanges {
                        valuation_time: Some(end.into()),
                        ..Default::default()
                    })
                    .unwrap();
                let expected = if (direction == OptionType::Call && spot > 100.0)
                    || (direction == OptionType::Put && spot < 100.0)
                {
                    option.payout().unwrap_or(spot)
                } else {
                    0.0
                };
                let result = engine
                    .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert_eq!(result.price(), expected);
                assert!(Greek::ALL.into_iter().all(|g| result.get(g).is_none()));
            }
            for at in [
                Date::new(start.year() - 1, 12, 31).unwrap().into(),
                Timestamp::new(end, 0, 0, 0, 1).unwrap(),
            ] {
                let invalid = original
                    .replace(PricingContextChanges {
                        valuation_time: Some(at),
                        ..Default::default()
                    })
                    .unwrap();
                assert!(matches!(
                    engine.price(&option, &invalid),
                    Err(Error::InvalidInput(_))
                ));
            }
            let before_expiry = original
                .replace(PricingContextChanges {
                    valuation_time: Some(
                        Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999)
                            .unwrap(),
                    ),
                    ..Default::default()
                })
                .unwrap();
            assert!(
                (engine.price(&option, &before_expiry).unwrap()
                    - AnalyticDigitalEngine
                        .price(&option, &before_expiry)
                        .unwrap())
                .abs()
                    < 1e-7
            );
            assert_eq!(engine.price(&option, &original).unwrap(), baseline);
        }
    }
}

#[test]
fn invalid_controls_and_unsupported_curves_keep_error_categories() {
    for settings in [
        QuadratureSettings {
            asset_step_count: 3,
            ..Default::default()
        },
        QuadratureSettings {
            integration_standard_deviations: 0.0,
            ..Default::default()
        },
        QuadratureSettings {
            integration_standard_deviations: f64::INFINITY,
            ..Default::default()
        },
        QuadratureSettings {
            integration_standard_deviations: f64::NAN,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            QuadratureDigitalEngine::new(settings),
            Err(Error::InvalidInput(_))
        ));
    }
    let engine = QuadratureDigitalEngine::default();
    let option = &options(OptionType::Call, 100.0, 10.0, dates().1)[0];
    for at in [dates().0, dates().1] {
        for dividend_curve in [false, true] {
            let curve =
                RateTermStructure::new(dates().0.into(), vec![dates().1.into()], vec![0.95])
                    .unwrap();
            let model = if dividend_curve {
                BlackScholesMertonParameters::new(Rate::Flat(0.05), Rate::Curve(curve), 0.2)
            } else {
                BlackScholesMertonParameters::new(Rate::Curve(curve), Rate::Flat(0.02), 0.2)
            }
            .unwrap();
            let context = PricingContext::new(model, 100.0, at.into()).unwrap();
            assert!(matches!(
                engine.price(option, &context),
                Err(Error::UnsupportedOperation(_))
            ));
            assert!(matches!(
                engine.price_with_greeks(
                    option,
                    &context,
                    GreeksRequest::default(),
                    Default::default()
                ),
                Err(Error::UnsupportedOperation(_))
            ));
        }
    }
    let context = market(100.0, 0.05, 0.02, 0.2);
    assert!(matches!(
        engine.price_with_greeks(
            option,
            &context,
            GreeksRequest::default(),
            Default::default()
        ),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        engine.price(option, &market(100.0, f64::MAX, -f64::MAX, 0.2)),
        Err(Error::ComputationFailure(_))
    ));
    assert!(matches!(
        engine.price(
            &options(
                OptionType::Call,
                100.0,
                10.0,
                Date::new(2027, 1, 1).unwrap()
            )[0],
            &market(100.0, 0.05, 0.02, f64::MAX)
        ),
        Err(Error::ComputationFailure(_))
    ));
    assert_eq!(engine.settings(), QuadratureSettings::default());
    assert!((engine.price(option, &context).unwrap() - 4.945_810_910_532_236).abs() < 1e-8);
}

#[test]
fn log_weights_and_tiny_volatility_preserve_representable_prices() {
    let engine = QuadratureDigitalEngine::new(QuadratureSettings {
        asset_step_count: 2048,
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for option in options(direction, 1.75e308, 1.75e308, dates().1) {
            let context = market(1.75e308, -0.03, -0.05, 0.2);
            let expected = AnalyticDigitalEngine.price(&option, &context).unwrap();
            assert!((engine.price(&option, &context).unwrap() / expected - 1.0).abs() < 1e-10);
            let overflow = market(1.75e308, -0.98, -1.0, 0.2);
            assert!(matches!(
                engine.price(&option, &overflow),
                Err(Error::ComputationFailure(_))
            ));
        }
    }
    for rate in [750.0, -750.0] {
        let amount = if rate > 0.0 { 1e300 } else { 1e-300 };
        for direction in [OptionType::Call, OptionType::Put] {
            for option in options(direction, amount, amount, dates().1) {
                let context = market(amount, rate, rate, 0.2);
                let expected = AnalyticDigitalEngine.price(&option, &context).unwrap();
                assert!((engine.price(&option, &context).unwrap() / expected - 1.0).abs() < 1e-10);
            }
        }
    }
    for sigma in [1e-200, f64::from_bits(1)] {
        for spot in [80.0, 100.0, 120.0] {
            let context = market(spot, 0.0, 0.0, sigma)
                .replace(PricingContextChanges {
                    valuation_time: Some(
                        Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999)
                            .unwrap(),
                    ),
                    ..Default::default()
                })
                .unwrap();
            for direction in [OptionType::Call, OptionType::Put] {
                for option in options(direction, 100.0, 10.0, dates().1) {
                    assert!(
                        (engine.price(&option, &context).unwrap()
                            - AnalyticDigitalEngine.price(&option, &context).unwrap())
                        .abs()
                            < 1e-10
                    );
                }
            }
        }
    }
}
