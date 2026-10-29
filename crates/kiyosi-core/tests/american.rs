use kiyosi_core::*;

fn day(year: i32, month: u32, day: u32) -> Date {
    Date::new(year, month, day).unwrap()
}

fn market(spot: f64, rate: f64, dividend: f64, volatility: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility).unwrap(),
        spot,
        day(2025, 1, 1).into(),
    )
    .unwrap()
}

#[test]
fn american_construction_and_replacement_validate_without_mutating() {
    let (start, end) = (day(2025, 1, 1), day(2026, 1, 1));
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    for strike in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(matches!(
            AmericanOption::new(OptionType::Call, strike, end, start).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            option
                .replace(AmericanOptionChanges {
                    strike: Some(strike),
                    ..Default::default()
                })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(matches!(
        AmericanOption::new(OptionType::Call, 100.0, end, start).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        option
            .replace(AmericanOptionChanges {
                expiry_date: Some(start),
                effective_date: Some(end),
                ..Default::default()
            })
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(option.replace(Default::default()).unwrap(), option);
    let replaced = option
        .replace(AmericanOptionChanges {
            option_type: Some(OptionType::Call),
            strike: Some(105.0),
            effective_date: Some(end),
            expiry_date: Some(end),
        })
        .unwrap();
    assert_eq!(replaced.option_type(), OptionType::Call);
    assert_eq!(replaced.strike(), 105.0);
    assert_eq!(replaced.effective_date(), end);
    assert_eq!(replaced.expiry_date(), end);
    assert_eq!(
        option,
        AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap()
    );
}

#[test]
fn american_early_exercise_and_european_limits() {
    let (start, end) = (day(2025, 1, 1), day(2026, 1, 1));
    let engine = BjerksundStenslandVanillaEngine;
    for (direction, spot, rate, dividend, expected) in [
        (OptionType::Call, 200.0, 0.03, 0.1, 100.0),
        (OptionType::Put, 50.0, 0.1, 0.03, 50.0),
    ] {
        let option = AmericanOption::new(direction, 100.0, start, end).unwrap();
        let context = market(spot, rate, dividend, 0.2);
        assert_eq!(engine.price(&option, &context).unwrap(), expected);
    }
    for (direction, rate, dividend) in [
        (OptionType::Call, 0.05, 0.0),
        (OptionType::Call, 0.05, -0.01),
        (OptionType::Put, 0.0, 0.05),
    ] {
        let american = AmericanOption::new(direction, 100.0, start, end).unwrap();
        let european = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let context = market(100.0, rate, dividend, 0.2);
        assert_eq!(
            engine.price(&american, &context).unwrap(),
            AnalyticEuropeanEngine.price(&european, &context).unwrap()
        );
    }
}

#[test]
fn american_expiry_and_exercise_window_are_inclusive() {
    let (start, end) = (day(2025, 1, 1), day(2026, 1, 1));
    let engine = BjerksundStenslandVanillaEngine;
    for direction in [OptionType::Call, OptionType::Put] {
        let option = AmericanOption::new(direction, 100.0, start, end).unwrap();
        for spot in [90.0, 100.0, 110.0] {
            let context = market(spot, -0.05, -0.03, 0.2)
                .replace(PricingContextChanges {
                    valuation_time: Some(end.into()),
                    ..Default::default()
                })
                .unwrap();
            let expected = (if direction == OptionType::Call {
                spot - 100.0
            } else {
                100.0 - spot
            })
            .max(0.0);
            let result = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            assert_eq!(result.price(), expected);
            assert!(result.values().iter().all(Option::is_none));
            assert!(matches!(
                implied_volatility(&engine, &option, &context, expected, Default::default())
                    .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
        }
        for time in [
            Timestamp::new(day(2024, 12, 31), 23, 59, 59, 999_999).unwrap(),
            Timestamp::new(end, 0, 0, 0, 1).unwrap(),
        ] {
            let context = market(100.0, 0.05, 0.03, 0.2)
                .replace(PricingContextChanges {
                    valuation_time: Some(time),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&option, &context).unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
        let context = market(100.0, 0.05, 0.03, 0.2)
            .replace(PricingContextChanges {
                valuation_time: Some(
                    Timestamp::new(day(2025, 12, 31), 23, 59, 59, 999_999).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(engine.price(&option, &context).unwrap() >= 0.0);
    }
}

#[test]
fn american_analysis_defaults_selection_solving_and_failure_state() {
    let (start, end) = (day(2025, 1, 1), day(2026, 1, 1));
    let option = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = BjerksundStenslandVanillaEngine;
    let context = market(100.0, 0.05, 0.03, 0.2);
    let original = context.clone();
    let all = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    let scalar = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::all(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(all, scalar);
    for greek in Greek::ALL {
        let result = engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([greek]),
                Default::default(),
            )
            .unwrap();
        assert_eq!(result.require(greek).unwrap(), all.require(greek).unwrap());
        assert_eq!(result.values().iter().flatten().count(), 1);
    }
    // Independent finite differences of the legacy 2002 approximation (spot h=0.01).
    assert!((all.require(Greek::Delta).unwrap() - 0.5621718020461197).abs() < 1e-7);
    assert!((all.require(Greek::Gamma).unwrap() - 0.018979671807509157).abs() < 1e-7);
    assert!((all.require(Greek::Vega).unwrap() - 0.37957057743800166).abs() < 1e-7);
    assert!(
        (implied_volatility(&engine, &option, &context, all.price(), Default::default()).unwrap()
            - 0.2)
            .abs()
            < 1e-6
    );
    for (settings, quote, expected) in [
        (
            ImpliedVolatilitySettings::default(),
            -1.0,
            (|error| matches!(error, Error::RootNotBracketed(_))) as fn(Error) -> bool,
        ),
        (
            ImpliedVolatilitySettings {
                max_iterations: 0,
                ..Default::default()
            },
            all.price(),
            (|error| matches!(error, Error::InvalidInput(_))) as fn(Error) -> bool,
        ),
        (
            ImpliedVolatilitySettings {
                max_iterations: 1,
                price_tolerance: 1e-16,
                parameter_tolerance: 1e-16,
                ..Default::default()
            },
            all.price(),
            (|error| matches!(error, Error::NonConvergence(_))) as fn(Error) -> bool,
        ),
    ] {
        assert!(expected(
            implied_volatility(&engine, &option, &context, quote, settings).unwrap_err()
        ));
    }
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    spot_shift: 0.0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(context, original);
    assert_eq!(engine.price(&option, &context).unwrap(), all.price());
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    let quote = engine.price(&put, &context).unwrap();
    assert!(matches!(
        implied_volatility(&engine, &put, &context, quote, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(
        (implied_volatility(
            &engine,
            &put,
            &context,
            quote,
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.4,
                ..Default::default()
            }
        )
        .unwrap()
            - 0.2)
            .abs()
            < 1e-6
    );
    assert_eq!(context, original);
}

#[test]
fn american_applicability_and_error_order_are_preserved() {
    let (start, end) = (day(2025, 1, 1), day(2026, 1, 1));
    let engine = BjerksundStenslandVanillaEngine;
    for direction in [OptionType::Call, OptionType::Put] {
        let option = AmericanOption::new(direction, 100.0, start, end).unwrap();
        for (r, q, vol, category) in [
            (
                -0.01,
                0.02,
                0.2,
                (|error| matches!(error, Error::UnsupportedOperation(_))) as fn(Error) -> bool,
            ),
            (
                0.05,
                0.1,
                0.02,
                (|error| matches!(error, Error::UnsupportedOperation(_))) as fn(Error) -> bool,
            ),
            (
                0.0,
                0.02,
                0.01,
                (|error| matches!(error, Error::UnsupportedOperation(_))) as fn(Error) -> bool,
            ),
            (
                0.05,
                0.03,
                1e200,
                (|error| matches!(error, Error::ComputationFailure(_))) as fn(Error) -> bool,
            ),
            (
                0.1,
                0.05,
                1e-200,
                (|error| matches!(error, Error::ComputationFailure(_))) as fn(Error) -> bool,
            ),
        ] {
            let (r, q) = if direction == OptionType::Call {
                (r, q)
            } else {
                (q, r)
            };
            let context = market(100.0, r, q, vol);
            assert!(category(engine.price(&option, &context).unwrap_err()));
            assert!(category(
                engine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Delta]),
                        Default::default()
                    )
                    .unwrap_err()
            ));
        }
        let curve = RateTermStructure::new(start.into(), vec![end.into()], vec![(-0.05_f64).exp()])
            .unwrap();
        for model in [
            BlackScholesMertonParameters::new(curve.clone(), 0.03, 0.2).unwrap(),
            BlackScholesMertonParameters::new(0.05, curve, 0.2).unwrap(),
        ] {
            let context = PricingContext::new(model, 100.0, end.into()).unwrap();
            assert!(matches!(
                engine.price(&option, &context).unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(matches!(
                engine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::default(),
                        Default::default()
                    )
                    .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
        }
    }
    let call = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let zero_rate = market(100.0, 0.0, 0.02, 0.2);
    assert!(engine.price(&call, &zero_rate).is_ok());
    assert!(matches!(
        engine
            .price_with_greeks(
                &call,
                &zero_rate,
                GreeksRequest::new([Greek::Rho]),
                Default::default()
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn american_numerical_stability_regressions() {
    let start = day(2025, 1, 1);
    let engine = BjerksundStenslandVanillaEngine;
    for direction in [OptionType::Call, OptionType::Put] {
        let call = direction == OptionType::Call;
        let option = AmericanOption::new(
            direction,
            if call { 100.0 } else { 150.0 },
            start,
            day(2035, 1, 1),
        )
        .unwrap();
        for sigma in [1e-6, 1e-10, 1e-20, 1e-100, 1e-150] {
            let context = market(
                if call { 150.0 } else { 100.0 },
                if call { 0.1 } else { 0.05 },
                if call { 0.05 } else { 0.1 },
                sigma,
            );
            assert!((engine.price(&option, &context).unwrap() - 56.25).abs() < 1e-8);
        }
        for spot in [80.0, 100.0, 120.0] {
            let price = |scale| {
                let option =
                    AmericanOption::new(direction, 100.0 * scale, start, day(2026, 1, 1)).unwrap();
                engine
                    .price(&option, &market(spot * scale, 0.05, 0.03, 0.2))
                    .unwrap()
                    / scale
            };
            let base = price(1.0);
            for scale in [1e155, 1e-170, 1e-200] {
                assert!((price(scale) - base).abs() < 1e-9);
            }
        }
        let option = AmericanOption::new(
            direction,
            if call { 1e304 } else { 1e300 },
            start,
            day(2026, 1, 1),
        )
        .unwrap();
        let value = engine
            .price(
                &option,
                &market(if call { 1e300 } else { 1e304 }, 0.0, 0.0, 0.2),
            )
            .unwrap();
        assert!((value / 1.1367038364232515e-163 - 1.0).abs() < 1e-8);
    }
}

#[test]
fn american_price_preserves_2002_regression() {
    let start = day(2025, 1, 1);
    let end = day(2026, 1, 1);
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.03, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    for (direction, expected) in [
        (OptionType::Call, 8.652695436032069),
        (OptionType::Put, 6.932464292493037),
    ] {
        let option = AmericanOption::new(direction, 100.0, start, end).unwrap();
        let price = BjerksundStenslandVanillaEngine
            .price(&option, &context)
            .unwrap();
        assert!((price - expected).abs() < 1e-11, "{direction:?}: {price}");
    }
}
