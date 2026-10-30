use kiyosi_core::*;

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
fn terminal_prices_converge_to_independent_references() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for (direction, vanilla, cash, asset) in [
        (
            OptionType::Call,
            9.227005508154036,
            4.945810910532236,
            58.6851146134764,
        ),
        (
            OptionType::Put,
            6.330080627549918,
            4.566483334474903,
            39.33475271719913,
        ),
    ] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let digital = DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap();
        let asset_option = DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap();
        let mut previous = [f64::INFINITY; 3];
        for steps in [100, 200, 400, 800] {
            let settings = FiniteDifferenceSettings {
                asset_step_count: steps,
                time_step_count: steps,
                ..Default::default()
            };
            let v = FiniteDifferenceVanillaEngine::new(settings).unwrap();
            let d = FiniteDifferenceDigitalEngine::new(settings).unwrap();
            for (i, (actual, expected)) in [
                (v.price(&option, &context).unwrap(), vanilla),
                (d.price(&digital, &context).unwrap(), cash),
                (d.price(&asset_option, &context).unwrap(), asset),
            ]
            .into_iter()
            .enumerate()
            {
                let error = (actual - expected).abs();
                assert!(error < previous[i], "{steps}: {actual} != {expected}");
                previous[i] = error;
            }
        }
        assert!(previous[0] < 0.002 && previous[1] < 0.001 && previous[2] < 0.01);
    }
}

#[test]
fn settings_validate_without_allocating_or_imposing_grid_caps() {
    let defaults = FiniteDifferenceSettings::default();
    assert_eq!(
        (defaults.asset_step_count, defaults.time_step_count),
        (800, 200)
    );
    assert_eq!(defaults.scheme, FiniteDifferenceScheme::CrankNicolson);
    assert_eq!(defaults.asset_upper_boundary, None);
    for (assets, times) in [(3, 1), (10_001, 100_001), (i32::MAX, i32::MAX)] {
        let settings = FiniteDifferenceSettings {
            asset_step_count: assets,
            time_step_count: times,
            ..defaults
        };
        assert_eq!(
            FiniteDifferenceVanillaEngine::new(settings)
                .unwrap()
                .settings(),
            settings
        );
        assert_eq!(
            FiniteDifferenceDigitalEngine::new(settings)
                .unwrap()
                .settings(),
            settings
        );
    }
    for settings in [
        FiniteDifferenceSettings {
            asset_step_count: 2,
            ..defaults
        },
        FiniteDifferenceSettings {
            time_step_count: 0,
            ..defaults
        },
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(0.0),
            ..defaults
        },
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(f64::NAN),
            ..defaults
        },
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(f64::INFINITY),
            ..defaults
        },
    ] {
        assert!(matches!(
            FiniteDifferenceVanillaEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            FiniteDifferenceDigitalEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

fn check_damping<I: OptionContract, E: PricingEngine<I>>(
    cn: &E,
    implicit: &E,
    option: &I,
    context: &PricingContext,
) {
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let actual = cn
        .price_with_greeks(option, context, request, Default::default())
        .unwrap();
    let expected = implicit
        .price_with_greeks(option, context, request, Default::default())
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.price(), cn.price(option, context).unwrap());
}

#[test]
fn rannacher_startup_equals_implicit_half_steps_for_all_contracts() {
    let (start, end) = dates();
    let context = market(99.25, 0.12, 0.02, 0.3);
    for steps in [1, 2] {
        let settings = FiniteDifferenceSettings {
            asset_step_count: 400,
            time_step_count: steps,
            asset_upper_boundary: Some(400.0),
            ..Default::default()
        };
        let implicit = FiniteDifferenceSettings {
            time_step_count: 2 * steps,
            scheme: FiniteDifferenceScheme::ImplicitEuler,
            ..settings
        };
        let v = FiniteDifferenceVanillaEngine::new(settings).unwrap();
        let vi = FiniteDifferenceVanillaEngine::new(implicit).unwrap();
        let d = FiniteDifferenceDigitalEngine::new(settings).unwrap();
        let di = FiniteDifferenceDigitalEngine::new(implicit).unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            check_damping(
                &v,
                &vi,
                &EuropeanOption::new(direction, 100.0, start, end).unwrap(),
                &context,
            );
            check_damping(
                &v,
                &vi,
                &AmericanOption::new(direction, 100.0, start, end).unwrap(),
                &context,
            );
            check_damping(
                &d,
                &di,
                &DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                &context,
            );
            check_damping(
                &d,
                &di,
                &DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
                &context,
            );
        }
    }
}

#[test]
fn schemes_track_prices_and_stability_includes_drift() {
    let (start, end) = dates();
    let context = market(100.25, 0.04, 0.01, 0.2);
    for scheme in [
        FiniteDifferenceScheme::ExplicitEuler,
        FiniteDifferenceScheme::ImplicitEuler,
        FiniteDifferenceScheme::CrankNicolson,
    ] {
        let settings = FiniteDifferenceSettings {
            asset_step_count: 200,
            time_step_count: 2000,
            scheme,
            ..Default::default()
        };
        let v = FiniteDifferenceVanillaEngine::new(settings).unwrap();
        let d = FiniteDifferenceDigitalEngine::new(settings).unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
            assert!(
                (v.price(&option, &context).unwrap()
                    - AnalyticEuropeanEngine.price(&option, &context).unwrap())
                .abs()
                    < 0.03
            );
            for digital in [
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                assert!(
                    (d.price(&digital, &context).unwrap()
                        - AnalyticDigitalEngine.price(&digital, &context).unwrap())
                    .abs()
                        < 0.05
                );
            }
        }
    }
    let option = EuropeanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    let unstable = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 200,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        unstable
            .price(&option, &market(95.0, 2.0, 0.0, 0.001))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn spline_greeks_use_one_mesh_and_selective_mixed_requests() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let context = market(100.25, 0.04, 0.01, 0.3);
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 400,
        time_step_count: 400,
        asset_upper_boundary: Some(410.0),
        ..Default::default()
    })
    .unwrap();
    let actual = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    let expected = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, GreeksRequest::all())
        .unwrap();
    for greek in Greek::ALL {
        let tolerance = expected.require(greek).unwrap().abs() * 0.02 + 1e-6;
        assert!(
            (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs() < tolerance,
            "{greek:?}: {actual:?} != {expected:?}"
        );
        let selected = engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([greek]),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            selected.require(greek).unwrap(),
            actual.require(greek).unwrap()
        );
        assert_eq!(selected.values().iter().flatten().count(), 1);
    }
    for shift in [0.01, 0.25] {
        let h = (context.spot_price() + shift) - context.spot_price();
        let gamma = |spot| {
            engine
                .price_with_greeks(
                    &option,
                    &market(spot, 0.04, 0.01, 0.3),
                    GreeksRequest::new([Greek::Gamma]),
                    Default::default(),
                )
                .unwrap()
                .require(Greek::Gamma)
                .unwrap()
        };
        let speed = engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Speed]),
                NumericalShiftSettings {
                    spot_shift: shift,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            speed.require(Greek::Speed).unwrap(),
            (gamma(100.25 + h) - gamma(100.25 - h)) / (2.0 * h)
        );
    }
    let collapsed = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Speed]),
            NumericalShiftSettings {
                spot_shift: 1e-300,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!collapsed.has(Greek::Speed));
    let near = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(100.5),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        near.price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Speed]),
            NumericalShiftSettings {
                spot_shift: 0.5,
                ..Default::default()
            }
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn american_exercise_and_continuation_boundaries_survive_negative_rates() {
    let (start, end) = dates();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        time_step_count: 800,
        ..Default::default()
    })
    .unwrap();
    for (direction, spot, rate, dividend, exercise) in [
        (OptionType::Call, 200.0, 0.03, 0.1, true),
        (OptionType::Put, 50.0, 0.1, 0.03, true),
        (OptionType::Call, 100.0, 0.05, 0.0, false),
        (OptionType::Call, 390.0, 0.05, 0.0, false),
        (OptionType::Put, 0.1, -0.05, 0.0, false),
    ] {
        let context = market(spot, rate, dividend, 0.2);
        let american = AmericanOption::new(direction, 100.0, start, end).unwrap();
        let price = engine.price(&american, &context).unwrap();
        let expected = if exercise {
            (spot - 100.0).abs()
        } else {
            AnalyticEuropeanEngine
                .price(
                    &EuropeanOption::new(direction, 100.0, start, end).unwrap(),
                    &context,
                )
                .unwrap()
        };
        assert!(
            (price - expected).abs() < 0.004,
            "{direction:?}: {price} != {expected}"
        );
    }
}

#[test]
fn node_curves_align_time_steps_and_keep_optimal_intermediate_exercise() {
    let (start, end) = dates();
    let middle = Date::new(2025, 7, 1).unwrap();
    let risk = RateTermStructure::new(
        start.into(),
        vec![middle.into(), end.into()],
        vec![0.1_f64.exp(), 1.0],
    )
    .unwrap();
    let model = BlackScholesMertonParameters::new(risk.clone(), 0.0, 0.2).unwrap();
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceVanillaEngine::default();
    for valuation in [
        start,
        Date::new(2025, 4, 1).unwrap(),
        middle,
        Date::new(2025, 10, 1).unwrap(),
    ] {
        let context = PricingContext::new(model.clone(), 0.01, valuation.into()).unwrap();
        let expected = 100.0
            * risk
                .factor(valuation.into(), valuation.max(middle).into())
                .unwrap()
            - 0.01;
        let actual = engine.price(&put, &context).unwrap();
        assert!(
            (actual - expected).abs() < 1e-5,
            "{valuation:?}: {actual} != {expected}"
        );
    }
    let context = PricingContext::new(model, 100.0, start.into()).unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        assert!(
            (engine.price(&option, &context).unwrap()
                - AnalyticEuropeanEngine.price(&option, &context).unwrap())
            .abs()
                < 0.004
        );
    }
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    assert!(matches!(
        FiniteDifferenceDigitalEngine::default()
            .price_with_greeks(
                &digital,
                &context,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn payoff_bounds_allow_carry_error_but_reject_oscillations() {
    let (start, end) = dates();
    let option =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.5, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceDigitalEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1000,
        time_step_count: 3,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine
            .price(&option, &market(20.0, 0.05, 0.02, 10.0))
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    for rate in [-0.05, 0.05] {
        for steps in [1, 200] {
            let context = market(100.0, rate, 0.4 * rate, 0.2);
            let engine = FiniteDifferenceDigitalEngine::new(FiniteDifferenceSettings {
                time_step_count: steps,
                ..Default::default()
            })
            .unwrap();
            for option in [
                DigitalOption::cash_or_nothing(OptionType::Call, 1.0, 100.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(OptionType::Call, 1.0, start, end).unwrap(),
            ] {
                let bound = 100.0
                    * (-if option.payout().is_some() {
                        rate
                    } else {
                        0.4 * rate
                    })
                    .exp();
                let price = engine.price(&option, &context).unwrap();
                assert!(price <= bound && (price - bound).abs() < 1e-8);
            }
        }
    }
}

#[test]
fn validation_timing_expiry_and_failures_preserve_inputs() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine.price(&option, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(end.into()),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&option, &expiry, GreeksRequest::all(), Default::default())
        .unwrap();
    assert_eq!(result.price(), 0.0);
    assert!(result.values().iter().all(Option::is_none));
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
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &expiry,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    rate_shift: 0.0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let before = context
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2024, 12, 31).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &before).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let tiny = f64::from_bits(1);
    let tiny_option = EuropeanOption::new(OptionType::Call, tiny, start, end).unwrap();
    assert!(matches!(
        FiniteDifferenceVanillaEngine::default()
            .price(&tiny_option, &market(tiny, 0.05, 0.02, 0.2))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(context, market(100.0, 0.05, 0.02, 0.2));
    assert_eq!(engine.settings().asset_upper_boundary, Some(1.0));
    let huge = FiniteDifferenceSettings {
        time_step_count: i32::MAX,
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    };
    let clipped = FiniteDifferenceVanillaEngine::new(huge).unwrap();
    assert!(matches!(
        clipped.price(&option, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        clipped
            .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    assert!(matches!(
        FiniteDifferenceDigitalEngine::new(huge)
            .unwrap()
            .price(&digital, &context)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

fn check_solver<I: OptionContract, E: PricingEngine<I>>(
    engine: &E,
    option: &I,
    context: &PricingContext,
) {
    let quote = engine.price(option, context).unwrap();
    let settings = ImpliedVolatilitySettings {
        lower_bound: 0.1,
        upper_bound: 0.22,
        ..Default::default()
    };
    assert!(
        (implied_volatility(engine, option, context, quote, settings).unwrap() - 0.2).abs() < 1e-7
    );
    assert!(matches!(
        implied_volatility(engine, option, context, -1.0, settings).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert_eq!(engine.price(option, context).unwrap(), quote);
}

#[test]
fn implied_volatility_reuses_the_scalar_engine_for_all_contracts() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for direction in [OptionType::Call, OptionType::Put] {
        check_solver(
            &FiniteDifferenceVanillaEngine::default(),
            &EuropeanOption::new(direction, 100.0, start, end).unwrap(),
            &context,
        );
        check_solver(
            &FiniteDifferenceVanillaEngine::default(),
            &AmericanOption::new(direction, 100.0, start, end).unwrap(),
            &context,
        );
        check_solver(
            &FiniteDifferenceDigitalEngine::default(),
            &DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
            &context,
        );
        check_solver(
            &FiniteDifferenceDigitalEngine::default(),
            &DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            &context,
        );
    }
}

#[test]
fn interpolation_and_implicit_euler_preserve_large_currency_scales() {
    let (start, end) = dates();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 200,
        scheme: FiniteDifferenceScheme::ImplicitEuler,
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let base = engine
            .price(
                &EuropeanOption::new(direction, 100.0, start, end).unwrap(),
                &market(100.0, 0.04, 0.01, 0.2),
            )
            .unwrap();
        for amount in [1e305, 1e306] {
            let actual = engine
                .price(
                    &EuropeanOption::new(direction, amount, start, end).unwrap(),
                    &market(amount, 0.04, 0.01, 0.2),
                )
                .unwrap();
            assert!((actual / amount / (base / 100.0) - 1.0).abs() < 1e-12);
        }
    }
}

#[test]
fn low_volatility_drift_stabilization_converges_and_large_grids_are_supported() {
    let (start, end) = dates();
    for direction in [OptionType::Call, OptionType::Put] {
        let call = direction == OptionType::Call;
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        for sigma in [0.001, 0.01] {
            let context = market(
                if call { 105.0 } else { 95.0 },
                if call { 0.0 } else { 0.1 },
                if call { 0.1 } else { 0.0 },
                sigma,
            );
            let expected = AnalyticEuropeanEngine.price(&option, &context).unwrap();
            for scheme in [
                FiniteDifferenceScheme::ExplicitEuler,
                FiniteDifferenceScheme::ImplicitEuler,
                FiniteDifferenceScheme::CrankNicolson,
            ] {
                let price = |count| {
                    FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
                        asset_step_count: count,
                        time_step_count: 2000,
                        scheme,
                        ..Default::default()
                    })
                    .unwrap()
                    .price(&option, &context)
                    .unwrap()
                };
                let (coarse, fine) = (price(200), price(800));
                assert!(
                    fine >= 0.0 && fine <= (if call { 105.0 } else { 100.0 }) * (-0.1_f64).exp()
                );
                assert!((fine - expected).abs() < (coarse - expected).abs());
            }
        }
    }
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    for (assets, times) in [(10_001, 200), (200, 100_001)] {
        let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_step_count: assets,
            time_step_count: times,
            ..Default::default()
        })
        .unwrap();
        assert!(
            (engine
                .price(&option, &market(100.0, 0.05, 0.02, 0.2))
                .unwrap()
                - 9.227005508154036)
                .abs()
                < 0.03
        );
    }
}

#[test]
fn scaled_digital_discounts_and_strict_expiry_payments_remain_representable() {
    let (start, end) = dates();
    let engine = FiniteDifferenceDigitalEngine::new(FiniteDifferenceSettings {
        asset_step_count: 100,
        time_step_count: 100_000,
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = DigitalOption::cash_or_nothing(direction, 100.0, 1e-308, start, end).unwrap();
        let context = market(100.0, -710.0, -710.0, 0.2);
        let expected = AnalyticDigitalEngine.price(&option, &context).unwrap();
        assert!((engine.price(&option, &context).unwrap() / expected - 1.0).abs() < 0.006);
        for payout in [Some(10.0), None] {
            let digital = if let Some(amount) = payout {
                DigitalOption::cash_or_nothing(direction, 100.0, amount, start, end).unwrap()
            } else {
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap()
            };
            for spot in [
                libm::nextafter(100.0, 0.0),
                100.0,
                libm::nextafter(100.0, 101.0),
            ] {
                let context = market(spot, 0.05, 0.02, 0.2)
                    .replace(PricingContextChanges {
                        valuation_time: Some(end.into()),
                        ..Default::default()
                    })
                    .unwrap();
                let result = engine
                    .price_with_greeks(&digital, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                let pays = if direction == OptionType::Call {
                    spot > 100.0
                } else {
                    spot < 100.0
                };
                assert_eq!(
                    result.price(),
                    if pays { payout.unwrap_or(spot) } else { 0.0 }
                );
                assert!(result.values().iter().all(Option::is_none));
            }
        }
    }
}
