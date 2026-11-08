mod support;
use support::dated;

use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}
fn terms() -> PhoenixTerms {
    let mut terms = PhoenixTerms::new(
        kiyosi::PhoenixCoupon::Annual(0.365),
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
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.observation_clock_time = None;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    terms.history_time = Some(terms.effective_time);
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
fn quoted_coupons_preserve_units_and_reprice_solved_quotes() {
    let engine = QuadratureAutocallableEngine::default();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0).unwrap(),
        100.0,
        date(1, 1),
    )
    .unwrap();
    for original_coupon in [PhoenixCoupon::Annual(0.08), PhoenixCoupon::PerPeriod(0.01)] {
        let mut input = terms();
        input.coupon = original_coupon;
        input.observations =
            autocallable_observations(&[date(4, 1)], &[110.0], None, Some(&[0.0])).unwrap();
        let note = PhoenixOption::new(input.clone()).unwrap();
        for coupon in [-0.02, 0.0, 0.02] {
            let (expected_coupon, payment) = match original_coupon {
                PhoenixCoupon::Annual(_) => (PhoenixCoupon::Annual(coupon), coupon * 90.0 / 365.0),
                PhoenixCoupon::PerPeriod(_) => (PhoenixCoupon::PerPeriod(coupon), coupon),
            };
            let quoted = note.with_quoted_coupon(expected_coupon).unwrap();
            let mut expected = input.clone();
            expected.coupon = expected_coupon;
            assert_eq!(quoted.terms(), &expected);
            let quote = 1.0 + payment;
            assert!(
                (engine.price(&dated(&quoted, &context), &context).unwrap() - quote).abs() < 1e-14
            );
            let solved = implied_phoenix_coupon(
                &engine,
                &dated(&note, &context),
                &context,
                quote,
                ImpliedCouponSettings {
                    lower_bound: -1.0,
                    ..Default::default()
                },
            )
            .unwrap();
            let ((PhoenixCoupon::Annual(solved_value), PhoenixCoupon::Annual(_))
            | (PhoenixCoupon::PerPeriod(solved_value), PhoenixCoupon::PerPeriod(_))) =
                (solved, original_coupon)
            else {
                panic!("solved coupon has the wrong payment convention")
            };
            assert!((solved_value - coupon).abs() < 1e-12);
            let repriced = note
                .replace(PhoenixOptionChanges {
                    coupon: Some(solved),
                    ..Default::default()
                })
                .unwrap();
            assert!(
                (engine.price(&dated(&repriced, &context), &context).unwrap() - quote).abs()
                    < 1e-12
            );
        }
        for coupon in [PhoenixCoupon::Annual(0.12), PhoenixCoupon::PerPeriod(0.12)] {
            let quoted = note.with_quoted_coupon(coupon).unwrap();
            assert_eq!(quoted.terms().coupon, coupon);
            assert_eq!(quoted.barrier_state(), note.barrier_state());
        }
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                note.with_quoted_coupon(PhoenixCoupon::Annual(invalid)),
                Err(Error::InvalidInput(_))
            ));
        }
        assert_eq!(note.terms(), &input);
    }
}

#[test]
fn conditional_period_coupons_are_inclusive_and_independent_of_principal_and_loss() {
    let engine = QuadratureAutocallableEngine::default();
    for coupon in [0.365, -0.365] {
        let mut terms = terms();
        terms.coupon = PhoenixCoupon::Annual(coupon);
        terms.principal_redemption_ratio = 0.25;
        let note = PhoenixOption::new(terms).unwrap();
        for (spot, base, paid) in [
            (60.0, -0.15, false),
            (70.0, -0.05, false),
            (80.0, 0.25, false),
            (90.0, 0.25, true),
            (110.0, 0.25, true),
        ] {
            let expected = base + if paid { coupon * 57.0 / 365.0 } else { 0.0 };
            assert!(
                (engine
                    .price(
                        &dated(&note, &market(spot, date(4, 1))),
                        &market(spot, date(4, 1))
                    )
                    .unwrap()
                    - expected)
                    .abs()
                    < 1e-14
            );
        }
        assert!(
            (engine
                .price(
                    &dated(&note, &market(110.0, date(2, 3))),
                    &market(110.0, date(2, 3))
                )
                .unwrap()
                - (0.25 + coupon * 33.0 / 365.0))
                .abs()
                < 1e-14
        );
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
            t.coupon = PhoenixCoupon::Annual(coupon);
            t.observations =
                autocallable_observations(&[end], &[100.0], None, Some(&[100.0])).unwrap();
            let put = |strike| {
                AnalyticEuropeanEngine
                    .price(
                        &dated(
                            &EuropeanOption::new(OptionType::Put, strike, t.effective_time, end)
                                .unwrap(),
                            &ctx,
                        ),
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
                + coupon.signum()
                    * AnalyticDigitalEngine
                        .price(&dated(&digital, &ctx), &ctx)
                        .unwrap()
                - (put(100.0) - if lower == 0.0 { 0.0 } else { put(lower) }) / 100.0;
            let actual = QuadratureAutocallableEngine::default()
                .price(&dated(&PhoenixOption::new(t).unwrap(), &ctx), &ctx)
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
    t.coupon = PhoenixCoupon::Annual(f64::NAN);
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
        engine.price(&dated(&note, &ctx), &ctx).unwrap_err(),
        Error::MissingHistory(_)
    ));
    let known = note
        .replace_history(
            Some(AutocallableBarrierState::NoEvent),
            Some(note.history_time().unwrap_or(note.effective_time())),
        )
        .unwrap();
    assert_eq!(known.observations(), note.observations());
    assert_eq!(note.barrier_state(), None);
    assert!(
        engine
            .price(&dated(&known, &ctx), &ctx)
            .unwrap()
            .is_finite()
    );
    let impossible = note
        .replace_history(
            Some(AutocallableBarrierState::KnockedOut),
            Some(note.history_time().unwrap_or(note.effective_time())),
        )
        .unwrap();
    assert!(matches!(
        engine
            .price(
                &dated(&impossible, &market(100.0, date(1, 1))),
                &market(100.0, date(1, 1))
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.barrier_state = Some(AutocallableBarrierState::KnockedIn);
    t.history_time = Some(t.effective_time);
    assert!(matches!(
        engine
            .price(&dated(&PhoenixOption::new(t).unwrap(), &ctx), &ctx)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn phoenix_greeks_and_volatility_preserve_thresholds_and_failure_state() {
    let note = PhoenixOption::new(terms()).unwrap();
    let ctx = market(103.0, date(1, 1));
    let engine = QuadratureAutocallableEngine::default();
    let result = kiyosi::numerical_greeks(
        &engine,
        &dated(&note, &ctx),
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
        &dated(&note, &ctx),
        &ctx,
        result.price(),
        ImpliedVolatilitySettings::new(0.2, 0.4),
    )
    .unwrap();
    assert!((sigma - 0.3).abs() < 1e-6);
    assert_eq!(ctx.model_parameters().as_bsm().unwrap().volatility(), 0.3);
    let ctx = market(90.0, date(2, 3));
    let result = engine
        .price_with_greeks(&dated(&note, &ctx), &ctx, GreeksRequest::all())
        .unwrap();
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Theta));
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let settled = note
        .replace_history(
            Some(AutocallableBarrierState::KnockedOut),
            Some(note.history_time().unwrap_or(note.effective_time())),
        )
        .unwrap();
    assert_eq!(
        engine
            .price(
                &dated(&settled, &market(100.0, date(3, 3))),
                &market(100.0, date(3, 3))
            )
            .unwrap(),
        0.0
    );
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&settled, &market(100.0, date(3, 3))),
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
    t.coupon = PhoenixCoupon::Annual(-0.5);
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
    let expected = (-0.5 * time + 0.25 * (1.0 - survival)) * (-0.04 * time).exp()
        + (0.25 - 0.5 * (1.0 - time)) * survival * (-0.04_f64).exp();
    for settings in [
        QuadratureSettings::default(),
        QuadratureSettings {
            asset_step_count: 128,
            integration_standard_deviations: 0.5,
        },
    ] {
        let actual = QuadratureAutocallableEngine::new(settings)
            .unwrap()
            .price(&dated(&note, &ctx), &ctx)
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
                .price(
                    &dated(&note, &market(100.0, date(1, 1))),
                    &market(100.0, date(1, 1)),
                )
                .unwrap()
            })
            .collect();
        assert!((values[0] - values[1]).abs() < 1e-4, "{values:?}");
        assert!((values[1] - values[2]).abs() < 1e-4, "{values:?}");
        prices.push(values[0]);
    }
    assert!(prices[1] < prices[0] - 0.001, "{prices:?}");
}
