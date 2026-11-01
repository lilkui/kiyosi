use kiyosi::*;

fn dates() -> (Date, Date) {
    (
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
}
fn surface(values: Vec<Vec<f64>>) -> LocalVolatilitySurface {
    let (a, b) = dates();
    LocalVolatilitySurface::new(vec![a.into(), b.into()], vec![80.0, 120.0], values).unwrap()
}
fn context(surface: LocalVolatilitySurface) -> PricingContext {
    PricingContext::new(
        LocalVolatilityParameters::new(0.05, 0.02, surface).unwrap(),
        100.0,
        dates().0.into(),
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
        close(s.volatility(time.into(), spot).unwrap(), expected, 1e-14);
    }
    for time in [
        Date::new(2024, 12, 31).unwrap(),
        Date::new(2026, 1, 2).unwrap(),
    ] {
        assert!(matches!(
            s.volatility(time.into(), 100.0),
            Err(Error::InvalidInput(_))
        ));
    }
    for spot in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            s.volatility(a.into(), spot),
            Err(Error::InvalidInput(_))
        ));
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
    let valid_times = vec![a.into(), b.into()];
    for (times, spots, values) in [
        (vec![], vec![80.0, 120.0], vec![]),
        (vec![a.into()], vec![80.0, 120.0], vec![vec![0.2, 0.2]]),
        (
            vec![b.into(), a.into()],
            vec![80.0, 120.0],
            vec![vec![0.2; 2]; 2],
        ),
        (
            vec![a.into(), a.into()],
            vec![80.0, 120.0],
            vec![vec![0.2; 2]; 2],
        ),
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
                s.volatility(Date::new(2025, 7, 2).unwrap().into(), spot)
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
    assert_eq!(m.risk_free_curve().flat_rate(), Some(0.05));
    let later = c
        .replace(PricingContextChanges {
            spot_price: Some(140.0),
            valuation_time: Some(Date::new(2025, 1, 2).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(later.model_parameters(), c.model_parameters());
    assert_eq!(c.spot_price(), 100.0);
    assert!(matches!(
        c.replace(PricingContextChanges {
            valuation_time: Some(Date::new(2026, 1, 2).unwrap().into()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    close(
        m.surface().volatility(dates().0.into(), 100.0).unwrap(),
        0.3,
        1e-14,
    );
}

#[test]
fn constant_surface_matches_bsm_prices_and_native_greeks() {
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        dates().0.into(),
    )
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, dates().0, dates().1).unwrap();
        let actual = engine(800, 800, 400.0)
            .price_with_greeks(&option, &c, request, Default::default())
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
    let mid = Date::new(2025, 7, 2).unwrap();
    let values = [0.15_f64, 0.35, 0.25];
    let variance = ((0.15_f64.powi(2) + 0.15 * 0.35 + 0.35_f64.powi(2)) * 182.0
        + (0.35_f64.powi(2) + 0.35 * 0.25 + 0.25_f64.powi(2)) * 183.0)
        / 3.0
        / 365.0;
    let s = LocalVolatilitySurface::new(
        vec![a.into(), mid.into(), b.into()],
        vec![80.0, 120.0],
        values.iter().map(|&v| vec![v; 2]).collect(),
    )
    .unwrap();
    let c = context(s);
    let expected = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, variance.sqrt()).unwrap(),
        100.0,
        a.into(),
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
            valuation_time: Some(mid.into()),
            ..Default::default()
        })
        .unwrap();
    let remaining_sigma = ((0.35_f64.powi(2) + 0.35 * 0.25 + 0.25_f64.powi(2)) / 3.0).sqrt();
    let reference = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, remaining_sigma).unwrap(),
        100.0,
        mid.into(),
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
        .price_with_greeks(&option, &c, request, Default::default())
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
            valuation_time: Some(dates().1.into()),
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
            assert!(matches!(
                engine(100, 100, 400.0).price_with_greeks(&option, &c, request, Default::default()),
                Err(Error::UnsupportedOperation(_))
            ));
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
        let american = AmericanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap();
        assert!(matches!(
            FiniteDifferenceVanillaEngine::default().price(&american, &c),
            Err(Error::UnsupportedOperation(_))
        ));
    }
}

#[test]
fn fd_expiry_coverage_stability_and_numerical_errors_are_explicit() {
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap();
    let at_expiry = c
        .replace(PricingContextChanges {
            valuation_time: Some(dates().1.into()),
            spot_price: Some(110.0),
            ..Default::default()
        })
        .unwrap();
    let result = engine(100, 100, 400.0)
        .price_with_greeks(
            &option,
            &at_expiry,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            Default::default(),
        )
        .unwrap();
    assert_eq!(result.price(), 10.0);
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Gamma));
    let too_late = EuropeanOption::new(
        OptionType::Call,
        100.0,
        dates().0,
        Date::new(2026, 1, 2).unwrap(),
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
            asset_step_count: 200,
            time_step_count: 4000,
            scheme,
            asset_upper_boundary: Some(400.0),
        })
        .unwrap();
        close(e.price(&option, &c).unwrap(), 9.227005508154036, 0.015);
    }
}

#[test]
fn short_maturity_prices_and_greeks_match_bsm() {
    let end = Date::new(2025, 1, 2).unwrap();
    let c = context(surface(vec![vec![0.2; 2]; 2]));
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        dates().0.into(),
    )
    .unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, dates().0, end).unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let actual = engine(12800, 1600, 400.0)
        .price_with_greeks(&option, &c, request, Default::default())
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
            1.569872379641753,
            0.1461106985057415,
            0.009280650483754862,
        ),
        (
            100.0,
            OptionType::Call,
            11.001747461841767,
            0.5151635413397592,
            0.015233318213016668,
        ),
        (
            140.0,
            OptionType::Call,
            42.36913398112566,
            0.9527405403527488,
            0.002700002133963205,
        ),
        (
            100.0,
            OptionType::Put,
            8.104816263669061,
            -0.46503569580517934,
            0.01523331237043073,
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
                Default::default(),
            )
            .unwrap();
        close(actual.price(), price, 0.002);
        close(actual.require(Greek::Delta).unwrap(), delta, 0.0002);
        close(actual.require(Greek::Gamma).unwrap(), gamma, 0.00003);
    }
}
