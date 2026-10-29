use kiyosi_core::{
    AnalyticVanillaEngine, BlackScholesMertonParameters, Date, Error, EuropeanOption, OptionType,
    PricingContext, Timestamp, TradingCalendar,
};

#[test]
fn european_call_and_put_match_independent_bsm_prices() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    let parameters = BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap();
    let context = PricingContext::new(parameters, 100.0, start.into()).unwrap();
    for (direction, expected) in [
        (OptionType::Call, 9.227005508154036),
        (OptionType::Put, 6.330080627549918),
    ] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let price = AnalyticVanillaEngine.price(&option, &context).unwrap();
        assert!((price - expected).abs() < 1e-12, "{price} != {expected}");
    }
}

#[test]
fn valuation_uses_microseconds_and_expiry_is_midnight() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let at = Timestamp::new(start, 12, 9, 10, 123456).unwrap();
    assert_eq!(at.date(), start);
    assert_eq!(at.micros_since_midnight(), 43_750_123_456);
    let context = PricingContext::new(model.clone(), 110.0, at).unwrap();
    assert_eq!(context.calendar(), &TradingCalendar::default());
    assert_eq!(context.calendar().trading_days_per_year(), 252);
    let earlier = PricingContext::new(model.clone(), 110.0, start.into()).unwrap();
    assert!(
        AnalyticVanillaEngine.price(&option, &context).unwrap()
            < AnalyticVanillaEngine.price(&option, &earlier).unwrap()
    );
    let expired = PricingContext::new(model.clone(), 110.0, end.into()).unwrap();
    assert_eq!(
        AnalyticVanillaEngine.price(&option, &expired).unwrap(),
        10.0
    );
    for at in [
        Date::new(2024, 12, 31).unwrap().into(),
        Timestamp::new(end, 0, 0, 0, 1).unwrap(),
    ] {
        let outside = PricingContext::new(model.clone(), 110.0, at).unwrap();
        assert!(matches!(
            AnalyticVanillaEngine.price(&option, &outside).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let same_day = EuropeanOption::new(OptionType::Put, 120.0, end, end).unwrap();
    assert_eq!(
        AnalyticVanillaEngine.price(&same_day, &expired).unwrap(),
        10.0
    );
}

#[test]
fn invalid_domain_values_are_rejected_before_construction() {
    for date in [(2025, 2, 29), (2024, 13, 1), (-32768, 1, 1), (32768, 1, 1)] {
        assert!(matches!(
            Date::new(date.0, date.1, date.2).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for year in [-32767, -1, 0, 1600, 2500, 9999, 32767] {
        let day = Date::new(year, 1, 1).unwrap();
        assert_eq!(Timestamp::new(day, 23, 59, 59, 999999).unwrap().date(), day);
    }
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            EuropeanOption::new(OptionType::Call, bad, start, end).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            BlackScholesMertonParameters::new(0.0, 0.0, bad).unwrap_err(),
            Error::InvalidInput(_)
        ));
        let model = BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap();
        assert!(matches!(
            PricingContext::new(model, bad, start.into()).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            BlackScholesMertonParameters::new(bad, 0.0, 0.2).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            BlackScholesMertonParameters::new(0.0, bad, 0.2).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(BlackScholesMertonParameters::new(-0.05, -0.02, 0.2).is_ok());
    assert!(matches!(
        EuropeanOption::new(OptionType::Put, 100.0, end, start).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        Timestamp::new(start, 24, 0, 0, 0).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn analytic_regressions_retain_tail_prices_and_extreme_scales() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    // Independent high-precision references from the existing analytic regression cases.
    for (direction, spot, strike, sigma, expected, tolerance) in [
        (
            OptionType::Call,
            100.0,
            100.00000000013,
            1e-13,
            4.645_908_059_169_899e-51,
            1e-10,
        ),
        (
            OptionType::Put,
            100.0,
            99.99999999987,
            1e-13,
            4.645908058131258e-51,
            1e-10,
        ),
        (
            OptionType::Call,
            100.0,
            100.00099900499002,
            1e-6,
            8.276374696508508e-29,
            1e-10,
        ),
        (
            OptionType::Call,
            100.0,
            100.00100100501001,
            1e-6,
            6.749830430787083e-29,
            1e-10,
        ),
        (
            OptionType::Put,
            100.0,
            99.99700004499955,
            1e-6,
            1.6319322561070434e-203,
            1e-10,
        ),
        (
            OptionType::Call,
            1e300,
            1e304,
            0.2,
            1.1367038364232515e-163,
            1e-8,
        ),
        (
            OptionType::Put,
            1e304,
            1e300,
            0.2,
            1.1367038364232515e-163,
            1e-8,
        ),
        (
            OptionType::Call,
            100.0,
            100.0,
            1e-200,
            3.989422804014327e-199,
            2e-12,
        ),
        (
            OptionType::Put,
            100.0,
            100.0,
            1e-200,
            3.989422804014327e-199,
            2e-12,
        ),
    ] {
        let model = BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap();
        let context = PricingContext::new(model, spot, start.into()).unwrap();
        let option = EuropeanOption::new(direction, strike, start, end).unwrap();
        let price = AnalyticVanillaEngine.price(&option, &context).unwrap();
        assert!(
            (price / expected - 1.0).abs() < tolerance,
            "{price} != {expected}"
        );
    }
    for (direction, expected) in [
        (OptionType::Call, 98.01986733067552),
        (OptionType::Put, 95.1229424500714),
    ] {
        let model = BlackScholesMertonParameters::new(0.05, 0.02, 1e308).unwrap();
        let context = PricingContext::new(model, 100.0, start.into()).unwrap();
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        assert!((AnalyticVanillaEngine.price(&option, &context).unwrap() - expected).abs() < 1e-12);
    }
    let model = BlackScholesMertonParameters::new(-1000.0, 0.0, 0.3).unwrap();
    let context = PricingContext::new(model, 100.0, start.into()).unwrap();
    let option = EuropeanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    assert!(matches!(
        AnalyticVanillaEngine.price(&option, &context).unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn owned_values_allow_shared_and_distinct_instance_concurrent_pricing() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EuropeanOption>();
    assert_send_sync::<PricingContext>();
    assert_send_sync::<AnalyticVanillaEngine>();
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let engine = AnalyticVanillaEngine;
    std::thread::scope(|scope| {
        let shared = scope.spawn(|| engine.price(&option, &context).unwrap());
        let owned = context.clone();
        let owned_option = option.clone();
        let distinct =
            scope.spawn(move || AnalyticVanillaEngine.price(&owned_option, &owned).unwrap());
        assert_eq!(shared.join().unwrap(), distinct.join().unwrap());
    });
    assert_eq!(context.spot_price(), 100.0);
}

#[test]
fn time_scaled_subnormal_volatility_preserves_positive_value() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2025, 1, 2).unwrap();
    let model = BlackScholesMertonParameters::new(0.0, 0.0, f64::from_bits(1)).unwrap();
    let context = PricingContext::new(model, 1e308, start.into()).unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 1e308, start, end).unwrap();
        // Existing small-width reference: S * sigma * sqrt(1/365) / sqrt(2*pi).
        let expected = 1.031_687_789_452_394_2e-17;
        let actual = AnalyticVanillaEngine.price(&option, &context).unwrap();
        assert!((actual / expected - 1.0).abs() < 2e-12, "{actual}");
    }
}
