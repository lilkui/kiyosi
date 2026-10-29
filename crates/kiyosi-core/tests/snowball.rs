use kiyosi_core::*;

fn date(month: u32, day: u32) -> Date {
    Date::new(2025, month, day).unwrap()
}

fn terms() -> SnowballTerms {
    SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        vec![AutocallableObservation::new(date(4, 1), 110.0)],
    )
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
fn current_settlement_preserves_strict_knock_in_and_inclusive_knock_out() {
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    t.barrier_state = Some(AutocallableBarrierState::None);
    t.observations[0].coupon_rate = 0.08;
    let note = SnowballOption::new(t).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    let term = 90.0 / 365.0;
    for (spot, expected) in [
        (60.0, 0.6),
        (70.0, 0.7),
        (80.0, 1.0 + 0.06 * term),
        (110.0, 1.0 + 0.08 * term),
    ] {
        assert!((engine.price(&note, &market(spot, date(4, 1))).unwrap() - expected).abs() < 1e-14);
    }
}

#[test]
fn invalid_terms_and_history_fail_without_changing_the_note() {
    let mut t = terms();
    t.observations.push(t.observations[0]);
    assert!(matches!(
        SnowballOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.lower_strike = 101.0;
    assert!(matches!(
        SnowballOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        autocallable_observations(&[date(4, 1)], &[], None, None).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let note = SnowballOption::new(terms()).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    assert!(matches!(
        engine.price(&note, &market(100.0, date(1, 2))).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let known = note.replace(Some(AutocallableBarrierState::None)).unwrap();
    assert!(
        engine
            .price(&known, &market(100.0, date(1, 2)))
            .unwrap()
            .is_finite()
    );
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
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    t.barrier_state = Some(AutocallableBarrierState::KnockedIn);
    assert!(matches!(
        engine
            .price(&SnowballOption::new(t).unwrap(), &market(70.0, date(4, 1)))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(note.barrier_state(), None);
}

#[test]
fn terminal_portfolios_preserve_signed_coupons_affine_tails_and_principal() {
    let engine = QuadratureAutocallableEngine::default();
    for principal in [0.25, 1.0] {
        for lower in [0.0, 60.0] {
            for coupon in [-4.0, 0.1] {
                let mut t = terms();
                t.effective_date = date(1, 1);
                t.expiry_date = Date::new(2026, 1, 1).unwrap();
                t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
                t.maturity_coupon_rate = 0.0;
                t.knock_in_level = 100.0;
                t.lower_strike = lower;
                t.principal_ratio = principal;
                t.observations =
                    autocallable_observations(&[t.expiry_date], &[100.0], Some(&[coupon]), None)
                        .unwrap();
                let ctx = market(100.0, date(1, 1));
                let put = |strike| {
                    AnalyticVanillaEngine
                        .price(
                            &EuropeanOption::new(
                                OptionType::Put,
                                strike,
                                t.effective_date,
                                t.expiry_date,
                            )
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
                    t.expiry_date,
                )
                .unwrap();
                let expected = principal * (-0.04_f64).exp()
                    + coupon.signum() * AnalyticDigitalEngine.price(&digital, &ctx).unwrap()
                    - (put(100.0) - if lower > 0.0 { put(lower) } else { 0.0 }) / 100.0;
                let note = SnowballOption::new(t).unwrap();
                let actual = engine.price(&note, &ctx).unwrap();
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "principal={principal}, lower={lower}, coupon={coupon}, actual={actual}, expected={expected}"
                );
            }
        }
    }
}

#[test]
fn daily_monitoring_converges_and_prices_recovery_risk() {
    let ctx = market(100.0, date(1, 1));
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
            Some(&[0.08; 3]),
            None,
        )
        .unwrap();
        let note = SnowballOption::new(t).unwrap();
        let values: Vec<_> = [(512, 8.0), (1024, 8.0), (1024, 10.0)]
            .into_iter()
            .map(|(steps, extent)| {
                QuadratureAutocallableEngine::new(QuadratureSettings {
                    asset_step_count: steps,
                    integration_standard_deviations: extent,
                })
                .unwrap()
                .price(&note, &ctx)
                .unwrap()
            })
            .collect();
        assert!((values[0] - values[1]).abs() < 1e-4, "{values:?}");
        assert!((values[1] - values[2]).abs() < 1e-4, "{values:?}");
        prices.push(values[0]);
    }
    assert!(prices[1] < prices[0] - 0.001, "{prices:?}");
}

#[test]
fn greeks_and_volatility_use_public_scalar_analysis_contract() {
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    let note = SnowballOption::new(t).unwrap();
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
    let solved = implied_volatility(
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
    assert!((solved - 0.3).abs() < 1e-6);
    assert_eq!(ctx.model_parameters().volatility(), 0.3);
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    t.barrier_state = Some(AutocallableBarrierState::None);
    t.observations[0].date = date(2, 3);
    let note = SnowballOption::new(t).unwrap();
    let result = engine
        .price_with_greeks(
            &note,
            &market(110.0, date(2, 3)),
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Theta));
    assert_eq!(result.get(Greek::Vega), Some(0.0));
    assert_eq!(result.get(Greek::Rho), Some(0.0));
    assert!(matches!(
        implied_volatility(
            &engine,
            &note,
            &market(110.0, date(2, 3)),
            result.price(),
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn prepared_pricing_rebuilds_when_the_calendar_changes_monitoring_dates() {
    let mut t = terms();
    t.expiry_date = date(1, 20);
    t.knock_in_level = 95.0;
    t.observations = autocallable_observations(
        &[date(1, 8), date(1, 20)],
        &[110.0, 105.0],
        Some(&[0.08, 0.08]),
        None,
    )
    .unwrap();
    let note = SnowballOption::new(t).unwrap();
    let engine = QuadratureAutocallableEngine::new(QuadratureSettings {
        asset_step_count: 128,
        ..Default::default()
    })
    .unwrap();
    let weekdays = market(100.0, date(1, 1));
    let all_days = weekdays
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        })
        .unwrap();
    let prepared = engine.prepare_request().unwrap().unwrap();
    for context in [&weekdays, &all_days, &weekdays] {
        assert_eq!(
            prepared.price(&note, context).unwrap(),
            engine.price(&note, context).unwrap()
        );
    }
}
