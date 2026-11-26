use kiyosi::*;

#[test]
fn continuous_upper_edge_uses_barrier_for_scalar_and_saved_prices() -> Result<()> {
    let option = BarrierOption::new(terms(BarrierType::UpAndOut, OptionType::Call))?;
    let context = market(100.0);
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(110.0),
        ..Default::default()
    })?;
    let expected = FiniteDifferenceBarrierEngine::default().price(&option, &context)?;
    assert!((engine.price(&option, &context)? - expected).abs() <= 1e-12);
    let saved = engine.solve(&option, &context, 90.0..=130.0)?;
    assert!((saved.price(100.0)? - expected).abs() < 1e-4);
    assert_eq!(saved.price(130.0)?, 0.0);
    Ok(())
}

#[test]
fn scheduled_barrier_rejects_drift_that_overwhelms_grid_diffusion() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Put,
        100.0,
        start,
        end,
        102.0,
        BarrierType::UpAndIn,
    );
    terms.rebate = 2.0;
    terms.observation_times = Some(vec![start.add_days(30)?, start.add_days(100)?, end]);
    terms.calendar = TradingCalendar::all_days();
    let option = BarrierOption::new(terms)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.001)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceBarrierEngine::default();
    assert!(matches!(
        engine.price(&option, &context),
        Err(Error::ComputationFailure(_))
    ));
    assert!(matches!(
        engine.solve(&option, &context, 99.9..=100.1),
        Err(Error::ComputationFailure(_))
    ));
    Ok(())
}

#[test]
fn nearby_grid_sizes_resolve_narrow_up_and_out_payoff() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let option = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        start.add_days(30)?,
        100.15,
        BarrierType::UpAndOut,
    ))?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.03, 0.01, 0.005)?,
        99.9,
        start,
    )?;
    let expected = AnalyticBarrierEngine.price(&option, &context)?;
    for asset_step_count in [799, 800] {
        let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
            asset_step_count,
            ..Default::default()
        })?;
        let saved = engine.solve(&option, &context, 99.89..=99.91)?;
        for actual in [engine.price(&option, &context)?, saved.price(99.9)?] {
            assert!(
                (actual - expected).abs() < 0.005 * expected,
                "asset steps={asset_step_count}: {actual} != {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn short_continuous_barrier_resolves_diffusion_and_strike() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(1)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.05)?,
        99.5,
        start,
    )?;
    let engine = FiniteDifferenceBarrierEngine::default();
    for rebate in [0.0, 3.0] {
        let mut terms = BarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            99.0,
            BarrierType::DownAndOut,
        );
        terms.rebate = rebate;
        let option = BarrierOption::new(terms)?;
        let expected = AnalyticBarrierEngine.price(&option, &context)?;
        let saved = engine.solve(&option, &context, 99.4..=99.6)?;
        for actual in [engine.price(&option, &context)?, saved.price(99.5)?] {
            assert!(
                (actual - expected).abs() < 0.001 * expected,
                "rebate={rebate}: {actual} != {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn continuous_barrier_resolves_low_volatility_near_the_edge() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let option = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        95.0,
        start,
        end,
        99.9,
        BarrierType::DownAndOut,
    ))?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.0, 0.01)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceBarrierEngine::default();
    let expected = AnalyticBarrierEngine.price(&option, &context)?;
    let saved = engine.solve(&option, &context, 99.95..=100.05)?;
    for actual in [engine.price(&option, &context)?, saved.price(100.0)?] {
        assert!(
            (actual - expected).abs() < 0.005 * expected,
            "{actual} != {expected}"
        );
    }
    Ok(())
}

#[test]
fn constant_local_volatility_resolves_continuous_barriers() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.0, 0.01)?,
        100.0,
        start,
    )?;
    let local = PricingContext::new(
        LocalVolatilityParameters::new(
            0.05,
            0.0,
            LocalVolatilitySurface::new(
                vec![start, end],
                vec![80.0, 120.0],
                vec![vec![0.01; 2]; 2],
            )?,
        )?,
        100.0,
        start,
    )?;
    let vanilla = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        95.0,
        start,
        end,
        99.9,
        BarrierType::DownAndOut,
    ))?;
    let touch = TouchOption::cash(
        TouchOptionTerms::new(start, end, 99.9, BarrierType::DownAndOut),
        1.0,
    )?;
    let vanilla_engine = FiniteDifferenceBarrierEngine::default();
    let touch_engine = FiniteDifferenceBinaryBarrierEngine::default();
    let range = 99.95..=100.05;
    for (actual, saved, expected) in [
        (
            vanilla_engine.price(&vanilla, &local)?,
            vanilla_engine
                .solve(&vanilla, &local, range.clone())?
                .price(100.0)?,
            AnalyticBarrierEngine.price(&vanilla, &bsm)?,
        ),
        (
            touch_engine.price(&touch, &local)?,
            touch_engine.solve(&touch, &local, range)?.price(100.0)?,
            AnalyticBinaryBarrierEngine.price(&touch, &bsm)?,
        ),
    ] {
        assert!(
            (actual - expected).abs() < 0.005 * expected,
            "{actual} != {expected}"
        );
        assert!(
            (saved - expected).abs() < 0.005 * expected,
            "{saved} != {expected}"
        );
    }
    Ok(())
}

#[test]
fn earlier_knock_out_preserves_rebate_greeks_at_later_barrier() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let first = start.add_days(365)?;
    let expiry = start.add_days(730)?;
    let risk = RateTermStructure::new(start, vec![first, expiry], vec![0.9375, 0.96875])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(risk, 0.0, 0.0)?,
        77.5,
        start,
    )?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        75.0,
        start,
        expiry,
        80.0,
        BarrierType::UpAndOut,
    );
    terms.observation_times = Some(vec![first, expiry]);
    terms.rebate = 10.0;
    let option = BarrierOption::new(terms)?;
    let engine = FiniteDifferenceBarrierEngine::default();
    let request = GreeksRequest::new([Greek::Delta, Greek::Rho]);
    for (strike, observation_times) in [
        (75.0, Some(vec![first, expiry])),
        (80.0, Some(vec![first, expiry])),
        (80.0, None),
    ] {
        let contract = option.replace(BarrierOptionChanges {
            strike: Some(strike),
            observation_times: Some(observation_times),
            ..Default::default()
        })?;
        let result = engine.price_with_greeks(&contract, &context, request)?;
        assert!((result.price() - 9.6875).abs() < 1e-12);
        assert!(result.require(Greek::Delta)?.abs() < 1e-12);
        assert!((result.require(Greek::Rho)? + 19.375).abs() < 2e-7);
        let saved = engine.solve(&contract, &context, 77.0..=78.0)?;
        let saved = saved.price_with_greeks(77.5, request, None)?;
        assert!(saved.require(Greek::Delta)?.abs() < 1e-12);
    }
    let last_only = option.replace(BarrierOptionChanges {
        observation_times: Some(Some(vec![expiry])),
        ..Default::default()
    })?;
    let result = engine.price_with_greeks(&last_only, &context, request)?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
    Ok(())
}

fn terms(kind: BarrierType, direction: OptionType) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        direction,
        100.0,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
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
        Date::new(2025, 1, 1).unwrap().start_of_day(),
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
            t.observation_times = Some(vec![
                Date::new(2025, 6, 30).unwrap().start_of_day(),
                t.expiry_time,
            ]);
            t.rebate = 2.0;
            t.rebate_timing = timing;
            let option = BarrierOption::new(t).unwrap();
            // Conditional analytic payoffs integrated over the first fixing's normal density.
            let expected = match (up, timing) {
                (true, RebateTiming::AtHit) => [
                    1.455_415_832_497_453_8,
                    0.040_123_419_859_492_415,
                    -0.001_857_583_751_530_000_4,
                ],
                (true, RebateTiming::AtExpiry) => [
                    1.445_061_204_657_554_4,
                    0.039_211_235_365_161_91,
                    -0.001_889_546_085_465_098_7,
                ],
                (false, RebateTiming::AtHit) => [
                    1.375_695_064_464_372_4,
                    -0.043_561_039_438_402_28,
                    -0.000_799_252_307_714_937,
                ],
                (false, RebateTiming::AtExpiry) => [
                    1.367_279_339_807_246_3,
                    -0.042_746_898_470_831_24,
                    -0.000_853_236_609_543_728,
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
                        assert!(error < [0.0008, 0.00003, 0.000_003][i]);
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
            valuation_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &later).unwrap_err(),
        Error::MissingHistory(_)
    ));
    let paid = option
        .replace(BarrierOptionChanges {
            touch_state: Some(Some(BarrierTouchState::Touched)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(engine.price(&paid, &later).unwrap(), 0.0);
    assert!(matches!(
        engine.price(&paid, &market(100.0)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        implied_volatility(
            &engine,
            &paid,
            &later,
            0.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let at_expiry = later
        .replace(PricingContextChanges {
            valuation_time: Some(t.expiry_time),
            ..Default::default()
        })
        .unwrap();
    let untouched = option
        .replace(BarrierOptionChanges {
            touch_state: Some(Some(BarrierTouchState::Untouched)),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&untouched, &at_expiry, GreeksRequest::all())
        .unwrap();
    assert_eq!(result.price(), 0.0);
    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
        assert_eq!(result.get(greek), None);
    }
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        if result.supported().has(greek) {
            assert_eq!(result.require(greek).unwrap(), 0.0);
        }
    }
    t.observation_times = Some(vec![t.expiry_time]);
    assert!(matches!(
        engine
            .price(&BarrierOption::new(t).unwrap(), &market(100.0))
            .unwrap_err(),
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
        .price_with_greeks(&option, &context, GreeksRequest::all())
        .unwrap();
    let scalar = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::all(),
        NumericalShiftSettings {
            spot_shift: Some(0.1),
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
    let collapsed = kiyosi::numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed]),
        NumericalShiftSettings {
            spot_shift: Some(1e-300),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!collapsed.has(Greek::Delta) && !collapsed.has(Greek::Gamma));
    assert!(!collapsed.has(Greek::Speed));
    let sigma = implied_volatility(
        &engine,
        &option,
        &context,
        result.price(),
        ImpliedVolatilitySettings::new(0.15, 0.25),
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-7);
    assert_eq!(engine.price(&option, &context).unwrap(), result.price());
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
}

#[test]
fn explicit_stability_uses_the_continuous_domain_and_settings_validate_eagerly() {
    let option = BarrierOption::new(terms(BarrierType::UpAndOut, OptionType::Call)).unwrap();
    let context = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.0, 0.0, 0.1)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    for (steps, stable) in [(800, true), (50, false), (1, false)] {
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
    let first = Date::new(2025, 4, 2).unwrap().start_of_day();
    t.observation_times = Some(vec![first, t.expiry_time]);
    t.rebate = 7.0;
    let curve = RateTermStructure::new(
        t.effective_time,
        vec![first, t.expiry_time],
        vec![(-0.08_f64 * 91.0 / 365.0).exp(), (-0.01_f64).exp()],
    )
    .unwrap();
    let context = market(150.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(curve.clone(), curve, 1e-12)
                    .unwrap()
                    .into(),
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
    let short = RateTermStructure::new(t.effective_time, vec![first], vec![0.99]).unwrap();
    let uncovered = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(short, 0.0, 0.2)
                    .unwrap()
                    .into(),
            ),
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
    t.expiry_time = Date::new(2025, 12, 27).unwrap().start_of_day();
    t.observation_times = Some(vec![
        Date::new(2025, 4, 1).unwrap().start_of_day(),
        Date::new(2025, 6, 30).unwrap().start_of_day(),
        Date::new(2025, 9, 28).unwrap().start_of_day(),
        t.expiry_time,
    ]);
    t.barrier_level = 110.125;
    t.rebate = 2.0;
    let context = market(100.25);
    for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
        t.rebate_timing = timing;
        let option = BarrierOption::new(BarrierOptionTerms {
            calendar: TradingCalendar::all_days(),
            ..t.clone()
        })
        .unwrap();
        let results: Vec<_> = [
            (FiniteDifferenceScheme::Rannacher, 4),
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
    t.observation_times = Some(vec![t.expiry_time]);
    let option = BarrierOption::new(t).unwrap();
    let settings = FiniteDifferenceSettings {
        asset_step_count: 200,
        time_step_count: 100,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        asset_upper_boundary: Some(400.0),
    };
    let engine = FiniteDifferenceBarrierEngine::new(settings).unwrap();
    assert!(matches!(
        engine.price(&option, &market(100.0)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let stable = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        time_step_count: 800,
        ..settings
    })
    .unwrap();
    assert!(stable.price(&option, &market(100.0)).unwrap() > 0.0);
}

#[test]
fn scheduled_barrier_work_limit_counts_damping_half_steps() -> Result<()> {
    let mut terms = terms(BarrierType::UpAndOut, OptionType::Call);
    let start = terms.effective_time;
    terms.expiry_time = start.add_days(2000)?;
    terms.calendar = TradingCalendar::all_days();
    terms.observation_times = Some(
        (1..=2000)
            .map(|day| start.add_days(day))
            .collect::<Result<_>>()?,
    );
    let option = BarrierOption::new(terms)?;
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        asset_step_count: 300_000,
        time_step_count: 1,
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &market(100.0)),
        Err(Error::InvalidInput(message)) if message.contains("rollback")
    ));
    Ok(())
}

#[test]
fn scheduled_barrier_market_tangents_match_scalar_bumps() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.09)?,
        100.25,
        start,
    )?;
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    let greeks = [Greek::Vega, Greek::Rho, Greek::DividendRho];
    let request = GreeksRequest::new(greeks);
    for (barrier, rebate_timing) in [
        (BarrierType::DownAndOut, RebateTiming::AtHit),
        (BarrierType::DownAndOut, RebateTiming::AtExpiry),
        (BarrierType::DownAndIn, RebateTiming::AtExpiry),
    ] {
        let mut terms = BarrierOptionTerms::new(OptionType::Put, 100.0, start, end, 80.0, barrier);
        terms.rebate = 2.0;
        terms.rebate_timing = rebate_timing;
        terms.observation_times = Some(vec![start.add_days(100)?, start.add_days(201)?, end]);
        let option = BarrierOption::new(terms)?;
        let result = engine.price_with_greeks(&option, &context, request)?;
        let expected = engine.price_with_greeks_with_settings(
            &option,
            &context,
            request,
            NumericalShiftSettings {
                volatility_shift: Some(0.0001),
                rate_shift: 0.000_101,
                ..Default::default()
            },
        )?;
        assert_eq!(result.price().to_bits(), expected.price().to_bits());
        for greek in greeks {
            let actual = result.require(greek)?;
            let reference = expected.require(greek)?;
            assert!(
                (actual - reference).abs() < 0.0002,
                "{greek:?}: {actual} != {reference}"
            );
        }
    }
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
        100.25,
        start,
    )?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Put,
        100.0,
        start,
        end,
        80.0,
        BarrierType::DownAndOut,
    );
    terms.rebate = 2.0;
    terms.rebate_timing = RebateTiming::AtHit;
    terms.observation_times = Some(vec![start.add_days(100)?, start.add_days(201)?, end]);
    let option = BarrierOption::new(terms)?;
    let reference = QuadratureBarrierEngine::new(QuadratureSettings {
        asset_step_count: 5000,
        ..Default::default()
    })?
    .price_with_greeks(&option, &context, request)?;
    let actual = engine.price_with_greeks(&option, &context, request)?;
    for greek in greeks {
        assert!((actual.require(greek)? - reference.require(greek)?).abs() < 0.003);
    }
    Ok(())
}
