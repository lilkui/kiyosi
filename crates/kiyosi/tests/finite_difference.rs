use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}

fn market(spot: f64, rate: f64, dividend: f64, sigma: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        dates().0,
    )
    .unwrap()
}

#[test]
fn european_market_greeks_follow_analytic_risk_and_explicit_bumps() -> Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let context = market(105.0, 0.05, 0.02, 0.2);
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1000,
        time_step_count: 1000,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    let greeks = [
        Greek::Vega,
        Greek::Volga,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Rho,
        Greek::DividendRho,
    ];
    let request = GreeksRequest::new(greeks);
    let actual = engine.price_with_greeks(&option, &context, request)?;
    let reference = AnalyticEuropeanEngine.price_with_greeks(&option, &context, request)?;
    assert_eq!(
        actual.price().to_bits(),
        engine.price(&option, &context)?.to_bits()
    );
    for greek in greeks {
        let expected = reference.require(greek)?;
        assert!(
            (actual.require(greek)? - expected).abs() < 0.001 * expected.abs() + 1e-6,
            "{greek:?}"
        );
    }
    let shift = 0.015_625;
    let high = engine.price(&option, &market(105.0, 0.05, 0.02, 0.2 + shift))?;
    let low = engine.price(&option, &market(105.0, 0.05, 0.02, 0.2 - shift))?;
    let explicit = engine.price_with_greeks_with_settings(
        &option,
        &context,
        GreeksRequest::new([Greek::Vega]),
        NumericalShiftSettings {
            volatility_shift: Some(shift),
            ..Default::default()
        },
    )?;
    assert!((explicit.require(Greek::Vega)? - (high - low) / (2.0 * shift)).abs() < 1e-9);
    Ok(())
}

#[test]
fn shared_time_slices_match_independent_vanilla_rollbacks() -> Result<()> {
    fn check<I: OptionContract>(
        engine: &impl PricingEngine<I>,
        option: &I,
        context: &PricingContext,
        low: &PricingContext,
        high: &PricingContext,
    ) -> Result<()> {
        let measures = [Greek::Theta, Greek::Charm, Greek::Color];
        let actual = engine.price_with_greeks(option, context, GreeksRequest::new(measures))?;
        let stencil = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
        let low = engine.price_with_greeks(option, low, stencil)?;
        let high = engine.price_with_greeks(option, high, stencil)?;
        assert_eq!(actual.price(), engine.price(option, context)?);
        assert!(actual.get(Greek::Delta).is_none() && actual.get(Greek::Gamma).is_none());
        for (greek, difference) in measures.into_iter().zip([
            high.price() - low.price(),
            high.require(Greek::Delta)? - low.require(Greek::Delta)?,
            high.require(Greek::Gamma)? - low.require(Greek::Gamma)?,
        ]) {
            let expected = difference * 365.0 / 2.0;
            assert!(
                (actual.require(greek)? - expected).abs() <= 0.002 * expected.abs() + 1e-6,
                "{greek:?} differs from the independent stencil"
            );
        }
        Ok(())
    }
    let (start, end) = dates();
    let base = market(100.0, 0.05, 0.02, 0.2);
    let day = |days| {
        base.replace(PricingContextChanges {
            valuation_time: Some(start.add_days(days)?),
            ..Default::default()
        })
    };
    let (context, low, high) = (day(30)?, day(29)?, day(31)?);
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1000,
        time_step_count: 1000,
        ..Default::default()
    })?;
    let american = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let european = EuropeanOption::new(OptionType::Put, 100.0, start, end)?;
    check(&engine, &american, &context, &low, &high)?;
    check(&engine, &european, &context, &low, &high)?;
    Ok(())
}

#[test]
fn automatic_american_grids_resolve_the_strike_before_expiry() -> Result<()> {
    let (start, _) = dates();
    let engine = FiniteDifferenceVanillaEngine::default();
    for micros in [1, 1_000_000] {
        let end = start.add_micros(micros)?;
        // Without dividends a call at positive rates matches European exercise.
        // At zero carry and rates a put also gains nothing from early exercise.
        for (direction, rate) in [(OptionType::Call, 0.05), (OptionType::Put, 0.0)] {
            let context = market(100.0, rate, 0.0, 0.2);
            let american = AmericanOption::new(direction, 100.0, start, end)?;
            let european = EuropeanOption::new(direction, 100.0, start, end)?;
            let expected = AnalyticEuropeanEngine.price(&european, &context)?;
            let saved = engine.solve(&american, &context, 99.9..=100.1)?;
            for actual in [engine.price(&american, &context)?, saved.price(100.0)?] {
                assert!(
                    (actual / expected - 1.0).abs() < 0.01,
                    "{direction:?} at {micros} microseconds: {actual} != {expected}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn automatic_american_vega_uses_consistent_strike_concentration() -> Result<()> {
    let (start, _) = dates();
    let end = start.add_micros(3 * 3_600_000_000)?;
    let context = market(100.0, 0.05, 0.0, 0.200_097);
    let american = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
    let european = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let request = GreeksRequest::new([Greek::Vega]);
    let actual = FiniteDifferenceVanillaEngine::default().price_with_greeks_with_settings(
        &american,
        &context,
        request,
        NumericalShiftSettings {
            volatility_shift: Some(1e-5),
            ..Default::default()
        },
    )?;
    let expected = AnalyticEuropeanEngine.price_with_greeks(&european, &context, request)?;
    let (actual, expected) = (actual.require(Greek::Vega)?, expected.require(Greek::Vega)?);
    assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
    Ok(())
}

#[test]
fn american_upper_boundary_includes_intermediate_exercise() -> Result<()> {
    let (start, end) = dates();
    let exercise = start.add_days(182)?;
    let curve = RateTermStructure::new(
        start,
        vec![exercise, end],
        vec![(0.15 * start.years_until(exercise)).exp(), 1.0],
    )?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, curve, 0.05)?,
        120.0,
        start,
    )?;
    let american = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
    let european = EuropeanOption::new(OptionType::Call, 100.0, start, exercise)?;
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(130.0),
        ..Default::default()
    })?;
    let lower_bound = AnalyticEuropeanEngine.price(&european, &context)?;
    assert!(engine.price(&american, &context)? >= lower_bound - 0.0005);
    Ok(())
}

#[test]
fn materially_negative_prices_fail_instead_of_being_clipped() {
    let (start, end) = dates();
    for scale in [1e-100, 1.0, 1e100] {
        let option = EuropeanOption::new(OptionType::Call, 100.0 * scale, start, end).unwrap();
        let context = market(150.0 * scale, 0.05, 2.0, 0.5);
        let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_step_count: 20,
            time_step_count: 3,
            asset_upper_boundary: Some(600.0 * scale),
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(
            engine.price(&option, &context),
            Err(Error::ComputationFailure(_))
        ));
        assert!(matches!(
            engine.price_with_greeks(&option, &context, GreeksRequest::new([Greek::Delta])),
            Err(Error::ComputationFailure(_))
        ));
        let refined = FiniteDifferenceVanillaEngine::default()
            .price(&option, &context)
            .unwrap();
        let expected = AnalyticEuropeanEngine.price(&option, &context).unwrap();
        assert!((refined - expected).abs() / scale < 0.0001);
    }
}

#[test]
fn terminal_prices_converge_to_independent_references() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for (direction, vanilla, cash, asset) in [
        (
            OptionType::Call,
            9.227_005_508_154_036,
            4.945_810_910_532_236,
            58.685_114_613_476_4,
        ),
        (
            OptionType::Put,
            6.330_080_627_549_918,
            4.566_483_334_474_903,
            39.334_752_717_199_13,
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
fn coarse_automatic_grids_resolve_short_and_annual_payoffs_and_spot_greeks() {
    let (start, annual) = dates();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 200,
        time_step_count: 50,
        ..Default::default()
    })
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for end in [Date::new(2025, 1, 2).unwrap().start_of_day(), annual] {
        for direction in [OptionType::Call, OptionType::Put] {
            let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
            for spot in [99.875, 100.0, 100.125] {
                let context = market(spot, 0.05, 0.02, 0.2);
                let expected = AnalyticEuropeanEngine
                    .price_with_greeks(&option, &context, request)
                    .unwrap();
                let actual = engine
                    .price_with_greeks(&option, &context, request)
                    .unwrap();
                assert!((actual.price() - expected.price()).abs() < 0.002);
                for (greek, tolerance) in [(Greek::Delta, 1e-4), (Greek::Gamma, 3e-4)] {
                    assert!(
                        (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs()
                            < tolerance,
                        "{direction:?} {end:?} {spot} {greek:?}"
                    );
                }
                assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
            }
        }
    }
}

#[test]
fn settings_reject_oversized_grid_workspace() {
    for settings in [
        FiniteDifferenceSettings {
            asset_step_count: i32::MAX,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            time_step_count: i32::MAX,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceVanillaEngine::new(settings),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn settings_reject_grids_that_exceed_the_peak_matrix_cache_budget() {
    let settings = FiniteDifferenceSettings {
        asset_step_count: 5_000_000,
        time_step_count: 4,
        scheme: FiniteDifferenceScheme::ImplicitEuler,
        ..Default::default()
    };
    assert!(matches!(
        FiniteDifferenceVanillaEngine::new(settings),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn uniform_european_grids_resolve_strike_phases_and_narrow_payoffs() {
    let (start, annual) = dates();
    let short = Date::new(2025, 1, 2).unwrap().start_of_day();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for (strike, sigma, end, count, tolerances) in [
        (99.9, 0.2, annual, 800, [5e-5, 1e-5, 3e-6]),
        (100.0, 0.2, annual, 800, [5e-5, 1e-5, 3e-6]),
        (100.1, 0.2, annual, 800, [5e-5, 1e-5, 3e-6]),
        (100.25, 0.2, annual, 800, [5e-5, 1e-5, 3e-6]),
        (100.0, 0.05, annual, 800, [0.001, 0.0001, 0.0002]),
        (100.0, 0.2, short, 3200, [0.0001, 0.0002, 0.002]),
    ] {
        let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_step_count: count,
            time_step_count: 400,
            asset_upper_boundary: Some(400.0),
            ..Default::default()
        })
        .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let option = EuropeanOption::new(direction, strike, start, end).unwrap();
            let mut previous = None;
            for spot in [99.75, 100.0, 100.25] {
                let context = market(spot, 0.05, 0.02, sigma);
                let actual = engine
                    .price_with_greeks(&option, &context, request)
                    .unwrap();
                let expected = AnalyticEuropeanEngine
                    .price_with_greeks(&option, &context, request)
                    .unwrap();
                for ((a, b), tolerance) in [
                    (actual.price(), expected.price()),
                    (
                        actual.require(Greek::Delta).unwrap(),
                        expected.require(Greek::Delta).unwrap(),
                    ),
                    (
                        actual.require(Greek::Gamma).unwrap(),
                        expected.require(Greek::Gamma).unwrap(),
                    ),
                ]
                .into_iter()
                .zip(tolerances)
                {
                    assert!(
                        (a - b).abs() < tolerance,
                        "{direction:?} {strike} {sigma} {end:?} {spot}: {a} != {b}"
                    );
                }
                assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
                let t = if end == annual { 1.0 } else { 1.0 / 365.0 };
                let asset = spot * (-0.02_f64 * t).exp();
                let cash = strike * (-0.05_f64 * t).exp();
                let (lower, upper) = if direction == OptionType::Call {
                    ((asset - cash).max(0.0), asset)
                } else {
                    ((cash - asset).max(0.0), cash)
                };
                assert!(actual.price() >= lower && actual.price() <= upper);
                if let Some(old) = previous {
                    assert!(if direction == OptionType::Call {
                        actual.price() > old
                    } else {
                        actual.price() < old
                    });
                }
                previous = Some(actual.price());
            }
        }
    }
}

#[test]
fn automatic_mesh_policy_is_preserved_for_bumped_greeks() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let context = market(100.0, 0.05, 0.02, 0.2);
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 200,
        time_step_count: 50,
        ..Default::default()
    })
    .unwrap();
    let request = GreeksRequest::new([Greek::Vega, Greek::Volga]);
    let actual = engine
        .price_with_greeks(&option, &context, request)
        .unwrap();
    let expected =
        numerical_greeks(&engine, &option, &context, request, Default::default()).unwrap();
    for greek in [Greek::Vega, Greek::Volga] {
        assert!(
            (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs() < 1e-8,
            "{greek:?}"
        );
    }
}

#[test]
fn grid_work_includes_time_steps_added_for_curve_knots() -> Result<()> {
    let oversized = FiniteDifferenceSettings {
        asset_step_count: 200_000,
        time_step_count: 200_000,
        ..Default::default()
    };
    assert!(
        matches!(oversized.validate(), Err(Error::InvalidInput(message)) if message.contains("rollback"))
    );

    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(1)?, end], vec![0.999, 0.95])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1_000_000,
        time_step_count: 1000,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })?;
    assert!(
        matches!(engine.price(&option, &context), Err(Error::InvalidInput(message)) if message.contains("rollback"))
    );
    Ok(())
}

#[test]
fn grid_workspace_includes_off_grid_curve_knots() -> Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(1)?, end], vec![0.999, 0.95])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 3_532_044,
        time_step_count: 6,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &context),
        Err(Error::InvalidInput(message)) if message.contains("workspace")
    ));
    Ok(())
}

#[test]
fn grid_workspace_counts_pricing_curves_even_when_knots_align() -> Result<()> {
    let (start, end) = dates();
    let knot = start.add_days(60)?.add_micros(72_000_000_000)?;
    let curve = RateTermStructure::new(start, vec![knot, end], vec![0.999, 0.95])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 3_532_044,
        time_step_count: 6,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &context),
        Err(Error::InvalidInput(message)) if message.contains("workspace")
    ));
    Ok(())
}

#[test]
fn settings_validate_without_allocating_within_the_workspace_bound() {
    let defaults = FiniteDifferenceSettings::default();
    assert_eq!(
        (defaults.asset_step_count, defaults.time_step_count),
        (800, 400)
    );
    assert_eq!(defaults.scheme, FiniteDifferenceScheme::Rannacher);
    assert_eq!(defaults.asset_upper_boundary, None);
    for (assets, times) in [(3, 1), (10_000, 100_000), (1_000_000, 1000)] {
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
        .price_with_greeks_with_settings(option, context, request, Default::default())
        .unwrap();
    let expected = implicit
        .price_with_greeks_with_settings(option, context, request, Default::default())
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
        FiniteDifferenceScheme::Rannacher,
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
        time_step_count: 20,
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
        .price_with_greeks(&option, &context, GreeksRequest::all())
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
            .price_with_greeks(&option, &context, GreeksRequest::new([greek]))
            .unwrap();
        assert_eq!(
            selected.require(greek).unwrap(),
            actual.require(greek).unwrap()
        );
        assert_eq!(selected.values().iter().flatten().count(), 1);
    }
    {
        let shift = NumericalShiftSettings::default().fallback_spot_shift();
        let h = (context.spot_price() + shift) - context.spot_price();
        let gamma = |spot| {
            engine
                .price_with_greeks(
                    &option,
                    &market(spot, 0.04, 0.01, 0.3),
                    GreeksRequest::new([Greek::Gamma]),
                )
                .unwrap()
                .require(Greek::Gamma)
                .unwrap()
        };
        let speed = engine
            .price_with_greeks(&option, &context, GreeksRequest::new([Greek::Speed]))
            .unwrap();
        assert_eq!(
            speed.require(Greek::Speed).unwrap(),
            (gamma(100.25 + h) - gamma(100.25 - h)) / (2.0 * h)
        );
    }
    let collapsed = kiyosi::numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([Greek::Speed]),
        NumericalShiftSettings {
            spot_shift: Some(1e-300),
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
        kiyosi::numerical_greeks(
            &near,
            &option,
            &context,
            GreeksRequest::new([Greek::Speed]),
            NumericalShiftSettings {
                spot_shift: Some(0.5),
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
    let middle = Date::new(2025, 7, 1).unwrap().start_of_day();
    let risk = RateTermStructure::new(start, vec![middle, end], vec![0.1_f64.exp(), 1.0]).unwrap();
    let model = BlackScholesMertonParameters::new(risk.clone(), 0.0, 0.2).unwrap();
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceVanillaEngine::default();
    for valuation in [
        start,
        Date::new(2025, 4, 1).unwrap().start_of_day(),
        middle,
        Date::new(2025, 10, 1).unwrap().start_of_day(),
    ] {
        let context = PricingContext::new(model.clone(), 0.01, valuation).unwrap();
        let expected = 100.0 * risk.factor(valuation, valuation.max(middle)).unwrap() - 0.01;
        let actual = engine.price(&put, &context).unwrap();
        assert!(
            (actual - expected).abs() < 1e-5,
            "{valuation:?}: {actual} != {expected}"
        );
    }
    let context = PricingContext::new(model, 100.0, start).unwrap();
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
    let expected = AnalyticDigitalEngine.price(&digital, &context).unwrap();
    let actual = FiniteDifferenceDigitalEngine::default()
        .price_with_greeks(&digital, &context, GreeksRequest::new([]))
        .unwrap()
        .price();
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
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
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&option, &expiry, GreeksRequest::all())
        .unwrap();
    assert_eq!(result.price(), 0.0);
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Theta), None);
    assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);

    assert!(matches!(
        kiyosi::numerical_greeks(
            &engine,
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
            valuation_time: Some(Date::new(2024, 12, 31).unwrap().start_of_day()),
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
        time_step_count: 1_000_000,
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
            .price_with_greeks(&option, &context, GreeksRequest::all())
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
    let settings = ImpliedVolatilitySettings::new(0.1, 0.22);
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
fn automatic_european_grid_resolves_low_volatility_at_the_forward_strike() -> Result<()> {
    let (start, end) = dates();
    let engine = FiniteDifferenceVanillaEngine::default();
    for (rate, dividend) in [(0.1_f64, 0.0), (0.0, 0.1)] {
        let context = market(100.0, rate, dividend, 0.005);
        for direction in [OptionType::Call, OptionType::Put] {
            let option =
                EuropeanOption::new(direction, 100.0 * (rate - dividend).exp(), start, end)?;
            let expected = AnalyticEuropeanEngine.price(&option, &context)?;
            let saved = engine.solve(&option, &context, 95.0..=105.0)?;
            for price in [engine.price(&option, &context)?, saved.price(100.0)?] {
                assert!((price - expected).abs() < 5e-5, "{price} != {expected}");
            }
        }
    }
    Ok(())
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
                FiniteDifferenceScheme::Rannacher,
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
                - 9.227_005_508_154_036)
                .abs()
                < 0.03
        );
    }
}

#[test]
fn implicit_euler_prices_digitals_with_near_certain_payouts() -> Result<()> {
    let (start, end) = dates();
    let engine = FiniteDifferenceDigitalEngine::new(FiniteDifferenceSettings {
        scheme: FiniteDifferenceScheme::ImplicitEuler,
        ..Default::default()
    })?;
    for rate in [-0.05_f64, 0.05] {
        let context = market(250.0, rate, rate, 0.15);
        for (option, payout) in [
            (
                DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?,
                10.0,
            ),
            (
                DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end)?,
                250.0,
            ),
        ] {
            let price = engine.price(&option, &context)?;
            let expected = AnalyticDigitalEngine.price(&option, &context)?;
            let bound = payout * (-rate).exp();
            assert!(price <= bound + 1e-12);
            assert!(
                (price - expected).abs() <= 1e-7 * expected,
                "{price} vs {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn near_certain_digital_payout_allows_rollback_roundoff() -> Result<()> {
    let (start, end) = dates();
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 1.0, start, end)?;
    let context = market(200.0, 0.0, 0.0, 0.01);
    let engine = FiniteDifferenceDigitalEngine::default();
    let price = engine.price(&option, &context)?;
    assert!(price <= 1.0 && (price - 1.0).abs() < 1e-12);
    let solution = engine.solve(&option, &context, 199.0..=201.0)?;
    assert!((solution.price(200.0)? - price).abs() < 1e-12);
    Ok(())
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
                        valuation_time: Some(end),
                        ..Default::default()
                    })
                    .unwrap();
                let result = engine
                    .price_with_greeks(&digital, &context, GreeksRequest::all())
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
                assert_eq!(result.get(Greek::Delta), None);
                assert_eq!(result.get(Greek::Theta), None);
                assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
            }
        }
    }
}

#[test]
fn bsm_strike_batch_matches_analytic_prices_in_input_order() -> Result<()> {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    let strikes = [121.0, 77.0, 101.0, 89.0, 121.0, 200.0];
    for asset_upper_boundary in [None, Some(140.0)] {
        let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_upper_boundary,
            asset_step_count: 800,
            time_step_count: 200,
            ..Default::default()
        })?;
        let strikes = &strikes[..if asset_upper_boundary.is_some() { 5 } else { 6 }];
        for direction in [OptionType::Call, OptionType::Put] {
            let template = EuropeanOption::new(direction, 130.0, start, end)?;
            let prices = engine.price_strikes(&template, &context, strikes)?;
            assert_eq!(prices[0].to_bits(), prices[4].to_bits());
            for (price, &strike) in prices.into_iter().zip(strikes) {
                let contract = EuropeanOption::new(direction, strike, start, end)?;
                let expected = AnalyticEuropeanEngine.price(&contract, &context)?;
                assert!(
                    (price - expected).abs() < 0.0003,
                    "{strike}: {price} vs {expected}"
                );
            }
            if asset_upper_boundary.is_some() {
                assert!(matches!(
                    engine.price_strikes(&template, &context, &[121.0, 77.0, 101.0, 89.0, 140.0]),
                    Err(kiyosi::Error::InvalidInput(_))
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn weekday_strike_batch_uses_one_rescaled_solution() -> Result<()> {
    let (start, end) = dates();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?.replace(
            BlackScholesMertonParameterChanges {
                variance_time_convention: Some(VarianceTimeConvention::Business(
                    TradingCalendar::weekdays(),
                )),
                ..Default::default()
            },
        )?,
        100.0,
        start,
    )?;
    let instrument = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let strikes = [110.0, 80.0, 100.0, 95.0, 85.0, 110.0];
    let spots = strikes.map(|strike| 100.0 * (100.0 / strike));
    let engine = FiniteDifferenceVanillaEngine::default();
    let saved = engine.solve(&instrument, &context, spots[0]..=spots[1])?;
    let prices = engine.price_strikes(&instrument, &context, &strikes)?;
    for ((price, normalized), strike) in prices
        .into_iter()
        .zip(saved.price_many(&spots)?)
        .zip(strikes)
    {
        assert_eq!(price.to_bits(), (normalized * (strike / 100.0)).to_bits());
        let contract = EuropeanOption::new(OptionType::Call, strike, start, end)?;
        assert!((price - engine.price(&contract, &context)?).abs() < 1e-4);
    }
    Ok(())
}

#[test]
fn american_strike_batch_matches_scalar_prices_in_input_order() -> Result<()> {
    let (start, end) = dates();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let context = market(100.0, 0.05, 0.02, 0.25);
    let engine = FiniteDifferenceVanillaEngine::default();
    let strikes = [110.0, 90.0, 100.0, 95.0, 105.0, 90.0];
    let prices = engine.price_american_strikes(&option, &context, &strikes)?;
    assert_eq!(prices[1], prices[5]);
    for (&strike, price) in strikes.iter().zip(prices) {
        let scalar = option.replace(AmericanOptionChanges {
            strike: Some(strike),
            ..Default::default()
        })?;
        let reference = engine.price(&scalar, &context)?;
        assert!(
            (price - reference).abs() < 0.001,
            "strike {strike}: {price} vs {reference}"
        );
    }
    Ok(())
}

#[test]
fn bsm_european_surface_matches_analytic_across_dated_rates() -> Result<()> {
    let (start, end) = dates();
    let curve = RateTermStructure::new(start, vec![start.add_days(150)?, end], vec![0.98, 0.945])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.02, 0.25)?,
        100.0,
        start,
    )?;
    let template = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let expiries = [end, start, start.add_days(110)?, start.add_days(225)?, end];
    let strikes = [110.0, 90.0, 100.0, 95.0, 90.0];
    let engine = FiniteDifferenceVanillaEngine::default();
    let rows = engine.price_european_surface(&template, &context, &expiries, &strikes)?;
    assert_eq!(rows[0], rows[4]);
    for (&expiry, row) in expiries.iter().zip(rows) {
        assert_eq!(row[1].to_bits(), row[4].to_bits());
        for (&strike, value) in strikes.iter().zip(row) {
            let option = EuropeanOption::new(OptionType::Call, strike, start, expiry)?;
            let reference = AnalyticEuropeanEngine.price(&option, &context)?;
            assert!(
                (value - reference).abs() < 0.001,
                "{expiry:?}, {strike}: {value} vs {reference}"
            );
        }
    }
    Ok(())
}

#[test]
fn european_surface_caps_combined_expiry_rollbacks() -> Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 2048,
        time_step_count: 4096,
        ..Default::default()
    })?;
    let expiries = (0..180)
        .map(|days| end.add_days(-days))
        .collect::<Result<Vec<_>>>()?;
    assert!(matches!(
        engine.price_european_surface(&option, &market(100.0, 0.05, 0.02, 0.2), &expiries, &[100.0]),
        Err(Error::InvalidInput(message)) if message.contains("one billion grid updates")
    ));
    let context = market(100.0, 0.05, 0.02, 0.0);
    assert_eq!(
        engine
            .price_european_surface(&option, &context, &expiries, &[100.0])?
            .len(),
        180
    );
    Ok(())
}

#[test]
fn american_bsm_surface_matches_independent_strips_and_preserves_order() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let template = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.25)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let short = start.add_days(1)?;
    let expiries = [
        end,
        start,
        start.add_days(145)?,
        short,
        start.add_days(260)?,
        end,
    ];
    let strikes = [110.0, 90.0, 100.0, 95.0, 90.0];
    let rows = engine.price_american_surface(&template, &context, &expiries, &strikes)?;
    assert_eq!(rows[0], rows[5]);
    assert_eq!(rows[1], [10.0, 0.0, 0.0, 0.0, 0.0]);
    for (expiry, row) in expiries.into_iter().zip(rows) {
        assert_eq!(row[1].to_bits(), row[4].to_bits());
        let option = template.replace(AmericanOptionChanges {
            expiry_time: Some(expiry),
            ..Default::default()
        })?;
        let expected = engine.price_american_strikes(&option, &context, &strikes)?;
        if expiry == short {
            assert_eq!(row, expected);
        }
        for (actual, expected) in row.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.002, "{actual} vs {expected}");
        }
    }
    Ok(())
}

#[test]
fn american_surface_keeps_the_shared_grid_with_outliers_and_duplicates() -> Result<()> {
    let (start, end) = dates();
    let template = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.25)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let expiries = [end, start.add_days(145)?, start.add_days(260)?];
    let shared =
        engine.price_american_surface(&template, &context, &expiries, &[90.0, 100.0, 110.0])?;
    let mixed = engine.price_american_surface(
        &template,
        &context,
        &expiries,
        &[110.0, 220.0, 90.0, 100.0, 90.0],
    )?;
    for ((&expiry, shared), row) in expiries.iter().zip(shared).zip(mixed) {
        let outlier = AmericanOption::new(OptionType::Put, 220.0, start, expiry)?;
        assert_eq!(
            row,
            [
                shared[2],
                engine.price(&outlier, &context)?,
                shared[0],
                shared[1],
                shared[0]
            ]
        );
    }
    assert_eq!(
        engine.price_american_strikes(&template, &context, &[100.0; 8])?,
        vec![engine.price(&template, &context)?; 8]
    );
    Ok(())
}
