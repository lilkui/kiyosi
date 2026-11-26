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
fn european_surface_matches_independent_strips_and_preserves_order() -> Result<()> {
    let (start, end) = dates();
    let market = context(surface(vec![vec![0.30, 0.15], vec![0.35, 0.20]]));
    let engine = engine(800, 800, 400.0);
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let short = start.add_days(1)?;
    let expiries = [
        end,
        start,
        start.add_days(30)?,
        start.add_days(90)?,
        short,
        end,
    ];
    let strikes = [110.0, 90.0, 100.0, 90.0, 95.0];
    let rows = engine.price_european_surface(&option, &market, &expiries, &strikes)?;
    assert_eq!(rows.len(), expiries.len(), "expiry row count");
    assert_eq!(rows[0], rows[5]);
    assert_eq!(rows[1], [0.0, 10.0, 0.0, 10.0, 5.0]);
    for (&expiry, row) in expiries.iter().zip(rows) {
        assert_eq!(row.len(), strikes.len(), "strike quote count");
        assert_eq!(row[1].to_bits(), row[3].to_bits());
        let option = option.replace(EuropeanOptionChanges {
            expiry_time: Some(expiry),
            ..Default::default()
        })?;
        let expected = engine.price_strikes(&option, &market, &strikes)?;
        assert_eq!(
            expected.len(),
            strikes.len(),
            "independent strike quote count"
        );
        if expiry == short {
            assert_eq!(row, expected);
        }
        for (actual, expected) in row.into_iter().zip(expected) {
            close(actual, expected, 0.001);
        }
    }
    assert!(matches!(
        engine.price_european_surface(&option, &market, &[end.add_days(1)?], &strikes),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn stationary_skew_is_reused_across_scheme_weights_and_rate_knots() -> Result<()> {
    let (start, end) = dates();
    let middle = start.add_days(182)?;
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let risk = RateTermStructure::new(start, vec![middle, end], vec![0.98, 0.95])?;
    let settings = FiniteDifferenceSettings {
        asset_step_count: 400,
        time_step_count: 160,
        ..Default::default()
    };
    let engine = FiniteDifferenceVanillaEngine::new(settings)?;
    let price = |row: Vec<f64>| -> Result<f64> {
        let surface = LocalVolatilitySurface::new(
            vec![start, end],
            vec![80.0, 120.0],
            vec![vec![0.23, 0.17], row],
        )?;
        let context = PricingContext::new(
            LocalVolatilityParameters::new(risk.clone(), 0.02, surface)?,
            100.0,
            start,
        )?;
        engine.price(&option, &context)
    };
    let stationary = price(vec![0.23, 0.17])?;
    let changing = price(vec![0.23 + 1e-10, 0.17 + 1e-10])?;
    close(stationary, changing, 1e-7);
    Ok(())
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
fn skewed_uniform_european_payoff_matches_independent_reference() {
    let (start, end) = dates();
    let context = context(surface(vec![vec![0.3, 0.15], vec![0.35, 0.2]]));
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = engine(800, 400, 400.0);
    let result = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
        )
        .unwrap();
    // QuantLib 1.44 refined reference and empirical uncertainty from the native benchmark.
    assert!((result.price() - 11.001_742_706_191_068).abs() + 1.102_296_046_616_996e-5 < 1e-4);
    close(
        result.require(Greek::Delta).unwrap(),
        0.515_163_541_339_759_2,
        2e-5,
    );
    close(
        result.require(Greek::Gamma).unwrap(),
        0.015_233_318_213_016_668,
        5e-6,
    );
    assert_eq!(result.price(), engine.price(&option, &context).unwrap());
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
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
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
            let cash_payout = 10.0;
            for (option, payout) in [
                (
                    DigitalOption::cash_or_nothing(
                        direction,
                        100.0,
                        cash_payout,
                        dates().0,
                        dates().1,
                    )
                    .unwrap(),
                    Some(cash_payout),
                ),
                (
                    DigitalOption::asset_or_nothing(direction, 100.0, dates().0, dates().1)
                        .unwrap(),
                    None,
                ),
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
                    payout.unwrap_or(110.0)
                } else {
                    0.0
                };
                assert_eq!(result.price(), expected);
                assert!(
                    (result.require(Greek::Delta).unwrap()
                        - f64::from(direction == OptionType::Call && payout.is_none()))
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
fn digital_term_structure_prices_and_greeks_match_integrated_variance() -> Result<()> {
    let (start, end) = dates();
    let mid = start.add_days(180)?;
    let risk = RateTermStructure::new(start, vec![mid, end], vec![0.99, 0.95])?;
    let dividend = RateTermStructure::new(start, vec![mid, end], vec![0.999, 0.97])?;
    let (first, last) = (0.19_f64, 0.23_f64);
    // The surface is flat in spot and linear in time. Integrate squared volatility.
    let sigma = ((first.powi(2) + first * last + last.powi(2)) / 3.0).sqrt();
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(risk.clone(), dividend.clone(), sigma)?,
        100.0,
        start,
    )?;
    let local = PricingContext::new(
        LocalVolatilityParameters::new(
            risk,
            dividend,
            surface(vec![vec![first; 2], vec![last; 2]]),
        )?,
        100.0,
        start,
    )?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Rho, Greek::DividendRho]);
    for (option, terminal_price) in [
        (
            DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?,
            10.0,
        ),
        (
            DigitalOption::asset_or_nothing(OptionType::Put, 105.0, start, end)?,
            0.0,
        ),
    ] {
        let expected = AnalyticDigitalEngine.price_with_greeks(&option, &bsm, request)?;
        for (context, supports_rate_greeks) in [(&bsm, true), (&local, false)] {
            let terminal = context.replace(PricingContextChanges {
                spot_price: Some(110.0),
                valuation_time: Some(end),
                ..Default::default()
            })?;
            for upper in [None, Some(400.0)] {
                let engine = FiniteDifferenceDigitalEngine::new(FiniteDifferenceSettings {
                    asset_step_count: 1600,
                    time_step_count: 800,
                    asset_upper_boundary: upper,
                    ..Default::default()
                })?;
                close(engine.price(&option, context)?, expected.price(), 0.002);
                let solution = engine.solve(&option, context, 95.0..=105.0)?;
                close(solution.price(100.0)?, expected.price(), 0.002);
                let actual = engine.price_with_greeks(&option, context, request)?;
                close(actual.price(), expected.price(), 0.002);
                close(
                    actual.require(Greek::Delta)?,
                    expected.require(Greek::Delta)?,
                    0.0002,
                );
                close(
                    actual.require(Greek::Gamma)?,
                    expected.require(Greek::Gamma)?,
                    0.00003,
                );
                for greek in [Greek::Rho, Greek::DividendRho] {
                    if supports_rate_greeks {
                        close(actual.require(greek)?, expected.require(greek)?, 0.03);
                    } else {
                        assert_eq!(actual.status(greek), GreekStatus::Unsupported);
                    }
                }
                assert_eq!(engine.price(&option, &terminal)?, terminal_price);
            }
        }
    }
    Ok(())
}

#[test]
fn every_fd_engine_keeps_schemes_coverage_and_explicit_stability_contracts() {
    let (start, _) = dates();
    let end = Date::new(2025, 1, 31).unwrap().start_of_day();
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
                    asset_step_count: 80,
                    time_step_count: 1600,
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
            autocallable_observations(&[end], &[120.0], &[0.08]).unwrap(),
            TradingCalendar::weekdays()
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

#[test]
fn european_strike_strip_matches_refined_local_volatility_prices() -> Result<()> {
    let (start, end) = dates();
    let batch = engine(800, 400, 400.0);
    let refined = engine(1600, 800, 400.0);
    let strikes = [115.0, 85.0, 100.0, 105.0, 85.0, 180.0];
    for clock in [
        VarianceTimeConvention::Actual365Fixed,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
    ] {
        let surface = LocalVolatilitySurface::new(
            vec![start, start.add_days(183)?, end],
            vec![80.0, 120.0],
            vec![vec![0.25, 0.18], vec![0.25, 0.20], vec![0.27, 0.20]],
        )?;
        let model = LocalVolatilityParameters::new_with_variance_time_convention(
            0.05, 0.02, surface, clock,
        )?;
        let context = PricingContext::new(model, 100.0, start)?;
        for direction in [OptionType::Call, OptionType::Put] {
            let template = EuropeanOption::new(direction, 100.0, start, end)?;
            let prices = batch.price_strikes(&template, &context, &strikes)?;
            assert_eq!(prices.len(), strikes.len(), "strike quote count");
            assert_eq!(
                prices[..5],
                batch.price_strikes(&template, &context, &strikes[..5])?
            );
            assert_eq!(prices[1].to_bits(), prices[4].to_bits());
            for (price, strike) in prices.into_iter().zip(strikes) {
                let scalar = EuropeanOption::new(direction, strike, start, end)?;
                close(price, refined.price(&scalar, &context)?, 0.001);
            }
        }
    }
    Ok(())
}

#[test]
fn american_local_volatility_strips_match_refined_scalar_pricing() -> Result<()> {
    let (start, end) = dates();
    let market = context(surface(vec![vec![0.30, 0.15], vec![0.35, 0.20]]));
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let strikes = [110.0, 88.0, 100.0, 96.0, 105.0, 100.0, 90.0, 101.0, 113.0];
    let batch = engine(800, 400, 400.0);
    let reference = engine(1600, 800, 400.0);
    let prices = batch.price_american_strikes(&option, &market, &strikes)?;
    assert_eq!(prices.len(), strikes.len(), "strike quote count");
    assert_eq!(prices[2].to_bits(), prices[5].to_bits());
    for (strike, price) in strikes.into_iter().zip(prices) {
        let contract = option.replace(AmericanOptionChanges {
            strike: Some(strike),
            ..Default::default()
        })?;
        close(price, reference.price(&contract, &market)?, 0.002);
    }
    Ok(())
}

#[test]
fn american_local_surface_matches_separate_expiry_strips() -> Result<()> {
    let (start, end) = dates();
    let snapshot = context(surface(vec![vec![0.30, 0.21], vec![0.34, 0.23]]));
    let engine = engine(800, 400, 400.0);
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let expiries = [
        end,
        start.add_days(220)?,
        start,
        start.add_days(260)?,
        start.add_days(320)?,
        end,
    ];
    let strikes = [105.0, 90.0, 100.0, 95.0, 110.0, 95.0];
    let rows = engine.price_american_surface(&put, &snapshot, &expiries, &strikes)?;
    assert_eq!(rows.len(), expiries.len(), "expiry row count");
    assert_eq!(rows[0], rows[5]);
    assert_eq!(rows[2], [5.0, 0.0, 0.0, 0.0, 10.0, 0.0]);
    for (expiry, row) in expiries.into_iter().zip(rows) {
        let option = AmericanOption::new(OptionType::Put, 100.0, start, expiry)?;
        let reference = engine.price_american_strikes(&option, &snapshot, &strikes)?;
        assert_eq!(row.len(), strikes.len(), "strike quote count");
        assert_eq!(
            reference.len(),
            strikes.len(),
            "independent strike quote count"
        );
        assert_eq!(row[3].to_bits(), row[5].to_bits());
        for (actual, expected) in row.into_iter().zip(reference) {
            close(actual, expected, 0.002);
        }
    }
    Ok(())
}
