use kiyosi_core::*;

fn dates() -> (Date, Date) {
    (
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
}

fn market(spot: f64, rate: f64, dividend: f64, sigma: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        dates().0.into(),
    )
    .unwrap()
}

#[test]
fn terminal_quadrature_matches_vanilla_cash_and_asset_references() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for (direction, vanilla, cash, asset) in [
        (
            OptionType::Call,
            9.227005508154036,
            4.945810910532236,
            58.6851146134764,
        ),
        (
            OptionType::Put,
            6.330080627549918,
            4.566483334474903,
            39.33475271719913,
        ),
    ] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let digital = DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap();
        let asset_option = DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap();
        assert!((QuadratureVanillaEngine.price(&option, &context).unwrap() - vanilla).abs() < 1e-9);
        assert!((QuadratureDigitalEngine.price(&digital, &context).unwrap() - cash).abs() < 1e-9);
        assert!(
            (QuadratureDigitalEngine
                .price(&asset_option, &context)
                .unwrap()
                - asset)
                .abs()
                < 1e-9
        );
    }
}

#[test]
fn high_variance_negative_rates_and_small_widths_match_analytic_limits() {
    let (start, _) = dates();
    for (end, sigma, rate, dividend) in [
        (Date::new(2035, 1, 1).unwrap(), 3.0, 0.05, 0.02),
        (Date::new(2035, 1, 1).unwrap(), 5.0, 0.05, 0.02),
        (dates().1, 0.45, -0.03, 0.01),
        (dates().1, 0.3, 0.01, 0.07),
        (dates().1, 1e-10, 0.0, 0.0),
        (dates().1, f64::from_bits(1), 0.0, 0.0),
    ] {
        let context = market(100.0, rate, dividend, sigma);
        for direction in [OptionType::Call, OptionType::Put] {
            let vanilla = EuropeanOption::new(direction, 100.0, start, end).unwrap();
            let expected = AnalyticVanillaEngine.price(&vanilla, &context).unwrap();
            let actual = QuadratureVanillaEngine.price(&vanilla, &context).unwrap();
            assert!((actual - expected).abs() <= 1e-9 * expected.abs().max(1e-300));
            for digital in [
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                let expected = AnalyticDigitalEngine.price(&digital, &context).unwrap();
                assert!(
                    (QuadratureDigitalEngine.price(&digital, &context).unwrap() - expected).abs()
                        < 1e-8
                );
            }
        }
    }
}

#[test]
fn scaled_tails_and_underflowed_discounts_preserve_representable_payments() {
    let (start, end) = dates();
    for (ratio, vanilla, call_cash, call_asset, put_asset) in [
        (
            2392.274820537378,
            6.592380619674553e-32,
            5.3531191121506334e-33,
            1.287205586953211e-29,
            1.2806132063335364e-29,
        ),
        (
            10000.0,
            1.1367038364232515e-163,
            2.6141386421114433e-165,
            2.625505680475676e-161,
            2.6141386421114432e-161,
        ),
    ] {
        for direction in [OptionType::Call, OptionType::Put] {
            let call = direction == OptionType::Call;
            let spot = if call { 1e300 } else { 1e300 * ratio };
            let strike = if call { 1e300 * ratio } else { 1e300 };
            let context = market(spot, 0.0, 0.0, 0.2);
            let option = EuropeanOption::new(direction, strike, start, end).unwrap();
            let cash =
                DigitalOption::cash_or_nothing(direction, strike, 1e300, start, end).unwrap();
            let asset = DigitalOption::asset_or_nothing(direction, strike, start, end).unwrap();
            // Independent 100-digit Decimal references from the legacy regression suite.
            for (actual, expected) in [
                (
                    QuadratureVanillaEngine.price(&option, &context).unwrap(),
                    vanilla,
                ),
                (
                    QuadratureDigitalEngine.price(&cash, &context).unwrap(),
                    if call { call_cash } else { call_asset },
                ),
                (
                    QuadratureDigitalEngine.price(&asset, &context).unwrap(),
                    if call { call_asset } else { put_asset },
                ),
            ] {
                assert!(
                    (actual / expected - 1.0).abs() < 1e-8,
                    "{actual} != {expected}"
                );
            }
        }
    }
    for rate in [710.0_f64, 740.0, 750.0, -750.0] {
        let amount = if rate > 0.0 { 1e300_f64 } else { 1e-300_f64 };
        let context = market(amount, rate, rate, 0.2);
        let discounted = (amount.ln() - rate).exp();
        for direction in [OptionType::Call, OptionType::Put] {
            let sign = if direction == OptionType::Call {
                1.0
            } else {
                -1.0
            };
            let cash =
                DigitalOption::cash_or_nothing(direction, amount, amount, start, end).unwrap();
            let asset = DigitalOption::asset_or_nothing(direction, amount, start, end).unwrap();
            let option = EuropeanOption::new(direction, amount, start, end).unwrap();
            for (actual, expected) in [
                (
                    QuadratureDigitalEngine.price(&cash, &context).unwrap(),
                    discounted * 0.5 * libm::erfc(sign * 0.1 / 2.0_f64.sqrt()),
                ),
                (
                    QuadratureDigitalEngine.price(&asset, &context).unwrap(),
                    discounted * 0.5 * libm::erfc(-sign * 0.1 / 2.0_f64.sqrt()),
                ),
                (
                    QuadratureVanillaEngine.price(&option, &context).unwrap(),
                    discounted * libm::erf(0.1 / 2.0_f64.sqrt()),
                ),
            ] {
                assert!((actual / expected - 1.0).abs() < 1e-10);
            }
        }
    }
}

#[test]
fn expiry_is_strict_for_digitals_and_omits_all_greeks() {
    let (start, end) = dates();
    for direction in [OptionType::Call, OptionType::Put] {
        for spot in [
            libm::nextafter(100.0, 0.0),
            100.0,
            libm::nextafter(100.0, 101.0),
        ] {
            let context = market(spot, 0.05, 0.02, 0.2)
                .replace(PricingContextChanges {
                    valuation_time: Some(end.into()),
                    ..Default::default()
                })
                .unwrap();
            let sign = if direction == OptionType::Call {
                1.0
            } else {
                -1.0
            };
            let vanilla = EuropeanOption::new(direction, 100.0, start, end).unwrap();
            let result = QuadratureVanillaEngine
                .price_with_greeks(&vanilla, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            assert_eq!(result.price(), (sign * (spot - 100.0)).max(0.0));
            assert!(result.values().iter().all(Option::is_none));
            for option in [
                DigitalOption::cash_or_nothing(direction, 100.0, 7.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                let result = QuadratureDigitalEngine
                    .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert_eq!(
                    result.price(),
                    if sign * (spot - 100.0) > 0.0 {
                        option.payout().unwrap_or(spot)
                    } else {
                        0.0
                    }
                );
                assert!(result.values().iter().all(Option::is_none));
                assert!(matches!(
                    implied_volatility(
                        &QuadratureDigitalEngine,
                        &option,
                        &context,
                        result.price(),
                        Default::default()
                    )
                    .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
            }
            assert!(matches!(
                implied_volatility(
                    &QuadratureVanillaEngine,
                    &vanilla,
                    &context,
                    result.price(),
                    Default::default()
                )
                .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
        }
    }
}

#[test]
fn numerical_greeks_refine_and_implied_volatility_preserves_inputs() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2)
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2025, 6, 1).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    fn check<I: OptionContract, E: PricingEngine<I>, A: PricingEngine<I>>(
        engine: E,
        analytic: A,
        option: I,
        context: &PricingContext,
    ) {
        let expected = analytic
            .price_with_greeks(&option, context, GreeksRequest::all(), Default::default())
            .unwrap();
        let result = engine
            .price_with_greeks(&option, context, GreeksRequest::all(), Default::default())
            .unwrap();
        for greek in Greek::ALL {
            let budget = match greek {
                Greek::Theta => 3e-5,
                Greek::Delta | Greek::Vega | Greek::Rho => 1e-6,
                _ => 1e-7,
            };
            assert!(
                (result.require(greek).unwrap() - expected.require(greek).unwrap()).abs() < budget,
                "{greek:?}: actual {:?}, expected {:?}",
                result.get(greek),
                expected.get(greek)
            );
        }
        let mut previous = f64::INFINITY;
        for h in [0.2, 0.1, 0.05] {
            let result = engine
                .price_with_greeks(
                    &option,
                    context,
                    GreeksRequest::new([Greek::Delta]),
                    NumericalShiftSettings {
                        spot_shift: h,
                        ..Default::default()
                    },
                )
                .unwrap();
            let error = (result.require(Greek::Delta).unwrap()
                - expected.require(Greek::Delta).unwrap())
            .abs();
            assert!(error < previous);
            previous = error;
        }
        let quote = engine.price(&option, context).unwrap();
        let sigma = implied_volatility(
            &engine,
            &option,
            context,
            quote,
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.22,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((sigma - 0.2).abs() < 1e-7);
        assert!(matches!(
            implied_volatility(&engine, &option, context, -1.0, Default::default()).unwrap_err(),
            Error::RootNotBracketed(_)
        ));
        assert_eq!(engine.price(&option, context).unwrap(), quote);
        assert_eq!(context.model_parameters().volatility(), 0.2);
    }
    for direction in [OptionType::Call, OptionType::Put] {
        check(
            QuadratureVanillaEngine,
            AnalyticVanillaEngine,
            EuropeanOption::new(direction, 100.0, start, end).unwrap(),
            &context,
        );
        for option in [
            DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
            DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
        ] {
            check(
                QuadratureDigitalEngine,
                AnalyticDigitalEngine,
                option,
                &context,
            );
        }
    }
}

#[test]
fn applicability_validation_order_and_nonfinite_results_are_structured() {
    let (start, end) = dates();
    fn check<I: OptionContract, E: PricingEngine<I>>(engine: E, option: I) {
        let (start, end) = dates();
        let context = market(100.0, 0.05, 0.02, 0.2);
        for field in [true, false] {
            let curve = RateTermStructure::new(start.into(), vec![end.into()], vec![0.95]).unwrap();
            let parameters = if field {
                BlackScholesMertonParameters::new(curve, 0.02, 0.2)
            } else {
                BlackScholesMertonParameters::new(0.05, curve, 0.2)
            }
            .unwrap();
            let curved = context
                .replace(PricingContextChanges {
                    model_parameters: Some(parameters),
                    valuation_time: Some(end.into()),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&option, &curved).unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(matches!(
                engine
                    .price_with_greeks(
                        &option,
                        &curved,
                        GreeksRequest::default(),
                        Default::default()
                    )
                    .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
        }
        for time in [
            Date::new(2024, 12, 31).unwrap().into(),
            Timestamp::new(end, 0, 0, 0, 1).unwrap(),
        ] {
            let outside = context
                .replace(PricingContextChanges {
                    valuation_time: Some(time),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&option, &outside).unwrap_err(),
                Error::InvalidInput(_)
            ));
            assert!(matches!(
                engine
                    .price_with_greeks(
                        &option,
                        &outside,
                        GreeksRequest::default(),
                        Default::default()
                    )
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
        for shifts in [
            NumericalShiftSettings {
                spot_shift: 0.0,
                ..Default::default()
            },
            NumericalShiftSettings {
                volatility_shift: f64::NAN,
                ..Default::default()
            },
            NumericalShiftSettings {
                rate_shift: f64::INFINITY,
                ..Default::default()
            },
            NumericalShiftSettings {
                time_shift_days: 0,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                engine
                    .price_with_greeks(&option, &context, GreeksRequest::all(), shifts)
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
        let invalid_discount = market(100.0, -1000.0, -1000.0, 0.2);
        assert!(matches!(
            engine.price(&option, &invalid_discount).unwrap_err(),
            Error::ComputationFailure(_)
        ));
        assert_eq!(context.model_parameters().volatility(), 0.2);
    }
    check(
        QuadratureVanillaEngine,
        EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap(),
    );
    for option in [
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap(),
        DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap(),
    ] {
        check(QuadratureDigitalEngine, option);
    }
    let end = Date::new(2035, 1, 1).unwrap();
    let context = market(100.0, 0.05, 0.02, f64::MAX);
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let digital = DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap();
    assert!(matches!(
        QuadratureVanillaEngine
            .price(&vanilla, &context)
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert!(matches!(
        QuadratureDigitalEngine
            .price(&digital, &context)
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn feasible_bumped_failures_preserve_inputs_and_dated_flat_rates_are_supported() {
    let (start, end) = dates();
    let context = market(1e308, 0.0, -0.4, 0.2);
    let vanilla = EuropeanOption::new(OptionType::Call, 1e300, start, end).unwrap();
    let asset = DigitalOption::asset_or_nothing(OptionType::Call, 1e300, start, end).unwrap();
    let shifts = NumericalShiftSettings {
        spot_shift: 3e307,
        ..Default::default()
    };
    let vanilla_price = QuadratureVanillaEngine.price(&vanilla, &context).unwrap();
    let asset_price = QuadratureDigitalEngine.price(&asset, &context).unwrap();
    assert!(matches!(
        QuadratureVanillaEngine
            .price_with_greeks(
                &vanilla,
                &context,
                GreeksRequest::new([Greek::Delta]),
                shifts
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert!(matches!(
        QuadratureDigitalEngine
            .price_with_greeks(&asset, &context, GreeksRequest::new([Greek::Delta]), shifts)
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(
        QuadratureVanillaEngine.price(&vanilla, &context).unwrap(),
        vanilla_price
    );
    assert_eq!(
        QuadratureDigitalEngine.price(&asset, &context).unwrap(),
        asset_price
    );
    assert_eq!(context.spot_price(), 1e308);

    let parameters = BlackScholesMertonParameters::new(
        RateTermStructure::flat(0.05, Some(start.into())).unwrap(),
        RateTermStructure::flat(0.02, Some(start.into())).unwrap(),
        0.2,
    )
    .unwrap();
    let dated = PricingContext::new(parameters, 100.0, start.into()).unwrap();
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    assert!(
        (QuadratureVanillaEngine.price(&vanilla, &dated).unwrap() - 9.227005508154036).abs() < 1e-9
    );
    assert!(
        (QuadratureDigitalEngine.price(&cash, &dated).unwrap() - 4.945810910532236).abs() < 1e-9
    );
}

#[test]
fn infinite_discount_limits_preserve_zero_prices_at_small_width() {
    let (start, _) = dates();
    let end = Date::new(2035, 1, 1).unwrap();
    for (rate, dividend) in [(1e308, 1e307), (1e307, 1e308), (1e308, 1e308)] {
        for sigma in [0.2, 1e-10, f64::from_bits(1)] {
            let context = market(100.0, rate, dividend, sigma);
            for direction in [OptionType::Call, OptionType::Put] {
                let vanilla = EuropeanOption::new(direction, 100.0, start, end).unwrap();
                let cash =
                    DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap();
                let asset = DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap();
                assert_eq!(
                    QuadratureVanillaEngine.price(&vanilla, &context).unwrap(),
                    0.0
                );
                assert_eq!(QuadratureDigitalEngine.price(&cash, &context).unwrap(), 0.0);
                assert_eq!(
                    QuadratureDigitalEngine.price(&asset, &context).unwrap(),
                    0.0
                );
            }
        }
    }
}
