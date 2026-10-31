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

#[test]
fn european_tree_refines_to_analytic_price_and_node_greeks() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let expected = AnalyticEuropeanEngine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        for steps in [255, 256, 4095, 4096] {
            let engine =
                CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: steps })
                    .unwrap();
            let actual = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            assert!((actual.price() - expected.price()).abs() < 3.0 / f64::from(steps));
            assert!(
                (actual.require(Greek::Delta).unwrap() - expected.require(Greek::Delta).unwrap())
                    .abs()
                    < 0.3 / f64::from(steps)
            );
            assert!(
                (actual.require(Greek::Gamma).unwrap() - expected.require(Greek::Gamma).unwrap())
                    .abs()
                    < 0.04 / f64::from(steps)
            );
            assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
            assert!(!actual.has(Greek::Vega));
        }
    }
}

#[test]
fn digital_tree_matches_one_step_cash_and_asset_payments() {
    let (start, end) = dates();
    let engine = CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: 1 }).unwrap();
    let context = market(100.0, 0.05, 0.02, 0.2);
    // One up branch pays 10 cash or 100*exp(0.2) assets, discounted at 5%.
    for (option, price, delta) in [
        (
            DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap(),
            5.0015369256733875,
            0.2483410784407258,
        ),
        (
            DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap(),
            61.08890996057412,
            3.033244781719736,
        ),
    ] {
        let result = engine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        assert!((result.price() - price).abs() < 1e-10);
        assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-10);
        for greek in Greek::ALL {
            assert_eq!(result.has(greek), greek == Greek::Delta);
        }
    }
}

#[test]
fn american_early_exercise_and_no_dividend_limit() {
    let (start, end) = dates();
    for steps in [1, 2, 3, 17, 255, 256, 257] {
        let engine =
            CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: steps }).unwrap();
        for (direction, strike, spot, rate, dividend, delta) in [
            (OptionType::Put, 100.0, 88.0, 0.15, 0.0, -1.0),
            (OptionType::Call, 88.0, 100.0, 0.0, 0.15, 1.0),
        ] {
            let option = AmericanOption::new(direction, strike, start, end).unwrap();
            let result = engine
                .price_with_greeks(
                    &option,
                    &market(spot, rate, dividend, 0.2),
                    GreeksRequest::all(),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(result.price(), 12.0);
            assert_eq!(result.get(Greek::Delta), Some(delta));
            assert_eq!(result.get(Greek::Gamma), Some(0.0));
        }
        let european = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let american = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let context = market(100.0, 0.05, 0.0, 0.2);
        assert!(
            (engine.price(&european, &context).unwrap()
                - engine.price(&american, &context).unwrap())
            .abs()
                < 1e-10
        );
        let result = engine
            .price_with_greeks(
                &american,
                &context,
                GreeksRequest::all(),
                Default::default(),
            )
            .unwrap();
        assert!(Greek::ALL.into_iter().all(|g| result.has(g)));
    }
}

#[test]
fn digital_odd_even_and_off_strike_grids_refine_with_negative_carry() {
    let (start, end) = dates();
    for (rate, dividend, sigma) in [
        (0.05, 0.02, 0.2),
        (-0.03, 0.01, 0.45),
        (0.01, 0.07, 0.3),
        (-0.02, -0.05, 0.15),
    ] {
        for spot in [95.0, 100.0, 105.0] {
            let context = market(spot, rate, dividend, sigma);
            for direction in [OptionType::Call, OptionType::Put] {
                for option in [
                    DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                    DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
                ] {
                    let expected = AnalyticDigitalEngine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                            Default::default(),
                        )
                        .unwrap();
                    for steps in [255, 256, 4095, 4096] {
                        let engine = CoxRossRubinsteinDigitalEngine::new(BinomialSettings {
                            step_count: steps,
                        })
                        .unwrap();
                        let actual = engine
                            .price_with_greeks(&option, &context, GreeksRequest::all())
                            .unwrap();
                        let budget = option.payout().unwrap_or(spot) / f64::from(steps).sqrt();
                        assert!((actual.price() - expected.price()).abs() < budget);
                        assert!(
                            (actual.require(Greek::Delta).unwrap()
                                - expected.require(Greek::Delta).unwrap())
                            .abs()
                                < budget / spot / sigma
                        );
                        assert!(
                            (actual.require(Greek::Gamma).unwrap()
                                - expected.require(Greek::Gamma).unwrap())
                            .abs()
                                < budget / spot / spot / sigma / sigma
                        );
                        assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
                        assert!(!actual.has(Greek::Vega));
                    }
                }
            }
        }
    }
}

#[test]
fn expiry_boundaries_settle_strictly_and_life_errors_precede_trees() {
    let (start, end) = dates();
    let vanilla = CoxRossRubinsteinVanillaEngine::default();
    let digital = CoxRossRubinsteinDigitalEngine::default();
    for direction in [OptionType::Call, OptionType::Put] {
        let european = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let american = AmericanOption::new(direction, 100.0, start, end).unwrap();
        for spot in [90.0, 100.0, 110.0] {
            let context = market(spot, 100.0, -100.0, 1e-200)
                .replace(PricingContextChanges {
                    valuation_time: Some(end.into()),
                    ..Default::default()
                })
                .unwrap();
            let intrinsic = (if direction == OptionType::Call {
                spot - 100.0
            } else {
                100.0 - spot
            })
            .max(0.0);
            for result in [
                vanilla
                    .price_with_greeks(
                        &european,
                        &context,
                        GreeksRequest::all(),
                        Default::default(),
                    )
                    .unwrap(),
                vanilla
                    .price_with_greeks(
                        &american,
                        &context,
                        GreeksRequest::all(),
                        Default::default(),
                    )
                    .unwrap(),
            ] {
                assert_eq!(result.price(), intrinsic);
                assert!(result.values().iter().all(Option::is_none));
            }
            for option in [
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                let result = digital
                    .price_with_greeks(&option, &context, GreeksRequest::all())
                    .unwrap();
                assert_eq!(
                    result.price(),
                    if intrinsic > 0.0 {
                        option.payout().unwrap_or(spot)
                    } else {
                        0.0
                    }
                );
                assert!(result.values().iter().all(Option::is_none));
                assert!(matches!(
                    implied_volatility(
                        &digital,
                        &option,
                        &context,
                        result.price(),
                        Default::default()
                    )
                    .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
            }
        }
        for time in [
            Date::new(2024, 12, 31).unwrap().into(),
            Timestamp::new(end, 0, 0, 0, 1).unwrap(),
        ] {
            let context = market(100.0, 0.05, 0.02, 1e-200)
                .replace(PricingContextChanges {
                    valuation_time: Some(time),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                vanilla.price(&european, &context).unwrap_err(),
                Error::InvalidInput(_)
            ));
            assert!(matches!(
                vanilla.price(&american, &context).unwrap_err(),
                Error::InvalidInput(_)
            ));
            let option =
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap();
            assert!(matches!(
                digital.price(&option, &context).unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
    }
}

#[test]
fn settings_flat_curve_validation_and_unstable_trees_preserve_inputs() {
    assert_eq!(BinomialSettings::default().step_count, 256);
    for steps in [-1, 0, 1_000_001] {
        assert!(matches!(
            CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: steps })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: steps })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for steps in [1, 1_000_000] {
        assert_eq!(
            CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: steps })
                .unwrap()
                .settings()
                .step_count,
            steps
        );
        assert_eq!(
            CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: steps })
                .unwrap()
                .settings()
                .step_count,
            steps
        );
    }
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let vanilla = CoxRossRubinsteinVanillaEngine::default();
    let digital = CoxRossRubinsteinDigitalEngine::default();
    let nodes =
        RateTermStructure::new(start.into(), vec![end.into()], vec![(-0.05_f64).exp()]).unwrap();
    let nonflat = PricingContext::new(
        BlackScholesMertonParameters::new(nodes, 0.02, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    assert!(matches!(
        vanilla
            .price_with_greeks(
                &option,
                &nonflat,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        digital
            .price_with_greeks(&cash, &nonflat, GreeksRequest::default())
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let flat = RateTermStructure::flat(0.05, Some(start.into())).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(flat, 0.02, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    assert_eq!(
        vanilla.price(&option, &context).unwrap(),
        vanilla
            .price(&option, &market(100.0, 0.05, 0.02, 0.2))
            .unwrap()
    );
    for settings in [
        NumericalShiftSettings {
            spot_shift: 0.0,
            ..Default::default()
        },
        NumericalShiftSettings {
            time_shift_days: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            vanilla
                .price_with_greeks(&option, &context, GreeksRequest::all(), settings)
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for (sigma, rate, dividend) in [
        (1e-200, 0.05, 0.02),
        (50.0, 0.05, 0.02),
        (0.2, -1000.0, -1000.0),
    ] {
        let bad = market(100.0, rate, dividend, sigma);
        assert!(matches!(
            vanilla.price(&option, &bad).unwrap_err(),
            Error::ComputationFailure(_)
        ));
        assert!(matches!(
            digital.price(&cash, &bad).unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    let before = context.clone();
    let quote = vanilla.price(&option, &context).unwrap();
    assert!(matches!(
        implied_volatility(&vanilla, &option, &context, -1.0, Default::default()).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert_eq!(context, before);
    assert_eq!(vanilla.settings(), BinomialSettings::default());
    assert_eq!(vanilla.price(&option, &context).unwrap(), quote);
}

#[test]
fn implied_brackets_intersect_tree_domain_for_all_supported_contracts() {
    let (start, end) = dates();
    let vanilla = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 17 }).unwrap();
    let digital = CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: 17 }).unwrap();
    let context = market(100.0, 0.15, 0.0, 0.2);
    fn recover<I: OptionContract, E: PricingEngine<I>>(
        engine: &E,
        option: &I,
        context: &PricingContext,
    ) {
        let quote = engine.price(option, context).unwrap();
        let settings = ImpliedVolatilitySettings {
            lower_bound: 0.0001,
            upper_bound: 0.4,
            ..Default::default()
        };
        assert!(
            (implied_volatility(engine, option, context, quote, settings).unwrap() - 0.2).abs()
                < 1e-6
        );
        assert!(matches!(
            implied_volatility(
                engine,
                option,
                context,
                quote,
                ImpliedVolatilitySettings {
                    upper_bound: 0.01,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            Error::RootNotBracketed(_)
        ));
    }
    recover(
        &vanilla,
        &EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap(),
        &context,
    );
    recover(
        &vanilla,
        &AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap(),
        &context,
    );
    for direction in [OptionType::Call, OptionType::Put] {
        recover(
            &digital,
            &DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            &context,
        );
        recover(
            &digital,
            &DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
            &context,
        );
    }
}

#[test]
fn deterministic_endpoints_and_underflowed_tail_weights_remain_finite() {
    let (start, end) = dates();
    let digital = CoxRossRubinsteinDigitalEngine::default();
    for rate in [-3.2_f64, 3.2] {
        let context = market(100.0, rate, 0.0, 0.2);
        for direction in [OptionType::Call, OptionType::Put] {
            let pays = (direction == OptionType::Call) == (rate > 0.0);
            for option in [
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                let expected = if !pays {
                    0.0
                } else {
                    option
                        .payout()
                        .map_or(100.0, |payout| payout * (-rate).exp())
                };
                assert!((digital.price(&option, &context).unwrap() - expected).abs() < 1e-9);
            }
        }
    }
    let option = EuropeanOption::new(OptionType::Call, 1e280 * 8.0_f64.exp(), start, end).unwrap();
    let context = market(1e280, 0.04, 0.01, 0.2);
    let engine =
        CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 2048 }).unwrap();
    // Tail probabilities underflow individually, while their payoff-weighted sum is positive.
    let price = engine.price(&option, &context).unwrap();
    // Independently rolled back in the public Python contract test.
    assert!((price / 2.249054371067649e-140 - 1.0).abs() < 1e-9);
    assert_eq!(
        price,
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                Default::default()
            )
            .unwrap()
            .price()
    );
}

#[test]
fn vanilla_numerical_completion_tracks_analytic_and_propagates_bump_failures() {
    let (start, end) = dates();
    let engine =
        CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 1000 }).unwrap();
    let context = market(100.0, 0.04, 0.01, 0.2)
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(start, 12, 0, 0, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let actual = engine
            .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
            .unwrap();
        let expected = AnalyticEuropeanEngine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        for greek in [
            Greek::Speed,
            Greek::Vanna,
            Greek::Zomma,
            Greek::Charm,
            Greek::Color,
        ] {
            let reference = expected.require(greek).unwrap();
            assert!(
                (actual.require(greek).unwrap() - reference).abs() < reference.abs() * 0.02 + 1e-7,
                "{greek:?}"
            );
        }
        let generic = PricingEngine::price_with_greeks(
            &engine,
            &option,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(generic, actual);
    }
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 1 }).unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let context = market(100.0, 0.15, 0.0, 0.2);
    let before = context.clone();
    let price = engine.price(&option, &context).unwrap();
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Vega]),
                NumericalShiftSettings {
                    volatility_shift: 0.1,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(context, before);
    assert_eq!(engine.price(&option, &context).unwrap(), price);
}

#[test]
fn one_step_speed_omits_an_infeasible_scalar_stencil() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 0.015, start, end).unwrap();
    let context = market(0.015, 0.05, 0.02, 0.2);
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 1 }).unwrap();
    let result = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Speed]),
            Default::default(),
        )
        .unwrap();
    assert_eq!(result.price(), engine.price(&option, &context).unwrap());
    assert_eq!(result.get(Greek::Speed), None);
}
