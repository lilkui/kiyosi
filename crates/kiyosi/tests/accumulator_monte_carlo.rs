mod support;
use support::dated;

use kiyosi::*;

fn day(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}

fn option(end: Timestamp) -> Accumulator {
    Accumulator::new(AccumulatorTerms {
        history_time: Some(day(1, 2)),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, day(1, 1), end, (end).time_of_day())
    })
    .unwrap()
}

fn market(spot: f64, at: impl Into<Timestamp>) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        at.into(),
    )
    .unwrap()
}

#[test]
fn current_events_settle_before_sampling_with_strict_acceleration() {
    let end = day(1, 6);
    let option = option(end);
    let engine = MonteCarloAccumulatorEngine::default();
    for (spot, expected) in [(90.0, -50.0), (100.0, 0.0), (105.0, 20.0), (110.0, 30.0)] {
        let estimate = engine
            .price_with_statistics(&dated(&option, &market(spot, end)), &market(spot, end))
            .unwrap();
        assert_eq!(estimate.price(), expected);
        assert_eq!(estimate.standard_error(), Some(0.0));
        assert_eq!(estimate.confidence_interval(), Some((expected, expected)));
    }
    assert_eq!(
        engine
            .price(
                &dated(&option, &market(110.0, day(1, 2))),
                &market(110.0, day(1, 2))
            )
            .unwrap(),
        30.0
    );
}

#[test]
fn scalar_price_survives_confidence_interval_overflow() {
    let start = Date::new(2024, 12, 31).unwrap().start_of_day();
    let end = day(1, 2);
    let option = Accumulator::new(AccumulatorTerms {
        history_time: Some(start.add_days(1).unwrap()),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(1.0),
        ..AccumulatorTerms::new(100.0, 200.0, 0.0, 1.0, start, end, (end).time_of_day())
    })
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.01, 0.0, 1.0).unwrap(),
        100.0,
        day(1, 1),
    )
    .unwrap();
    let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(0),
        max_active_workers: 1,
        ..Default::default()
    })
    .unwrap();
    let quantity = 6e307;
    let expected = engine
        .price_with_statistics(&dated(&option, &context), &context)
        .unwrap()
        .price()
        * quantity;
    let scaled = option
        .replace_history(
            AccumulatorHistoryChanges {
                accumulated_quantity: Some(Some(quantity)),
                ..Default::default()
            },
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    assert!(expected.is_finite() && expected > 0.0);
    assert!(
        (engine.price(&dated(&scaled, &context), &context).unwrap() / expected - 1.0).abs() < 1e-12
    );
    assert!(matches!(
        engine.price_with_statistics(&dated(&scaled, &context), &context),
        Err(Error::ComputationFailure(_))
    ));
}

#[test]
fn simulated_cashflows_match_quadrature_with_sampling_error() {
    let settings = MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        max_active_workers: 3,
        ..Default::default()
    };
    let engine = MonteCarloAccumulatorEngine::new(settings).unwrap();
    for (end, spot, at) in [
        (
            day(1, 6),
            100.25,
            Timestamp::new(day(1, 3).date(), 12, 0, 0, 0).unwrap(),
        ),
        (day(2, 3), 99.0, day(1, 2)),
        (
            day(2, 3),
            111.0,
            Timestamp::new(day(1, 1).date(), 12, 0, 0, 0).unwrap(),
        ),
    ] {
        let option = option(end);
        let context = market(spot, at);
        let expected = QuadratureAccumulatorEngine::default()
            .price(&dated(&option, &context), &context)
            .unwrap();
        let estimate = engine
            .price_with_statistics(&dated(&option, &context), &context)
            .unwrap();
        assert_eq!(
            engine.price(&dated(&option, &context), &context).unwrap(),
            estimate.price()
        );
        let error = estimate.standard_error().unwrap();
        assert!(error > 0.0);
        assert!(
            (estimate.price() - expected).abs() < 4.0 * error + 0.02,
            "{} != {expected}, SE {error}",
            estimate.price()
        );
        let (lower, upper) = estimate.confidence_interval().unwrap();
        assert!((upper - lower - 2.0 * 1.959_963_984_540_054 * error).abs() < 1e-12);
    }
}

#[test]
fn counts_pair_uncertainty_sobol_and_threads_preserve_the_same_samples() {
    let option = option(day(2, 3));
    let context = market(100.25, day(1, 2));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            for path_count in [1, 2, 4, 1024] {
                let settings = MonteCarloSettings {
                    path_count,
                    sampling,
                    antithetic,
                    seed: Some(u64::MAX),
                    max_active_workers: 1,
                };
                let serial = MonteCarloAccumulatorEngine::new(settings)
                    .unwrap()
                    .price_with_statistics(&dated(&option, &context), &context)
                    .unwrap();
                for threads in [0, 3] {
                    let parallel = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
                        max_active_workers: threads,
                        ..settings
                    })
                    .unwrap()
                    .price_with_statistics(&dated(&option, &context), &context)
                    .unwrap();
                    assert_eq!(serial, parallel);
                }
                assert_eq!(
                    serial.standard_error().is_some(),
                    sampling == MonteCarloSampling::PseudoRandom
                        && path_count > if antithetic { 2 } else { 1 }
                );
            }
        }
    }
    let odd = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 31,
        antithetic: true,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap()
    .price_with_statistics(&dated(&option, &context), &context)
    .unwrap();
    assert!(odd.standard_error().is_none());
    assert!(odd.confidence_interval().is_none());
    for antithetic in [false, true] {
        let estimate = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
            path_count: 16384,
            antithetic,
            sampling: MonteCarloSampling::Sobol,
            seed: Some(42),
            ..Default::default()
        })
        .unwrap()
        .price_with_statistics(&dated(&option, &context), &context)
        .unwrap();
        let reference = QuadratureAccumulatorEngine::default()
            .price(&dated(&option, &context), &context)
            .unwrap();
        assert!((estimate.price() - reference).abs() < 1.0);
        assert!(estimate.standard_error().is_none());
    }
}

#[test]
fn shared_sample_greeks_and_volatility_solves_preserve_inputs() {
    let option = option(day(1, 6));
    let original = option.clone();
    let context = market(
        100.25,
        Timestamp::new(day(1, 3).date(), 12, 0, 0, 0).unwrap(),
    );
    let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 4096,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let shifts = NumericalShiftSettings {
        spot_shift: Some(0.25),
        ..Default::default()
    };
    let result = kiyosi::numerical_greeks(
        &engine,
        &dated(&option, &context),
        &context,
        GreeksRequest::all(),
        shifts,
    )
    .unwrap();
    let scalar = numerical_greeks(
        &engine,
        &dated(&option, &context),
        &context,
        GreeksRequest::all(),
        shifts,
    )
    .unwrap();
    assert_eq!(result, scalar);
    let bump = |spot| {
        engine
            .price(
                &dated(
                    &option,
                    &context
                        .replace(PricingContextChanges {
                            spot_price: Some(spot),
                            ..Default::default()
                        })
                        .unwrap(),
                ),
                &context
                    .replace(PricingContextChanges {
                        spot_price: Some(spot),
                        ..Default::default()
                    })
                    .unwrap(),
            )
            .unwrap()
    };
    let (up, down) = (bump(100.5), bump(100.0));
    assert_eq!(result.require(Greek::Delta).unwrap(), (up - down) / 0.5);
    let gamma = (up - 2.0 * result.price() + down) / 0.25 / 0.25;
    // Equivalent second-difference groupings can round differently.
    let rounding = 4.0 * f64::EPSILON * (up.abs() + 2.0 * result.price().abs() + down.abs())
        / shifts.fallback_spot_shift().powi(2);
    assert!((result.require(Greek::Gamma).unwrap() - gamma).abs() <= rounding);
    for greek in [
        Greek::Delta,
        Greek::Gamma,
        Greek::Speed,
        Greek::Vega,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Rho,
    ] {
        assert!(result.has(greek));
    }
    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
        assert!(result.require(greek).unwrap().is_finite());
    }
    let sigma = implied_volatility(
        &engine,
        &dated(&option, &context),
        &context,
        result.price(),
        ImpliedVolatilitySettings::new(0.1, 0.3),
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-6);
    let unseeded = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 1024,
        ..Default::default()
    })
    .unwrap();
    let original_settings = unseeded.sampling_settings();
    let prepared = unseeded.prepare_request().unwrap().unwrap();
    assert!(prepared.prepare_request().unwrap().is_none());
    assert_eq!(
        prepared.price(&dated(&option, &context), &context).unwrap(),
        prepared.price(&dated(&option, &context), &context).unwrap()
    );
    assert_eq!(unseeded.sampling_settings(), original_settings);
    assert_eq!(option, original);
    assert_eq!(engine.sampling_settings().seed, Some(42));
}

#[test]
fn trading_midnights_intraday_history_and_calendar_control_purchases() {
    let option = option(day(1, 6));
    let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let model = BlackScholesMertonParameters::new(0.0, 0.0, 1e-20).unwrap();
    for (at, calendar, expected) in [
        (day(1, 3), TradingCalendar::weekdays(), -70.0),
        (
            Timestamp::new(day(1, 3).date(), 0, 0, 0, 1).unwrap(),
            TradingCalendar::weekdays(),
            -50.0,
        ),
        (day(1, 4), TradingCalendar::weekdays(), -50.0),
        (day(1, 3), TradingCalendar::all_days(), -110.0),
        (
            day(1, 3),
            TradingCalendar::new(|d| d == day(1, 1).date() || d == day(1, 6).date(), 252).unwrap(),
            -50.0,
        ),
    ] {
        let option = option
            .replace(AccumulatorChanges {
                calendar: Some(calendar),
                accumulated_quantity: Some(option.accumulated_quantity()),
                knock_out_settled: Some(option.knock_out_settled()),
                ..Default::default()
            })
            .unwrap();
        let context = PricingContext::new(model.clone(), 90.0, at).unwrap();
        assert_eq!(
            engine.price(&dated(&option, &context), &context).unwrap(),
            expected
        );
    }
    let empty = option
        .replace_history(
            AccumulatorHistoryChanges {
                accumulated_quantity: Some(Some(0.0)),
                ..Default::default()
            },
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    assert_eq!(
        engine
            .price(
                &dated(
                    &empty,
                    &PricingContext::new(model.clone(), 90.0, day(1, 6)).unwrap()
                ),
                &PricingContext::new(model, 90.0, day(1, 6)).unwrap()
            )
            .unwrap(),
        -20.0
    );
}

#[test]
fn antithetic_pair_uses_both_opposite_normals_and_exact_path_counts() {
    let start = day(1, 3);
    let end = day(1, 6);
    let option = Accumulator::new(AccumulatorTerms {
        history_time: Some(start.add_days(1).unwrap()),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(100.0, 1e300, 0.0, 2.0, start, end, (end).time_of_day())
    })
    .unwrap();
    let context = market(100.25, Timestamp::new(start.date(), 12, 0, 0, 0).unwrap());
    let settings = MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    };
    let one = MonteCarloAccumulatorEngine::new(settings)
        .unwrap()
        .price(&dated(&option, &context), &context)
        .unwrap();
    let time = 2.5_f64 / 365.0;
    let discount = (-0.04 * time).exp();
    let first_spot = one / (3.0 * discount) + 100.0;
    let opposite_spot = context.spot_price().powi(2)
        * (2.0 * (0.04 - 0.01 - 0.2 * 0.2 / 2.0) * time).exp()
        / first_spot;
    let expected = f64::midpoint(one, 3.0 * (opposite_spot - 100.0) * discount);
    let paired = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 2,
        antithetic: true,
        ..settings
    })
    .unwrap()
    .price_with_statistics(&dated(&option, &context), &context)
    .unwrap();
    assert!((paired.price() - expected).abs() < 1e-12);
    assert!(paired.standard_error().is_none());
    let unpaired = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        antithetic: true,
        ..settings
    })
    .unwrap()
    .price(&dated(&option, &context), &context)
    .unwrap();
    assert_eq!(unpaired, one);
}

#[test]
fn validation_precedes_settlement_and_failures_preserve_reuse() {
    let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let option = option(day(1, 6));
    for (contract, context, category) in [
        (
            option.clone(),
            market(110.0, Date::new(2024, 12, 31).unwrap().start_of_day()),
            (|error| matches!(error, Error::InvalidInput(_))) as fn(Error) -> bool,
        ),
        (
            option.clone(),
            market(110.0, Timestamp::new(day(1, 6).date(), 0, 0, 0, 1).unwrap()),
            (|error| matches!(error, Error::InvalidInput(_))) as fn(Error) -> bool,
        ),
        (
            option
                .replace_history(
                    AccumulatorHistoryChanges {
                        accumulated_quantity: Some(Some(f64::MAX)),
                        ..Default::default()
                    },
                    Some(option.history_time().unwrap_or(option.effective_time())),
                )
                .unwrap(),
            market(111.0, day(1, 2)),
            (|error| matches!(error, Error::ComputationFailure(_))) as fn(Error) -> bool,
        ),
    ] {
        assert!(category(
            engine
                .price(&dated(&contract, &context), &context)
                .unwrap_err()
        ));
    }
    let context = market(110.0, day(1, 2));
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
            path_count: 31,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloAccumulatorEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }

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
    let curve = RateTermStructure::new(
        day(1, 1),
        vec![day(1, 6), (day(1, 6)).add_days(1).unwrap()],
        vec![0.99, (0.99) * 0.99],
    )
    .unwrap();
    let curved = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                context
                    .model_parameters()
                    .as_bsm()
                    .unwrap()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(curve.into()),
                        ..Default::default()
                    })
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
    assert_eq!(
        engine.price(&dated(&option, &context), &context).unwrap(),
        30.0
    );
    assert_eq!(option.accumulated_quantity().unwrap(), 3.0);
}

#[test]
fn event_greeks_and_fixed_payoff_volatility_identifiability_follow_core_rules() {
    let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let option = option(day(1, 6));
    for spot in [100.0, 110.0] {
        let context = market(spot, day(1, 2));
        let result = engine
            .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
            .unwrap();
        for greek in [
            Greek::Delta,
            Greek::Gamma,
            Greek::Speed,
            Greek::Vanna,
            Greek::Zomma,
            Greek::Theta,
            Greek::Charm,
            Greek::Color,
        ] {
            assert!(!result.has(greek));
        }
        assert!(result.has(Greek::Vega));
        assert!(result.has(Greek::Rho));
    }
    let expired = market(90.0, day(1, 6));
    let result = engine
        .price_with_greeks(&dated(&option, &expired), &expired, GreeksRequest::all())
        .unwrap();
    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
        assert_eq!(result.get(greek), None);
    }
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        if result.supported().has(greek) {
            assert_eq!(result.require(greek).unwrap(), 0.0);
        }
    }
    let fixed = Accumulator::new(AccumulatorTerms {
        history_time: Some(day(1, 2)),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(
            100.0,
            110.0,
            0.0,
            2.0,
            day(1, 1),
            day(1, 6),
            (day(1, 6)).time_of_day(),
        )
    })
    .unwrap();
    let zero_rates = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
        90.0,
        day(1, 2),
    )
    .unwrap();
    for (contract, context, expected) in [
        (option.clone(), market(110.0, day(1, 2)), 30.0),
        (option.clone(), expired, -50.0),
        (fixed.clone(), zero_rates, -30.0),
        (
            fixed
                .replace_history(
                    AccumulatorHistoryChanges {
                        accumulated_quantity: Some(Some(0.0)),
                        ..Default::default()
                    },
                    Some(fixed.history_time().unwrap_or(fixed.effective_time())),
                )
                .unwrap(),
            market(100.25, day(1, 2)),
            0.0,
        ),
    ] {
        assert_eq!(
            engine
                .price_with_statistics(&dated(&contract, &context), &context)
                .unwrap()
                .standard_error(),
            Some(0.0)
        );
        assert_eq!(
            engine.price(&dated(&contract, &context), &context).unwrap(),
            expected
        );
        assert!(matches!(
            implied_volatility(
                &engine,
                &dated(&contract, &context),
                &context,
                expected,
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let context = market(
        100.25,
        Timestamp::new(day(1, 3).date(), 12, 0, 0, 0).unwrap(),
    );
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&option, &context),
            &context,
            f64::NAN,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&option, &context),
            &context,
            1e100,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::RootNotBracketed(_)
    ));
}

#[test]
fn sobol_dimension_limit_uses_future_trading_dates_after_known_settlements() {
    let end = Date::new(2090, 1, 2).unwrap().start_of_day();
    let option = option(end);
    let option = option
        .replace(AccumulatorChanges {
            calendar: Some(TradingCalendar::all_days()),
            accumulated_quantity: Some(option.accumulated_quantity()),
            knock_out_settled: Some(option.knock_out_settled()),
            ..Default::default()
        })
        .unwrap();
    let context = market(100.25, day(1, 2));
    let engine = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 2,
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
    let knocked_out = context
        .replace(PricingContextChanges {
            spot_price: Some(110.0),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_statistics(&dated(&option, &knocked_out), &knocked_out)
        .unwrap();
    assert_eq!(result.price(), 30.0);
    assert!(result.standard_error().is_none());
    let zero = Accumulator::new(AccumulatorTerms::new(
        100.0,
        110.0,
        0.0,
        2.0,
        day(1, 1),
        end,
        (end).time_of_day(),
    ))
    .unwrap()
    .replace_history(
        AccumulatorHistoryChanges {
            accumulated_quantity: Some(Some(0.0)),
            knock_out_settled: Some(Some(false)),
        },
        Some(day(1, 1)),
    )
    .unwrap();
    assert_eq!(
        engine.price(&dated(&zero, &context), &context).unwrap(),
        0.0
    );
}
