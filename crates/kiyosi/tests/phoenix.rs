use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}
fn terms() -> PhoenixTerms {
    let mut terms = PhoenixTerms::new(
        0.01,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        autocallable_observations(
            &[date(2, 3), date(4, 1)],
            &[110.0, 110.0],
            None,
            Some(&[90.0, 90.0]),
        )
        .unwrap(),
        Some((date(4, 1)).time_of_day()),
        TradingCalendar::weekdays(),
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.observation_clock_time = None;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);

    terms
}
fn market(spot: f64, valuation: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation,
    )
    .unwrap()
}

#[test]
fn lockout_preserves_conditional_coupons_and_continuation_in_all_engines() {
    let context = market(100.0, date(1, 1));
    let mut input = terms();
    input.lower_strike = 100.0;
    input.lockout_periods = 2;
    input.observations = autocallable_observations(
        &[date(1, 1), date(2, 3), date(4, 1)],
        &[100.0; 3],
        None,
        Some(&[90.0; 3]),
    )
    .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let mut expected = (-0.04 * date(1, 1).years_until(date(4, 1))).exp() + 0.01;
    for time in [date(2, 3), date(4, 1)] {
        let digital =
            DigitalOption::cash_or_nothing(OptionType::Call, 90.0, 0.01, date(1, 1), time).unwrap();
        expected += AnalyticDigitalEngine.price(&digital, &context).unwrap();
    }
    for (engine, tolerance) in [
        (
            &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
            1e-9,
        ),
        (&FiniteDifferenceAutocallableEngine::default(), 2e-5),
    ] {
        let actual = engine.price(&note, &context).unwrap();
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} != {expected}"
        );
    }
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let estimate = engine.price_with_statistics(&note, &context).unwrap();
    assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap() + 1e-6);
}

#[test]
fn lockout_preserves_daily_knock_in_and_first_eligible_knock_out() {
    let mut input = terms();
    input.lockout_periods = 1;
    input.lower_strike = 0.0;
    input.knock_in_observation_mode = KnockInObservationMode::EveryTradingDay;
    input.observation_clock_time = Some(date(1, 1).time_of_day());
    input.observations = autocallable_observations(
        &[date(1, 1), date(4, 1)],
        &[50.0, 200.0],
        None,
        Some(&[0.0; 2]),
    )
    .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let horizon = date(1, 1).years_until(date(4, 1));
    let rate = (90.0_f64 / 70.0).ln() / horizon;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rate, 0.0, 0.0).unwrap(),
        70.0,
        date(1, 1),
    )
    .unwrap();
    for engine in [
        &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
        &FiniteDifferenceAutocallableEngine::default(),
        &MonteCarloAutocallableEngine::default(),
    ] {
        let expected = 0.01 + (0.9 + 0.01) * (-rate * horizon).exp();
        assert!((engine.price(&note, &context).unwrap() - expected).abs() < 1e-12);
        let context = market(200.0, date(4, 1));
        assert!((engine.price(&note, &context).unwrap() - 1.01).abs() < 1e-14);
    }
}

#[test]
fn lockout_daily_layers_agree_across_engines_and_constant_local_volatility() {
    let end = date(1, 10);
    let mut input = terms();
    input.expiry_time = end;
    input.knock_in_level = 99.0;
    input.lower_strike = 0.0;
    input.lockout_periods = 2;
    input.knock_in_observation_mode = KnockInObservationMode::EveryTradingDay;
    input.observation_clock_time = Some(date(1, 1).time_of_day());
    input.observations = autocallable_observations(
        &[date(1, 2), date(1, 6), end],
        &[100.0; 3],
        None,
        Some(&[90.0; 3]),
    )
    .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let context = market(100.0, date(1, 1));
    let expected = QuadratureAutocallableEngine::default()
        .price(&note, &context)
        .unwrap();
    let fd = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 800,
        ..Default::default()
    })
    .unwrap();
    let actual = fd.price(&note, &context).unwrap();
    assert!((actual - expected).abs() < 2e-5, "{actual} != {expected}");
    let surface = LocalVolatilitySurface::new(
        vec![date(1, 1), end],
        vec![50.0, 150.0],
        vec![vec![0.3; 2]; 2],
    )
    .unwrap();
    let local = PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface).unwrap(),
        100.0,
        date(1, 1),
    )
    .unwrap();
    assert!((fd.price(&note, &local).unwrap() - actual).abs() < 1e-12);
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let estimate = mc.price_with_statistics(&note, &context).unwrap();
    assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap());
}

#[test]
fn lockout_history_depends_only_on_eligible_knock_out_observations() {
    let mut input = terms();
    input.lockout_periods = 1;
    input.barrier_state = None;

    input.observations = autocallable_observations(
        &[date(2, 3), date(3, 3), date(4, 1)],
        &[110.0; 3],
        None,
        Some(&[90.0; 3]),
    )
    .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    assert!(engine.price(&note, &market(100.0, date(3, 3))).is_ok());
    assert!(matches!(
        engine.price(&note, &market(100.0, date(3, 4))),
        Err(Error::MissingHistory(_))
    ));
    let known = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        known
            .replace(PhoenixOptionChanges {
                lockout_periods: Some(0),
                ..Default::default()
            })
            .unwrap()
            .barrier_state(),
        known.barrier_state()
    );
    assert_eq!(known.lockout_periods(), 1);
    assert_eq!(
        known
            .replace(PhoenixOptionChanges {
                lockout_periods: Some(2),
                ..Default::default()
            })
            .unwrap()
            .lockout_periods(),
        2
    );
    assert!(matches!(
        engine.price(
            &note
                .replace(PhoenixOptionChanges {
                    barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
                    ..Default::default()
                })
                .unwrap(),
            &market(100.0, date(2, 4))
        ),
        Err(Error::InvalidInput(_))
    ));
    let possible = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.price(&possible, &market(100.0, date(3, 4))).unwrap(),
        0.0
    );
    let mut daily = note.terms().clone();
    daily.knock_in_observation_mode = KnockInObservationMode::EveryTradingDay;
    daily.observation_clock_time = Some(date(1, 1).time_of_day());
    assert!(matches!(
        engine.price(
            &PhoenixOption::new(daily).unwrap(),
            &market(100.0, date(1, 2))
        ),
        Err(Error::MissingHistory(_))
    ));
}

#[test]
fn lockout_validates_counts_and_ignores_disabled_greek_discontinuities() {
    let mut input = terms();
    input.lockout_periods = 1;
    let note = PhoenixOption::new(input.clone()).unwrap();
    for count in [2, usize::MAX] {
        input.lockout_periods = count;
        assert!(matches!(
            PhoenixOption::new(input.clone()),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            note.replace(PhoenixOptionChanges {
                lockout_periods: Some(count),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    let context = market(110.0, date(2, 3));
    let greeks = QuadratureAutocallableEngine::default()
        .price_with_greeks(&note, &context, GreeksRequest::new([Greek::Delta]))
        .unwrap();
    assert!(greeks.require(Greek::Delta).unwrap().is_finite());
    assert_eq!(note.lockout_periods(), 1);
}
#[test]
fn quoted_coupons_preserve_units_and_reprice_solved_quotes() {
    let engine = QuadratureAutocallableEngine::default();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0).unwrap(),
        100.0,
        date(1, 1),
    )
    .unwrap();
    let mut input = terms();
    input.observations =
        autocallable_observations(&[date(4, 1)], &[110.0], None, Some(&[0.0])).unwrap();
    let note = PhoenixOption::new(input.clone()).unwrap();
    for coupon in [-0.02, 0.0, 0.02] {
        let quoted = note.with_quoted_coupon(coupon).unwrap();
        let mut expected = input.clone();
        expected.coupon_per_period = coupon;
        assert_eq!(quoted.terms(), &expected);
        let quote = 1.0 + coupon;
        assert!((engine.price(&quoted, &context).unwrap() - quote).abs() < 1e-14);
        let solved = implied_phoenix_coupon(
            &engine,
            &note,
            &context,
            quote,
            ImpliedCouponSettings {
                lower_bound: -1.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((solved - coupon).abs() < 1e-12);
        let repriced = note
            .replace(PhoenixOptionChanges {
                coupon_per_period: Some(solved),
                ..Default::default()
            })
            .unwrap();
        assert!((engine.price(&repriced, &context).unwrap() - quote).abs() < 1e-12);
    }
    let quoted = note.with_quoted_coupon(0.12).unwrap();
    assert_eq!(quoted.terms().coupon_per_period, 0.12);
    assert_eq!(quoted.barrier_state(), note.barrier_state());
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            note.with_quoted_coupon(invalid),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(note.terms(), &input);
}

#[test]
fn conditional_period_coupons_are_inclusive_and_independent_of_principal_and_loss() {
    let engine = QuadratureAutocallableEngine::default();
    for coupon in [0.365, -0.365] {
        let mut terms = terms();
        terms.coupon_per_period = coupon;
        terms.principal_redemption_ratio = 0.25;
        let note = PhoenixOption::new(terms).unwrap();
        for (spot, base, paid) in [
            (60.0, -0.15, false),
            (70.0, -0.05, false),
            (80.0, 0.25, false),
            (90.0, 0.25, true),
            (110.0, 0.25, true),
        ] {
            let expected = base + if paid { coupon } else { 0.0 };
            assert!(
                (engine.price(&note, &market(spot, date(4, 1))).unwrap() - expected).abs() < 1e-14
            );
        }
        assert!(
            (engine.price(&note, &market(110.0, date(2, 3))).unwrap() - (0.25 + coupon)).abs()
                < 1e-14
        );
    }
}

#[test]
fn periodic_coupons_ignore_unequal_and_zero_length_periods_in_all_engines() {
    for coupon in [-0.01, 0.0, 0.01] {
        let mut input = terms();
        input.coupon_per_period = coupon;
        input.observations = autocallable_observations(
            &[date(1, 1), date(2, 3), date(4, 1)],
            &[110.0; 3],
            None,
            Some(&[90.0; 3]),
        )
        .unwrap();
        let note = PhoenixOption::new(input).unwrap();
        for at in [date(1, 1), date(2, 3), date(4, 1)] {
            let context = market(110.0, at);
            for engine in [
                &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
                &FiniteDifferenceAutocallableEngine::default(),
                &MonteCarloAutocallableEngine::default(),
            ] {
                assert!((engine.price(&note, &context).unwrap() - (1.0 + coupon)).abs() < 1e-14);
            }
        }
    }
}

#[test]
fn terminal_coupon_and_downside_match_an_independent_vanilla_digital_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let ctx = market(100.0, date(1, 1));
    for coupon in [-4.0, 0.1] {
        for lower in [0.0, 60.0] {
            let mut t = terms();
            t.expiry_time = end;
            t.knock_in_level = 100.0;
            t.lower_strike = lower;
            t.principal_redemption_ratio = 0.25;
            t.coupon_per_period = coupon;
            t.observations =
                autocallable_observations(&[end], &[100.0], None, Some(&[100.0])).unwrap();
            let put = |strike| {
                AnalyticEuropeanEngine
                    .price(
                        &EuropeanOption::new(OptionType::Put, strike, t.effective_time, end)
                            .unwrap(),
                        &ctx,
                    )
                    .unwrap()
            };
            let digital = DigitalOption::cash_or_nothing(
                OptionType::Call,
                100.0,
                coupon.abs(),
                t.effective_time,
                end,
            )
            .unwrap();
            let expected = 0.25 * (-0.04_f64).exp()
                + coupon.signum() * AnalyticDigitalEngine.price(&digital, &ctx).unwrap()
                - (put(100.0) - if lower == 0.0 { 0.0 } else { put(lower) }) / 100.0;
            let actual = QuadratureAutocallableEngine::default()
                .price(&PhoenixOption::new(t).unwrap(), &ctx)
                .unwrap();
            assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
        }
    }
}

#[test]
fn phoenix_validation_defaults_and_history_share_core_categories() {
    let mut t = terms();
    t.observations[0] = AutocallableObservation::snowball(
        t.observations[0].time(),
        t.observations[0].knock_out_level(),
        0.1,
    )
    .unwrap();
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        let t = terms();
        assert!(matches!(
            t.observations[0].replace(AutocallableObservationChanges {
                coupon_barrier_level: Some(value),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    let mut t = terms();
    t.observations.clear();
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.coupon_per_period = f64::NAN;
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::EveryTradingDay;
    t.observation_clock_time = Some(t.expiry_time.time_of_day());
    t.barrier_state = None;
    let note = PhoenixOption::new(t).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    let ctx = market(100.0, date(2, 3));
    assert!(matches!(
        engine.price(&note, &ctx).unwrap_err(),
        Error::MissingHistory(_)
    ));
    let known = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(known.observations(), note.observations());
    assert_eq!(note.barrier_state(), None);
    assert!(engine.price(&known, &ctx).unwrap().is_finite());
    let impossible = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price(&impossible, &market(100.0, date(1, 1)))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.barrier_state = Some(AutocallableBarrierState::KnockedIn);

    assert!(matches!(
        engine.price(&PhoenixOption::new(t).unwrap(), &market(100.0, date(1, 1))),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn phoenix_greeks_and_volatility_preserve_thresholds_and_failure_state() {
    let note = PhoenixOption::new(terms()).unwrap();
    let ctx = market(103.0, date(1, 1));
    let engine = QuadratureAutocallableEngine::default();
    let result = kiyosi::numerical_greeks(
        &engine,
        &note,
        &ctx,
        GreeksRequest::all(),
        NumericalShiftSettings {
            spot_shift: Some(0.25),
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
        &ctx,
        result.price(),
        ImpliedVolatilitySettings::new(0.2, 0.4),
    )
    .unwrap();
    assert!((sigma - 0.3).abs() < 1e-6);
    assert_eq!(ctx.model_parameters().as_bsm().unwrap().volatility(), 0.3);
    let ctx = market(90.0, date(2, 3));
    let result = engine
        .price_with_greeks(&note, &ctx, GreeksRequest::all())
        .unwrap();
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Theta));
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let settled = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.price(&settled, &market(100.0, date(3, 3))).unwrap(),
        0.0
    );
    assert!(matches!(
        implied_volatility(
            &engine,
            &settled,
            &market(100.0, date(3, 3)),
            0.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn unconditional_coupon_tails_match_a_two_date_cashflow_portfolio() {
    let middle = date(7, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let mut t = terms();
    t.coupon_per_period = -0.5;
    t.lower_strike = 100.0;
    t.principal_redemption_ratio = 0.25;
    t.expiry_time = end;
    t.observations =
        autocallable_observations(&[middle, end], &[100.0; 2], None, Some(&[0.0; 2])).unwrap();
    let note = PhoenixOption::new(t).unwrap();
    let ctx = market(100.0, date(1, 1))
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.04, 0.01, 2.0)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let time: f64 = 181.0 / 365.0;
    let survival = 0.5 * libm::erfc((0.03 - 2.0) * time.sqrt() / (2.0 * 2.0_f64.sqrt()));
    let expected = (-0.5 + 0.25 * (1.0 - survival)) * (-0.04 * time).exp()
        + (0.25 - 0.5) * survival * (-0.04_f64).exp();
    for settings in [
        QuadratureSettings::default(),
        QuadratureSettings {
            asset_step_count: 128,
            integration_standard_deviations: 0.5,
        },
    ] {
        let actual = QuadratureAutocallableEngine::new(settings)
            .unwrap()
            .price(&note, &ctx)
            .unwrap();
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }
}

#[test]
fn phoenix_daily_layers_converge_with_conditional_coupons() {
    let mut prices = Vec::new();
    for mode in [
        KnockInObservationMode::AtExpiry,
        KnockInObservationMode::EveryTradingDay,
    ] {
        let mut t = terms();
        t.knock_in_level = 95.0;
        t.knock_in_observation_mode = mode;
        t.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(t.expiry_time.time_of_day());
        t.observations = autocallable_observations(
            &[date(2, 3), date(3, 3), date(4, 1)],
            &[110.0, 108.0, 105.0],
            None,
            Some(&[95.0; 3]),
        )
        .unwrap();
        let note = PhoenixOption::new(t).unwrap();
        let values: Vec<_> = [(512, 8.0), (1024, 8.0), (1024, 10.0)]
            .into_iter()
            .map(|(n, e)| {
                QuadratureAutocallableEngine::new(QuadratureSettings {
                    asset_step_count: n,
                    integration_standard_deviations: e,
                })
                .unwrap()
                .price(&note, &market(100.0, date(1, 1)))
                .unwrap()
            })
            .collect();
        assert!((values[0] - values[1]).abs() < 1e-4, "{values:?}");
        assert!((values[1] - values[2]).abs() < 1e-4, "{values:?}");
        prices.push(values[0]);
    }
    assert!(prices[1] < prices[0] - 0.001, "{prices:?}");
}
