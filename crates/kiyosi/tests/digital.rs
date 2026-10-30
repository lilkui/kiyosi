use kiyosi::*;

fn dates() -> (Date, Date) {
    (
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
}

fn market(spot: f64, rate: f64, dividend: f64, volatility: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility).unwrap(),
        spot,
        dates().0.into(),
    )
    .unwrap()
}

#[test]
fn cash_and_asset_prices_match_independent_quantlib_values() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for (direction, cash, asset) in [
        (OptionType::Call, 4.945810910532236, 58.6851146134764),
        (OptionType::Put, 4.566483334474903, 39.33475271719913),
    ] {
        let options = [
            (
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                cash,
            ),
            (
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
                asset,
            ),
        ];
        for (option, expected) in options {
            assert!(
                (AnalyticDigitalEngine.price(&option, &context).unwrap() - expected).abs() < 1e-12
            );
        }
    }
}

#[test]
fn analytic_delta_gamma_and_completed_greeks_preserve_selections() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for option in [
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap(),
        DigitalOption::asset_or_nothing(OptionType::Put, 100.0, start, end).unwrap(),
    ] {
        let full = AnalyticDigitalEngine
            .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
            .unwrap();
        for greek in Greek::ALL {
            let selected = AnalyticDigitalEngine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([greek]),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                selected.require(greek).unwrap(),
                full.require(greek).unwrap()
            );
            assert_eq!(selected.values().iter().flatten().count(), 1);
        }
        let numeric = numerical_greeks(
            &AnalyticDigitalEngine,
            &option,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
        for greek in Greek::ALL {
            assert!(
                (numeric.require(greek).unwrap() - full.require(greek).unwrap()).abs() < 1e-4,
                "{greek:?}"
            );
        }
        let sigma = implied_volatility(
            &AnalyticDigitalEngine,
            &option,
            &context,
            full.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.22,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((sigma - 0.2).abs() < 1e-7);
    }
}

#[test]
fn expiry_is_strict_and_valuation_life_is_inclusive_to_the_microsecond() {
    let (start, end) = dates();
    for direction in [OptionType::Call, OptionType::Put] {
        for spot in [
            99.0,
            f64::from_bits(100.0_f64.to_bits() - 1),
            100.0,
            f64::from_bits(100.0_f64.to_bits() + 1),
            101.0,
        ] {
            let context = market(spot, 0.05, 0.02, 0.2)
                .replace(PricingContextChanges {
                    valuation_time: Some(end.into()),
                    ..Default::default()
                })
                .unwrap();
            for option in [
                DigitalOption::cash_or_nothing(direction, 100.0, 7.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                let pays = if direction == OptionType::Call {
                    spot > 100.0
                } else {
                    spot < 100.0
                };
                let expected = if pays {
                    option.payout().unwrap_or(spot)
                } else {
                    0.0
                };
                for result in [
                    AnalyticDigitalEngine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap(),
                    numerical_greeks(
                        &AnalyticDigitalEngine,
                        &option,
                        &context,
                        GreeksRequest::all(),
                        Default::default(),
                    )
                    .unwrap(),
                ] {
                    assert_eq!(result.price(), expected);
                    assert!(result.values().iter().all(Option::is_none));
                }
                assert!(matches!(
                    implied_volatility(
                        &AnalyticDigitalEngine,
                        &option,
                        &context,
                        expected,
                        Default::default()
                    )
                    .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
                for at in [
                    Timestamp::new(Date::new(2024, 12, 31).unwrap(), 23, 59, 59, 999999).unwrap(),
                    Timestamp::new(end, 0, 0, 0, 1).unwrap(),
                ] {
                    let outside = context
                        .replace(PricingContextChanges {
                            valuation_time: Some(at),
                            ..Default::default()
                        })
                        .unwrap();
                    assert!(matches!(
                        AnalyticDigitalEngine.price(&option, &outside).unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                    assert!(matches!(
                        AnalyticDigitalEngine
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
            }
        }
    }
}

#[test]
fn constructors_and_replacements_validate_before_returning_owned_values() {
    let (start, end) = dates();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let asset = DigitalOption::asset_or_nothing(OptionType::Put, 100.0, start, end).unwrap();
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            DigitalOption::cash_or_nothing(OptionType::Call, 0.0, bad, end, start).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            DigitalOption::asset_or_nothing(OptionType::Call, bad, start, end).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            cash.replace(DigitalOptionChanges {
                payout: Some(bad),
                ..Default::default()
            })
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        for option in [&cash, &asset] {
            assert!(matches!(
                option
                    .replace(DigitalOptionChanges {
                        strike: Some(bad),
                        ..Default::default()
                    })
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
    }
    for option in [&cash, &asset] {
        assert_eq!(option.replace(Default::default()).unwrap(), *option);
        assert!(matches!(
            option
                .replace(DigitalOptionChanges {
                    effective_date: Some(end),
                    expiry_date: Some(start),
                    ..Default::default()
                })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        let updated = option
            .replace(DigitalOptionChanges {
                option_type: Some(OptionType::Put),
                strike: Some(105.0),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(updated.option_type(), OptionType::Put);
        assert_eq!(updated.strike(), 105.0);
        assert_eq!(updated.payout(), option.payout());
        assert_eq!(option.strike(), 100.0);
        assert_eq!(option.effective_date(), start);
        assert_eq!(option.expiry_date(), end);
    }
    assert_eq!(cash.payout(), Some(10.0));
    assert!(matches!(
        asset
            .replace(DigitalOptionChanges {
                payout: Some(10.0),
                ..Default::default()
            })
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn log_scaled_prices_survive_discount_and_settlement_overflow_and_underflow() {
    let (start, end) = dates();
    for rate in [710.0, 740.0, 750.0, -710.0, -750.0] {
        let amount = if rate > 0.0 { 1e300_f64 } else { 1e-300_f64 };
        let context = market(amount, rate, rate, 0.2);
        let scale = (amount.ln() - rate).exp();
        for (direction, cash_probability, asset_probability) in [
            (OptionType::Call, 0.460172162722971, 0.539827837277029),
            (OptionType::Put, 0.539827837277029, 0.460172162722971),
        ] {
            for (option, probability) in [
                (
                    DigitalOption::cash_or_nothing(direction, amount, amount, start, end).unwrap(),
                    cash_probability,
                ),
                (
                    DigitalOption::asset_or_nothing(direction, amount, start, end).unwrap(),
                    asset_probability,
                ),
            ] {
                let actual = AnalyticDigitalEngine.price(&option, &context).unwrap();
                assert!((actual / (scale * probability) - 1.0).abs() < 2e-12);
            }
        }
    }
    for direction in [OptionType::Call, OptionType::Put] {
        let strike = if direction == OptionType::Call {
            1e-304
        } else {
            1e-296
        };
        let context = market(1e-300, -710.0, -710.0, 0.2);
        for option in [
            DigitalOption::cash_or_nothing(direction, strike, 1e-300, start, end).unwrap(),
            DigitalOption::asset_or_nothing(direction, strike, start, end).unwrap(),
        ] {
            let price = AnalyticDigitalEngine.price(&option, &context).unwrap();
            assert!((price / (1e-300_f64.ln() + 710.0).exp() - 1.0).abs() < 1e-12);
        }
    }
}

#[test]
fn selected_greeks_preserve_finite_results_and_tiny_volatility_gamma() {
    let (start, end) = dates();
    let option =
        DigitalOption::cash_or_nothing(OptionType::Call, 1e-200, 1e100, start, end).unwrap();
    let context = market(1e-200, 0.0, 0.0, 0.2);
    let selected = AnalyticDigitalEngine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta]),
            Default::default(),
        )
        .unwrap();
    assert!((selected.require(Greek::Delta).unwrap() / 1.984762737385059e300 - 1.0).abs() < 1e-12);
    assert_eq!(selected.get(Greek::Gamma), None);
    assert!(matches!(
        AnalyticDigitalEngine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Gamma]),
                Default::default()
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    for sigma in [1e-200, f64::from_bits(1)] {
        let context = market(100.0, 0.0, 0.0, sigma)
            .replace(PricingContextChanges {
                valuation_time: Some(
                    Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999999).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        for (option, expected, sign) in [
            (
                DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap(),
                5.0,
                -1.0,
            ),
            (
                DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap(),
                50.0,
                1.0,
            ),
        ] {
            assert_eq!(
                AnalyticDigitalEngine.price(&option, &context).unwrap(),
                expected
            );
            let gamma = AnalyticDigitalEngine.price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Gamma]),
                Default::default(),
            );
            if sigma == 1e-200 {
                let value = gamma.unwrap().require(Greek::Gamma).unwrap();
                assert!(value.is_finite() && value.signum() == sign && value.abs() > 1e200);
            } else {
                assert!(matches!(gamma.unwrap_err(), Error::ComputationFailure(_)));
            }
        }
    }
}

#[test]
fn speed_uses_native_gamma_at_large_price_scales() {
    let (start, end) = dates();
    let context = market(100000.0, 0.05, 0.02, 0.2);
    for (option, expected) in [
        (
            DigitalOption::cash_or_nothing(OptionType::Call, 100000.0, 1e8, start, end).unwrap(),
            -4.204659661267558e-6,
        ),
        (
            DigitalOption::asset_or_nothing(OptionType::Call, 100000.0, start, end).unwrap(),
            -4.631047683255255e-9,
        ),
    ] {
        let result = AnalyticDigitalEngine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Speed]),
                Default::default(),
            )
            .unwrap();
        assert!((result.require(Greek::Speed).unwrap() / expected - 1.0).abs() < 1e-7);
    }
}

#[test]
fn native_gamma_speed_needs_only_a_single_positive_spot_bump() {
    let (start, end) = dates();
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 1.5, 1.0, start, end).unwrap();
    let context = market(1.5, 0.05, 0.02, 0.2);
    let settings = NumericalShiftSettings {
        spot_shift: 1.0,
        ..Default::default()
    };
    let result = AnalyticDigitalEngine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Speed]),
            settings,
        )
        .unwrap();
    assert!((result.require(Greek::Speed).unwrap() - -0.07172406990163582).abs() < 1e-14);
    assert_eq!(
        numerical_greeks(
            &AnalyticDigitalEngine,
            &option,
            &context,
            GreeksRequest::new([Greek::Speed]),
            settings
        )
        .unwrap()
        .get(Greek::Speed),
        None
    );
}

#[test]
fn itm_and_otm_native_greeks_match_quantlib() {
    let (start, end) = dates();
    // QuantLib 1.43 AnalyticEuropeanEngine, Actual/365 Fixed, same market as Python.
    for (spot, asset, direction, price, delta, gamma) in [
        (
            80.0,
            false,
            OptionType::Call,
            1.3628798953670263,
            0.13441552679922802,
            0.007272869269308953,
        ),
        (
            80.0,
            false,
            OptionType::Put,
            8.149414349640113,
            -0.13441552679922802,
            -0.007272869269308953,
        ),
        (
            80.0,
            true,
            OptionType::Call,
            15.159555075504228,
            1.5336497064360832,
            0.08953063354299304,
        ),
        (
            80.0,
            true,
            OptionType::Put,
            63.25633878903621,
            -0.5534510331293278,
            -0.08953063354299304,
        ),
        (
            120.0,
            false,
            OptionType::Call,
            7.913055660794069,
            0.09958417269992391,
            -0.004819906257017575,
        ),
        (
            120.0,
            false,
            OptionType::Put,
            1.5992385842130703,
            -0.09958417269992391,
            0.004819906257017575,
        ),
        (
            120.0,
            true,
            OptionType::Call,
            103.19170024752152,
            1.8557725623952517,
            -0.03990038151184874,
        ),
        (
            120.0,
            true,
            OptionType::Put,
            14.43214054928911,
            -0.8755738890884963,
            0.03990038151184874,
        ),
    ] {
        let option = if asset {
            DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap()
        } else {
            DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap()
        };
        let result = AnalyticDigitalEngine
            .price_with_greeks(
                &option,
                &market(spot, 0.05, 0.02, 0.2),
                GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                Default::default(),
            )
            .unwrap();
        assert!((result.price() - price).abs() < 1e-12);
        assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-12);
        assert!((result.require(Greek::Gamma).unwrap() - gamma).abs() < 1e-12);
    }
}

#[test]
fn asset_delta_retains_independent_high_precision_tail_values() {
    let (start, end) = dates();
    for (direction, log_strike, expected) in [
        (OptionType::Call, 8.0_f64, 8.923870780328184e-38),
        (OptionType::Put, -8.0_f64, -2.9638565030696756e-41),
    ] {
        for rate in [-710.0_f64, -700.0] {
            for spot in [1.0, 1e-200] {
                let option =
                    DigitalOption::asset_or_nothing(direction, spot * log_strike.exp(), start, end)
                        .unwrap();
                let result = AnalyticDigitalEngine
                    .price_with_greeks(
                        &option,
                        &market(spot, rate, rate, 0.2),
                        GreeksRequest::new([Greek::Delta]),
                        Default::default(),
                    )
                    .unwrap();
                assert!(
                    (result.require(Greek::Delta).unwrap() / (expected * (-rate - 710.0).exp())
                        - 1.0)
                        .abs()
                        < 1e-10
                );
            }
        }
    }
}

#[test]
fn digital_analysis_preserves_settings_solver_and_curve_horizon_failures() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for option in [
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap(),
        DigitalOption::asset_or_nothing(OptionType::Put, 100.0, start, end).unwrap(),
    ] {
        let quote = AnalyticDigitalEngine.price(&option, &context).unwrap();
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            for settings in [
                NumericalShiftSettings {
                    spot_shift: bad,
                    ..Default::default()
                },
                NumericalShiftSettings {
                    volatility_shift: bad,
                    ..Default::default()
                },
                NumericalShiftSettings {
                    rate_shift: bad,
                    ..Default::default()
                },
            ] {
                assert!(matches!(
                    AnalyticDigitalEngine
                        .price_with_greeks(&option, &context, GreeksRequest::all(), settings)
                        .unwrap_err(),
                    Error::InvalidInput(_)
                ));
            }
        }
        for quote in [-1.0, 1000.0] {
            assert!(matches!(
                implied_volatility(
                    &AnalyticDigitalEngine,
                    &option,
                    &context,
                    quote,
                    Default::default()
                )
                .unwrap_err(),
                Error::RootNotBracketed(_)
            ));
        }
        assert!(matches!(
            implied_volatility(
                &AnalyticDigitalEngine,
                &option,
                &context,
                f64::NAN,
                Default::default()
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        let curve = RateTermStructure::new(
            start.into(),
            vec![Date::new(2025, 6, 1).unwrap().into()],
            vec![0.99],
        )
        .unwrap();
        let short = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    context
                        .model_parameters()
                        .replace(BlackScholesMertonParameterChanges {
                            dividend_yield: Some(curve.into()),
                            ..Default::default()
                        })
                        .unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            AnalyticDigitalEngine.price(&option, &short).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            implied_volatility(
                &AnalyticDigitalEngine,
                &option,
                &short,
                quote,
                Default::default()
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert_eq!(context.model_parameters().volatility(), 0.2);
        assert_eq!(
            AnalyticDigitalEngine.price(&option, &context).unwrap(),
            quote
        );
    }
}

#[test]
fn expiry_reaching_time_stencils_use_paired_scalar_fallback() {
    let (start, end) = dates();
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 1.0, 10.0, start, end).unwrap();
    let context = market(1.0, 0.05, 0.02, 0.2)
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2025, 12, 31).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    let request = GreeksRequest::new([Greek::Charm, Greek::Color]);
    let actual = AnalyticDigitalEngine
        .price_with_greeks(&option, &context, request, Default::default())
        .unwrap();
    // Existing scalar stencil: both legs use price differences when one reaches expiry.
    assert!((actual.require(Greek::Charm).unwrap() - 124.8761593919428).abs() < 1e-9);
    assert!((actual.require(Greek::Color).unwrap() - 50137.33821086929).abs() < 1e-7);
    let numeric = numerical_greeks(
        &AnalyticDigitalEngine,
        &option,
        &context,
        request,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        actual.require(Greek::Charm).unwrap(),
        numeric.require(Greek::Charm).unwrap()
    );
    assert_eq!(
        actual.require(Greek::Color).unwrap(),
        numeric.require(Greek::Color).unwrap()
    );
}
