mod support;
use support::dated;

use kiyosi::*;

fn day(month: u32, date: u32) -> Timestamp {
    Date::new(2025, month, date).unwrap().start_of_day()
}
fn terms(kind: BarrierType, direction: OptionType) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        direction,
        100.0,
        day(1, 1),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
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
        day(1, 1),
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
fn prepared_scheduled_prices_match_scalar_after_spot_and_model_changes() {
    let engine = QuadratureBarrierEngine::new(QuadratureSettings {
        asset_step_count: 128,
        ..Default::default()
    })
    .unwrap();
    for kind in KINDS {
        let mut terms = terms(kind, OptionType::Call);
        terms.observation_times = Some(vec![day(2, 3), day(5, 1), terms.expiry_time]);
        let option = BarrierOption::new(terms).unwrap();
        let context = market(0.04);
        let spot = context
            .replace(PricingContextChanges {
                spot_price: Some(
                    if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
                        105.0
                    } else {
                        95.0
                    },
                ),
                ..Default::default()
            })
            .unwrap();
        let model = spot
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(-0.02, 0.01, 0.35)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let prepared = engine.prepare_request().unwrap().unwrap();
        for context in [&context, &spot, &model, &context] {
            assert_eq!(
                prepared.price(&dated(&option, context), context).unwrap(),
                engine.price(&dated(&option, context), context).unwrap(),
                "{kind:?} {context:?}"
            );
        }
    }
}

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
                        let actual = engine.price(&dated(&option, &context), &context).unwrap();
                        let expected = AnalyticBarrierEngine
                            .price(&dated(&option, &context), &context)
                            .unwrap();
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
        t.observation_times = Some(vec![t.expiry_time]);
        t.rebate = 3.0;
        let option = BarrierOption::new(t).unwrap();
        for (steps, extent) in [(4, 0.1), (512, 8.0)] {
            let engine = QuadratureBarrierEngine::new(QuadratureSettings {
                asset_step_count: steps,
                integration_standard_deviations: extent,
            })
            .unwrap();
            assert!(
                (engine.price(&dated(&option, &context), &context).unwrap() - expected).abs()
                    < 1e-11
            );
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
            t.observation_times = Some(vec![day(4, 1)]);
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
                .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
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
    t.observation_times = Some(vec![day(4, 1), day(7, 1), day(10, 1)]);
    let option = BarrierOption::new(t).unwrap();
    let context = market(0.04);
    let price = |steps, extent| {
        QuadratureBarrierEngine::new(QuadratureSettings {
            asset_step_count: steps,
            integration_standard_deviations: extent,
        })
        .unwrap()
        .price(&dated(&option, &context), &context)
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
            valuation_time: Some(observed),
            ..Default::default()
        })
        .unwrap();
    let past = boundary
        .replace(PricingContextChanges {
            spot_price: Some(110.0),
            valuation_time: Some(Timestamp::new(observed.date(), 1, 0, 0, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.rebate = 3.0;
    t.rebate_timing = RebateTiming::AtHit;
    t.observation_times = Some(vec![observed, t.expiry_time]);
    let option = BarrierOption::new(t.clone()).unwrap();
    assert_eq!(
        engine.price(&dated(&option, &boundary), &boundary).unwrap(),
        3.0
    );
    let result = engine
        .price_with_greeks(&dated(&option, &boundary), &boundary, GreeksRequest::all())
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Vega), Some(0.0));
    assert_eq!(result.get(Greek::Rho), Some(0.0));
    assert!(matches!(
        engine.price(&dated(&option, &past), &past).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let paid = option
        .replace_history(
            Some(BarrierTouchState::Touched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    assert_eq!(engine.price(&dated(&paid, &past), &past).unwrap(), 0.0);
    let untouched = option
        .replace_history(
            Some(BarrierTouchState::Untouched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    assert!(engine.price(&dated(&untouched, &past), &past).unwrap() > 0.0);
    assert!(matches!(
        engine.price(&dated(&paid, &context), &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    t.observation_times = Some(vec![observed]);
    t.rebate_timing = RebateTiming::AtExpiry;
    t.touch_state = Some(BarrierTouchState::Untouched);
    t.history_time = Some(t.effective_time);
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
                valuation_time: Some(t.expiry_time),
                ..Default::default()
            })
            .unwrap();
        let result = engine
            .price_with_greeks(&dated(&stopped, &expiry), &expiry, GreeksRequest::all())
            .unwrap();
        assert_eq!(
            result.price(),
            if matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn) {
                3.0
            } else {
                10.0
            }
        );
        for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
            assert_eq!(result.get(greek), None);
        }
        for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
            if result.supported().has(greek) {
                assert_eq!(result.require(greek).unwrap(), 0.0);
            }
        }
        assert!(matches!(
            implied_volatility(
                &engine,
                &dated(&stopped, &expiry),
                &expiry,
                result.price(),
                Default::default()
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    t.observation_times = Some(vec![day(1, 4)]);
    t.touch_state = Some(BarrierTouchState::Touched);
    t.history_time = Some(t.effective_time);
    assert!(matches!(BarrierOption::new(t), Err(Error::InvalidInput(_))));
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
    let quote = engine.price(&dated(&option, &context), &context).unwrap();
    for shifts in [
        NumericalShiftSettings {
            spot_shift: Some(0.0),
            ..Default::default()
        },
        NumericalShiftSettings {
            time_shift: Some(std::time::Duration::ZERO),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            kiyosi::numerical_greeks(
                &engine,
                &dated(&option, &context),
                &context,
                GreeksRequest::all(),
                shifts
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }

    for sigma in [f64::MAX, 1e200] {
        let model = BlackScholesMertonParameters::new(0.04, 0.01, sigma).unwrap();
        let invalid = context
            .replace(PricingContextChanges {
                model_parameters: Some(model.into()),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine
                .price(&dated(&option, &invalid), &invalid)
                .unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    for date in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        Date::new(2026, 1, 2).unwrap().start_of_day(),
    ] {
        let invalid = context
            .replace(PricingContextChanges {
                valuation_time: Some(date),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine
                .price(&dated(&option, &invalid), &invalid)
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let curve = RateTermStructure::new(
        day(1, 1),
        vec![
            option.expiry_time(),
            (option.expiry_time()).add_days(1).unwrap(),
        ],
        vec![(-0.04_f64).exp(), ((-0.04_f64).exp()) * 0.99],
    )
    .unwrap();
    let model = context
        .model_parameters()
        .as_bsm()
        .unwrap()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let nodes = context
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&dated(&option, &nodes), &nodes).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(&dated(&option, &nodes), &nodes, GreeksRequest::new([]))
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(
        engine.price(&dated(&option, &context), &context).unwrap(),
        quote
    );
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
        .replace_history(
            Some(BarrierTouchState::Untouched),
            Some((terms(BarrierType::DownAndOut, OptionType::Call)).effective_time),
        )
        .unwrap();
    let context = market(0.04);
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega, Greek::Rho]);
    let actual = engine
        .price_with_greeks(&dated(&option, &context), &context, request)
        .unwrap();
    let expected = AnalyticBarrierEngine
        .price_with_greeks(&dated(&option, &context), &context, request)
        .unwrap();
    let scalar = numerical_greeks(
        &engine,
        &dated(&option, &context),
        &context,
        request,
        Default::default(),
    )
    .unwrap();
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
        .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| all.has(g)));
    let solve = |option: &BarrierOption, context: &PricingContext, quote| {
        implied_volatility(
            &engine,
            &dated(option, context),
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
    let fixed = option
        .replace_history(
            Some(BarrierTouchState::Touched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    let later = context
        .replace(PricingContextChanges {
            valuation_time: Some(day(2, 3)),
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
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
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
            t.history_time = Some(t.effective_time);
            let expiry = t.expiry_time;
            let barrier = t.barrier_level;
            let up = matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut);
            let option = BarrierOption::new(t).unwrap();
            for (time, spot) in [
                (
                    Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 0).unwrap(),
                    100.0,
                ),
                (
                    Date::new(2025, 12, 2).unwrap().start_of_day(),
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
                assert!(time < expiry);
                assert!(
                    (engine.price(&dated(&option, &context), &context).unwrap()
                        - AnalyticBarrierEngine
                            .price(&dated(&option, &context), &context)
                            .unwrap())
                    .abs()
                        < 2e-8
                );
            }
        }
    }
    let option = BarrierOption::new(terms(BarrierType::DownAndIn, OptionType::Call)).unwrap();
    let context = market(0.04);
    let reference = AnalyticBarrierEngine
        .price(&dated(&option, &context), &context)
        .unwrap();
    let price = |steps, extent| {
        QuadratureBarrierEngine::new(QuadratureSettings {
            asset_step_count: steps,
            integration_standard_deviations: extent,
        })
        .unwrap()
        .price(&dated(&option, &context), &context)
        .unwrap()
    };
    let refined = price(1024, 10.0);
    assert!((refined - reference).abs() < 3e-8);
    assert!((refined - reference).abs() < (price(8, 10.0) - reference).abs());
    assert!((refined - reference).abs() < (price(1024, 1.0) - reference).abs());
}
