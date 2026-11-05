use kiyosi::*;

fn day(year: i32, month: u32, day: u32) -> Timestamp {
    Date::new(year, month, day).unwrap().start_of_day()
}

fn market(spot: f64, rate: f64, dividend: f64, volatility: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility).unwrap(),
        spot,
        day(2025, 1, 1),
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
                expiry_time: Some(start),
                effective_time: Some(end),
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
            effective_time: Some(end),
            expiry_time: Some(end),
        })
        .unwrap();
    assert_eq!(replaced.option_type(), OptionType::Call);
    assert_eq!(replaced.strike(), 105.0);
    assert_eq!(replaced.effective_time(), end);
    assert_eq!(replaced.expiry_time(), end);
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
                    valuation_time: Some(end),
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
                .price_with_greeks(&option, &context, GreeksRequest::all())
                .unwrap();
            assert_eq!(result.price(), expected);
            assert_eq!(result.get(Greek::Theta), None);
            assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
            if (spot - 100.0).abs() <= 0.01 {
                assert_eq!(result.get(Greek::Delta), None);
            } else {
                let delta = if expected == 0.0 {
                    0.0
                } else if direction == OptionType::Call {
                    1.0
                } else {
                    -1.0
                };
                assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-10);
            }
            assert!(matches!(
                implied_volatility(&engine, &option, &context, expected, Default::default())
                    .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
        }
        for time in [
            Timestamp::new(day(2024, 12, 31).date(), 23, 59, 59, 999_999).unwrap(),
            Timestamp::new(end.date(), 0, 0, 0, 1).unwrap(),
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
                    Timestamp::new(day(2025, 12, 31).date(), 23, 59, 59, 999_999).unwrap(),
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
        .price_with_greeks(&option, &context, GreeksRequest::all())
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
            .price_with_greeks(&option, &context, GreeksRequest::new([greek]))
            .unwrap();
        assert_eq!(result.require(greek).unwrap(), all.require(greek).unwrap());
        assert_eq!(result.values().iter().flatten().count(), 1);
    }
    // Independent finite differences of the Bjerksund–Stensland 2002 approximation (spot h=0.01).
    assert!((all.require(Greek::Delta).unwrap() - 0.562_171_802_046_119_7).abs() < 1e-7);
    assert!((all.require(Greek::Gamma).unwrap() - 0.018_979_671_807_509_157).abs() < 1e-7);
    assert!((all.require(Greek::Vega).unwrap() - 37.957_057_743_800_16).abs() < 1e-7);
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
            .price_with_greeks(&option, &context, GreeksRequest::new([]))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        kiyosi::numerical_greeks(
            &engine,
            &option,
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
                    .price_with_greeks(&option, &context, GreeksRequest::new([Greek::Delta]))
                    .unwrap_err()
            ));
        }
        let curve = RateTermStructure::new(
            start,
            vec![end, (end).add_days(1).unwrap()],
            vec![(-0.05_f64).exp(), ((-0.05_f64).exp()) * 0.99],
        )
        .unwrap();
        for model in [
            BlackScholesMertonParameters::new(curve.clone(), 0.03, 0.2).unwrap(),
            BlackScholesMertonParameters::new(0.05, curve, 0.2).unwrap(),
        ] {
            let context = PricingContext::new(model, 100.0, end).unwrap();
            assert!(matches!(
                engine.price(&option, &context).unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(matches!(
                engine
                    .price_with_greeks(&option, &context, GreeksRequest::new([]))
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
            .price_with_greeks(&call, &zero_rate, GreeksRequest::new([Greek::Rho]))
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
        assert!((value / 1.136_703_836_423_251_5e-163 - 1.0).abs() < 1e-8);
    }
}
