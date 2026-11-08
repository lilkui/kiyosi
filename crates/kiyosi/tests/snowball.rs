mod support;
use support::dated;

use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
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
        vec![AutocallableObservation::snowball(date(4, 1), 110.0, 0.0).unwrap()],
        Some((date(4, 1)).time_of_day()),
    )
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
fn current_settlement_preserves_strict_knock_in_and_inclusive_knock_out() {
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    t.observation_clock_time = None;
    t.barrier_state = Some(AutocallableBarrierState::NoEvent);
    t.history_time = Some(t.effective_time);
    t.observations[0] = t.observations[0]
        .replace(AutocallableObservationChanges {
            coupon_rate: Some(0.08),
            ..Default::default()
        })
        .unwrap();
    let note = SnowballOption::new(t).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    let term = 90.0 / 365.0;
    for (spot, expected) in [
        (60.0, 0.6),
        (70.0, 0.7),
        (80.0, 1.0 + 0.06 * term),
        (110.0, 1.0 + 0.08 * term),
    ] {
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
        autocallable_observations(&[date(4, 1)], &[], Some(&[0.0]), None).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let note = SnowballOption::new(terms()).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    assert!(matches!(
        engine
            .price(
                &dated(&note, &market(100.0, date(1, 2))),
                &market(100.0, date(1, 2))
            )
            .unwrap_err(),
        Error::MissingHistory(_)
    ));
    let known = note
        .replace_history(
            Some(AutocallableBarrierState::NoEvent),
            Some(note.history_time().unwrap_or(note.effective_time())),
        )
        .unwrap();
    assert!(
        engine
            .price(
                &dated(&known, &market(100.0, date(1, 2))),
                &market(100.0, date(1, 2))
            )
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
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    t.observation_clock_time = None;
    t.barrier_state = Some(AutocallableBarrierState::KnockedIn);
    t.history_time = Some(t.effective_time);
    assert!(matches!(
        engine
            .price(
                &dated(&SnowballOption::new(t).unwrap(), &market(70.0, date(4, 1))),
                &market(70.0, date(4, 1))
            )
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
                t.effective_time = date(1, 1);
                t.expiry_time = Date::new(2026, 1, 1).unwrap().start_of_day();
                t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
                t.observation_clock_time = None;
                t.maturity_coupon_rate = 0.0;
                t.knock_in_level = 100.0;
                t.lower_strike = lower;
                t.principal_redemption_ratio = principal;
                t.observations =
                    autocallable_observations(&[t.expiry_time], &[100.0], Some(&[coupon]), None)
                        .unwrap();
                let ctx = market(100.0, date(1, 1));
                let put = |strike| {
                    AnalyticEuropeanEngine
                        .price(
                            &dated(
                                &EuropeanOption::new(
                                    OptionType::Put,
                                    strike,
                                    t.effective_time,
                                    t.expiry_time,
                                )
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
                    t.expiry_time,
                )
                .unwrap();
                let expected = principal * (-0.04_f64).exp()
                    + coupon.signum()
                        * AnalyticDigitalEngine
                            .price(&dated(&digital, &ctx), &ctx)
                            .unwrap()
                    - (put(100.0) - if lower > 0.0 { put(lower) } else { 0.0 }) / 100.0;
                let note = SnowballOption::new(t).unwrap();
                let actual = engine.price(&dated(&note, &ctx), &ctx).unwrap();
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "principal={principal}, lower={lower}, coupon={coupon}, actual={actual}, expected={expected}"
                );
            }
        }
    }
}

#[test]
fn multi_observation_affine_tails_preserve_cash_asset_and_greeks() {
    let engine = QuadratureAutocallableEngine::default();
    let valuation = Timestamp::new(date(1, 2).date(), 12, 0, 0, 0).unwrap();
    let time = 4.5 / 365.0;
    for (principal, rate, dividend) in [(1.0, 0.0, 0.0), (0.25, 0.04, 0.03)] {
        for (mode, history) in [
            (KnockInObservationMode::AtExpiry, None),
            (
                KnockInObservationMode::EveryTradingDay,
                Some(AutocallableBarrierState::KnockedIn),
            ),
            (
                KnockInObservationMode::EveryTradingDay,
                Some(AutocallableBarrierState::NoEvent),
            ),
        ] {
            let mut t = terms();
            t.expiry_time = date(1, 7);
            t.lower_strike = 0.0;
            t.principal_redemption_ratio = principal;
            t.maturity_coupon_rate = 0.0;
            t.knock_in_observation_mode = mode;
            t.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
                .then_some(t.expiry_time.time_of_day());
            t.barrier_state = history;
            t.history_time = Some(t.effective_time);
            t.observations = autocallable_observations(
                &[date(1, 3), date(1, 6), date(1, 7)],
                &[110.0; 3],
                Some(&[0.0; 3]),
                None,
            )
            .unwrap();
            let note = SnowballOption::new(t).unwrap();
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
                15.0,
                valuation,
            )
            .unwrap();
            // Recovery to the knock-in, cap or knock-out levels is negligible over 4.5 days.
            let delta = (-dividend * time).exp() / 100.0;
            let expected = 15.0 * delta + (principal - 1.0) * (-rate * time).exp();
            let price = engine.price(&dated(&note, &context), &context).unwrap();
            assert!(
                (price - expected).abs() < 1e-9,
                "{mode:?}, {history:?}: {price} != {expected}"
            );
            let result = engine
                .price_with_greeks(
                    &dated(&note, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                )
                .unwrap();
            assert_eq!(result.price(), price);
            assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-8);
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
        t.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(t.expiry_time.time_of_day());
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
                .price(&dated(&note, &ctx), &ctx)
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
    t.observation_clock_time = None;
    let note = SnowballOption::new(t).unwrap();
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
    let solved = implied_volatility(
        &engine,
        &dated(&note, &ctx),
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
    assert_eq!(ctx.model_parameters().as_bsm().unwrap().volatility(), 0.3);
    let mut t = terms();
    t.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    t.observation_clock_time = None;
    t.barrier_state = Some(AutocallableBarrierState::NoEvent);
    t.history_time = Some(t.effective_time);
    t.observations[0] = t.observations[0]
        .replace(AutocallableObservationChanges {
            time: Some(date(2, 3)),
            ..Default::default()
        })
        .unwrap();
    let note = SnowballOption::new(t).unwrap();
    let result = engine
        .price_with_greeks(
            &dated(&note, &market(110.0, date(2, 3))),
            &market(110.0, date(2, 3)),
            GreeksRequest::all(),
        )
        .unwrap();
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Theta));
    assert_eq!(result.get(Greek::Vega), Some(0.0));
    assert_eq!(result.get(Greek::Rho), Some(0.0));
    assert!(matches!(
        implied_volatility(
            &engine,
            &dated(&note, &market(110.0, date(2, 3))),
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
    t.expiry_time = date(1, 20);
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
    let context = market(100.0, date(1, 1));
    let prepared = engine.prepare_request().unwrap().unwrap();
    for calendar in [
        TradingCalendar::weekdays(),
        TradingCalendar::all_days(),
        TradingCalendar::weekdays(),
    ] {
        let note = note
            .replace(SnowballOptionChanges {
                calendar: Some(calendar),
                barrier_state: Some(note.barrier_state()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            prepared.price(&dated(&note, &context), &context).unwrap(),
            engine.price(&dated(&note, &context), &context).unwrap()
        );
    }
}
