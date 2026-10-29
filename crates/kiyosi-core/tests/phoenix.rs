use kiyosi_core::*;

fn date(month: u32, day: u32) -> Date {
    Date::new(2025, month, day).unwrap()
}
fn terms() -> PhoenixTerms {
    let mut terms = PhoenixTerms::new(
        0.365,
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
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::None);
    terms
}
fn market(spot: f64, valuation: Date) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation.into(),
    )
    .unwrap()
}
#[test]
fn conditional_period_coupons_are_inclusive_and_independent_of_principal_and_loss() {
    let engine = QuadratureAutocallableEngine::default();
    for coupon in [0.365, -0.365] {
        let mut terms = terms();
        terms.coupon_rate = coupon;
        terms.principal_ratio = 0.25;
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
                (engine.price(&note, &market(spot, date(4, 1))).unwrap() - expected).abs() < 1e-14
            );
        }
        assert!(
            (engine.price(&note, &market(110.0, date(2, 3))).unwrap()
                - (0.25 + coupon * 33.0 / 365.0))
                .abs()
                < 1e-14
        );
    }
}

#[test]
fn terminal_coupon_and_downside_match_an_independent_vanilla_digital_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap();
    let ctx = market(100.0, date(1, 1));
    for coupon in [-4.0, 0.1] {
        for lower in [0.0, 60.0] {
            let mut t = terms();
            t.expiry_date = end;
            t.knock_in_level = 100.0;
            t.lower_strike = lower;
            t.principal_ratio = 0.25;
            t.coupon_rate = coupon;
            t.observations =
                autocallable_observations(&[end], &[100.0], None, Some(&[100.0])).unwrap();
            let put = |strike| {
                AnalyticVanillaEngine
                    .price(
                        &EuropeanOption::new(OptionType::Put, strike, t.effective_date, end)
                            .unwrap(),
                        &ctx,
                    )
                    .unwrap()
            };
            let digital = DigitalOption::cash_or_nothing(
                OptionType::Call,
                100.0,
                coupon.abs(),
                t.effective_date,
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
    t.observations[0].coupon_rate = 0.1;
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        let mut t = terms();
        t.observations[0].coupon_barrier_level = value;
        assert!(matches!(
            PhoenixOption::new(t).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut t = terms();
    t.observations.clear();
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.coupon_rate = f64::NAN;
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::EveryTradingDay;
    t.barrier_state = None;
    let note = PhoenixOption::new(t).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    let ctx = market(100.0, date(2, 3));
    assert!(matches!(
        engine.price(&note, &ctx).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let known = note.replace(Some(AutocallableBarrierState::None)).unwrap();
    assert_eq!(known.observations(), note.observations());
    assert_eq!(note.barrier_state(), None);
    assert!(engine.price(&known, &ctx).unwrap().is_finite());
    let impossible = note
        .replace(Some(AutocallableBarrierState::KnockedOut))
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
        engine
            .price(&PhoenixOption::new(t).unwrap(), &ctx)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn phoenix_greeks_and_volatility_preserve_thresholds_and_failure_state() {
    let note = PhoenixOption::new(terms()).unwrap();
    let ctx = market(103.0, date(1, 1));
    let engine = QuadratureAutocallableEngine::default();
    let result = engine
        .price_with_greeks(
            &note,
            &ctx,
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
        &ctx,
        result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.2,
            upper_bound: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.3).abs() < 1e-6);
    assert_eq!(ctx.model_parameters().volatility(), 0.3);
    let ctx = market(90.0, date(2, 3));
    let result = engine
        .price_with_greeks(&note, &ctx, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Theta));
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let settled = note
        .replace(Some(AutocallableBarrierState::KnockedOut))
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
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn unconditional_coupon_tails_match_a_two_date_cashflow_portfolio() {
    let middle = date(7, 1);
    let end = Date::new(2026, 1, 1).unwrap();
    let mut t = terms();
    t.coupon_rate = -0.5;
    t.lower_strike = 100.0;
    t.principal_ratio = 0.25;
    t.expiry_date = end;
    t.observations = autocallable_observations(&[middle, end], &[100.0; 2], None, None).unwrap();
    let note = PhoenixOption::new(t).unwrap();
    let ctx = market(100.0, date(1, 1))
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.04, 0.01, 2.0).unwrap()),
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
