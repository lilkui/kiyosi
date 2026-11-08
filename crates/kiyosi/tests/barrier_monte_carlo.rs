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
fn native_barrier_spot_controls_recover_near_boundary_greeks() -> Result<()> {
    let single = BarrierOption::new(terms(BarrierType::UpAndOut, OptionType::Call))?;
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        single.effective_time(),
        single.expiry_time(),
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))?;
    let context = market(0.05).replace(PricingContextChanges {
        spot_price: Some(119.995),
        ..Default::default()
    })?;
    let settings = MonteCarloSettings {
        path_count: 64,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    };
    let single_engine = MonteCarloBarrierEngine::new(settings)?;
    let double_engine = MonteCarloDoubleBarrierEngine::new(settings)?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let defaults = [
        single_engine.price_with_greeks(&single, &context, request)?,
        double_engine.price_with_greeks(&double, &context, request)?,
    ];
    for result in defaults {
        assert_eq!(result.status(Greek::Delta), GreekStatus::StencilUnavailable);
    }
    for (result, tighter) in [
        (
            single_engine.price_with_greeks_with_spot_shift(
                &single,
                &context,
                request,
                Some(0.0001),
            )?,
            single_engine.price_with_greeks_with_spot_shift(
                &single,
                &context,
                request,
                Some(0.00005),
            )?,
        ),
        (
            double_engine.price_with_greeks_with_spot_shift(
                &double,
                &context,
                request,
                Some(0.0001),
            )?,
            double_engine.price_with_greeks_with_spot_shift(
                &double,
                &context,
                request,
                Some(0.00005),
            )?,
        ),
    ] {
        assert!(result.require(Greek::Delta)? < 0.0);
        assert_eq!(result, tighter);
    }
    for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        for result in [
            single_engine.price_with_greeks_with_spot_shift(
                &single,
                &context,
                request,
                Some(invalid),
            ),
            double_engine.price_with_greeks_with_spot_shift(
                &double,
                &context,
                request,
                Some(invalid),
            ),
        ] {
            assert!(matches!(result, Err(Error::InvalidInput(_))));
        }
    }
    Ok(())
}

#[test]
fn at_hit_rebate_greeks_require_a_usable_spot_stencil() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for (spot, shift) in [(0.005, 0.01), (100.0, f64::MIN_POSITIVE)] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.04, 0.01, 1.0).unwrap(),
            spot,
            day(1, 1),
        )
        .unwrap();
        let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
        t.strike = 5.0 * spot;
        t.barrier_level = t.strike;
        t.rebate = 1.0;
        t.rebate_timing = RebateTiming::AtHit;
        let option = BarrierOption::new(t.clone()).unwrap();
        let result = kiyosi::numerical_greeks(
            &engine,
            &dated(&option, &context),
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(shift),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            result.price(),
            engine.price(&dated(&option, &context), &context).unwrap()
        );
        for greek in [Greek::Delta, Greek::Gamma] {
            assert!(!result.has(greek));
            assert!(matches!(
                result.require(greek),
                Err(Error::GreekUnavailable { greek: measure, status: GreekStatus::StencilUnavailable }) if measure == greek
            ));
        }
        let settings = NumericalShiftSettings {
            spot_shift: Some(1e-4 * spot),
            ..Default::default()
        };
        let expected = kiyosi::numerical_greeks(
            &AnalyticBarrierEngine,
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
}

#[test]
fn tiny_volatility_payoffs_match_the_vanilla_limit() {
    let sigma = 1e-17;
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
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
            day(1, 1),
        )
        .unwrap();
        let expected = spot * sigma / std::f64::consts::TAU.sqrt();
        for direction in [OptionType::Call, OptionType::Put] {
            for mode in [ObservationMode::Continuous, ObservationMode::Scheduled] {
                let mut t = terms(BarrierType::DownAndOut, direction);
                t.strike = spot;
                t.barrier_level = 0.8 * spot;
                t.observation_mode = mode;
                if mode == ObservationMode::Scheduled {
                    t.observation_times = vec![t.expiry_time];
                }
                let option = BarrierOption::new(t).unwrap();
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
fn conditional_survival_prices_and_likelihood_greeks_match_analytic() {
    let context = market(0.04);
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
            path_count: 131_072,
            seed: Some(42),
            sampling,
            ..Default::default()
        })
        .unwrap();
        for kind in KINDS {
            for direction in [OptionType::Call, OptionType::Put] {
                let mut t = terms(kind, direction);
                t.rebate = 2.0;
                let option = BarrierOption::new(t).unwrap();
                let actual = engine
                    .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                    .unwrap();
                let expected = AnalyticBarrierEngine
                    .price_with_greeks(
                        &dated(&option, &context),
                        &context,
                        GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    )
                    .unwrap();
                for (got, want, budget) in [
                    (actual.price(), expected.price(), 0.12),
                    (
                        actual.require(Greek::Delta).unwrap(),
                        expected.require(Greek::Delta).unwrap(),
                        0.04,
                    ),
                    (
                        actual.require(Greek::Gamma).unwrap(),
                        expected.require(Greek::Gamma).unwrap(),
                        0.012,
                    ),
                ] {
                    assert!(
                        (got - want).abs() < budget,
                        "{sampling:?} {kind:?} {direction:?}: {got} != {want}"
                    );
                }
                assert_eq!(
                    actual.price(),
                    engine.price(&dated(&option, &context), &context).unwrap()
                );
                for greek in Greek::ALL.into_iter().skip(2) {
                    assert!(!actual.has(greek));
                }
            }
        }
    }
}

#[test]
fn scheduled_likelihood_greeks_and_early_monitoring_match_quadrature() {
    let context = market(0.04);
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 131_072,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    for early in [false, true] {
        for kind in KINDS {
            let mut t = terms(kind, OptionType::Put);
            t.rebate = 2.0;
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_times = vec![day(4, 1), day(7, 1)];
            if !early {
                t.observation_times.push(t.expiry_time);
            }
            let option = BarrierOption::new(t).unwrap();
            let actual = engine
                .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                .unwrap();
            let expected = QuadratureBarrierEngine::default()
                .price_with_greeks(
                    &dated(&option, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                )
                .unwrap();
            for (got, want, budget) in [
                (actual.price(), expected.price(), 0.08),
                (
                    actual.require(Greek::Delta).unwrap(),
                    expected.require(Greek::Delta).unwrap(),
                    0.03,
                ),
                (
                    actual.require(Greek::Gamma).unwrap(),
                    expected.require(Greek::Gamma).unwrap(),
                    0.015,
                ),
            ] {
                assert!(
                    (got - want).abs() < budget,
                    "{early} {kind:?}: {got} != {want}"
                );
            }
        }
    }
}

#[test]
fn continuous_hit_rebates_keep_negative_rate_values_and_derivatives() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 65536,
        seed: Some(17),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    for kind in [BarrierType::UpAndOut, BarrierType::DownAndOut] {
        let mut t = terms(kind, OptionType::Call);
        t.rebate = 10.0;
        t.rebate_timing = RebateTiming::AtHit;
        let option = BarrierOption::new(t).unwrap();
        for rate in [-0.02, 0.04] {
            let context = market(rate);
            let actual = engine
                .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                .unwrap();
            let expected = AnalyticBarrierEngine
                .price_with_greeks(
                    &dated(&option, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                )
                .unwrap();
            for (got, want, budget) in [
                (actual.price(), expected.price(), 0.015),
                (
                    actual.require(Greek::Delta).unwrap(),
                    expected.require(Greek::Delta).unwrap(),
                    0.006,
                ),
                (
                    actual.require(Greek::Gamma).unwrap(),
                    expected.require(Greek::Gamma).unwrap(),
                    0.003,
                ),
            ] {
                assert!(
                    (got - want).abs() < budget,
                    "{kind:?} {rate}: {got} != {want}"
                );
            }
        }
    }
}

#[test]
fn threads_do_not_change_samples_even_with_early_exit_and_pairing() {
    for scheduled in [false, true] {
        let mut t = terms(BarrierType::UpAndOut, OptionType::Put);
        t.rebate = 3.0;
        t.rebate_timing = RebateTiming::AtHit;
        if scheduled {
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_times = vec![day(4, 1), day(7, 1)];
        }
        let option = BarrierOption::new(t).unwrap();
        let context = market(0.04);
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            for antithetic in [false, true] {
                let settings = MonteCarloSettings {
                    path_count: 1024,
                    seed: Some(17),
                    max_active_workers: 1,
                    sampling,
                    antithetic,
                };
                let serial = MonteCarloBarrierEngine::new(settings).unwrap();
                let parallel = MonteCarloBarrierEngine::new(MonteCarloSettings {
                    max_active_workers: 4,
                    ..settings
                })
                .unwrap();
                let expected = serial
                    .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                    .unwrap();
                std::thread::scope(|scope| {
                    let call = || {
                        parallel
                            .price_with_greeks(
                                &dated(&option, &context),
                                &context,
                                GreeksRequest::all(),
                            )
                            .unwrap()
                    };
                    let other = scope.spawn(call);
                    assert_eq!(call(), expected);
                    assert_eq!(other.join().unwrap(), expected);
                });
            }
        }
    }
}

#[test]
fn odd_antithetic_counts_keep_in_out_parity_with_the_terminal_sampler() {
    let context = market(0.04);
    let vanilla = EuropeanOption::new(
        OptionType::Call,
        100.0,
        day(1, 1),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
    .unwrap();
    let inside = BarrierOption::new(terms(BarrierType::DownAndIn, OptionType::Call)).unwrap();
    let outside = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    for path_count in [1, 2, 3, 5, 257] {
        let settings = MonteCarloSettings {
            path_count,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        };
        let engine = MonteCarloBarrierEngine::new(settings).unwrap();
        let sum = engine.price(&dated(&inside, &context), &context).unwrap()
            + engine.price(&dated(&outside, &context), &context).unwrap();
        let expected = MonteCarloVanillaEngine::new(settings)
            .unwrap()
            .price(&dated(&vanilla, &context), &context)
            .unwrap();
        assert!(
            (sum - expected).abs() < 1e-12,
            "{path_count}: {sum} != {expected}"
        );
    }
}

#[test]
fn solver_replays_request_samples_and_rejects_fixed_cashflows() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 8192,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    let context = market(0.04);
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let quote = engine.price(&dated(&option, &context), &context).unwrap();
    let solved = implied_volatility(
        &engine,
        &dated(&option, &context),
        &context,
        quote,
        ImpliedVolatilitySettings {
            lower_bound: 0.15,
            upper_bound: 0.25,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((solved - 0.2).abs() < 1e-5);
    let unseeded = MonteCarloBarrierEngine::default();
    let prepared = unseeded.prepare_request().unwrap().unwrap();
    assert_eq!(
        prepared.price(&dated(&option, &context), &context).unwrap(),
        prepared.price(&dated(&option, &context), &context).unwrap()
    );
    let later = context
        .replace(PricingContextChanges {
            valuation_time: Some(day(1, 2)),
            ..Default::default()
        })
        .unwrap();
    let fixed = option
        .replace_history(Some(BarrierTouchState::Touched), None)
        .unwrap();
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&fixed, &later),
            &later,
            0.0,
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(
        engine.price(&dated(&option, &context), &context).unwrap(),
        quote
    );
}

#[test]
fn history_current_equalities_and_finished_monitoring_resolve_before_sampling() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for kind in KINDS {
        let knock_in = matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn);
        for timing in [RebateTiming::AtExpiry, RebateTiming::AtHit] {
            if knock_in && timing == RebateTiming::AtHit {
                continue;
            }
            let mut t = terms(kind, OptionType::Call);
            t.rebate = 7.0;
            t.rebate_timing = timing;
            let option = BarrierOption::new(t.clone()).unwrap();
            let context = market(0.04)
                .replace(PricingContextChanges {
                    spot_price: Some(t.barrier_level),
                    ..Default::default()
                })
                .unwrap();
            let result = engine
                .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                .unwrap();
            assert_eq!(
                result.price(),
                AnalyticBarrierEngine
                    .price(&dated(&option, &context), &context)
                    .unwrap()
            );
            assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
            let later = context
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(day(1, 1).date(), 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&dated(&option, &later), &later).unwrap_err(),
                Error::InvalidInput(_)
            ));
            let touched = option
                .replace_history(Some(BarrierTouchState::Touched), None)
                .unwrap();
            assert!(matches!(
                engine
                    .price(&dated(&touched, &context), &context)
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            assert_eq!(
                engine.price(&dated(&touched, &later), &later).unwrap(),
                AnalyticBarrierEngine
                    .price(&dated(&touched, &later), &later)
                    .unwrap()
            );
            let expired = context
                .replace(PricingContextChanges {
                    valuation_time: Some(t.expiry_time),
                    ..Default::default()
                })
                .unwrap();
            for history in [BarrierTouchState::Touched, BarrierTouchState::Untouched] {
                let known = option.replace_history(Some(history), None).unwrap();
                let result = engine
                    .price_with_greeks(&dated(&known, &expired), &expired, GreeksRequest::all())
                    .unwrap();
                assert_eq!(
                    result.price(),
                    AnalyticBarrierEngine
                        .price(&dated(&known, &expired), &expired)
                        .unwrap()
                );
                for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
                    assert_eq!(result.get(greek), None);
                }
                for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
                    if result.supported().has(greek) {
                        assert_eq!(result.require(greek).unwrap(), 0.0);
                    }
                }
                let reference = AnalyticBarrierEngine
                    .price_with_greeks(&dated(&known, &expired), &expired, GreeksRequest::all())
                    .unwrap();
                assert_eq!(result.get(Greek::Delta), reference.get(Greek::Delta));
                assert_eq!(result.get(Greek::Gamma), reference.get(Greek::Gamma));
            }
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_times = vec![day(4, 1)];
            let scheduled = BarrierOption::new(t.clone()).unwrap();
            let on_date = context
                .replace(PricingContextChanges {
                    valuation_time: Some(day(4, 1)),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                engine
                    .price(&dated(&scheduled, &on_date), &on_date)
                    .unwrap(),
                AnalyticBarrierEngine
                    .price(&dated(&scheduled, &on_date), &on_date)
                    .unwrap()
            );
            let after = on_date
                .replace(PricingContextChanges {
                    valuation_time: Some(Timestamp::new(day(4, 1).date(), 0, 0, 0, 1).unwrap()),
                    ..Default::default()
                })
                .unwrap();
            let untouched = scheduled
                .replace_history(Some(BarrierTouchState::Untouched), None)
                .unwrap();
            assert_eq!(
                engine.price(&dated(&untouched, &after), &after).unwrap(),
                AnalyticBarrierEngine
                    .price(&dated(&untouched, &after), &after)
                    .unwrap()
            );
            assert_eq!(option.touch_state(), None);
        }
    }
}

#[test]
fn scheduled_hit_rebates_use_fixing_discount_and_expiry_is_not_an_extra_fixing() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 131_072,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let context = market(0.04);
    let mut t = terms(BarrierType::UpAndOut, OptionType::Put);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = vec![day(7, 1)];
    t.rebate = 10.0;
    t.rebate_timing = RebateTiming::AtHit;
    let with_rebate = BarrierOption::new(t.clone()).unwrap();
    t.rebate = 0.0;
    let without_rebate = BarrierOption::new(t).unwrap();
    let time = 181.0 / 365.0;
    let threshold = ((1.2_f64).ln() - 0.01 * time) / (0.2 * time.sqrt());
    let probability = 0.5 * libm::erfc(threshold / 2.0_f64.sqrt());
    let difference = engine
        .price(&dated(&with_rebate, &context), &context)
        .unwrap()
        - engine
            .price(&dated(&without_rebate, &context), &context)
            .unwrap();
    assert!((difference - 10.0 * (-0.04 * time).exp() * probability).abs() < 0.025);
    let mut t = terms(BarrierType::UpAndOut, OptionType::Call);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = vec![day(4, 1)];
    t.strike = 130.0;
    // The payoff would require a touch if expiry were silently added to the schedule.
    let early = BarrierOption::new(t).unwrap();
    let value = engine.price(&dated(&early, &context), &context).unwrap();
    assert!(value > 0.01);
    assert!(
        (value
            - QuadratureBarrierEngine::default()
                .price(&dated(&early, &context), &context)
                .unwrap())
        .abs()
            < 0.04
    );
}

#[test]
fn settings_applicability_and_failure_categories_preserve_inputs() {
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 10_000_001,
            ..Default::default()
        },
        MonteCarloSettings {
            max_active_workers: -1,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 3,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloBarrierEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert_eq!(
        MonteCarloBarrierEngine::default().sampling_settings(),
        MonteCarloSettings::default()
    );
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let context = market(0.04);
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let quote = engine.price(&dated(&option, &context), &context).unwrap();
    let curve = RateTermStructure::new(
        day(1, 1),
        vec![
            option.expiry_time(),
            (option.expiry_time()).add_days(1).unwrap(),
        ],
        vec![(-0.04_f64).exp(), ((-0.04_f64).exp()) * 0.99],
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
    let mut t = terms(BarrierType::DownAndOut, OptionType::Call);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = vec![day(1, 4)];
    assert!(matches!(BarrierOption::new(t), Err(Error::InvalidInput(_))));

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
    for (sigma, rate) in [(50.0, 0.04), (0.2, -1000.0)] {
        let extreme = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, 0.01, sigma)
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
fn rare_hit_samples_retain_scaled_value_and_unrequested_greeks_do_not_fail_price() {
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let mut t = terms(BarrierType::DownAndIn, OptionType::Call);
    t.strike = 1.0;
    t.barrier_level = 1e300 * (-20.0_f64).exp();
    let option = BarrierOption::new(t).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.5, 0.0, 1.0).unwrap(),
        1e300,
        day(1, 1),
    )
    .unwrap();
    assert!(engine.price(&dated(&option, &context), &context).unwrap() > 0.0);
    let option = BarrierOption::new(terms(BarrierType::DownAndOut, OptionType::Call)).unwrap();
    let tiny = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 1e-200).unwrap(),
        100.0,
        day(1, 1),
    )
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
}

#[test]
fn sobol_dimensions_count_fixings_and_unobserved_expiry_leg() {
    let mut t = terms(BarrierType::UpAndOut, OptionType::Put);
    t.expiry_time = Date::new(2100, 1, 1).unwrap().start_of_day();
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_times = (2025..2100)
        .flat_map(|year| {
            (1..=12).flat_map(move |month| {
                (1..=28).map(move |d| Date::new(year, month, d).unwrap().start_of_day())
            })
        })
        .filter(|d| *d > day(1, 1))
        .take(21201)
        .collect();
    t.calendar = TradingCalendar::all_days();
    let option = BarrierOption::new(t).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        day(1, 1),
    )
    .unwrap();
    let engine = MonteCarloBarrierEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine
            .price(&dated(&option, &context), &context)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}
