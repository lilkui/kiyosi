use kiyosi_core::*;

fn terms(kind: BarrierType, direction: OptionType) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        direction,
        100.0,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
        if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
            120.13
        } else {
            79.87
        },
        kind,
    )
}
fn market(spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        Date::new(2025, 1, 1).unwrap().into(),
    )
    .unwrap()
}

#[test]
fn continuous_exact_edges_match_analytic_for_all_payoffs_and_rebates() {
    let engine = FiniteDifferenceBarrierEngine::default();
    for kind in [
        BarrierType::UpAndIn,
        BarrierType::UpAndOut,
        BarrierType::DownAndIn,
        BarrierType::DownAndOut,
    ] {
        let up = matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut);
        for direction in [OptionType::Call, OptionType::Put] {
            for rebate in [0.0, 2.0] {
                let mut t = terms(kind, direction);
                t.rebate = rebate;
                let option = BarrierOption::new(t).unwrap();
                for spot in [
                    100.0,
                    option.barrier_level() + if up { -0.01 } else { 0.01 },
                ] {
                    let context = market(spot);
                    let expected = AnalyticBarrierEngine.price(&option, &context).unwrap();
                    let actual = engine.price(&option, &context).unwrap();
                    assert!(
                        (actual - expected).abs() < 1e-9 + 0.005 * expected.abs(),
                        "{kind:?} {direction:?} {rebate} {spot}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn scheduled_events_match_independent_two_fixing_integrals_and_time_convergence() {
    for up in [false, true] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut t = terms(
                if up {
                    BarrierType::UpAndOut
                } else {
                    BarrierType::DownAndOut
                },
                if up {
                    OptionType::Call
                } else {
                    OptionType::Put
                },
            );
            t.barrier_level = if up { 110.125 } else { 90.125 };
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![Date::new(2025, 6, 30).unwrap(), t.expiry_date];
            t.rebate = 2.0;
            t.rebate_timing = timing;
            let option = BarrierOption::new(t).unwrap();
            // Conditional analytic payoffs integrated over the first fixing's normal density.
            let expected = match (up, timing) {
                (true, RebateTiming::AtHit) => [
                    1.4554158324974538,
                    0.040123419859492415,
                    -0.0018575837515300004,
                ],
                (true, RebateTiming::AtExpiry) => [
                    1.4450612046575544,
                    0.03921123536516191,
                    -0.0018895460854650987,
                ],
                (false, RebateTiming::AtHit) => [
                    1.3756950644643724,
                    -0.04356103943840228,
                    -0.000799252307714937,
                ],
                (false, RebateTiming::AtExpiry) => [
                    1.3672793398072463,
                    -0.04274689847083124,
                    -0.000853236609543728,
                ],
            };
            let mut refinements = Vec::new();
            for steps in [16, 64, 256, 2000] {
                let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
                    asset_step_count: 800,
                    time_step_count: steps,
                    asset_upper_boundary: Some(400.0),
                    ..Default::default()
                })
                .unwrap();
                let result = engine
                    .price_with_greeks(
                        &option,
                        &market(100.25),
                        GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                        Default::default(),
                    )
                    .unwrap();
                let actual = [
                    result.price(),
                    result.require(Greek::Delta).unwrap(),
                    result.require(Greek::Gamma).unwrap(),
                ];
                for i in 0..3 {
                    let error = (actual[i] - expected[i]).abs();
                    if steps == 2000 {
                        assert!(error < [0.0008, 0.00003, 0.000003][i]);
                    }
                }
                refinements.push(actual);
                assert_eq!(
                    engine.price(&option, &market(100.25)).unwrap(),
                    result.price()
                );
            }
            let fine = refinements[3];
            let mut previous = [f64::INFINITY; 3];
            for actual in &refinements[..3] {
                for i in 0..3 {
                    let error = (actual[i] - fine[i]).abs();
                    assert!(error < previous[i]);
                    previous[i] = error;
                }
            }
            for i in 0..3 {
                assert!(previous[i] < [2e-6, 5e-7, 3e-8][i]);
            }
        }
    }
}

#[test]
fn current_history_expiry_and_resolved_values_precede_grid_validation() {
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    })
    .unwrap();
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.rebate = 3.0;
    t.rebate_timing = RebateTiming::AtHit;
    let option = BarrierOption::new(t.clone()).unwrap();
    assert_eq!(
        engine.price(&option, &market(t.barrier_level)).unwrap(),
        3.0
    );
    let later = market(100.0)
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2025, 1, 2).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &later).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let paid = option.replace(Some(BarrierTouchState::Touched)).unwrap();
    assert_eq!(engine.price(&paid, &later).unwrap(), 0.0);
    assert!(matches!(
        engine.price(&paid, &market(100.0)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        implied_volatility(&engine, &paid, &later, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let at_expiry = later
        .replace(PricingContextChanges {
            valuation_time: Some(t.expiry_date.into()),
            ..Default::default()
        })
        .unwrap();
    let untouched = option.replace(Some(BarrierTouchState::Untouched)).unwrap();
    let result = engine
        .price_with_greeks(
            &untouched,
            &at_expiry,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(result.price(), 0.0);
    assert!(Greek::ALL.iter().all(|&g| !result.has(g)));
    assert!(matches!(
        engine.price(&option, &market(100.0)).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn native_and_mixed_greeks_match_scalar_stencils_and_solver_preserves_inputs() {
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    let mut t = terms(BarrierType::DownAndOut, OptionType::Call);
    t.touch_state = Some(BarrierTouchState::Untouched);
    let option = BarrierOption::new(t).unwrap();
    let context = market(100.0);
    let result = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    let scalar = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::all(),
        NumericalShiftSettings {
            spot_shift: 0.1,
            ..Default::default()
        },
    )
    .unwrap();
    for greek in Greek::ALL {
        let actual = result.require(greek).unwrap();
        let expected = scalar.require(greek).unwrap();
        assert!(
            (actual - expected).abs() < 0.002 * expected.abs() + 1e-6,
            "{greek:?}: {actual} != {expected}"
        );
    }
    let collapsed = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed]),
            NumericalShiftSettings {
                spot_shift: 1e-300,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(collapsed.has(Greek::Delta) && collapsed.has(Greek::Gamma));
    assert!(!collapsed.has(Greek::Speed));
    let sigma = implied_volatility(
        &engine,
        &option,
        &context,
        result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.15,
            upper_bound: 0.25,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-7);
    assert_eq!(engine.price(&option, &context).unwrap(), result.price());
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn explicit_stability_uses_the_continuous_domain_and_settings_validate_eagerly() {
    let option = BarrierOption::new(terms(BarrierType::UpAndOut, OptionType::Call)).unwrap();
    let context = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.0, 0.0, 0.1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    for (steps, stable) in [(50, true), (1, false)] {
        let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
            asset_step_count: 200,
            time_step_count: steps,
            scheme: FiniteDifferenceScheme::ExplicitEuler,
            ..Default::default()
        })
        .unwrap();
        let result = engine.price(&option, &context);
        if stable {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result.unwrap_err(), Error::InvalidInput(_)));
        }
    }
    for settings in [
        FiniteDifferenceSettings {
            asset_step_count: 2,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(f64::NAN),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceBarrierEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn node_curves_align_with_observations_and_preserve_rebate_discounts() {
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    let first = Date::new(2025, 4, 2).unwrap();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![first, t.expiry_date];
    t.rebate = 7.0;
    let curve = RateTermStructure::new(
        t.effective_date.into(),
        vec![first.into(), t.expiry_date.into()],
        vec![(-0.08_f64 * 91.0 / 365.0).exp(), (-0.01_f64).exp()],
    )
    .unwrap();
    let context = market(150.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(curve.clone(), curve, 1e-12).unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        time_step_count: 800,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
        t.rebate_timing = timing;
        let option = BarrierOption::new(t.clone()).unwrap();
        let expected = 7.0
            * if timing == RebateTiming::AtHit {
                (-0.08_f64 * 91.0 / 365.0).exp()
            } else {
                (-0.01_f64).exp()
            };
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-5);
    }
    let short =
        RateTermStructure::new(t.effective_date.into(), vec![first.into()], vec![0.99]).unwrap();
    let uncovered = context
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(short, 0.0, 0.2).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price(&BarrierOption::new(t).unwrap(), &uncovered)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn scheduled_damping_restarts_equal_implicit_half_steps_without_new_fixings() {
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.expiry_date = Date::new(2025, 12, 27).unwrap();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![
        Date::new(2025, 4, 1).unwrap(),
        Date::new(2025, 6, 30).unwrap(),
        Date::new(2025, 9, 28).unwrap(),
        t.expiry_date,
    ];
    t.barrier_level = 110.125;
    t.rebate = 2.0;
    let context = market(100.25)
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        })
        .unwrap();
    for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
        t.rebate_timing = timing;
        let option = BarrierOption::new(t.clone()).unwrap();
        let results: Vec<_> = [
            (FiniteDifferenceScheme::CrankNicolson, 4),
            (FiniteDifferenceScheme::ImplicitEuler, 8),
        ]
        .into_iter()
        .map(|(scheme, steps)| {
            FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
                asset_step_count: 200,
                time_step_count: steps,
                scheme,
                asset_upper_boundary: Some(400.0),
            })
            .unwrap()
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                Default::default(),
            )
            .unwrap()
        })
        .collect();
        assert!((results[0].price() - results[1].price()).abs() < 1e-12);
        for greek in [Greek::Delta, Greek::Gamma] {
            assert!(
                (results[0].require(greek).unwrap() - results[1].require(greek).unwrap()).abs()
                    < 1e-12
            );
        }
    }
}

#[test]
fn scheduled_explicit_stability_preserves_the_uniform_grid_boundary() {
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![t.expiry_date];
    let option = BarrierOption::new(t).unwrap();
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        asset_step_count: 200,
        time_step_count: 1600,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        asset_upper_boundary: Some(400.0),
    })
    .unwrap();
    assert!(matches!(
        engine.price(&option, &market(100.0)).unwrap_err(),
        Error::InvalidInput(_)
    ));
}
