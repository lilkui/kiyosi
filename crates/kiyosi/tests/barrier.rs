mod support;
use support::dated;

use kiyosi::*;

fn terms(kind: BarrierType) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
        120.0,
        kind,
    )
}
fn market(spot: f64, time: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        spot,
        time,
    )
    .unwrap()
}

#[test]
fn history_is_strictly_prior_and_boundary_hit_rebates_are_not_paid_twice() {
    let mut t = terms(BarrierType::UpAndOut);
    t.rebate = 7.0;
    t.rebate_timing = RebateTiming::AtHit;
    let option = BarrierOption::new(t.clone()).unwrap();
    let start = t.effective_time;
    assert_eq!(
        AnalyticBarrierEngine
            .price(
                &dated(&option, &market(120.0, start)),
                &market(120.0, start)
            )
            .unwrap(),
        7.0
    );
    let later = Timestamp::new(Date::new(2025, 1, 2).unwrap(), 0, 0, 0, 0).unwrap();
    assert!(matches!(
        AnalyticBarrierEngine
            .price(
                &dated(&option, &market(100.0, later)),
                &market(100.0, later)
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let touched = option
        .replace_history(
            Some(BarrierTouchState::Touched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    assert!(matches!(
        AnalyticBarrierEngine
            .price(
                &dated(&touched, &market(100.0, start)),
                &market(100.0, start)
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(
        AnalyticBarrierEngine
            .price(
                &dated(&touched, &market(100.0, later)),
                &market(100.0, later)
            )
            .unwrap(),
        0.0
    );
    assert_eq!(option.touch_state(), None);
}

#[test]
fn live_barriers_preserve_in_out_parity_and_analytics() {
    let engine = AnalyticBarrierEngine;
    for direction in [OptionType::Call, OptionType::Put] {
        for (inside, outside, level) in [
            (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.0),
            (BarrierType::DownAndIn, BarrierType::DownAndOut, 80.0),
        ] {
            for strike in [70.0, 100.0, 130.0] {
                let mut t = terms(inside);
                t.option_type = direction;
                t.strike = strike;
                t.barrier_level = level;
                let context = market(100.0, t.effective_time);
                let vanilla =
                    EuropeanOption::new(direction, strike, t.effective_time, t.expiry_time)
                        .unwrap();
                let a = engine
                    .price(
                        &dated(&BarrierOption::new(t.clone()).unwrap(), &context),
                        &context,
                    )
                    .unwrap();
                t.barrier_type = outside;
                let b = engine
                    .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
                    .unwrap();
                assert!(
                    (a + b
                        - AnalyticEuropeanEngine
                            .price(&dated(&vanilla, &context), &context)
                            .unwrap())
                    .abs()
                        < 1e-10
                );
            }
        }
    }
    let option = BarrierOption::new(terms(BarrierType::UpAndOut))
        .unwrap()
        .replace_history(
            Some(BarrierTouchState::Untouched),
            Some((terms(BarrierType::UpAndOut)).effective_time),
        )
        .unwrap();
    let context = market(100.0, option.effective_time());
    let result = engine
        .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| result.has(g)));
    let solved = implied_volatility(
        &engine,
        &dated(&option, &context),
        &context,
        result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.15,
            upper_bound: 0.25,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((solved - 0.2).abs() < 1e-7);
}

#[test]
fn greeks_do_not_cross_touch_discontinuities_or_history_regimes() {
    let mut t = terms(BarrierType::UpAndOut);
    t.rebate = 7.0;
    t.touch_state = Some(BarrierTouchState::Untouched);
    t.history_time = Some(t.effective_time);
    let option = BarrierOption::new(t.clone()).unwrap();
    let context = market(120.0, t.effective_time);
    let result = AnalyticBarrierEngine
        .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Vega), Some(0.0));
    assert!((result.require(Greek::Rho).unwrap() + 7.0 * (-0.05_f64).exp()).abs() < 2e-8);
    let near = market(119.995, t.effective_time);
    let result = numerical_greeks(
        &AnalyticBarrierEngine,
        &dated(&option, &near),
        &near,
        GreeksRequest::all(),
        Default::default(),
    )
    .unwrap();
    for g in [
        Greek::Delta,
        Greek::Gamma,
        Greek::Speed,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Charm,
        Greek::Color,
    ] {
        assert!(!result.has(g), "{g:?}");
    }
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    t.observation_times = Some(vec![
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        t.expiry_time,
    ]);
    let scheduled = BarrierOption::new(t).unwrap();
    let result = AnalyticBarrierEngine
        .price_with_greeks(
            &dated(&scheduled, &market(100.0, scheduled.effective_time())),
            &market(100.0, scheduled.effective_time()),
            GreeksRequest::all(),
        )
        .unwrap();
    assert!(result.require(Greek::Theta).unwrap().is_finite());
    assert!(result.has(Greek::Vega));
}

#[test]
fn construction_defaults_validation_and_calendar_are_core_semantics() {
    let t = terms(BarrierType::UpAndOut);
    let option = BarrierOption::new(t.clone()).unwrap();
    assert_eq!(option.rebate(), 0.0);
    assert_eq!(option.rebate_timing(), RebateTiming::AtExpiry);
    assert_eq!(option.observation_mode(), ObservationMode::Continuous);
    assert!(option.observation_schedule().is_empty());
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut invalid = t.clone();
        invalid.barrier_level = value;
        invalid.strike = -1.0;
        assert!(matches!(
            BarrierOption::new(invalid).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut invalid = t.clone();
    invalid.strike = -1.0;
    assert!(matches!(
        BarrierOption::new(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut invalid = t.clone();
    invalid.expiry_time = Date::new(2024, 1, 1).unwrap().start_of_day();
    assert!(matches!(
        BarrierOption::new(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut invalid = t.clone();
    invalid.barrier_type = BarrierType::UpAndIn;
    invalid.rebate_timing = RebateTiming::AtHit;
    assert!(matches!(
        BarrierOption::new(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut scheduled = t.clone();

    for dates in [
        vec![],
        vec![t.expiry_time, t.effective_time],
        vec![t.expiry_time; 2],
        vec![Date::new(2026, 1, 2).unwrap().start_of_day()],
    ] {
        scheduled.observation_times = Some(dates);
        assert!(matches!(
            BarrierOption::new(scheduled.clone()).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    scheduled.observation_times = Some(vec![Date::new(2025, 1, 4).unwrap().start_of_day()]);
    assert!(matches!(
        BarrierOption::new(scheduled),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(option, BarrierOption::new(t).unwrap());
}

#[test]
fn expiry_settlement_matrix_includes_equality_and_paid_rebates() {
    for kind in [
        BarrierType::UpAndIn,
        BarrierType::UpAndOut,
        BarrierType::DownAndIn,
        BarrierType::DownAndOut,
    ] {
        for state in [BarrierTouchState::Touched, BarrierTouchState::Untouched] {
            for level in [90.0, 100.0, 120.0] {
                for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
                    let knock_in = matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn);
                    if knock_in && timing == RebateTiming::AtHit {
                        continue;
                    }
                    let mut t = terms(kind);
                    t.strike = 80.0;
                    t.barrier_level = level;
                    t.rebate = 7.0;
                    t.rebate_timing = timing;
                    t.touch_state = Some(state);
                    t.history_time = Some(t.effective_time);
                    let context = market(100.0, t.expiry_time);
                    let prior = state == BarrierTouchState::Touched;
                    let hit = prior
                        || if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
                            100.0 >= level
                        } else {
                            100.0 <= level
                        };
                    let unpaid = if prior && timing == RebateTiming::AtHit {
                        0.0
                    } else {
                        7.0
                    };
                    let expected = if knock_in {
                        if hit { 20.0 } else { 7.0 }
                    } else if hit {
                        unpaid
                    } else {
                        20.0
                    };
                    let option = BarrierOption::new(t).unwrap();
                    assert_eq!(
                        AnalyticBarrierEngine
                            .price(&dated(&option, &context), &context)
                            .unwrap(),
                        expected
                    );
                    assert!(matches!(
                        implied_volatility(
                            &AnalyticBarrierEngine,
                            &dated(&option, &context),
                            &context,
                            expected,
                            Default::default()
                        )
                        .unwrap_err(),
                        Error::UnsupportedOperation(_)
                    ));
                }
            }
        }
    }
}

#[test]
fn completed_schedules_resolve_before_analytic_applicability_and_do_not_repeat_fixings() {
    let mut t = terms(BarrierType::UpAndOut);

    let fixing = Date::new(2025, 1, 2).unwrap().start_of_day();
    t.observation_times = Some(vec![fixing]);
    t.rebate = 7.0;
    let option = BarrierOption::new(t.clone()).unwrap();
    assert!(matches!(
        AnalyticBarrierEngine
            .price(
                &dated(&option, &market(100.0, t.effective_time)),
                &market(100.0, t.effective_time)
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let hit = market(120.0, fixing);
    assert!(
        (AnalyticBarrierEngine
            .price(&dated(&option, &hit), &hit)
            .unwrap()
            - 7.0 * (-0.05_f64 * 364.0 / 365.0).exp())
        .abs()
            < 1e-12
    );
    let noon = Timestamp::new(fixing.date(), 12, 0, 0, 0).unwrap();
    assert!(matches!(
        AnalyticBarrierEngine
            .price(&dated(&option, &market(150.0, noon)), &market(150.0, noon))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let untouched = option
        .replace_history(
            Some(BarrierTouchState::Untouched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    let vanilla =
        EuropeanOption::new(t.option_type, t.strike, t.effective_time, t.expiry_time).unwrap();
    assert_eq!(
        AnalyticBarrierEngine
            .price(
                &dated(&untouched, &market(150.0, noon)),
                &market(150.0, noon)
            )
            .unwrap(),
        AnalyticEuropeanEngine
            .price(&dated(&vanilla, &market(150.0, noon)), &market(150.0, noon))
            .unwrap()
    );
    let touched = option
        .replace_history(
            Some(BarrierTouchState::Touched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    assert!(matches!(
        AnalyticBarrierEngine
            .price(
                &dated(&touched, &market(150.0, fixing)),
                &market(150.0, fixing)
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn tiny_positive_volatility_retains_time_value_and_extreme_monetary_scales() {
    for kind in [
        BarrierType::UpAndIn,
        BarrierType::UpAndOut,
        BarrierType::DownAndIn,
        BarrierType::DownAndOut,
    ] {
        let up = matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut);
        let knock_in = matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn);
        for direction in [OptionType::Call, OptionType::Put] {
            for spot in [1e16, 1e150] {
                for sigma in [1e-12, 1e-16, 1e-20, 1e-200] {
                    let mut t = terms(kind);
                    t.option_type = direction;
                    t.strike = spot;
                    t.barrier_level = (if up { 1.2 } else { 0.8 }) * spot;
                    let context = PricingContext::new(
                        BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap(),
                        spot,
                        t.effective_time,
                    )
                    .unwrap();
                    let actual = AnalyticBarrierEngine
                        .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
                        .unwrap();
                    let expected = if knock_in {
                        0.0
                    } else {
                        spot * sigma / (2.0 * std::f64::consts::PI).sqrt()
                    };
                    assert!(
                        (actual - expected).abs() <= 1e-12 * expected.abs(),
                        "{kind:?} {direction:?} {sigma} {actual} {expected}"
                    );
                }
            }
            let mut t = terms(kind);
            t.option_type = direction;
            t.barrier_level = if up { 120.0 } else { 80.0 };
            let unit = AnalyticBarrierEngine
                .price(
                    &dated(
                        &BarrierOption::new(t.clone()).unwrap(),
                        &market(100.0, t.effective_time),
                    ),
                    &market(100.0, t.effective_time),
                )
                .unwrap();
            for scale in [1e-300, 1e300] {
                t.strike = 100.0 * scale;
                t.barrier_level = (if up { 120.0 } else { 80.0 }) * scale;
                let actual = AnalyticBarrierEngine
                    .price(
                        &dated(
                            &BarrierOption::new(t.clone()).unwrap(),
                            &market(100.0 * scale, t.effective_time),
                        ),
                        &market(100.0 * scale, t.effective_time),
                    )
                    .unwrap();
                assert!((actual / scale - unit).abs() < 1e-9);
            }
        }
    }
}

#[test]
fn hit_rebates_match_independent_first_passage_integration_including_negative_rates() {
    // Integrate on squared elapsed time, independently of the engine's inverse-root transform.
    let integral = |distance: f64, drift: f64, sigma: f64, rate: f64| {
        let density = |fraction: f64| {
            if fraction == 0.0 {
                return 0.0;
            }
            let time = fraction * fraction;
            let z = (distance + drift * time) / (sigma * time.sqrt());
            2.0 * fraction * distance / (sigma * (2.0 * std::f64::consts::PI * time).sqrt() * time)
                * (-0.5 * z * z - rate * time).exp()
        };
        let n = 131_072;
        let mut sum = density(1.0);
        for i in 1..n {
            sum += (if i % 2 == 0 { 2.0 } else { 4.0 }) * density(f64::from(i) / f64::from(n));
        }
        sum / (3.0 * f64::from(n))
    };
    for drift in [-0.04, 0.0, 0.04] {
        for up in [false, true] {
            for distance in [0.001_f64, 0.1, 1.0] {
                let rate = -0.02;
                let sigma = 0.2;
                let dividend = rate - drift - 0.5 * sigma * sigma;
                let mut t = terms(if up {
                    BarrierType::UpAndOut
                } else {
                    BarrierType::DownAndOut
                });
                t.option_type = if up {
                    OptionType::Call
                } else {
                    OptionType::Put
                };
                t.barrier_level = 100.0 * (if up { distance } else { -distance }).exp();
                t.strike = t.barrier_level;
                t.rebate = 7.0;
                t.rebate_timing = RebateTiming::AtHit;
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
                    100.0,
                    t.effective_time,
                )
                .unwrap();
                let price = AnalyticBarrierEngine
                    .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
                    .unwrap();
                let expected =
                    7.0 * integral(distance, if up { -drift } else { drift }, sigma, rate);
                assert!(
                    (price - expected).abs() < 2e-8,
                    "{drift} {up} {distance}: {price} {expected}"
                );
            }
        }
    }
}

#[test]
fn scheduled_requests_recover_volatility_and_support_concurrent_reads() {
    let mut t = terms(BarrierType::DownAndOut);
    t.barrier_level = 80.0;
    t.observation_times = Some(vec![
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        t.expiry_time,
    ]);
    t.touch_state = Some(BarrierTouchState::Untouched);
    t.history_time = Some(t.effective_time);
    let option = BarrierOption::new(t.clone()).unwrap();
    let context = market(
        100.25,
        Timestamp::new(t.effective_time.date(), 12, 0, 0, 0).unwrap(),
    );
    let engine = AnalyticBarrierEngine;
    let result = engine
        .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
        .unwrap();
    assert!(result.require(Greek::Theta).unwrap().is_finite());
    let settings = ImpliedVolatilitySettings {
        lower_bound: 0.15,
        upper_bound: 0.25,
        ..Default::default()
    };
    let sigma = implied_volatility(
        &engine,
        &dated(&option, &context),
        &context,
        result.price(),
        settings,
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-7);
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
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            engine
                .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                .unwrap()
        });
        let second = engine
            .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
            .unwrap();
        assert_eq!(first.join().unwrap().values(), second.values());
    });
}

#[test]
fn scheduled_bgk_domain_flat_rate_restrictions_and_solver_failures_are_explicit() {
    for (kind, level, sign, outside) in [
        (BarrierType::UpAndOut, 120.0, 1.0, 150.0),
        (BarrierType::DownAndOut, 80.0, -1.0, 50.0),
    ] {
        let mut t = terms(kind);
        t.barrier_level = level;
        t.observation_times = Some(vec![t.expiry_time]);
        t.touch_state = Some(BarrierTouchState::Untouched);
        t.history_time = Some(t.effective_time);
        let scheduled = BarrierOption::new(t.clone()).unwrap();
        t.observation_times = None;
        t.barrier_level *= (sign * 0.582_597_157_939_010_7_f64 * 0.2).exp();
        let continuous = BarrierOption::new(t.clone()).unwrap();
        let context = market(
            100.0,
            Timestamp::new(t.effective_time.date(), 12, 0, 0, 0).unwrap(),
        );
        assert!(
            (AnalyticBarrierEngine
                .price(&dated(&scheduled, &context), &context)
                .unwrap()
                - AnalyticBarrierEngine
                    .price(&dated(&continuous, &context), &context)
                    .unwrap())
            .abs()
                < 1e-11
        );
        let outside = context
            .replace(PricingContextChanges {
                spot_price: Some(outside),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            AnalyticBarrierEngine
                .price(&dated(&scheduled, &outside), &outside)
                .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let t = terms(BarrierType::UpAndOut);
    let option = BarrierOption::new(t.clone())
        .unwrap()
        .replace_history(
            Some(BarrierTouchState::Untouched),
            Some((terms(BarrierType::UpAndOut)).effective_time),
        )
        .unwrap();
    let context = market(100.0, t.effective_time);
    let curve = RateTermStructure::new(
        t.effective_time,
        vec![t.expiry_time, (t.expiry_time).add_days(1).unwrap()],
        vec![0.95, (0.95) * 0.99],
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
    let curved = context
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        AnalyticBarrierEngine
            .price_with_greeks(&dated(&option, &curved), &curved, GreeksRequest::new([]))
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let controls = ImpliedVolatilitySettings {
        lower_bound: -0.1,
        ..Default::default()
    };
    assert!(matches!(
        implied_volatility(
            &AnalyticBarrierEngine,
            &dated(&option, &curved),
            &curved,
            1.0,
            controls
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        implied_volatility(
            &AnalyticBarrierEngine,
            &dated(&option, &context),
            &context,
            1.0,
            controls
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let controls = ImpliedVolatilitySettings {
        lower_bound: 0.15,
        upper_bound: 0.25,
        ..Default::default()
    };
    assert!(matches!(
        implied_volatility(
            &AnalyticBarrierEngine,
            &dated(&option, &context),
            &context,
            -1.0,
            controls
        )
        .unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert_eq!(option.touch_state(), Some(BarrierTouchState::Untouched));
    let resolved = option
        .replace_history(
            Some(BarrierTouchState::Touched),
            Some(option.history_time().unwrap_or(option.effective_time())),
        )
        .unwrap();
    let later = market(100.0, Date::new(2025, 1, 2).unwrap().start_of_day());
    assert!(matches!(
        implied_volatility(
            &AnalyticBarrierEngine,
            &dated(&resolved, &later),
            &later,
            0.0,
            controls
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn quantlib_reference_prices_cover_direction_activation_and_rebate_timing() {
    // QuantLib 1.43 analytic barriers; expiry knock-out rebates use an independent bond/knock-in portfolio.
    for (kind, direction, timing, expected) in [
        (
            BarrierType::UpAndIn,
            OptionType::Call,
            RebateTiming::AtExpiry,
            12.232_027_467_703_755,
        ),
        (
            BarrierType::UpAndIn,
            OptionType::Put,
            RebateTiming::AtExpiry,
            4.368_127_409_260_037,
        ),
        (
            BarrierType::UpAndOut,
            OptionType::Call,
            RebateTiming::AtHit,
            3.718_230_415_424_074_4,
        ),
        (
            BarrierType::UpAndOut,
            OptionType::Call,
            RebateTiming::AtExpiry,
            3.653_584_011_955_294,
        ),
        (
            BarrierType::UpAndOut,
            OptionType::Put,
            RebateTiming::AtHit,
            8.685_205_593_263_646,
        ),
        (
            BarrierType::UpAndOut,
            OptionType::Put,
            RebateTiming::AtExpiry,
            8.620_559_189_794_864,
        ),
        (
            BarrierType::DownAndIn,
            OptionType::Call,
            RebateTiming::AtExpiry,
            5.087_560_973_330_836,
        ),
        (
            BarrierType::DownAndIn,
            OptionType::Put,
            RebateTiming::AtExpiry,
            9.591_264_766_055_238,
        ),
        (
            BarrierType::DownAndOut,
            OptionType::Call,
            RebateTiming::AtHit,
            10.835_112_903_397_224,
        ),
        (
            BarrierType::DownAndOut,
            OptionType::Call,
            RebateTiming::AtExpiry,
            10.798_050_506_328_213,
        ),
        (
            BarrierType::DownAndOut,
            OptionType::Put,
            RebateTiming::AtHit,
            3.434_484_230_068_676_4,
        ),
        (
            BarrierType::DownAndOut,
            OptionType::Put,
            RebateTiming::AtExpiry,
            3.397_421_832_999_665_3,
        ),
    ] {
        let mut t = terms(kind);
        t.option_type = direction;
        t.barrier_level = if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
            120.0
        } else {
            80.0
        };
        t.rebate = 7.0;
        t.rebate_timing = timing;
        let context = market(100.0, t.effective_time);
        let actual = AnalyticBarrierEngine
            .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
            .unwrap();
        assert!((actual - expected).abs() < 1e-8);
    }
}

#[test]
fn rare_hit_rebates_survive_tail_scaling_and_cancellation_is_nonnegative() {
    for up in [false, true] {
        for rate in [-0.03, 0.0, 0.04] {
            for amount in [10.0, 1e9, 1e300] {
                let barrier: f64 = if up { 500.0 } else { 20.0 };
                let distance = (barrier / 100.0).ln().abs();
                let drift = (if up { 1.0 } else { -1.0 }) * (rate - 0.01 - 0.02);
                let normal = |x: f64| 0.5 * libm::erfc(-x / std::f64::consts::SQRT_2);
                let probability = normal((drift - distance) / 0.2)
                    + (2.0 * drift * distance / 0.04).exp() * normal((-drift - distance) / 0.2);
                let expected = amount * (-rate).exp() * probability;
                let mut t = terms(if up {
                    BarrierType::UpAndOut
                } else {
                    BarrierType::DownAndOut
                });
                t.option_type = if up {
                    OptionType::Call
                } else {
                    OptionType::Put
                };
                t.strike = if up { 1000.0 } else { 10.0 };
                t.barrier_level = barrier;
                t.rebate = amount;
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(rate, 0.01, 0.2).unwrap(),
                    100.0,
                    t.effective_time,
                )
                .unwrap();
                let actual = AnalyticBarrierEngine
                    .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
                    .unwrap();
                assert!(actual > 0.0);
                assert!((actual / expected - 1.0).abs() < 1e-11);
            }
        }
    }
    let mut t = terms(BarrierType::UpAndIn);
    t.option_type = OptionType::Put;
    t.strike = 187.0;
    t.barrier_level = 127.0;
    t.expiry_time = Date::new(2025, 2, 1).unwrap().start_of_day();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.01, 0.0, 0.09).unwrap(),
        100.0,
        t.effective_time,
    )
    .unwrap();
    let actual = AnalyticBarrierEngine
        .price(&dated(&BarrierOption::new(t).unwrap(), &context), &context)
        .unwrap();
    assert!((0.0..1e-10).contains(&actual));
}

#[test]
fn contract_calendars_reject_closed_observations_on_construction_and_replacement() {
    let mut t = terms(BarrierType::UpAndOut);
    t.observation_times = Some(vec![t.expiry_time]);
    let option = BarrierOption::new(t.clone()).unwrap();
    let closed = TradingCalendar::new(|_| false, 365).unwrap();
    assert!(matches!(
        option.replace_calendar(closed),
        Err(Error::InvalidInput(_))
    ));
    t.observation_times = Some(vec![Date::new(2025, 1, 4).unwrap().start_of_day()]);
    assert!(matches!(BarrierOption::new(t), Err(Error::InvalidInput(_))));
}
