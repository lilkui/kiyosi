use kiyosi_core::*;

fn terms(kind: DoubleBarrierType, direction: OptionType) -> DoubleBarrierOptionTerms {
    DoubleBarrierOptionTerms::new(
        direction,
        100.0,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
        79.87,
        120.13,
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
fn negative_narrow_band_price_requires_a_stable_scheme() {
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
    t.lower_barrier = 99.5;
    t.upper_barrier = 100.5;
    t.strike = 90.0;
    let option = DoubleBarrierOption::new(t).unwrap();
    let settings = FiniteDifferenceSettings {
        asset_step_count: 80,
        time_step_count: 400,
        ..Default::default()
    };
    assert!(matches!(
        FiniteDifferenceDoubleBarrierEngine::new(settings)
            .unwrap()
            .price(&option, &market(100.0)),
        Err(Error::ComputationFailure(_))
    ));
    let stable = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
        scheme: FiniteDifferenceScheme::ImplicitEuler,
        ..settings
    })
    .unwrap()
    .price(&option, &market(100.0))
    .unwrap();
    assert!((0.0..1e-8).contains(&stable));
}

#[test]
fn continuous_prices_match_analytic_at_both_edges_with_all_rebates() {
    let engine = FiniteDifferenceDoubleBarrierEngine::default();
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        for direction in [OptionType::Call, OptionType::Put] {
            for rebate in [0.0, 2.0] {
                for timing in [RebateTiming::AtExpiry, RebateTiming::AtHit] {
                    if kind == DoubleBarrierType::KnockIn && timing == RebateTiming::AtHit {
                        continue;
                    }
                    let mut t = terms(kind, direction);
                    t.rebate = rebate;
                    t.rebate_timing = timing;
                    let option = DoubleBarrierOption::new(t).unwrap();
                    for spot in [100.0, 79.88, 120.12] {
                        let context = market(spot);
                        let expected = AnalyticDoubleBarrierEngine
                            .price(&option, &context)
                            .unwrap();
                        let actual = engine.price(&option, &context).unwrap();
                        assert!(
                            (actual - expected).abs() < 1e-8 + 0.001 * expected.abs(),
                            "{kind:?} {direction:?} {timing:?} {spot}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn current_history_expiry_and_resolved_values_precede_grid_validation() {
    let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    })
    .unwrap();
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
    t.rebate = 3.0;
    t.rebate_timing = RebateTiming::AtHit;
    let option = DoubleBarrierOption::new(t.clone()).unwrap();
    assert_eq!(
        engine.price(&option, &market(t.upper_barrier)).unwrap(),
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
    let paid = option.replace(Some(BarrierTouchState::Touched));
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
    let untouched = option.replace(Some(BarrierTouchState::Untouched));
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
    let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
    t.touch_state = Some(BarrierTouchState::Untouched);
    let option = DoubleBarrierOption::new(t).unwrap();
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
fn node_curves_align_with_observations_and_preserve_rebate_discounts() {
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
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
    let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
        time_step_count: 800,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
        t.rebate_timing = timing;
        let option = DoubleBarrierOption::new(t.clone()).unwrap();
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
            .price(&DoubleBarrierOption::new(t).unwrap(), &uncovered)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn scheduled_damping_restarts_equal_implicit_half_steps_without_new_fixings() {
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
    t.expiry_date = Date::new(2025, 12, 27).unwrap();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![
        Date::new(2025, 4, 1).unwrap(),
        Date::new(2025, 6, 30).unwrap(),
        Date::new(2025, 9, 28).unwrap(),
        t.expiry_date,
    ];
    t.upper_barrier = 110.125;
    t.rebate = 2.0;
    let context = market(100.25)
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        })
        .unwrap();
    for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
        t.rebate_timing = timing;
        let option = DoubleBarrierOption::new(t.clone()).unwrap();
        let results: Vec<_> = [
            (FiniteDifferenceScheme::CrankNicolson, 4),
            (FiniteDifferenceScheme::ImplicitEuler, 8),
        ]
        .into_iter()
        .map(|(scheme, steps)| {
            FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
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
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![t.expiry_date];
    let option = DoubleBarrierOption::new(t).unwrap();
    let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
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

#[test]
fn one_two_and_early_fixings_match_independent_lognormal_integrals() {
    let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 2000,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            // Independent truncated lognormal moments and conditional Simpson integration.
            let expected = match (direction, timing) {
                (OptionType::Call, RebateTiming::AtHit) => {
                    [3.500902014735285, 3.271334905944365, 6.639938084779594]
                }
                (OptionType::Call, RebateTiming::AtExpiry) => {
                    [3.500902014735285, 3.2651695299351875, 6.633772708770416]
                }
                (OptionType::Put, RebateTiming::AtHit) => {
                    [3.7513175938487926, 3.6254035470561354, 5.417319947449319]
                }
                (OptionType::Put, RebateTiming::AtExpiry) => {
                    [3.7513175938487926, 3.619238171046958, 5.411154571440141]
                }
            };
            let mut t = terms(DoubleBarrierType::KnockOut, direction);
            t.lower_barrier = 80.125;
            t.upper_barrier = 120.125;
            t.rebate = 2.0;
            t.rebate_timing = timing;
            t.observation_mode = ObservationMode::Scheduled;
            let first = Date::new(2025, 6, 30).unwrap();
            for (index, dates) in [vec![t.expiry_date], vec![first, t.expiry_date], vec![first]]
                .into_iter()
                .enumerate()
            {
                t.observation_dates = dates;
                let option = DoubleBarrierOption::new(t.clone()).unwrap();
                let actual = engine.price(&option, &market(100.25)).unwrap();
                assert!(
                    (actual - expected[index]).abs() < 0.002,
                    "{direction:?} {timing:?} {index}: {actual} != {}",
                    expected[index]
                );
            }
        }
    }
}

#[test]
fn narrow_interim_event_cells_remain_positive_and_converge() {
    let mut worst = [0.0_f64; 3];
    for (lower, upper, expected) in [
        (80.05, 80.0501, 4.679426692850714e-6),
        (80.24995, 80.25005, 4.9118628396575025e-6),
        (80.74995, 80.75005, 5.5303622727065195e-6),
    ] {
        let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
        t.strike = 80.0;
        t.lower_barrier = lower;
        t.upper_barrier = upper;
        t.observation_mode = ObservationMode::Scheduled;
        t.observation_dates = vec![Date::new(2025, 6, 30).unwrap()];
        let option = DoubleBarrierOption::new(t).unwrap();
        for scheme in [
            FiniteDifferenceScheme::CrankNicolson,
            FiniteDifferenceScheme::ImplicitEuler,
        ] {
            for (index, count) in [800, 1600, 3200].into_iter().enumerate() {
                let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
                    asset_step_count: count,
                    time_step_count: 2000,
                    scheme,
                    asset_upper_boundary: Some(400.0),
                })
                .unwrap();
                let actual = engine.price(&option, &market(100.25)).unwrap();
                assert!(actual > 0.0);
                let relative = (actual - expected).abs() / expected;
                assert!(relative < 0.02, "{lower} {scheme:?} {count}: {relative}");
                worst[index] = worst[index].max(relative);
            }
        }
    }
    assert!(worst[2] < worst[0] && worst[2] < 0.01);
}

#[test]
fn continuous_grid_refinement_schemes_and_settings_preserve_contracts() {
    let mut t = terms(DoubleBarrierType::KnockOut, OptionType::Call);
    t.lower_barrier = 80.0;
    t.upper_barrier = 120.0;
    let option = DoubleBarrierOption::new(t).unwrap();
    for spot in [80.125, 100.0, 119.875] {
        let context = market(spot);
        let expected = AnalyticDoubleBarrierEngine
            .price(&option, &context)
            .unwrap();
        let mut errors = Vec::new();
        for count in [80, 160, 320] {
            let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
                asset_step_count: count,
                time_step_count: count,
                ..Default::default()
            })
            .unwrap();
            errors.push((engine.price(&option, &context).unwrap() - expected).abs());
        }
        assert!(errors[2] < errors[0] && errors[2] < 0.001);
    }
    for (scheme, count) in [
        (FiniteDifferenceScheme::ImplicitEuler, 320),
        (FiniteDifferenceScheme::ExplicitEuler, 40),
    ] {
        let engine = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
            asset_step_count: count,
            time_step_count: 2000,
            scheme,
            ..Default::default()
        })
        .unwrap();
        assert!((engine.price(&option, &market(100.0)).unwrap() - 1.0838814592583166).abs() < 0.01);
    }
    let unstable = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
        asset_step_count: 400,
        time_step_count: 1,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        unstable.price(&option, &market(100.0)).unwrap_err(),
        Error::InvalidInput(_)
    ));
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
            FiniteDifferenceDoubleBarrierEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for spot in [80.0, 120.0] {
        let result = FiniteDifferenceDoubleBarrierEngine::default()
            .price_with_greeks(
                &option,
                &market(spot),
                GreeksRequest::all(),
                Default::default(),
            )
            .unwrap();
        assert!(Greek::ALL.iter().all(|&g| !result.has(g)));
    }
}
