use kiyosi_core::*;

fn date(month: u32, day: u32) -> Date {
    Date::new(2025, month, day).unwrap()
}
fn market(spot: f64, valuation: impl Into<Timestamp>) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        valuation.into(),
    )
    .unwrap()
}

#[test]
fn terminal_binary_matches_independent_cash_and_digital_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        date(1, 1),
        end,
        autocallable_observations(&[end], &[100.0], Some(&[0.1]), None).unwrap(),
    ))
    .unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 0.1, date(1, 1), end).unwrap();
    let context = market(100.0, date(1, 1));
    let expected = (-0.04_f64).exp() + AnalyticDigitalEngine.price(&digital, &context).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::default();
    let actual = engine.price(&note, &context).unwrap();
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}

#[test]
fn daily_layers_and_all_variants_converge_to_quadrature() {
    let start = date(1, 1);
    let end = date(4, 1);
    let observations = autocallable_observations(
        &[date(2, 3), date(3, 3), end],
        &[110.0, 108.0, 105.0],
        Some(&[0.08; 3]),
        None,
    )
    .unwrap();
    let context = market(100.0, start);
    for mode in [
        KnockInObservationMode::AtExpiry,
        KnockInObservationMode::EveryTradingDay,
    ] {
        let mut snow = SnowballTerms::new(
            0.06,
            100.0,
            95.0,
            100.0,
            60.0,
            start,
            end,
            observations.clone(),
        );
        snow.knock_in_observation_mode = mode;
        snow.barrier_state = Some(AutocallableBarrierState::None);
        let mut ternary =
            TernarySnowballTerms::new(0.06, 0.01, 95.0, start, end, observations.clone());
        ternary.knock_in_observation_mode = mode;
        ternary.barrier_state = Some(AutocallableBarrierState::None);
        let mut phoenix = PhoenixTerms::new(
            0.08,
            100.0,
            95.0,
            100.0,
            60.0,
            start,
            end,
            autocallable_observations(
                &[date(2, 3), date(3, 3), end],
                &[110.0, 108.0, 105.0],
                None,
                Some(&[95.0; 3]),
            )
            .unwrap(),
        );
        phoenix.knock_in_observation_mode = mode;
        phoenix.barrier_state = Some(AutocallableBarrierState::None);
        fn check<T: AutocallableTerms>(note: &AutocallableNote<T>, context: &PricingContext) {
            let reference = QuadratureAutocallableEngine::new(QuadratureSettings {
                asset_step_count: 1024,
                ..Default::default()
            })
            .unwrap()
            .price(note, context)
            .unwrap();
            for n in [800, 1600] {
                let fd = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
                    asset_step_count: n,
                    time_step_count: 2000,
                    asset_upper_boundary: Some(400.0),
                    ..Default::default()
                })
                .unwrap();
                let actual = fd.price(note, context).unwrap();
                assert!(
                    (actual - reference).abs() < 1e-4,
                    "n={n} {actual} != {reference}"
                );
            }
        }
        check(&SnowballOption::new(snow).unwrap(), &context);
        check(&TernarySnowballOption::new(ternary).unwrap(), &context);
        check(&PhoenixOption::new(phoenix).unwrap(), &context);
        check(
            &BinarySnowballOption::new(BinarySnowballTerms::new(
                0.06,
                start,
                end,
                observations.clone(),
            ))
            .unwrap(),
            &context,
        );
    }
}

#[test]
fn greeks_and_solvers_preserve_inputs_and_unidentifiable_errors() {
    let end = date(4, 1);
    let mut terms = PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        end,
        autocallable_observations(&[date(2, 3), end], &[110.0, 105.0], None, Some(&[90.0; 2]))
            .unwrap(),
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let note = PhoenixOption::new(terms).unwrap();
    let original = note.clone();
    let context = market(103.0, date(1, 1));
    let engine = FiniteDifferenceAutocallableEngine::default();
    let result = engine
        .price_with_greeks(
            &note,
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings {
                spot_shift: 0.25,
                ..Default::default()
            },
        )
        .unwrap();
    for greek in Greek::ALL {
        assert!(result.has(greek), "{greek:?}");
    }
    let sigma = implied_volatility(
        &engine,
        &note,
        &context,
        result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.1,
            upper_bound: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-6, "{sigma}");
    for coupon in [-0.05, 0.18] {
        let target = note.with_coupon(coupon).unwrap();
        let quote = engine.price(&target, &context).unwrap();
        let solved = implied_coupon(
            &engine,
            &note,
            &context,
            quote,
            ImpliedCouponSettings {
                lower_bound: -0.1,
                upper_bound: 0.3,
                price_tolerance: 1e-11,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((solved - coupon).abs() < 1e-8);
        assert!(
            (engine
                .price(&note.with_coupon(solved).unwrap(), &context)
                .unwrap()
                - quote)
                .abs()
                < 1e-11
        );
    }
    let settled = note
        .replace(Some(AutocallableBarrierState::KnockedOut))
        .unwrap();
    let later = market(100.0, date(3, 3));
    assert_eq!(engine.price(&settled, &later).unwrap(), 0.0);
    assert!(matches!(
        implied_volatility(&engine, &settled, &later, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        implied_coupon(&engine, &settled, &later, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(note, original);
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn defaults_validation_history_and_node_curves_are_preserved() {
    let defaults = FiniteDifferenceAutocallableEngine::default().settings();
    assert_eq!(
        (defaults.asset_step_count, defaults.time_step_count),
        (800, 2000)
    );
    assert_eq!(
        FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings::default())
            .unwrap()
            .settings()
            .time_step_count,
        200
    );
    assert!(matches!(
        FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        })
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let start = date(1, 1);
    let end = date(4, 1);
    let note = SnowballOption::new(SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[110.0; 2], Some(&[0.08; 2]), None).unwrap(),
    ))
    .unwrap();
    let engine = FiniteDifferenceAutocallableEngine::default();
    assert!(matches!(
        engine.price(&note, &market(100.0, date(1, 2))).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let known = note.replace(Some(AutocallableBarrierState::None)).unwrap();
    let impossible = note
        .replace(Some(AutocallableBarrierState::KnockedOut))
        .unwrap();
    assert!(matches!(
        engine
            .price(&impossible, &market(100.0, start))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let clipped = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(110.0),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        clipped.price(&known, &market(100.0, start)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let unstable = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        time_step_count: 1,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        unstable.price(&known, &market(100.0, start)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let ctx = market(100.0, start);
    let curve = RateTermStructure::new(
        start.into(),
        vec![date(2, 3).into(), end.into()],
        vec![
            (-0.04_f64 * 33.0 / 365.0).exp(),
            (-0.04_f64 * 90.0 / 365.0).exp(),
        ],
    )
    .unwrap();
    let curved = ctx
        .replace(PricingContextChanges {
            model_parameters: Some(
                ctx.model_parameters()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(curve.into()),
                        ..Default::default()
                    })
                    .unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&known, &curved).unwrap() - engine.price(&known, &ctx).unwrap()).abs()
            < 1e-12
    );
    let short = RateTermStructure::new(start.into(), vec![date(2, 3).into()], vec![0.99]).unwrap();
    let short_ctx = ctx
        .replace(PricingContextChanges {
            model_parameters: Some(
                ctx.model_parameters()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(short.into()),
                        ..Default::default()
                    })
                    .unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&known, &short_ctx).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(note.barrier_state(), None);
}

#[test]
fn current_threshold_greeks_and_fixed_node_curve_identifiability() {
    let start = date(1, 1);
    let end = date(4, 1);
    let mut terms = PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[110.0; 2], None, Some(&[90.0; 2])).unwrap(),
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let engine = FiniteDifferenceAutocallableEngine::default();
    let note = PhoenixOption::new(terms).unwrap();
    let result = engine
        .price_with_greeks(
            &note,
            &market(90.0, date(2, 3)),
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
    for greek in [Greek::Delta, Greek::Gamma, Greek::Speed, Greek::Theta] {
        assert!(!result.has(greek), "{greek:?}");
    }
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let curve = RateTermStructure::new(start.into(), vec![end.into()], vec![0.99]).unwrap();
    let model = BlackScholesMertonParameters::new(curve, 0.01, 0.2).unwrap();
    let ctx = PricingContext::new(model, 100.0, start.into()).unwrap();
    let fixed = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[110.0], None, None).unwrap(),
    ))
    .unwrap();
    let result = engine
        .price_with_greeks(&fixed, &ctx, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!((result.price() - 0.99).abs() < 1e-14);
    assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
    assert!(matches!(
        implied_volatility(&engine, &fixed, &ctx, result.price(), Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn binary_domains_use_actual_levels_in_small_price_units() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let mut reference: Option<f64> = None;
    for level in [100.0, 1e-6, 1e-9] {
        let note = BinarySnowballOption::new(BinarySnowballTerms::new(
            0.0,
            start,
            end,
            autocallable_observations(&[end], &[level], Some(&[0.1]), None).unwrap(),
        ))
        .unwrap();
        let ctx = market(level, start);
        let price = FiniteDifferenceAutocallableEngine::default()
            .price(&note, &ctx)
            .unwrap();
        let expected = QuadratureAutocallableEngine::default()
            .price(&note, &ctx)
            .unwrap();
        assert!(
            (price - expected).abs() < 1e-5,
            "level={level}: {price} != {expected}"
        );
        if let Some(value) = reference {
            assert!((price - value).abs() < 1e-10);
        }
        reference = Some(price);
        let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
            time_step_count: 2000,
            asset_upper_boundary: Some(4.0 * level),
            ..Default::default()
        })
        .unwrap();
        assert!((engine.price(&note, &ctx).unwrap() - expected).abs() < 1e-5);
    }
}

#[test]
fn native_spot_greeks_do_not_require_a_feasible_scalar_bump() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let ctx = market(100.0, start);
    let engine = FiniteDifferenceAutocallableEngine::default();
    for coupon in [0.0, 0.1] {
        let note = BinarySnowballOption::new(BinarySnowballTerms::new(
            0.0,
            start,
            end,
            autocallable_observations(&[end], &[100.0], Some(&[coupon]), None).unwrap(),
        ))
        .unwrap();
        let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed]);
        let reference = engine
            .price_with_greeks(&note, &ctx, request, Default::default())
            .unwrap();
        for shift in [200.0, 1e-300] {
            let actual = engine
                .price_with_greeks(
                    &note,
                    &ctx,
                    request,
                    NumericalShiftSettings {
                        spot_shift: shift,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(actual.get(Greek::Delta), reference.get(Greek::Delta));
            assert_eq!(actual.get(Greek::Gamma), reference.get(Greek::Gamma));
            assert!(!actual.has(Greek::Speed));
        }
    }
}

#[test]
fn explicit_uniform_grid_uses_the_top_node_stability_bound() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[100.0; 2], Some(&[0.1; 2]), None).unwrap(),
    ))
    .unwrap();
    let ctx = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 11,
        time_step_count: 10,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        asset_upper_boundary: Some(400.0),
    })
    .unwrap();
    assert!(matches!(
        engine.price(&note, &ctx).unwrap_err(),
        Error::InvalidInput(_)
    ));
}
