use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}
fn surface(values: Vec<Vec<f64>>) -> LocalVolatilitySurface {
    let (a, b) = dates();
    LocalVolatilitySurface::new(vec![a, b], vec![80.0, 120.0], values).unwrap()
}
fn context(surface: LocalVolatilitySurface) -> PricingContext {
    PricingContext::new(
        LocalVolatilityParameters::new(0.05, 0.02, surface).unwrap(),
        100.0,
        dates().0,
    )
    .unwrap()
}
fn engine(n: i32, t: i32, upper: f64) -> FiniteDifferenceVanillaEngine {
    FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: n,
        time_step_count: t,
        asset_upper_boundary: Some(upper),
        ..Default::default()
    })
    .unwrap()
}
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b} (tolerance {tol})");
}

#[test]
fn surface_interpolates_sigma_and_flat_price_tails_without_time_extrapolation() {
    let s = surface(vec![vec![0.2, 0.4], vec![0.4, 0.8]]);
    let (a, b) = dates();
    let mid = Date::new(2025, 7, 2).unwrap().start_of_day();
    close(
        s.volatility(mid, 100.0).unwrap(),
        0.3 + 0.3 * 182.0 / 365.0,
        1e-14,
    );
    for (time, spot, expected) in [
        (a, 1.0, 0.2),
        (a, 140.0, 0.4),
        (b, 80.0, 0.4),
        (b, 120.0, 0.8),
    ] {
        close(s.volatility(time, spot).unwrap(), expected, 1e-14);
    }
    for time in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        Date::new(2026, 1, 2).unwrap().start_of_day(),
    ] {
        assert!(matches!(
            s.volatility(time, 100.0),
            Err(Error::InvalidInput(_))
        ));
    }
    for spot in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(s.volatility(a, spot), Err(Error::InvalidInput(_))));
    }
    assert_eq!(s.maximum_volatility(), 0.8);
    assert_eq!(
        s.values().collect::<Vec<_>>(),
        vec![&[0.2, 0.4][..], &[0.4, 0.8][..]]
    );
}

#[test]
fn invalid_table_partitions_are_rejected() {
    let (a, b) = dates();
    let valid_times = vec![a, b];
    for (times, spots, values) in [
        (vec![], vec![80.0, 120.0], vec![]),
        (vec![a], vec![80.0, 120.0], vec![vec![0.2, 0.2]]),
        (vec![b, a], vec![80.0, 120.0], vec![vec![0.2; 2]; 2]),
        (vec![a, a], vec![80.0, 120.0], vec![vec![0.2; 2]; 2]),
        (valid_times.clone(), vec![80.0], vec![vec![0.2]; 2]),
        (
            valid_times.clone(),
            vec![120.0, 80.0],
            vec![vec![0.2; 2]; 2],
        ),
        (valid_times.clone(), vec![80.0, 80.0], vec![vec![0.2; 2]; 2]),
        (valid_times.clone(), vec![80.0, 120.0], vec![vec![0.2]; 2]),
        (valid_times.clone(), vec![80.0, 120.0], vec![vec![0.2; 2]]),
    ] {
        assert!(matches!(
            LocalVolatilitySurface::new(times, spots, values),
            Err(Error::InvalidInput(_))
        ));
    }
    for bad in [0.0, -0.1, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            LocalVolatilitySurface::new(
                valid_times.clone(),
                vec![bad, 120.0],
                vec![vec![0.2; 2]; 2]
            ),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            LocalVolatilitySurface::new(
                valid_times.clone(),
                vec![80.0, 120.0],
                vec![vec![0.2, bad]; 2]
            ),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn interpolation_preserves_extreme_positive_constant_surfaces() {
    for value in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX] {
        let s = surface(vec![vec![value; 2]; 2]);
        for spot in [80.0, 90.0, 100.0, 110.0, 140.0] {
            assert_eq!(
                s.volatility(Date::new(2025, 7, 2).unwrap().start_of_day(), spot)
                    .unwrap(),
                value
            );
        }
    }
}

#[test]
fn context_replacement_checks_coverage_and_preserves_absolute_surface() {
    let c = context(surface(vec![vec![0.2, 0.4]; 2]));
    let ModelParameters::LocalVolatility(m) = c.model_parameters() else {
        panic!()
    };
    assert!(matches!(
        m.replace(LocalVolatilityParameterChanges {
            risk_free_rate: Some(f64::NAN.into()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(m.risk_free_curve().exact_flat_rate(), Some(0.05));
    let later = c
        .replace(PricingContextChanges {
            spot_price: Some(140.0),
            valuation_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(later.model_parameters(), c.model_parameters());
    assert_eq!(c.spot_price(), 100.0);
    assert!(matches!(
        c.replace(PricingContextChanges {
            valuation_time: Some(Date::new(2026, 1, 2).unwrap().start_of_day()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    close(
        m.surface().volatility(dates().0, 100.0).unwrap(),
        0.3,
        1e-14,
    );
    drop(c);
    let ModelParameters::LocalVolatility(model) = later.model_parameters() else {
        panic!()
    };
    close(
        model.surface().volatility(dates().0, 140.0).unwrap(),
        0.4,
        1e-14,
    );
}

#[test]
fn automatic_domains_ignore_surface_outside_remaining_horizon() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceVanillaEngine::default();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let baseline = engine
        .price_with_greeks(&option, &context(surface(vec![vec![0.2; 2]; 2])), request)
        .unwrap();
    close(baseline.price(), 9.227_005_508_154_036, 0.002);
    for (past, future) in [(1.5, 0.2), (0.2, 1.5), (1.5, 1.5)] {
        let extended = LocalVolatilitySurface::new(
            vec![
                Date::new(2024, 1, 1).unwrap().start_of_day(),
                start,
                end,
                Date::new(2030, 1, 1).unwrap().start_of_day(),
            ],
            vec![80.0, 120.0],
            vec![vec![past; 2], vec![0.2; 2], vec![0.2; 2], vec![future; 2]],
        )
        .unwrap();
        let actual = engine
            .price_with_greeks(&option, &context(extended), request)
            .unwrap();
        close(actual.price(), baseline.price(), 1e-12);
        for greek in [Greek::Delta, Greek::Gamma] {
            close(
                actual.require(greek).unwrap(),
                baseline.require(greek).unwrap(),
                1e-12,
            );
        }
    }
}

#[test]
fn automatic_domains_preserve_interpolated_endpoint_volatility() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let extended = LocalVolatilitySurface::new(
        vec![
            Date::new(2024, 1, 2).unwrap().start_of_day(),
            Date::new(2027, 1, 1).unwrap().start_of_day(),
        ],
        vec![80.0, 120.0],
        vec![vec![0.2; 2], vec![0.8; 2]],
    )
    .unwrap();
    let clipped = surface(vec![vec![0.4; 2], vec![0.6; 2]]);
    let engine = FiniteDifferenceVanillaEngine::default();
    close(
        engine.price(&option, &context(extended)).unwrap(),
        engine.price(&option, &context(clipped)).unwrap(),
        1e-10,
    );
}

#[test]
fn constant_surface_matches_bsm_prices_and_native_greeks() {
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        dates().0,
    )
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, dates().0, dates().1).unwrap();
        let actual = engine(800, 800, 400.0)
            .price_with_greeks(&option, &c, request)
            .unwrap();
        let expected = AnalyticEuropeanEngine
            .price_with_greeks(&option, &bsm, request)
            .unwrap();
        close(actual.price(), expected.price(), 0.002);
        close(
            actual.require(Greek::Delta).unwrap(),
            expected.require(Greek::Delta).unwrap(),
            0.0002,
        );
        close(
            actual.require(Greek::Gamma).unwrap(),
            expected.require(Greek::Gamma).unwrap(),
            0.00003,
        );
        close(
            actual.price(),
            engine(800, 800, 400.0).price(&option, &bsm).unwrap(),
            1e-8,
        );
    }
}

#[test]
fn time_varying_sigma_matches_independently_integrated_variance() {
    let (a, b) = dates();
    let mid = Date::new(2025, 7, 2).unwrap().start_of_day();
    let values = [0.15_f64, 0.35, 0.25];
    let variance = ((0.15_f64.powi(2) + 0.15 * 0.35 + 0.35_f64.powi(2)) * 182.0
        + (0.35_f64.powi(2) + 0.35 * 0.25 + 0.25_f64.powi(2)) * 183.0)
        / 3.0
        / 365.0;
    let s = LocalVolatilitySurface::new(
        vec![a, mid, b],
        vec![80.0, 120.0],
        values.iter().map(|&v| vec![v; 2]).collect(),
    )
    .unwrap();
    let c = context(s);
    let expected = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, variance.sqrt()).unwrap(),
        100.0,
        a,
    )
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, a, b).unwrap();
        close(
            engine(800, 800, 400.0).price(&option, &c).unwrap(),
            AnalyticEuropeanEngine.price(&option, &expected).unwrap(),
            0.002,
        );
    }
    let rolled = c
        .replace(PricingContextChanges {
            valuation_time: Some(mid),
            ..Default::default()
        })
        .unwrap();
    let remaining_sigma = ((0.35_f64.powi(2) + 0.35 * 0.25 + 0.25_f64.powi(2)) / 3.0).sqrt();
    let reference = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, remaining_sigma).unwrap(),
        100.0,
        mid,
    )
    .unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, a, b).unwrap();
    close(
        engine(800, 800, 400.0).price(&option, &rolled).unwrap(),
        AnalyticEuropeanEngine.price(&option, &reference).unwrap(),
        0.002,
    );
}

#[test]
fn skewed_surface_converges_and_greeks_hold_absolute_coordinates_fixed() {
    let c = context(surface(vec![vec![0.3, 0.15], vec![0.35, 0.2]]));
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let fine = engine(1600, 1600, 400.0)
        .price_with_greeks(&option, &c, request)
        .unwrap();
    let coarse = engine(800, 800, 400.0).price(&option, &c).unwrap();
    close(coarse, fine.price(), 0.002);
    close(
        engine(1600, 3200, 400.0).price(&option, &c).unwrap(),
        fine.price(),
        0.0001,
    );
    close(
        engine(2400, 1600, 600.0).price(&option, &c).unwrap(),
        fine.price(),
        0.0001,
    );
    let mut bumped = [0.0; 2];
    for (i, s) in [100.1, 99.9].into_iter().enumerate() {
        let c = c
            .replace(PricingContextChanges {
                spot_price: Some(s),
                ..Default::default()
            })
            .unwrap();
        bumped[i] = engine(1600, 1600, 400.0).price(&option, &c).unwrap();
    }
    close(
        fine.require(Greek::Delta).unwrap(),
        (bumped[0] - bumped[1]) / 0.2,
        1e-5,
    );
    close(
        fine.require(Greek::Gamma).unwrap(),
        (bumped[0] + bumped[1] - 2.0 * fine.price()) / 0.01,
        1e-5,
    );
}

#[test]
fn unsupported_models_requests_and_solves_fail_even_at_expiry() {
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap();
    for c in [
        c.clone(),
        c.replace(PricingContextChanges {
            valuation_time: Some(dates().1),
            ..Default::default()
        })
        .unwrap(),
    ] {
        assert!(matches!(
            AnalyticEuropeanEngine.price(&option, &c),
            Err(Error::UnsupportedOperation(_))
        ));
        assert!(matches!(
            MonteCarloVanillaEngine::default().price(&option, &c),
            Err(Error::UnsupportedOperation(_))
        ));
        assert!(matches!(
            CoxRossRubinsteinVanillaEngine::default().price(&option, &c),
            Err(Error::UnsupportedOperation(_))
        ));
        for request in [
            GreeksRequest::all(),
            GreeksRequest::new([Greek::Delta, Greek::Vega]),
        ] {
            assert_eq!(
                engine(100, 100, 400.0)
                    .price_with_greeks(&option, &c, request)
                    .unwrap()
                    .get(Greek::Vega),
                None
            );
        }
        assert!(matches!(
            implied_volatility(
                &engine(100, 100, 400.0),
                &option,
                &c,
                10.0,
                Default::default()
            ),
            Err(Error::UnsupportedOperation(_))
        ));
    }
}

#[test]
fn american_and_digital_constant_surfaces_match_bsm_and_expiry() {
    let settings = FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 800,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    };
    let vanilla = FiniteDifferenceVanillaEngine::new(settings).unwrap();
    let digital = FiniteDifferenceDigitalEngine::new(settings).unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for (r, q) in [(0.05, 0.02), (-0.03, -0.01), (0.02, 0.08)] {
        let bsm = PricingContext::new(
            BlackScholesMertonParameters::new(r, q, 0.2).unwrap(),
            100.0,
            dates().0,
        )
        .unwrap();
        let local = bsm
            .replace(PricingContextChanges {
                model_parameters: Some(
                    LocalVolatilityParameters::new(r, q, surface(vec![vec![0.2; 2]; 2]))
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let american = AmericanOption::new(direction, 100.0, dates().0, dates().1).unwrap();
            let a = vanilla
                .price_with_greeks(&american, &local, request)
                .unwrap();
            let b = vanilla.price_with_greeks(&american, &bsm, request).unwrap();
            for (actual, expected) in [
                (a.price(), b.price()),
                (
                    a.require(Greek::Delta).unwrap(),
                    b.require(Greek::Delta).unwrap(),
                ),
                (
                    a.require(Greek::Gamma).unwrap(),
                    b.require(Greek::Gamma).unwrap(),
                ),
            ] {
                close(actual, expected, 1e-7);
            }
            for option in [
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, dates().0, dates().1)
                    .unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, dates().0, dates().1).unwrap(),
            ] {
                let a = digital.price_with_greeks(&option, &local, request).unwrap();
                let b = digital.price_with_greeks(&option, &bsm, request).unwrap();
                close(a.price(), b.price(), 1e-7);
                close(
                    a.require(Greek::Delta).unwrap(),
                    b.require(Greek::Delta).unwrap(),
                    1e-7,
                );
                close(
                    a.require(Greek::Gamma).unwrap(),
                    b.require(Greek::Gamma).unwrap(),
                    1e-7,
                );
                let terminal = local
                    .replace(PricingContextChanges {
                        valuation_time: Some(dates().1),
                        spot_price: Some(110.0),
                        ..Default::default()
                    })
                    .unwrap();
                let result = digital
                    .price_with_greeks(&option, &terminal, request)
                    .unwrap();
                let expected = if direction == OptionType::Call {
                    option.payout().unwrap_or(110.0)
                } else {
                    0.0
                };
                assert_eq!(result.price(), expected);
                assert!(
                    (result.require(Greek::Delta).unwrap()
                        - f64::from(direction == OptionType::Call && option.payout().is_none()))
                    .abs()
                        < 1e-10
                );
                assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
                assert_eq!(
                    digital
                        .price_with_greeks(&option, &terminal, GreeksRequest::all())
                        .unwrap()
                        .get(Greek::Vega),
                    None
                );
            }
            let terminal = local
                .replace(PricingContextChanges {
                    valuation_time: Some(dates().1),
                    spot_price: Some(110.0),
                    ..Default::default()
                })
                .unwrap();
            let result = vanilla
                .price_with_greeks(&american, &terminal, request)
                .unwrap();
            assert_eq!(
                result.price(),
                if direction == OptionType::Call {
                    10.0
                } else {
                    0.0
                }
            );
            assert!(
                (result.require(Greek::Delta).unwrap() - f64::from(direction == OptionType::Call))
                    .abs()
                    < 1e-10
            );
            assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
            assert_eq!(
                vanilla
                    .price_with_greeks(&american, &terminal, GreeksRequest::all())
                    .unwrap()
                    .get(Greek::Vega),
                None
            );
        }
    }
}

#[test]
fn local_digital_keeps_flat_rate_restriction_before_terminal_shortcuts() {
    let curve = RateTermStructure::new(
        dates().0,
        vec![dates().1, (dates().1).add_days(1).unwrap()],
        vec![0.95, (0.95) * 0.99],
    )
    .unwrap();
    let model =
        LocalVolatilityParameters::new(curve, 0.02, surface(vec![vec![0.2; 2]; 2])).unwrap();
    let c = PricingContext::new(model, 110.0, dates().1).unwrap();
    let option =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, dates().0, dates().1)
            .unwrap();
    assert!(matches!(
        FiniteDifferenceDigitalEngine::default().price(&option, &c),
        Err(Error::UnsupportedOperation(_))
    ));
    assert!(matches!(
        <FiniteDifferenceDigitalEngine as PricingEngine<DigitalOption>>::validate_applicability(
            &FiniteDifferenceDigitalEngine::default(),
            &c
        ),
        Err(Error::UnsupportedOperation(_))
    ));
}

#[test]
fn every_fd_engine_keeps_schemes_coverage_and_explicit_stability_contracts() {
    let (start, end) = dates();
    let local = context(surface(vec![vec![0.2; 2]; 2]));
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let short = context(
        LocalVolatilitySurface::new(
            vec![start, Date::new(2025, 1, 2).unwrap().start_of_day()],
            vec![80.0, 120.0],
            vec![vec![0.2; 2]; 2],
        )
        .unwrap(),
    );
    macro_rules! check {
        ($engine:ty, $option:expr) => {{
            let option = $option;
            for scheme in [
                FiniteDifferenceScheme::ExplicitEuler,
                FiniteDifferenceScheme::ImplicitEuler,
                FiniteDifferenceScheme::CrankNicolson,
                FiniteDifferenceScheme::Rannacher,
            ] {
                let engine = <$engine>::new(FiniteDifferenceSettings {
                    spot_shift: None,
                    asset_step_count: 80,
                    time_step_count: 16000,
                    scheme,
                    asset_upper_boundary: Some(400.0),
                })
                .unwrap();
                close(
                    engine.price(&option, &local).unwrap(),
                    engine.price(&option, &bsm).unwrap(),
                    1e-6,
                );
            }
            assert!(matches!(
                <$engine>::default().price(&option, &short),
                Err(Error::InvalidInput(_))
            ));
            let unstable = <$engine>::new(FiniteDifferenceSettings {
                time_step_count: 1,
                scheme: FiniteDifferenceScheme::ExplicitEuler,
                ..Default::default()
            })
            .unwrap();
            assert!(matches!(
                unstable.price(&option, &local),
                Err(Error::InvalidInput(_))
            ));
        }};
    }
    check!(
        FiniteDifferenceVanillaEngine,
        AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap()
    );
    check!(
        FiniteDifferenceDigitalEngine,
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap()
    );
    check!(
        FiniteDifferenceBarrierEngine,
        BarrierOption::new(BarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            120.0,
            BarrierType::UpAndOut
        ))
        .unwrap()
    );
    check!(
        FiniteDifferenceDoubleBarrierEngine,
        DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            80.0,
            120.0,
            DoubleBarrierType::KnockOut
        ))
        .unwrap()
    );
    check!(
        FiniteDifferenceBinaryBarrierEngine,
        TouchOption::cash(
            TouchOptionTerms::new(start, end, 120.0, BarrierType::UpAndIn),
            10.0
        )
        .unwrap()
    );
    check!(
        FiniteDifferenceDoubleTouchEngine,
        DoubleTouchOption::cash(
            DoubleTouchOptionTerms::new(start, end, 80.0, 120.0, DoubleBarrierType::KnockOut),
            10.0
        )
        .unwrap()
    );
    check!(
        FiniteDifferenceAccumulatorEngine,
        Accumulator::new(AccumulatorTerms::new(
            100.0,
            120.0,
            1.0,
            2.0,
            start,
            end,
            (end).time_of_day()
        ))
        .unwrap()
    );
    check!(
        FiniteDifferenceAutocallableEngine,
        BinarySnowballOption::new(BinarySnowballTerms::new(
            0.06,
            start,
            end,
            autocallable_observations(&[end], &[120.0], Some(&[0.08]), None).unwrap()
        ))
        .unwrap()
    );
}

#[test]
fn american_and_digital_skew_match_frozen_independent_references() {
    // QuantLib 1.43, 3200 time/space nodes. Python measures reference refinement.
    let local = context(surface(vec![vec![0.3, 0.15], vec![0.35, 0.2]]));
    let settings = FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 1600,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    };
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let american = AmericanOption::new(OptionType::Put, 100.0, dates().0, dates().1).unwrap();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, dates().0, dates().1)
        .unwrap();
    let asset =
        DigitalOption::asset_or_nothing(OptionType::Put, 100.0, dates().0, dates().1).unwrap();
    for (actual, expected) in [
        (
            FiniteDifferenceVanillaEngine::new(settings)
                .unwrap()
                .price_with_greeks(&american, &local, request)
                .unwrap(),
            [
                8.360_927_652_711_77,
                -0.486_222_800_855_709_6,
                0.016_624_005_388_366_895,
            ],
        ),
        (
            FiniteDifferenceDigitalEngine::new(settings)
                .unwrap()
                .price_with_greeks(&cash, &local, request)
                .unwrap(),
            [
                5.390_974_151_168_607,
                0.159_479_130_689_178_02,
                -0.000_587_304_384_771_014,
            ],
        ),
        (
            FiniteDifferenceDigitalEngine::new(settings)
                .unwrap()
                .price_with_greeks(&asset, &local, request)
                .unwrap(),
            [
                33.108_384_697_704_08,
                -1.129_755_611_092_068_1,
                -0.009_360_268_511_100_315,
            ],
        ),
    ] {
        close(actual.price(), expected[0], 0.002);
        close(actual.require(Greek::Delta).unwrap(), expected[1], 0.0002);
        close(actual.require(Greek::Gamma).unwrap(), expected[2], 0.00003);
    }
}

#[test]
fn fd_expiry_coverage_stability_and_numerical_errors_are_explicit() {
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap();
    let at_expiry = c
        .replace(PricingContextChanges {
            valuation_time: Some(dates().1),
            spot_price: Some(110.0),
            ..Default::default()
        })
        .unwrap();
    let result = engine(100, 100, 400.0)
        .price_with_greeks(
            &option,
            &at_expiry,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
        )
        .unwrap();
    assert_eq!(result.price(), 10.0);
    assert!((result.require(Greek::Delta).unwrap() - 1.0).abs() < 1e-10);
    assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
    let too_late = EuropeanOption::new(
        OptionType::Call,
        100.0,
        dates().0,
        Date::new(2026, 1, 2).unwrap().start_of_day(),
    )
    .unwrap();
    assert!(matches!(
        FiniteDifferenceVanillaEngine::default().price(&too_late, &c),
        Err(Error::InvalidInput(_))
    ));
    let explicit = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        time_step_count: 1,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        explicit.price(&option, &c),
        Err(Error::InvalidInput(_))
    ));
    let huge = context(surface(vec![vec![f64::MAX; 2]; 2]));
    assert!(matches!(
        engine(100, 100, 400.0).price(&option, &huge),
        Err(Error::ComputationFailure(_))
    ));
}

#[test]
fn all_existing_schemes_support_local_volatility() {
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap();
    for scheme in [
        FiniteDifferenceScheme::ExplicitEuler,
        FiniteDifferenceScheme::ImplicitEuler,
        FiniteDifferenceScheme::CrankNicolson,
        FiniteDifferenceScheme::Rannacher,
    ] {
        let e = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            spot_shift: None,
            asset_step_count: 200,
            time_step_count: 4000,
            scheme,
            asset_upper_boundary: Some(400.0),
        })
        .unwrap();
        close(e.price(&option, &c).unwrap(), 9.227_005_508_154_036, 0.015);
    }
}

#[test]
fn short_maturity_prices_and_greeks_match_bsm() {
    let end = Date::new(2025, 1, 2).unwrap().start_of_day();
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        dates().0,
    )
    .unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, end).unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let actual = engine(12800, 1600, 400.0)
        .price_with_greeks(&option, &c, request)
        .unwrap();
    let expected = AnalyticEuropeanEngine
        .price_with_greeks(&option, &bsm, request)
        .unwrap();
    close(actual.price(), expected.price(), 0.001);
    close(
        actual.require(Greek::Delta).unwrap(),
        expected.require(Greek::Delta).unwrap(),
        0.0001,
    );
    close(
        actual.require(Greek::Gamma).unwrap(),
        expected.require(Greek::Gamma).unwrap(),
        0.0003,
    );
}

#[test]
fn skewed_prices_match_frozen_independent_quantlib_reference() {
    // QuantLib 1.43, 3200 time/space nodes; Python checks reference refinement.
    let fixtures = [
        (
            70.0,
            OptionType::Call,
            1.569_872_379_641_753,
            0.146_110_698_505_741_5,
            0.009_280_650_483_754_862,
        ),
        (
            100.0,
            OptionType::Call,
            11.001_747_461_841_767,
            0.515_163_541_339_759_2,
            0.015_233_318_213_016_668,
        ),
        (
            140.0,
            OptionType::Call,
            42.369_133_981_125_66,
            0.952_740_540_352_748_8,
            0.002_700_002_133_963_205,
        ),
        (
            100.0,
            OptionType::Put,
            8.104_816_263_669_061,
            -0.465_035_695_805_179_34,
            0.015_233_312_370_430_73,
        ),
    ];
    for (spot, direction, price, delta, gamma) in fixtures {
        let c = context(surface(vec![vec![0.3, 0.15], vec![0.35, 0.2]]))
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap();
        let option = EuropeanOption::new(direction, 100.0, dates().0, dates().1).unwrap();
        let actual = engine(1600, 1600, 400.0)
            .price_with_greeks(
                &option,
                &c,
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            )
            .unwrap();
        close(actual.price(), price, 0.002);
        close(actual.require(Greek::Delta).unwrap(), delta, 0.0002);
        close(actual.require(Greek::Gamma).unwrap(), gamma, 0.00003);
    }
}
