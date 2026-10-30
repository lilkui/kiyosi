use kiyosi::*;

fn day(month: u32, date: u32) -> Date {
    Date::new(2025, month, date).unwrap()
}
fn terms(kind: BarrierType, direction: OptionType) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        direction,
        100.0,
        day(1, 1),
        Date::new(2026, 1, 1).unwrap(),
        if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
            120.0
        } else {
            80.0
        },
        kind,
    )
}
fn market(rate: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, 0.01, 0.2).unwrap(),
        100.0,
        day(1, 1).into(),
    )
    .unwrap()
}
const KINDS: [BarrierType; 4] = [
    BarrierType::UpAndIn,
    BarrierType::UpAndOut,
    BarrierType::DownAndIn,
    BarrierType::DownAndOut,
];

#[test]
fn continuous_quadrature_matches_analytic_for_all_payoffs_and_rebates() {
    let engine = QuadratureBarrierEngine::new(QuadratureSettings {
        asset_step_count: 1024,
        integration_standard_deviations: 10.0,
    })
    .unwrap();
    for kind in KINDS {
        for direction in [OptionType::Call, OptionType::Put] {
            for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
                if matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn)
                    && timing == RebateTiming::AtHit
                {
                    continue;
                }
                for rebate in [0.0, 3.0] {
                    let mut t = terms(kind, direction);
                    t.rebate = rebate;
                    t.rebate_timing = timing;
                    let option = BarrierOption::new(t).unwrap();
                    for rate in [-0.02, 0.04] {
                        let context = market(rate);
                        let actual = engine.price(&option, &context).unwrap();
                        let expected = AnalyticBarrierEngine.price(&option, &context).unwrap();
                        assert!(
                            (actual - expected).abs() < 3e-8,
                            "{kind:?} {direction:?} {timing:?} {rebate} {rate}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn expiry_only_observations_use_exact_digital_moments_independent_of_grid() {
    let context = market(0.04);
    let normal = |z: f64| 0.5 * libm::erfc(-z / std::f64::consts::SQRT_2);
    let above = |strike: f64| {
        let d2 = ((100.0 / strike).ln() + 0.01) / 0.2;
        [
            (-0.04_f64).exp() * normal(d2),
            100.0 * (-0.01_f64).exp() * normal(d2 + 0.2),
        ]
    };
    let strike = above(100.0);
    let barrier = above(120.0);
    let out = strike[1] - barrier[1] - 100.0 * (strike[0] - barrier[0]) + 3.0 * barrier[0];
    let inside = barrier[1] - 100.0 * barrier[0] + 3.0 * ((-0.04_f64).exp() - barrier[0]);
    for (kind, expected) in [(BarrierType::UpAndOut, out), (BarrierType::UpAndIn, inside)] {
        let mut t = terms(kind, OptionType::Call);
        t.observation_mode = ObservationMode::Scheduled;
        t.observation_dates = vec![t.expiry_date];
        t.rebate = 3.0;
        let option = BarrierOption::new(t).unwrap();
        for (steps, extent) in [(4, 0.1), (512, 8.0)] {
            let engine = QuadratureBarrierEngine::new(QuadratureSettings {
                asset_step_count: steps,
                integration_standard_deviations: extent,
            })
            .unwrap();
            assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-11);
        }
    }
}

#[test]
fn early_observation_rebates_discount_at_the_contractual_payment_time() {
    let engine = QuadratureBarrierEngine::default();
    for rate in [-0.02, 0.04] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
            t.strike = 1e9;
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![day(4, 1)];
            t.rebate = 7.0;
            t.rebate_timing = timing;
            let context = market(rate);
            let time: f64 = 90.0 / 365.0;
            let z = ((1.2_f64).ln() - (rate - 0.03) * time) / (0.2 * time.sqrt());
            let expected = 7.0
                * (-rate
                    * if timing == RebateTiming::AtHit {
                        time
                    } else {
                        1.0
                    })
                .exp()
                * 0.5
                * libm::erfc(z / std::f64::consts::SQRT_2);
            let actual = engine
                .price(&BarrierOption::new(t).unwrap(), &context)
                .unwrap();
            assert!(
                (actual - expected).abs() < 2e-11,
                "{timing:?} {rate}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn scheduled_space_and_extent_converge_independently() {
    let mut t = terms(BarrierType::DownAndOut, OptionType::Put);
    t.rebate = 1.0;
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![day(4, 1), day(7, 1), day(10, 1)];
    let option = BarrierOption::new(t).unwrap();
    let context = market(0.04);
    let price = |steps, extent| {
        QuadratureBarrierEngine::new(QuadratureSettings {
            asset_step_count: steps,
            integration_standard_deviations: extent,
        })
        .unwrap()
        .price(&option, &context)
        .unwrap()
    };
    let reference = price(1024, 12.0);
    let medium = price(512, 12.0);
    let coarse = price(128, 12.0);
    assert!(
        (medium - reference).abs() < 2e-5,
        "{coarse} {medium} {reference}"
    );
    assert!((medium - reference).abs() < (coarse - reference).abs());
    assert!((price(1024, 8.0) - reference).abs() < 2e-5);
}

#[test]
fn history_current_observations_and_expiry_preserve_settlement_order() {
    let engine = QuadratureBarrierEngine::default();
    let context = market(0.04);
    let observed = day(4, 1);
    let boundary = context
        .replace(PricingContextChanges {
            spot_price: Some(120.0),
            valuation_time: Some(observed.into()),
            ..Default::default()
        })
        .unwrap();
    let past = boundary
        .replace(PricingContextChanges {
            spot_price: Some(110.0),
            valuation_time: Some(Timestamp::new(observed, 1, 0, 0, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.rebate = 3.0;
    t.rebate_timing = RebateTiming::AtHit;
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![observed, t.expiry_date];
    let option = BarrierOption::new(t.clone()).unwrap();
    assert_eq!(engine.price(&option, &boundary).unwrap(), 3.0);
    let result = engine
        .price_with_greeks(&option, &boundary, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
    assert!(matches!(
        engine.price(&option, &past).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let paid = option.replace(Some(BarrierTouchState::Touched));
    assert_eq!(engine.price(&paid, &past).unwrap(), 0.0);
    let untouched = option.replace(Some(BarrierTouchState::Untouched));
    assert!(engine.price(&untouched, &past).unwrap() > 0.0);
    assert!(matches!(
        engine.price(&paid, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    t.observation_dates = vec![observed];
    t.rebate_timing = RebateTiming::AtExpiry;
    t.touch_state = Some(BarrierTouchState::Untouched);
    for kind in KINDS {
        t.barrier_type = kind;
        t.barrier_level = if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
            120.0
        } else {
            80.0
        };
        let stopped = BarrierOption::new(t.clone()).unwrap();
        let expiry = past
            .replace(PricingContextChanges {
                valuation_time: Some(t.expiry_date.into()),
                ..Default::default()
            })
            .unwrap();
        let result = engine
            .price_with_greeks(&stopped, &expiry, GreeksRequest::all(), Default::default())
            .unwrap();
        assert_eq!(
            result.price(),
            if matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn) {
                3.0
            } else {
                10.0
            }
        );
        assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
        assert!(matches!(
            implied_volatility(
                &engine,
                &stopped,
                &expiry,
                result.price(),
                Default::default()
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    t.observation_dates = vec![day(1, 4)];
    t.touch_state = Some(BarrierTouchState::Touched);
    let invalid = BarrierOption::new(t).unwrap();
    assert!(matches!(
        engine.price(&invalid, &past).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn settings_applicability_and_numerical_failures_leave_inputs_unchanged() {
    let engine = QuadratureBarrierEngine::default();
    assert_eq!(
        engine.settings(),
        QuadratureSettings {
            asset_step_count: 512,
            integration_standard_deviations: 8.0
        }
    );
    for (steps, extent) in [(3, 8.0), (512, 0.0), (512, f64::INFINITY), (512, f64::NAN)] {
        assert!(matches!(
            QuadratureBarrierEngine::new(QuadratureSettings {
                asset_step_count: steps,
                integration_standard_deviations: extent
            })
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let context = market(0.04);
    let quote = engine.price(&option, &context).unwrap();
    for shifts in [
        NumericalShiftSettings {
            spot_shift: 0.0,
            ..Default::default()
        },
        NumericalShiftSettings {
            time_shift_days: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), shifts)
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
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
    for sigma in [f64::MAX, 1e200] {
        let model = BlackScholesMertonParameters::new(0.04, 0.01, sigma).unwrap();
        let invalid = context
            .replace(PricingContextChanges {
                model_parameters: Some(model),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &invalid).unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    for date in [
        Date::new(2024, 12, 31).unwrap(),
        Date::new(2026, 1, 2).unwrap(),
    ] {
        let invalid = context
            .replace(PricingContextChanges {
                valuation_time: Some(date.into()),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &invalid).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let curve = RateTermStructure::new(
        day(1, 1).into(),
        vec![option.expiry_date().into()],
        vec![(-0.04_f64).exp()],
    )
    .unwrap();
    let model = context
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let nodes = context
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &nodes).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &nodes,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
}

#[test]
fn greeks_sensitivities_and_solving_share_scalar_semantics() {
    let engine = QuadratureBarrierEngine::new(QuadratureSettings {
        asset_step_count: 1024,
        integration_standard_deviations: 10.0,
    })
    .unwrap();
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call))
        .unwrap()
        .replace(Some(BarrierTouchState::Untouched));
    let context = market(0.04);
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega, Greek::Rho]);
    let actual = engine
        .price_with_greeks(&option, &context, request, Default::default())
        .unwrap();
    let expected = AnalyticBarrierEngine
        .price_with_greeks(&option, &context, request, Default::default())
        .unwrap();
    let scalar = numerical_greeks(&engine, &option, &context, request, Default::default()).unwrap();
    for greek in [Greek::Delta, Greek::Gamma, Greek::Vega, Greek::Rho] {
        assert!(
            (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs() < 3e-6,
            "{greek:?}"
        );
        assert_eq!(
            actual.require(greek).unwrap(),
            scalar.require(greek).unwrap()
        );
    }
    assert!(!actual.has(Greek::Speed));
    let all = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| all.has(g)));
    let solve = |option: &BarrierOption, context: &PricingContext, quote| {
        implied_volatility(
            &engine,
            option,
            context,
            quote,
            ImpliedVolatilitySettings {
                lower_bound: 0.15,
                upper_bound: 0.25,
                price_tolerance: 1e-9,
                parameter_tolerance: 1e-9,
                ..Default::default()
            },
        )
    };
    assert!((solve(&option, &context, actual.price()).unwrap() - 0.2).abs() < 1e-8);
    let fixed = option.replace(Some(BarrierTouchState::Touched));
    let later = context
        .replace(PricingContextChanges {
            valuation_time: Some(day(2, 3).into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        solve(&fixed, &later, 0.0).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        solve(&option, &context, -1.0).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn short_maturity_near_barrier_and_continuous_refinement_retain_accuracy() {
    let engine = QuadratureBarrierEngine::new(QuadratureSettings {
        asset_step_count: 2048,
        integration_standard_deviations: 10.0,
    })
    .unwrap();
    for kind in KINDS {
        for direction in [OptionType::Call, OptionType::Put] {
            let mut t = terms(kind, direction);
            t.rebate = 1.0;
            t.touch_state = Some(BarrierTouchState::Untouched);
            let expiry = t.expiry_date;
            let barrier = t.barrier_level;
            let up = matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut);
            let option = BarrierOption::new(t).unwrap();
            for (time, spot) in [
                (
                    Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 0).unwrap(),
                    100.0,
                ),
                (
                    Date::new(2025, 12, 2).unwrap().into(),
                    barrier + if up { -1e-7 } else { 1e-7 },
                ),
            ] {
                let context = market(0.04)
                    .replace(PricingContextChanges {
                        valuation_time: Some(time),
                        spot_price: Some(spot),
                        ..Default::default()
                    })
                    .unwrap();
                assert!(time < expiry.start_of_day());
                assert!(
                    (engine.price(&option, &context).unwrap()
                        - AnalyticBarrierEngine.price(&option, &context).unwrap())
                    .abs()
                        < 2e-8
                );
            }
        }
    }
    let option = BarrierOption::new(terms(BarrierType::DownAndIn, OptionType::Call)).unwrap();
    let context = market(0.04);
    let reference = AnalyticBarrierEngine.price(&option, &context).unwrap();
    let price = |steps, extent| {
        QuadratureBarrierEngine::new(QuadratureSettings {
            asset_step_count: steps,
            integration_standard_deviations: extent,
        })
        .unwrap()
        .price(&option, &context)
        .unwrap()
    };
    let refined = price(1024, 10.0);
    assert!((refined - reference).abs() < 3e-8);
    assert!((refined - reference).abs() < (price(8, 10.0) - reference).abs());
    assert!((refined - reference).abs() < (price(1024, 1.0) - reference).abs());
}
