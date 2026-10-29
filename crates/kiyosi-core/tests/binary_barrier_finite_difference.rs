use kiyosi_core::*;

fn start() -> Date {
    Date::new(2025, 1, 1).unwrap()
}
fn end() -> Date {
    Date::new(2026, 1, 1).unwrap()
}
fn market() -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        start().into(),
    )
    .unwrap()
}

#[test]
fn continuous_cash_binary_converges_to_analytic_price() {
    let option = BinaryBarrierOption::cash(
        BinaryBarrierTerms::new(
            OptionType::Call,
            100.0,
            start(),
            end(),
            120.13,
            BarrierType::UpAndOut,
        ),
        10.0,
    )
    .unwrap();
    let expected = AnalyticBinaryBarrierEngine
        .price(&option, &market())
        .unwrap();
    let price = FiniteDifferenceBinaryBarrierEngine::default()
        .price(&option, &market())
        .unwrap();
    assert!((price - expected).abs() < 0.003, "{price} != {expected}");
}

fn engine(steps: i32) -> FiniteDifferenceBinaryBarrierEngine {
    FiniteDifferenceBinaryBarrierEngine::new(FiniteDifferenceSettings {
        asset_step_count: steps,
        time_step_count: steps,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn continuous_binary_matrix_matches_analytics_and_touch_settlements_converge() {
    for (inside, outside, level) in [
        (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.13),
        (BarrierType::DownAndIn, BarrierType::DownAndOut, 79.87),
    ] {
        for kind in [inside, outside] {
            for asset in [false, true] {
                for direction in [OptionType::Call, OptionType::Put] {
                    for strike in [70.0, 100.0, 130.0] {
                        let terms =
                            BinaryBarrierTerms::new(direction, strike, start(), end(), level, kind);
                        let option = if asset {
                            BinaryBarrierOption::asset(terms)
                        } else {
                            BinaryBarrierOption::cash(terms, 10.0)
                        }
                        .unwrap();
                        let expected = AnalyticBinaryBarrierEngine
                            .price(&option, &market())
                            .unwrap();
                        let fine = engine(1600).price(&option, &market()).unwrap();
                        let tolerance = if asset { 0.008 } else { 0.0008 };
                        assert!(
                            (fine - expected).abs() < tolerance,
                            "{kind:?} {direction:?} {asset} {strike}: {fine} != {expected}"
                        );
                    }
                }
                for timing in [SettlementTiming::AtExpiry, SettlementTiming::AtHit] {
                    if kind == outside && timing == SettlementTiming::AtHit {
                        continue;
                    }
                    let mut terms = TouchOptionTerms::new(start(), end(), level, kind);
                    terms.settlement_timing = timing;
                    let option = if asset {
                        TouchOption::asset(terms)
                    } else {
                        TouchOption::cash(terms, 10.0)
                    }
                    .unwrap();
                    let expected = AnalyticBinaryBarrierEngine
                        .price(&option, &market())
                        .unwrap();
                    let coarse = engine(80).price(&option, &market()).unwrap();
                    let fine = engine(1600).price(&option, &market()).unwrap();
                    assert!(
                        (fine - expected).abs() < if asset { 0.004 } else { 0.0004 },
                        "{kind:?} {asset} {timing:?}: {fine} != {expected}"
                    );
                    assert!((fine - expected).abs() <= (coarse - expected).abs());
                }
            }
        }
    }
}

#[test]
fn scheduled_single_expiry_observation_matches_digital_portfolios() {
    for (inside, outside, level) in [
        (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.125),
        (BarrierType::DownAndIn, BarrierType::DownAndOut, 79.875),
    ] {
        for asset in [false, true] {
            let digital = |direction, strike| {
                let option = if asset {
                    DigitalOption::asset_or_nothing(direction, strike, start(), end())
                } else {
                    DigitalOption::cash_or_nothing(direction, strike, 10.0, start(), end())
                }
                .unwrap();
                AnalyticDigitalEngine.price(&option, &market()).unwrap()
            };
            let touched = digital(
                if level > 100.0 {
                    OptionType::Call
                } else {
                    OptionType::Put
                },
                level,
            );
            let total = if asset {
                100.0 * (-0.01_f64).exp()
            } else {
                10.0 * (-0.04_f64).exp()
            };
            for kind in [inside, outside] {
                let mut terms = TouchOptionTerms::new(start(), end(), level, kind);
                terms.observation_mode = ObservationMode::Scheduled;
                terms.observation_dates = vec![end()];
                let option = if asset {
                    TouchOption::asset(terms)
                } else {
                    TouchOption::cash(terms, 10.0)
                }
                .unwrap();
                let expected = if kind == inside {
                    touched
                } else {
                    total - touched
                };
                assert!(
                    (engine(1600).price(&option, &market()).unwrap() - expected).abs()
                        < if asset { 0.003 } else { 0.0003 }
                );
                for direction in [OptionType::Call, OptionType::Put] {
                    let mut terms =
                        BinaryBarrierTerms::new(direction, 100.0, start(), end(), level, kind);
                    terms.observation_mode = ObservationMode::Scheduled;
                    terms.observation_dates = vec![end()];
                    let option = if asset {
                        BinaryBarrierOption::asset(terms)
                    } else {
                        BinaryBarrierOption::cash(terms, 10.0)
                    }
                    .unwrap();
                    let hit_payoff = if (level > 100.0) == (direction == OptionType::Call) {
                        touched
                    } else {
                        0.0
                    };
                    let expected = if kind == inside {
                        hit_payoff
                    } else {
                        digital(direction, 100.0) - hit_payoff
                    };
                    assert!(
                        (engine(1600).price(&option, &market()).unwrap() - expected).abs()
                            < if asset { 0.004 } else { 0.0004 }
                    );
                }
            }
        }
    }
}

fn context(spot: f64, time: Timestamp) -> PricingContext {
    market()
        .replace(PricingContextChanges {
            spot_price: Some(spot),
            valuation_time: Some(time),
            ..Default::default()
        })
        .unwrap()
}
#[test]
fn history_current_expiry_and_prior_payments_use_core_rules() {
    let engine = engine(400);
    for asset in [false, true] {
        for timing in [SettlementTiming::AtHit, SettlementTiming::AtExpiry] {
            let mut terms = TouchOptionTerms::new(start(), end(), 120.0, BarrierType::UpAndIn);
            terms.settlement_timing = timing;
            let option = if asset {
                TouchOption::asset(terms)
            } else {
                TouchOption::cash(terms, 10.0)
            }
            .unwrap();
            let amount = if asset { 130.0 } else { 10.0 };
            let expected = if timing == SettlementTiming::AtHit {
                amount
            } else {
                amount * (-if asset { 0.01_f64 } else { 0.04_f64 }).exp()
            };
            assert!(
                (engine
                    .price(&option, &context(130.0, start().into()))
                    .unwrap()
                    - expected)
                    .abs()
                    < 1e-12
            );
            let tomorrow = Date::new(2025, 1, 2).unwrap().start_of_day();
            assert!(matches!(
                engine
                    .price(&option, &context(130.0, tomorrow))
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            let known = option.replace(Some(BarrierTouchState::Touched)).unwrap();
            assert!(matches!(
                engine.price(&known, &market()).unwrap_err(),
                Error::InvalidInput(_)
            ));
            let value = engine.price(&known, &context(100.0, tomorrow)).unwrap();
            if timing == SettlementTiming::AtHit {
                assert_eq!(value, 0.0);
            }
            assert!(matches!(
                implied_volatility(
                    &engine,
                    &known,
                    &context(100.0, tomorrow),
                    value,
                    Default::default(),
                )
                .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            let untouched = option.replace(Some(BarrierTouchState::Untouched)).unwrap();
            for spot in [100.0, 120.0, 130.0] {
                let ctx = context(spot, end().into());
                let expected = if spot >= 120.0 {
                    if asset { spot } else { 10.0 }
                } else {
                    0.0
                };
                let result = engine
                    .price_with_greeks(&untouched, &ctx, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert_eq!(result.price(), expected);
                assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
            }
            assert_eq!(option.touch_state(), None);
        }
        for kind in [BarrierType::UpAndIn, BarrierType::UpAndOut] {
            let option = if asset {
                BinaryBarrierOption::asset(BinaryBarrierTerms::new(
                    OptionType::Call,
                    100.0,
                    start(),
                    end(),
                    120.0,
                    kind,
                ))
            } else {
                BinaryBarrierOption::cash(
                    BinaryBarrierTerms::new(OptionType::Call, 100.0, start(), end(), 120.0, kind),
                    10.0,
                )
            }
            .unwrap()
            .replace(Some(BarrierTouchState::Untouched))
            .unwrap();
            assert_eq!(
                engine
                    .price(&option, &context(100.0, end().into()))
                    .unwrap(),
                0.0
            );
            let ctx = context(110.0, end().into());
            assert_eq!(
                engine.price(&option, &ctx).unwrap(),
                if kind == BarrierType::UpAndOut {
                    if asset { 110.0 } else { 10.0 }
                } else {
                    0.0
                }
            );
        }
    }
}

#[test]
fn early_scheduled_at_hit_horizon_and_resolved_flows_use_only_required_curves() {
    let fixing = Date::new(2025, 6, 30).unwrap();
    let curve = RateTermStructure::new(start().into(), vec![fixing.into()], vec![0.97]).unwrap();
    let model = BlackScholesMertonParameters::new(curve.clone(), curve, 0.2).unwrap();
    let ctx = market()
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    let mut terms = TouchOptionTerms::new(start(), end(), 120.125, BarrierType::UpAndIn);
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_dates = vec![fixing];
    terms.settlement_timing = SettlementTiming::AtHit;
    let option = TouchOption::cash(terms.clone(), 10.0).unwrap();
    let digital = DigitalOption::cash_or_nothing(
        OptionType::Call,
        terms.barrier_level,
        10.0,
        start(),
        fixing,
    )
    .unwrap();
    let expected = AnalyticDigitalEngine.price(&digital, &ctx).unwrap();
    assert!((engine(1600).price(&option, &ctx).unwrap() - expected).abs() < 0.0004);
    terms.settlement_timing = SettlementTiming::AtExpiry;
    assert!(matches!(
        engine(400)
            .price(&TouchOption::cash(terms, 10.0).unwrap(), &ctx)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let at = context(130.0, fixing.into());
    assert_eq!(engine(400).price(&option, &at).unwrap(), 10.0);
    let after = context(130.0, Timestamp::new(fixing, 0, 0, 0, 1).unwrap());
    assert!(matches!(
        engine(400).price(&option, &after).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(
        engine(400)
            .price(
                &option.replace(Some(BarrierTouchState::Untouched)).unwrap(),
                &after
            )
            .unwrap(),
        0.0
    );
}

#[test]
fn greeks_and_implied_volatility_preserve_inputs_and_error_categories() {
    let mut terms = TouchOptionTerms::new(start(), end(), 120.13, BarrierType::UpAndIn);
    terms.touch_state = Some(BarrierTouchState::Untouched);
    let option = TouchOption::cash(terms, 10.0).unwrap();
    let engine = engine(800);
    let ctx = context(100.0, Date::new(2025, 1, 2).unwrap().into());
    let result = engine
        .price_with_greeks(&option, &ctx, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| result.has(g)));
    let scalar = numerical_greeks(
        &engine,
        &option,
        &ctx,
        GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega, Greek::Rho]),
        Default::default(),
    )
    .unwrap();
    for greek in [Greek::Delta, Greek::Gamma, Greek::Vega, Greek::Rho] {
        assert!(
            (scalar.require(greek).unwrap() - result.require(greek).unwrap()).abs() < 1e-5,
            "{greek:?}"
        );
    }
    let recovered = implied_volatility(
        &engine,
        &option,
        &ctx,
        result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.1,
            upper_bound: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((recovered - 0.2).abs() < 1e-7);
    assert!(matches!(
        implied_volatility(&engine, &option, &ctx, -1.0, Default::default()).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(&option, &ctx, GreeksRequest::default(), Default::default())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let at = context(120.13, start().into());
    let unknown = option.replace(None).unwrap();
    let result_at = engine
        .price_with_greeks(&unknown, &at, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| !result_at.has(g)));
    assert_eq!(engine.price(&option, &ctx).unwrap(), result.price());
    assert_eq!(ctx.spot_price(), 100.0);
}

#[test]
fn grids_schedules_and_validation_fail_before_mutating_inputs() {
    let mut terms = TouchOptionTerms::new(start(), end(), 120.13, BarrierType::UpAndIn);
    let option = TouchOption::cash(terms.clone(), 10.0).unwrap();
    for settings in [
        FiniteDifferenceSettings {
            asset_step_count: 2,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceBinaryBarrierEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for settings in [
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(120.0),
            ..Default::default()
        },
        FiniteDifferenceSettings {
            time_step_count: 1,
            scheme: FiniteDifferenceScheme::ExplicitEuler,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceBinaryBarrierEngine::new(settings)
                .unwrap()
                .price(&option, &market())
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_dates = vec![Date::new(2025, 1, 4).unwrap()];
    terms.touch_state = Some(BarrierTouchState::Touched);
    let weekend = TouchOption::cash(terms.clone(), 10.0).unwrap();
    assert!(matches!(
        engine(400).price(&weekend, &market()).unwrap_err(),
        Error::InvalidInput(_)
    ));
    terms.barrier_type = BarrierType::UpAndOut;
    terms.settlement_timing = SettlementTiming::AtHit;
    assert!(matches!(
        TouchOption::cash(terms, 10.0).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(option.touch_state(), None);
}

#[test]
fn two_fixing_cash_asset_and_settlement_prices_match_independent_integrals() {
    // Conditional lognormal integration from the Python public API regression.
    for (inside, outside, level, references) in [
        (
            BarrierType::DownAndIn,
            BarrierType::DownAndOut,
            79.875,
            [
                [1.334200186678343, 1.3442400990881977, 8.27369448526815],
                [9.978663535801967, 9.997735021151383, 89.0263223086682],
            ],
        ),
        (
            BarrierType::UpAndIn,
            BarrierType::UpAndOut,
            120.125,
            [
                [2.1707504345604853, 2.190815043056702, 7.437144244500472],
                [28.447837233767252, 28.512856309452633, 70.55714926041419],
            ],
        ),
    ] {
        for asset in [false, true] {
            for (index, (kind, timing)) in [
                (inside, SettlementTiming::AtExpiry),
                (inside, SettlementTiming::AtHit),
                (outside, SettlementTiming::AtExpiry),
            ]
            .into_iter()
            .enumerate()
            {
                let mut terms = TouchOptionTerms::new(start(), end(), level, kind);
                terms.observation_mode = ObservationMode::Scheduled;
                terms.observation_dates = vec![Date::new(2025, 6, 30).unwrap(), end()];
                terms.settlement_timing = timing;
                let option = if asset {
                    TouchOption::asset(terms)
                } else {
                    TouchOption::cash(terms, 10.0)
                }
                .unwrap();
                let expected = references[usize::from(asset)][index];
                let value = engine(1600).price(&option, &market()).unwrap();
                assert!((value - expected).abs() < if asset { 0.001 } else { 0.0001 });
            }
        }
    }
}

#[test]
fn explicit_implicit_and_damped_schemes_preserve_supported_payoffs() {
    for kind in [BarrierType::UpAndIn, BarrierType::UpAndOut] {
        let option =
            TouchOption::cash(TouchOptionTerms::new(start(), end(), 120.13, kind), 10.0).unwrap();
        let expected = AnalyticBinaryBarrierEngine
            .price(&option, &market())
            .unwrap();
        for scheme in [
            FiniteDifferenceScheme::ExplicitEuler,
            FiniteDifferenceScheme::ImplicitEuler,
            FiniteDifferenceScheme::CrankNicolson,
        ] {
            let engine = FiniteDifferenceBinaryBarrierEngine::new(FiniteDifferenceSettings {
                asset_step_count: 100,
                time_step_count: 2000,
                scheme,
                asset_upper_boundary: Some(400.0),
            })
            .unwrap();
            assert!((engine.price(&option, &market()).unwrap() - expected).abs() < 0.03);
        }
    }
}

#[test]
fn continuous_binary_domains_retain_configured_nodes_and_explicit_stability() {
    let option = TouchOption::cash(
        TouchOptionTerms::new(start(), end(), 120.0, BarrierType::UpAndIn),
        10.0,
    )
    .unwrap();
    let make = |upper, scheme| {
        FiniteDifferenceBinaryBarrierEngine::new(FiniteDifferenceSettings {
            asset_step_count: 800,
            time_step_count: 2000,
            scheme,
            asset_upper_boundary: Some(upper),
        })
        .unwrap()
    };
    let expected = make(400.0, FiniteDifferenceScheme::CrankNicolson)
        .price(&option, &market())
        .unwrap();
    for upper in [10_000.0, 1e9] {
        assert_eq!(
            make(upper, FiniteDifferenceScheme::CrankNicolson)
                .price(&option, &market())
                .unwrap(),
            expected
        );
    }
    assert!(matches!(
        make(480.0, FiniteDifferenceScheme::ExplicitEuler)
            .price(&option, &market())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}
