mod support;
use support::dated;

use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}

fn market(spot: f64, valuation: impl Into<Timestamp>) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation.into(),
    )
    .unwrap()
}

#[test]
fn binary_price_and_uncertainty_match_an_independent_terminal_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
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
    let expected = (-0.04_f64).exp()
        + AnalyticDigitalEngine
            .price(&dated(&digital, &context), &context)
            .unwrap();
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let estimate = engine
        .price_with_statistics(&dated(&note, &context), &context)
        .unwrap();
    assert_eq!(
        engine.price(&dated(&note, &context), &context).unwrap(),
        estimate.price()
    );
    let error = estimate.standard_error().unwrap();
    assert!((estimate.price() - expected).abs() < 4.0 * error);
    let (lower, upper) = estimate.confidence_interval().unwrap();
    assert!((upper - lower - 2.0 * 1.959_963_984_540_054 * error).abs() < 1e-14);
}

#[test]
fn scalar_price_survives_confidence_interval_overflow() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let mut terms = BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[100.0], Some(&[1.5e308]), None).unwrap(),
    );
    terms.principal_ratio = 0.0;
    let note = BinarySnowballOption::new(terms).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let settings = MonteCarloSettings {
        path_count: 2,
        seed: Some(0),
        thread_count: 1,
        ..Default::default()
    };
    let engine = MonteCarloAutocallableEngine::new(settings).unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 1.5e308, start, end).unwrap();
    let expected = MonteCarloDigitalEngine::new(settings)
        .unwrap()
        .price(&dated(&digital, &context), &context)
        .unwrap();
    assert!((expected / 7.5e307 - 1.0).abs() < 1e-12);
    assert!(
        (engine.price(&dated(&note, &context), &context).unwrap() / expected - 1.0).abs() < 1e-12
    );
    assert!(matches!(
        engine.price_with_statistics(&dated(&note, &context), &context),
        Err(Error::ComputationFailure(_))
    ));
}

#[test]
fn discounted_cashflows_preserve_representable_values_beyond_discount_range() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(1),
        ..Default::default()
    })
    .unwrap();
    for (principal, rate) in [(1e308_f64, 1000.0), (1e-308, -710.0)] {
        let mut terms = BinarySnowballTerms::new(
            0.0,
            start,
            end,
            autocallable_observations(&[end], &[100.0], Some(&[0.0]), None).unwrap(),
        );
        terms.principal_ratio = principal;
        let note = BinarySnowballOption::new(terms).unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, rate, 0.2).unwrap(),
            100.0,
            start,
        )
        .unwrap();
        let expected = (principal.ln() - rate).exp();
        let actual = engine.price(&dated(&note, &context), &context).unwrap();
        assert!(
            (actual / expected - 1.0).abs() < 1e-12,
            "{actual} != {expected}"
        );
    }
    let mut terms = PhoenixTerms::new(
        kiyosi::PhoenixCoupon::Annual(-1e308),
        100.0,
        80.0,
        100.0,
        100.0,
        start,
        end,
        autocallable_observations(&[end], &[1.0], None, Some(&[0.0])).unwrap(),
        Some((end).time_of_day()),
    );
    terms.principal_ratio = 1e308;
    let note = PhoenixOption::new(terms).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(-1.0, -1.0, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    assert_eq!(
        engine.price(&dated(&note, &context), &context).unwrap(),
        0.0
    );
}

#[test]
fn sampling_settings_counts_and_threads_preserve_prices_and_uncertainty() {
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.02,
        date(1, 1),
        date(4, 1),
        autocallable_observations(
            &[date(2, 3), date(4, 1)],
            &[101.0, 105.0],
            Some(&[0.08; 2]),
            None,
        )
        .unwrap(),
    ))
    .unwrap();
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            for path_count in [1, 2, 32, 1024] {
                let settings = MonteCarloSettings {
                    path_count,
                    seed: Some(u64::MAX),
                    sampling,
                    antithetic,
                    thread_count: 1,
                };
                let serial = MonteCarloAutocallableEngine::new(settings)
                    .unwrap()
                    .price_with_statistics(&dated(&note, &context), &context)
                    .unwrap();
                let parallel = MonteCarloAutocallableEngine::new(MonteCarloSettings {
                    thread_count: 3,
                    ..settings
                })
                .unwrap()
                .price_with_statistics(&dated(&note, &context), &context)
                .unwrap();
                assert_eq!(serial, parallel);
                assert_eq!(
                    serial.standard_error().is_some(),
                    sampling == MonteCarloSampling::PseudoRandom
                        && path_count > if antithetic { 2 } else { 1 }
                );
            }
        }
    }
    let odd = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 31,
        seed: Some(42),
        antithetic: true,
        ..Default::default()
    })
    .unwrap();
    assert!(
        odd.price_with_statistics(&dated(&note, &context), &context)
            .unwrap()
            .standard_error()
            .is_none()
    );
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
            path_count: 31,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloAutocallableEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn shared_greeks_and_solvers_leave_engine_and_inputs_unchanged() {
    let end = date(4, 1);
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        95.0,
        100.0,
        60.0,
        date(1, 1),
        end,
        autocallable_observations(&[date(2, 3), end], &[103.0, 105.0], Some(&[0.08; 2]), None)
            .unwrap(),
        Some((end).time_of_day()),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms).unwrap();
    let original = note.clone();
    let context = market(100.0, date(1, 1));
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
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
        &dated(&note, &context),
        &context,
        GreeksRequest::all(),
        shifts,
    )
    .unwrap();
    let bump = |spot| {
        engine
            .price(
                &dated(
                    &note,
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
    let (up, down) = (bump(100.25), bump(99.75));
    assert_eq!(result.require(Greek::Delta).unwrap(), (up - down) / 0.5);
    assert_eq!(
        result.require(Greek::Gamma).unwrap(),
        (up - 2.0 * result.price() + down) / 0.25 / 0.25
    );
    let volatility = implied_volatility(
        &engine,
        &dated(&note, &context),
        &context,
        result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.2,
            upper_bound: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((volatility - 0.3).abs() < 1e-6);
    for convention in [
        CouponQuoteConvention::ShiftMaturityCoupon,
        CouponQuoteConvention::PreserveMaturityCoupon,
    ] {
        let quote = engine
            .price(
                &dated(
                    &note.with_quoted_coupon(0.12, convention).unwrap(),
                    &context,
                ),
                &context,
            )
            .unwrap();
        let coupon = implied_snowball_coupon(
            &engine,
            &dated(&note, &context),
            &context,
            quote,
            convention,
            ImpliedCouponSettings::default(),
        )
        .unwrap();
        assert!((coupon - 0.12).abs() < 1e-8);
    }
    let unseeded = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 1024,
        ..Default::default()
    })
    .unwrap();
    let original_settings = unseeded.sampling_settings();
    let prepared =
        <MonteCarloAutocallableEngine as PricingEngine<SnowballOption>>::prepare_request(&unseeded)
            .unwrap()
            .unwrap();
    // A continuous terminal put spread avoids unidentified roots on sampled barrier jumps.
    let mut smooth_terms = SnowballTerms::new(
        0.0,
        100.0,
        100.0,
        100.0,
        60.0,
        date(1, 1),
        end,
        autocallable_observations(&[end], &[100.0], Some(&[0.0]), None).unwrap(),
        Some((end).time_of_day()),
    );
    smooth_terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    smooth_terms.observation_clock_time = None;
    let smooth = SnowballOption::new(smooth_terms).unwrap();
    let price = prepared.price(&dated(&smooth, &context), &context).unwrap();
    assert_eq!(
        price,
        prepared.price(&dated(&smooth, &context), &context).unwrap()
    );
    assert!(prepared.prepare_request().unwrap().is_none());
    let sigma = implied_volatility(
        prepared.as_ref(),
        &dated(&smooth, &context),
        &context,
        price,
        ImpliedVolatilitySettings {
            lower_bound: 0.2,
            upper_bound: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.3).abs() < 1e-6);
    assert_eq!(unseeded.sampling_settings(), original_settings);
    assert_eq!(note, original);
    assert_eq!(engine.sampling_settings().seed, Some(42));
}

#[test]
fn known_cashflows_and_history_validate_before_shortcuts() {
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        autocallable_observations(
            &[date(2, 3), date(4, 1)],
            &[110.0; 2],
            Some(&[0.08; 2]),
            None,
        )
        .unwrap(),
        Some((date(4, 1)).time_of_day()),
    );
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let missing = SnowballOption::new(terms.clone()).unwrap();
    assert!(matches!(
        engine
            .price(
                &dated(&missing, &market(100.0, date(1, 2))),
                &market(100.0, date(1, 2))
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    terms.barrier_state = Some(AutocallableBarrierState::KnockedOut);
    let out = SnowballOption::new(terms.clone()).unwrap();
    assert!(matches!(
        engine
            .price(
                &dated(&out, &market(100.0, date(1, 1))),
                &market(100.0, date(1, 1))
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let estimate = engine
        .price_with_statistics(
            &dated(&out, &market(100.0, date(3, 3))),
            &market(100.0, date(3, 3)),
        )
        .unwrap();
    assert_eq!(estimate.price(), 0.0);
    assert_eq!(estimate.standard_error(), Some(0.0));
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&out, &market(100.0, date(3, 3))),
            &market(100.0, date(3, 3)),
            0.0,
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms).unwrap();
    for (spot, expected) in [
        (70.0, 0.7),
        (80.0, 1.0 + 0.06 * 90.0 / 365.0),
        (110.0, 1.0 + 0.08 * 90.0 / 365.0),
    ] {
        let context = market(spot, date(4, 1));
        assert!(
            (engine.price(&dated(&note, &context), &context).unwrap() - expected).abs() < 1e-14
        );
        let result = engine
            .price_with_greeks(&dated(&note, &context), &context, GreeksRequest::all())
            .unwrap();
        for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
            assert_eq!(result.get(greek), None);
        }
        for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
            if result.supported().has(greek) {
                assert_eq!(result.require(greek).unwrap(), 0.0);
            }
        }
    }
    let context = market(100.0, date(1, 1));
    let node = RateTermStructure::new(
        date(1, 1),
        vec![date(4, 1), (date(4, 1)).add_days(1).unwrap()],
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
                        risk_free_rate: Some(Rate::Curve(node)),
                        ..Default::default()
                    })
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&dated(&note, &curved), &curved).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn sobol_dimension_limit_applies_only_to_remaining_simulation_dates() {
    let start = date(1, 1);
    let end = Date::new(2110, 1, 1).unwrap().start_of_day();
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(&[end], &[110.0], Some(&[0.08]), None).unwrap(),
        Some((end).time_of_day()),
    );
    let note = SnowballOption::new(terms.clone()).unwrap();
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine
            .price(&dated(&note, &market(100.0, start)), &market(100.0, start))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.observation_clock_time = None;
    assert!(
        engine
            .price(
                &dated(&SnowballOption::new(terms).unwrap(), &market(100.0, start)),
                &market(100.0, start)
            )
            .unwrap()
            .is_finite()
    );
}

#[test]
fn daily_monitoring_and_all_variants_agree_with_quadrature() {
    fn check<T: AutocallableTerms>(note: &AutocallableNote<T>, context: &PricingContext) {
        let reference = QuadratureAutocallableEngine::new(QuadratureSettings {
            asset_step_count: 1024,
            ..Default::default()
        })
        .unwrap()
        .price(&dated(note, context), context)
        .unwrap();
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 65536,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        })
        .unwrap();
        let actual = engine
            .price_with_statistics(&dated(note, context), context)
            .unwrap();
        assert!(
            (actual.price() - reference).abs() < 4.0 * actual.standard_error().unwrap() + 1e-5,
            "{} != {reference}",
            actual.price()
        );
    }

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
            Some((end).time_of_day()),
        );
        snow.knock_in_observation_mode = mode;
        snow.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(snow.expiry_time.time_of_day());
        snow.barrier_state = Some(AutocallableBarrierState::NoEvent);
        let mut ternary = TernarySnowballTerms::new(
            0.06,
            0.01,
            95.0,
            start,
            end,
            observations.clone(),
            Some((end).time_of_day()),
        );
        ternary.knock_in_observation_mode = mode;
        ternary.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(ternary.expiry_time.time_of_day());
        ternary.barrier_state = Some(AutocallableBarrierState::NoEvent);
        let mut phoenix = PhoenixTerms::new(
            kiyosi::PhoenixCoupon::Annual(0.08),
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
            Some((end).time_of_day()),
        );
        phoenix.knock_in_observation_mode = mode;
        phoenix.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(phoenix.expiry_time.time_of_day());
        phoenix.barrier_state = Some(AutocallableBarrierState::NoEvent);

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
