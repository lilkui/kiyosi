mod support;
use support::dated;

use kiyosi::*;

fn terms(direction: OptionType, kind: DoubleBarrierType) -> DoubleBarrierOptionTerms {
    DoubleBarrierOptionTerms::new(
        direction,
        100.0,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
        80.0,
        120.0,
        kind,
    )
}

fn market() -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
    )
    .unwrap()
}

#[test]
fn at_hit_rebate_greeks_require_a_usable_spot_stencil() {
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.strike = 1.2e16;
    t.lower_barrier = 8e15;
    t.upper_barrier = t.strike;
    t.rebate = 1.0;
    t.rebate_timing = RebateTiming::AtHit;
    let option = DoubleBarrierOption::new(t).unwrap();
    let context = market()
        .replace(PricingContextChanges {
            spot_price: Some(1e16),
            ..Default::default()
        })
        .unwrap();
    let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let result = engine
        .price_with_greeks(&dated(&option, &context), &context, request)
        .unwrap();
    assert_eq!(
        result.price(),
        engine.price(&dated(&option, &context), &context).unwrap()
    );
    for greek in [Greek::Delta, Greek::Gamma] {
        assert!(!result.has(greek));
        assert!(matches!(
            result.require(greek),
            Err(Error::UnsupportedOperation(_))
        ));
    }
    let settings = NumericalShiftSettings {
        spot_shift: Some(1e12),
        ..Default::default()
    };
    let expected = kiyosi::numerical_greeks(
        &AnalyticDoubleBarrierEngine,
        &dated(&option, &context),
        &context,
        request,
        settings,
    )
    .unwrap();
    let actual = kiyosi::numerical_greeks(
        &engine,
        &dated(&option, &context),
        &context,
        request,
        settings,
    )
    .unwrap();
    assert_eq!(actual.price(), expected.price());
    assert_eq!(actual.values(), expected.values());
    assert_eq!(actual.supported(), GreeksRequest::all());
}

#[test]
fn tiny_volatility_payoffs_match_the_vanilla_limit() {
    let sigma = 1e-17;
    let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    for spot in [1.0, 1e16] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap(),
            spot,
            Date::new(2025, 1, 1).unwrap().start_of_day(),
        )
        .unwrap();
        let expected = spot * sigma / std::f64::consts::TAU.sqrt();
        for direction in [OptionType::Call, OptionType::Put] {
            for mode in [ObservationMode::Continuous, ObservationMode::Scheduled] {
                let mut t = terms(direction, DoubleBarrierType::KnockOut);
                t.strike = spot;
                t.lower_barrier = 0.8 * spot;
                t.upper_barrier = 1.2 * spot;
                t.observation_mode = mode;
                if mode == ObservationMode::Scheduled {
                    t.observation_times = vec![t.expiry_time];
                }
                let option = DoubleBarrierOption::new(t).unwrap();
                let actual = engine.price(&dated(&option, &context), &context).unwrap();
                assert!(
                    (actual - expected).abs() < 1e-3 * expected,
                    "{spot} {direction:?} {mode:?}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn continuous_prices_and_shared_sample_greeks_match_independent_references() {
    for (direction, price, delta, gamma) in [
        (
            OptionType::Call,
            1.083_881_459_258_316_6,
            -0.009_437_716_341_942_348,
            -0.006_614_552_070_516_311,
        ),
        (
            OptionType::Put,
            1.576_043_514_776_614_5,
            -0.031_725_540_191_196_444,
            -0.008_683_048_910_995_694,
        ),
    ] {
        let option =
            DoubleBarrierOption::new(terms(direction, DoubleBarrierType::KnockOut)).unwrap();
        let context = market();
        for seed in [17, 42, 103] {
            let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
                path_count: 32768,
                seed: Some(seed),
                sampling: MonteCarloSampling::Sobol,
                ..Default::default()
            })
            .unwrap();
            let result = engine
                .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                .unwrap();
            assert!((result.price() - price).abs() < 0.003);
            assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 0.003);
            assert!((result.require(Greek::Gamma).unwrap() - gamma).abs() < 0.003);
            assert_eq!(
                result.price(),
                engine.price(&dated(&option, &context), &context).unwrap()
            );
            assert!(!result.has(Greek::Vega));
        }
    }
}

fn engine() -> MonteCarloDoubleBarrierEngine {
    MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 65536,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap()
}

fn compare(actual: &PricingResult, expected: &PricingResult, price: f64, delta: f64, gamma: f64) {
    for (got, want, budget) in [
        (actual.price(), expected.price(), price),
        (
            actual.require(Greek::Delta).unwrap(),
            expected.require(Greek::Delta).unwrap(),
            delta,
        ),
        (
            actual.require(Greek::Gamma).unwrap(),
            expected.require(Greek::Gamma).unwrap(),
            gamma,
        ),
    ] {
        assert!(
            (got - want).abs() < budget,
            "{got} != {want}, budget {budget}"
        );
    }
}

#[test]
fn pseudo_random_prices_and_likelihood_greeks_match_across_seeds() {
    for direction in [OptionType::Call, OptionType::Put] {
        for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
            let option = DoubleBarrierOption::new(terms(direction, kind)).unwrap();
            let expected = AnalyticDoubleBarrierEngine
                .price_with_greeks(
                    &dated(&option, &market()),
                    &market(),
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                )
                .unwrap();
            for seed in [17, 42, 103] {
                let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
                    path_count: 131_072,
                    seed: Some(seed),
                    antithetic: true,
                    ..Default::default()
                })
                .unwrap();
                let result = engine
                    .price_with_greeks(&dated(&option, &market()), &market(), GreeksRequest::all())
                    .unwrap();
                compare(&result, &expected, 0.12, 0.015, 0.003);
                for greek in [
                    Greek::Speed,
                    Greek::Theta,
                    Greek::Charm,
                    Greek::Color,
                    Greek::Vega,
                    Greek::Vanna,
                    Greek::Zomma,
                    Greek::Rho,
                ] {
                    assert!(!result.has(greek));
                }
            }
        }
    }
}

#[test]
fn image_and_spectral_series_and_extreme_bands_match_analytic() {
    let engine = engine();
    for sigma in [
        0.202_732_552_026_756_63,
        0.202_732_556_081_407_72,
        0.304_098_831_081_123_3,
    ] {
        let context = market()
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(0.04, 0.01, sigma)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let option =
            DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
        let actual = engine
            .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
            .unwrap();
        let expected = AnalyticDoubleBarrierEngine
            .price_with_greeks(
                &dated(&option, &context),
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            )
            .unwrap();
        compare(&actual, &expected, 0.003, 0.003, 0.003);
    }
    for (lower, upper) in [(99.99, 100.01), (0.01, 10000.0)] {
        let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
        t.lower_barrier = lower;
        t.upper_barrier = upper;
        let option = DoubleBarrierOption::new(t).unwrap();
        assert!(
            (engine.price(&dated(&option, &market()), &market()).unwrap()
                - AnalyticDoubleBarrierEngine
                    .price(&dated(&option, &market()), &market())
                    .unwrap())
            .abs()
                < 0.025
        );
    }
}

#[test]
fn continuous_rebates_and_tiny_volatility_hits_keep_values_and_derivatives() {
    let engine = engine();
    for rate in [-0.02, 0.04] {
        let context = market()
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, rate - 0.02, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        for (kind, timing) in [
            (DoubleBarrierType::KnockOut, RebateTiming::AtExpiry),
            (DoubleBarrierType::KnockOut, RebateTiming::AtHit),
            (DoubleBarrierType::KnockIn, RebateTiming::AtExpiry),
        ] {
            let mut t = terms(OptionType::Call, kind);
            t.upper_barrier = 125.0;
            t.rebate = 3.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t).unwrap();
            let actual = engine
                .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                .unwrap();
            let expected = AnalyticDoubleBarrierEngine
                .price_with_greeks(
                    &dated(&option, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                )
                .unwrap();
            compare(&actual, &expected, 0.01, 0.006, 0.003);
        }
    }
    for up in [false, true] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
            t.lower_barrier = if up { 80.0 } else { 98.0 };
            t.upper_barrier = if up { 102.0 } else { 120.0 };
            t.rebate = 3.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t).unwrap();
            let context = market()
                .replace(PricingContextChanges {
                    model_parameters: Some(
                        BlackScholesMertonParameters::new(
                            0.04,
                            if up { 0.01 } else { 0.07 },
                            1e-12,
                        )
                        .unwrap()
                        .into(),
                    ),
                    ..Default::default()
                })
                .unwrap();
            let time = if timing == RebateTiming::AtHit {
                (if up { 1.02_f64 } else { 100.0 / 98.0 }).ln() / 0.03
            } else {
                1.0
            };
            assert!(
                (engine.price(&dated(&option, &context), &context).unwrap()
                    - 3.0 * (-0.04 * time).exp())
                .abs()
                    < 1e-7
            );
        }
    }
}

#[test]
fn scheduled_dates_and_unobserved_expiry_leg_match_finite_difference() {
    let engine = engine();
    for direction in [OptionType::Call, OptionType::Put] {
        for (kind, timing) in [
            (DoubleBarrierType::KnockOut, RebateTiming::AtExpiry),
            (DoubleBarrierType::KnockOut, RebateTiming::AtHit),
            (DoubleBarrierType::KnockIn, RebateTiming::AtExpiry),
        ] {
            for final_fixing in [false, true] {
                let mut t = terms(direction, kind);
                t.rebate = 2.0;
                t.rebate_timing = timing;
                t.observation_mode = ObservationMode::Scheduled;
                t.observation_times = vec![Date::new(2025, 6, 30).unwrap().start_of_day()];
                if final_fixing {
                    t.observation_times.push(t.expiry_time);
                }
                let option = DoubleBarrierOption::new(t).unwrap();
                let actual = engine
                    .price_with_greeks(&dated(&option, &market()), &market(), GreeksRequest::all())
                    .unwrap();
                let expected = FiniteDifferenceDoubleBarrierEngine::new(FiniteDifferenceSettings {
                    time_step_count: 1200,
                    asset_step_count: 1200,
                    ..Default::default()
                })
                .unwrap()
                .price_with_greeks(
                    &dated(&option, &market()),
                    &market(),
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                )
                .unwrap();
                compare(&actual, &expected, 0.04, 0.025, 0.01);
            }
        }
    }
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.strike = 130.0;
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = vec![Date::new(2025, 4, 1).unwrap().start_of_day()];
    assert!(
        engine
            .price(
                &dated(&DoubleBarrierOption::new(t).unwrap(), &market()),
                &market()
            )
            .unwrap()
            > 0.01
    );
}

#[test]
fn thread_and_sampling_contracts_preserve_exact_samples_and_pairing() {
    assert_eq!(
        MonteCarloDoubleBarrierEngine::default().sampling_settings(),
        MonteCarloSettings::default()
    );
    for scheduled in [false, true] {
        let mut t = terms(OptionType::Put, DoubleBarrierType::KnockOut);
        t.rebate = 3.0;
        t.rebate_timing = RebateTiming::AtHit;
        if scheduled {
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_times = vec![
                Date::new(2025, 4, 1).unwrap().start_of_day(),
                Date::new(2025, 7, 1).unwrap().start_of_day(),
            ];
        }
        let option = DoubleBarrierOption::new(t).unwrap();
        let context = market();
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            for antithetic in [false, true] {
                let settings = MonteCarloSettings {
                    path_count: 1024,
                    seed: Some(17),
                    thread_count: 1,
                    sampling,
                    antithetic,
                };
                let serial = MonteCarloDoubleBarrierEngine::new(settings).unwrap();
                let parallel = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
                    thread_count: 4,
                    ..settings
                })
                .unwrap();
                let expected = serial
                    .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                    .unwrap();
                std::thread::scope(|scope| {
                    let handles = (0..4)
                        .map(|_| {
                            scope.spawn(|| {
                                parallel
                                    .price_with_greeks(
                                        &dated(&option, &context),
                                        &context,
                                        GreeksRequest::all(),
                                    )
                                    .unwrap()
                            })
                        })
                        .collect::<Vec<_>>();
                    for handle in handles {
                        assert_eq!(handle.join().unwrap(), expected);
                    }
                });
            }
        }
    }
    let context = market();
    let vanilla = EuropeanOption::new(
        OptionType::Call,
        100.0,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
    .unwrap();
    for path_count in [1, 2, 3, 5, 257] {
        let settings = MonteCarloSettings {
            path_count,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        };
        let engine = MonteCarloDoubleBarrierEngine::new(settings).unwrap();
        let total = [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut]
            .into_iter()
            .map(|kind| {
                engine
                    .price(
                        &dated(
                            &DoubleBarrierOption::new(terms(OptionType::Call, kind)).unwrap(),
                            &context,
                        ),
                        &context,
                    )
                    .unwrap()
            })
            .sum::<f64>();
        let expected = MonteCarloVanillaEngine::new(settings)
            .unwrap()
            .price(&dated(&vanilla, &context), &context)
            .unwrap();
        assert!((total - expected).abs() < 1e-12);
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "Exact comparisons select fixed test cases and payoff boundaries."
)]
fn current_and_prior_touches_expiry_and_finished_monitoring_resolve_before_sampling() {
    let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            if kind == DoubleBarrierType::KnockIn && timing == RebateTiming::AtHit {
                continue;
            }
            let mut t = terms(OptionType::Call, kind);
            t.rebate = 7.0;
            t.rebate_timing = timing;
            let option = DoubleBarrierOption::new(t.clone()).unwrap();
            for spot in [79.0, 80.0, 120.0, 121.0] {
                let context = market()
                    .replace(PricingContextChanges {
                        spot_price: Some(spot),
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(
                    engine.price(&dated(&option, &context), &context).unwrap(),
                    AnalyticDoubleBarrierEngine
                        .price(&dated(&option, &context), &context)
                        .unwrap()
                );
                if spot == 80.0 || spot == 120.0 {
                    let result = engine
                        .price_with_greeks(
                            &dated(&option, &context),
                            &context,
                            GreeksRequest::all(),
                        )
                        .unwrap();
                    assert!(!result.has(Greek::Delta) && !result.has(Greek::Gamma));
                }
                let later = context
                    .replace(PricingContextChanges {
                        valuation_time: Some(
                            Timestamp::new(option.effective_time().date(), 0, 0, 0, 1).unwrap(),
                        ),
                        ..Default::default()
                    })
                    .unwrap();
                assert!(matches!(
                    engine.price(&dated(&option, &later), &later).unwrap_err(),
                    Error::InvalidInput(_)
                ));
                let touched = option.replace_history(Some(BarrierTouchState::Touched));
                assert!(matches!(
                    engine
                        .price(&dated(&touched, &context), &context)
                        .unwrap_err(),
                    Error::InvalidInput(_)
                ));
                assert_eq!(
                    engine.price(&dated(&touched, &later), &later).unwrap(),
                    AnalyticDoubleBarrierEngine
                        .price(&dated(&touched, &later), &later)
                        .unwrap()
                );
                for history in [BarrierTouchState::Touched, BarrierTouchState::Untouched] {
                    let known = option.replace_history(Some(history));
                    let expired = context
                        .replace(PricingContextChanges {
                            valuation_time: Some(option.expiry_time()),
                            ..Default::default()
                        })
                        .unwrap();
                    let result = engine
                        .price_with_greeks(&dated(&known, &expired), &expired, GreeksRequest::all())
                        .unwrap();
                    assert_eq!(
                        result.price(),
                        AnalyticDoubleBarrierEngine
                            .price(&dated(&known, &expired), &expired)
                            .unwrap()
                    );
                    let reference = AnalyticDoubleBarrierEngine
                        .price_with_greeks(&dated(&known, &expired), &expired, GreeksRequest::all())
                        .unwrap();
                    assert_eq!(result.get(Greek::Delta), reference.get(Greek::Delta));
                    assert_eq!(result.get(Greek::Gamma), reference.get(Greek::Gamma));
                }
            }
            t.observation_mode = ObservationMode::Scheduled;
            let first = Date::new(2025, 7, 1).unwrap().start_of_day();
            t.observation_times = vec![first];
            let scheduled = DoubleBarrierOption::new(t).unwrap();
            let on_date = market()
                .replace(PricingContextChanges {
                    spot_price: Some(120.0),
                    valuation_time: Some(first),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                engine
                    .price(&dated(&scheduled, &on_date), &on_date)
                    .unwrap(),
                AnalyticDoubleBarrierEngine
                    .price(&dated(&scheduled, &on_date), &on_date)
                    .unwrap()
            );
            let after = on_date
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(first.date(), 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            let untouched = scheduled.replace_history(Some(BarrierTouchState::Untouched));
            assert_eq!(
                engine.price(&dated(&untouched, &after), &after).unwrap(),
                AnalyticDoubleBarrierEngine
                    .price(&dated(&untouched, &after), &after)
                    .unwrap()
            );
            assert_eq!(option.touch_state(), None);
        }
    }
    let option =
        DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
    for spot in [80.005, 119.995] {
        let context = market()
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap();
        let result = engine
            .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
            .unwrap();
        assert!(!result.has(Greek::Delta) && !result.has(Greek::Gamma));
    }
}

#[test]
fn solver_pins_request_samples_and_rejects_fixed_cashflows() {
    let engine = engine();
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.lower_barrier = 50.0;
    t.upper_barrier = 200.0;
    let option = DoubleBarrierOption::new(t.clone()).unwrap();
    let context = market();
    let quote = engine.price(&dated(&option, &context), &context).unwrap();
    let controls = ImpliedVolatilitySettings {
        lower_bound: 0.15,
        upper_bound: 0.25,
        ..Default::default()
    };
    assert!(
        (implied_volatility(
            &engine,
            &dated(&option, &context),
            &context,
            quote,
            controls
        )
        .unwrap()
            - 0.2)
            .abs()
            < 1e-5
    );
    let unseeded = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
        path_count: 2048,
        ..Default::default()
    })
    .unwrap();
    let prepared = unseeded.prepare_request().unwrap().unwrap();
    assert_eq!(
        prepared.price(&dated(&option, &context), &context).unwrap(),
        prepared.price(&dated(&option, &context), &context).unwrap()
    );
    for observed in [-1.0, f64::NAN] {
        let error = implied_volatility(
            &engine,
            &dated(&option, &context),
            &context,
            observed,
            controls,
        )
        .unwrap_err();
        if observed.is_nan() {
            assert!(matches!(error, Error::InvalidInput(_)));
        } else {
            assert!(matches!(error, Error::RootNotBracketed(_)));
        }
    }
    let later = context
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2025, 7, 2).unwrap().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    t.rebate = 3.0;
    t.touch_state = Some(BarrierTouchState::Touched);
    let touched = DoubleBarrierOption::new(t).unwrap();
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&touched, &later),
            &later,
            engine.price(&dated(&touched, &later), &later).unwrap(),
            controls,
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockIn);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = vec![Date::new(2025, 7, 1).unwrap().start_of_day()];
    t.touch_state = Some(BarrierTouchState::Untouched);
    let finished = DoubleBarrierOption::new(t).unwrap();
    assert!(matches!(
        implied_volatility(&engine, &dated(&finished, &later), &later, 0.0, controls).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    for direction in [OptionType::Call, OptionType::Put] {
        let mut t = terms(direction, DoubleBarrierType::KnockOut);
        t.strike = if direction == OptionType::Call {
            125.0
        } else {
            75.0
        };
        let excluded = DoubleBarrierOption::new(t).unwrap();
        assert_eq!(
            engine.price(&dated(&excluded, &context), &context).unwrap(),
            0.0
        );
        assert!(matches!(
            implied_volatility(
                &engine,
                &dated(&excluded, &context),
                &context,
                0.0,
                controls
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    assert_eq!(
        engine.price(&dated(&option, &context), &context).unwrap(),
        quote
    );
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
}

#[test]
fn invalid_settings_applicability_and_failure_categories_preserve_inputs() {
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            thread_count: -1,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 3,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloDoubleBarrierEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let engine = engine();
    let option =
        DoubleBarrierOption::new(terms(OptionType::Call, DoubleBarrierType::KnockOut)).unwrap();
    let context = market();
    let quote = engine.price(&dated(&option, &context), &context).unwrap();
    let curve = RateTermStructure::new(
        option.effective_time(),
        vec![
            option.expiry_time(),
            (option.expiry_time()).add_days(1).unwrap(),
        ],
        vec![(-0.03_f64).exp(), ((-0.03_f64).exp()) * 0.99],
    )
    .unwrap();
    for (rate, dividend) in [
        (Rate::Curve(curve.clone()), Rate::Flat(0.01)),
        (Rate::Flat(0.04), Rate::Curve(curve)),
    ] {
        let curved = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, dividend, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&dated(&option, &curved), &curved).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
        assert!(matches!(
            engine
                .price_with_greeks(&dated(&option, &curved), &curved, GreeksRequest::new([]))
                .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
        assert!(matches!(
            implied_volatility(
                &engine,
                &dated(&option, &curved),
                &curved,
                f64::NAN,
                Default::default()
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    assert!(matches!(
        engine
            .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::new([]))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        kiyosi::numerical_greeks(
            &engine,
            &dated(&option, &context),
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings {
                spot_shift: Some(0.0),
                ..Default::default()
            }
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(option.expiry_time().date(), 0, 0, 0, 1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price(&dated(&option, &outside), &outside)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = vec![Date::new(2025, 1, 4).unwrap().start_of_day()];
    assert!(matches!(
        DoubleBarrierOption::new(t),
        Err(Error::InvalidInput(_))
    ));
    for (rate, sigma) in [(-1000.0, 0.2), (0.04, 50.0)] {
        let extreme = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, rate, sigma)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine
                .price(&dated(&option, &extreme), &extreme)
                .unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    let tiny = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.04, 0.01, 1e-200)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let price = engine.price(&dated(&option, &tiny), &tiny).unwrap();
    assert_eq!(
        engine
            .price_with_greeks(
                &dated(&option, &tiny),
                &tiny,
                GreeksRequest::new([Greek::Rho])
            )
            .unwrap()
            .price(),
        price
    );
    assert!(matches!(
        engine
            .price_with_greeks(
                &dated(&option, &tiny),
                &tiny,
                GreeksRequest::new([Greek::Gamma])
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(
        engine.price(&dated(&option, &context), &context).unwrap(),
        quote
    );
}

#[test]
fn sobol_dimension_limit_counts_the_unobserved_expiry_leg() {
    let mut t = terms(OptionType::Call, DoubleBarrierType::KnockOut);
    t.expiry_time = Date::new(2100, 1, 1).unwrap().start_of_day();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = (2025..2100)
        .flat_map(|year| {
            (1..=12).flat_map(move |month| {
                (1..=28).map(move |day| Date::new(year, month, day).unwrap().start_of_day())
            })
        })
        .filter(|date| *date > t.effective_time)
        .take(21201)
        .collect();
    t.calendar = TradingCalendar::all_days();
    let option = DoubleBarrierOption::new(t).unwrap();
    assert!(matches!(
        engine()
            .price(&dated(&option, &market()), &market())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}
