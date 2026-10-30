use kiyosi_core::*;

fn dates() -> (Date, Date) {
    (
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
    )
}
fn market(at: Date) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        at.into(),
    )
    .unwrap()
}

#[test]
fn continuous_asian_prices_match_existing_independent_references() {
    let (start, end) = dates();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    assert!(
        (AnalyticGeometricAveragePriceEngine
            .price(&geometric, &market(start))
            .unwrap()
            - 5.035867546515384)
            .abs()
            < 1e-12
    );
    assert!(
        (TurnbullWakemanArithmeticAveragePriceEngine
            .price(&arithmetic, &market(start))
            .unwrap()
            - 5.246501860362777)
            .abs()
            < 1e-12
    );
}

macro_rules! contract_tests {
    ($module:ident, $option:ident, $engine:ident) => {
        mod $module {
            use super::*;
            #[test]
            fn construction_replacement_and_validation() {
                let (start, end) = dates();
                for strike in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                    assert!(matches!(
                        $option::new(OptionType::Call, strike, start, start, end, 0.0).unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                for (strike, average) in [(0.0, -1.0), (100.0, -1.0), (100.0, 0.0)] {
                    assert!(matches!(
                        $option::new(OptionType::Call, strike, end, start, start, average)
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                for average in [-1.0, f64::NAN, f64::INFINITY] {
                    assert!(matches!(
                        $option::new(OptionType::Call, 100.0, start, start, end, average)
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                assert!(matches!(
                    $option::new(
                        OptionType::Call,
                        100.0,
                        start,
                        Date::new(2026, 1, 2).unwrap(),
                        end,
                        0.0
                    )
                    .unwrap_err(),
                    Error::InvalidInput(_)
                ));
                let original = $option::new(
                    OptionType::Put,
                    100.0,
                    start,
                    start,
                    end,
                    DEFAULT_REALIZED_AVERAGE,
                )
                .unwrap();
                let updated = original.replace(110.0).unwrap();
                assert_eq!(updated.option_type(), OptionType::Put);
                assert_eq!(updated.strike(), 100.0);
                assert_eq!(updated.effective_date(), start);
                assert_eq!(updated.averaging_start_date(), start);
                assert_eq!(updated.expiry_date(), end);
                assert_eq!(updated.realized_average(), 110.0);
                assert!(matches!(
                    original.replace(-1.0).unwrap_err(),
                    Error::InvalidInput(_)
                ));
                assert_eq!(original.realized_average(), DEFAULT_REALIZED_AVERAGE);
            }
            #[test]
            fn history_life_single_fixing_and_expiry_boundaries() {
                let (start, end) = dates();
                let averaging = Date::new(2025, 2, 1).unwrap();
                let original =
                    $option::new(OptionType::Call, 100.0, start, averaging, end, 0.0).unwrap();
                let seasoned = original.replace(110.0).unwrap();
                for at in [start, averaging] {
                    assert!($engine.price(&original, &market(at)).is_ok());
                    assert!(matches!(
                        $engine.price(&seasoned, &market(at)).unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                for at in [Date::new(2025, 6, 1).unwrap(), end] {
                    assert!($engine.price(&seasoned, &market(at)).is_ok());
                    assert!(matches!(
                        $engine.price(&original, &market(at)).unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                for at in [
                    Date::new(2024, 12, 31).unwrap(),
                    Date::new(2026, 1, 2).unwrap(),
                ] {
                    assert!(matches!(
                        $engine.price(&seasoned, &market(at)).unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                let after = market(end)
                    .replace(PricingContextChanges {
                        valuation_time: Some(Timestamp::new(end, 0, 0, 0, 1).unwrap()),
                        ..Default::default()
                    })
                    .unwrap();
                assert!(matches!(
                    $engine.price(&seasoned, &after).unwrap_err(),
                    Error::InvalidInput(_)
                ));
                assert_eq!($engine.price(&seasoned, &market(end)).unwrap(), 10.0);
                let settled = $engine
                    .price_with_greeks(
                        &seasoned,
                        &market(end),
                        GreeksRequest::all(),
                        Default::default(),
                    )
                    .unwrap();
                assert!(settled.values().iter().all(Option::is_none));
                assert!(matches!(
                    implied_volatility(&$engine, &seasoned, &market(end), 10.0, Default::default())
                        .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
                for direction in [OptionType::Call, OptionType::Put] {
                    let single = $option::new(direction, 100.0, start, end, end, 0.0).unwrap();
                    let vanilla = EuropeanOption::new(direction, 100.0, start, end).unwrap();
                    for at in [start, Date::new(2025, 12, 31).unwrap(), end] {
                        assert_eq!(
                            $engine.price(&single, &market(at)).unwrap(),
                            AnalyticEuropeanEngine.price(&vanilla, &market(at)).unwrap()
                        );
                    }
                    assert!(matches!(
                        $engine
                            .price(&single.replace(110.0).unwrap(), &market(end))
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
            }
            #[test]
            fn greeks_solver_selection_errors_and_history_stencils() {
                let (start, end) = dates();
                let option = $option::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
                let context = market(start);
                let full = $engine
                    .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert!(full.require(Greek::Delta).unwrap() > 0.0);
                assert!(full.require(Greek::Vega).unwrap() > 0.0);
                for g in [Greek::Theta, Greek::Charm, Greek::Color] {
                    assert_eq!(full.get(g), None);
                }
                let scalar = numerical_greeks(
                    &$engine,
                    &option,
                    &context,
                    GreeksRequest::all(),
                    Default::default(),
                )
                .unwrap();
                assert_eq!(full, scalar);
                for greek in Greek::ALL {
                    let selected = $engine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::new([greek, greek]),
                            Default::default(),
                        )
                        .unwrap();
                    assert_eq!(selected.get(greek), full.get(greek));
                    assert!(
                        Greek::ALL
                            .into_iter()
                            .filter(|g| *g != greek)
                            .all(|g| selected.get(g).is_none())
                    );
                }
                assert!(
                    (implied_volatility(
                        &$engine,
                        &option,
                        &context,
                        full.price(),
                        Default::default()
                    )
                    .unwrap()
                        - 0.2)
                        .abs()
                        < 1e-7
                );
                assert!(matches!(
                    $engine
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
                    $engine
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
                assert!(matches!(
                    implied_volatility(&$engine, &option, &context, -1.0, Default::default())
                        .unwrap_err(),
                    Error::RootNotBracketed(_)
                ));
                let seasoned = option.replace(100.0).unwrap();
                let middle = market(Date::new(2025, 7, 1).unwrap());
                let result = $engine
                    .price_with_greeks(&seasoned, &middle, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert!(result.values().iter().all(Option::is_some));
                assert_eq!(option.realized_average(), 0.0);
                assert_eq!(context.model_parameters().volatility(), 0.2);
            }
        }
    };
}
contract_tests!(
    geometric,
    GeometricAveragePriceOption,
    AnalyticGeometricAveragePriceEngine
);
contract_tests!(
    arithmetic,
    ArithmeticAveragePriceOption,
    TurnbullWakemanArithmeticAveragePriceEngine
);

#[test]
fn seasoned_and_forward_geometric_prices_match_independent_regressions() {
    let (start, end) = dates();
    let context = market(Date::new(2025, 7, 1).unwrap())
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap()),
            ..Default::default()
        })
        .unwrap();
    for (averaging, realized, expected) in [
        (start, 80.0, 0.005141652127146822),
        (start, 120.0, 9.472410353379193),
        (Date::new(2025, 10, 1).unwrap(), 0.0, 5.064920706779442),
    ] {
        let option = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            realized,
        )
        .unwrap();
        assert!(
            (AnalyticGeometricAveragePriceEngine
                .price(&option, &context)
                .unwrap()
                - expected)
                .abs()
                < 1e-10
        );
    }
}

#[test]
fn arithmetic_solver_rejects_locked_payoffs_and_moments_handle_carry_singularities() {
    let (start, end) = dates();
    let engine = TurnbullWakemanArithmeticAveragePriceEngine;
    let context = market(Date::new(2025, 7, 2).unwrap());
    for direction in [OptionType::Call, OptionType::Put] {
        for realized in [100.0, 201.0, 220.0] {
            let option =
                ArithmeticAveragePriceOption::new(direction, 100.0, start, start, end, realized)
                    .unwrap();
            let quote = engine.price(&option, &context).unwrap();
            let implied = implied_volatility(
                &engine,
                &option,
                &context,
                quote,
                ImpliedVolatilitySettings {
                    lower_bound: 0.05,
                    upper_bound: 0.4,
                    ..Default::default()
                },
            );
            if realized == 100.0 {
                assert!((implied.unwrap() - 0.2).abs() < 1e-6);
            } else {
                assert!(matches!(
                    implied.unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
            }
        }
    }
    for (dividend, expected) in [
        (0.02, 4.097769938360),
        (0.04, 3.624512942693),
        (0.04 - 1e-6, 3.624535605482),
        (0.04 + 1e-6, 3.624490280002),
    ] {
        let option =
            ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0)
                .unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, dividend, 0.2).unwrap(),
            100.0,
            start.into(),
        )
        .unwrap();
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-9);
    }
}

#[test]
fn node_curves_use_weighted_geometric_carry_and_arithmetic_expiry_bypasses_applicability() {
    let (start, end) = dates();
    let knot = Date::new(2025, 7, 2).unwrap();
    let rates = RateTermStructure::new(
        start.into(),
        vec![knot.into(), end.into()],
        vec![(-0.02_f64 * 182.0 / 365.0).exp(), (-0.05_f64).exp()],
    )
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rates, 0.0, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let option =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    let actual = AnalyticGeometricAveragePriceEngine
        .price(&option, &context)
        .unwrap();
    // Integrate the two piecewise constant carries analytically.
    let h: f64 = 182.0 / 365.0;
    let second_rate = (0.05 - 0.02 * h) / (1.0 - h);
    let drift = 0.02 * (h - h * h / 2.0) + second_rate * (1.0 - h).powi(2) / 2.0 - 0.04 / 4.0;
    let width = (0.04_f64 / 3.0).sqrt();
    let cdf = |x: f64| libm::erfc(-x / 2.0_f64.sqrt()) / 2.0;
    let expected = (-0.05_f64).exp()
        * 100.0
        * ((drift + width * width / 2.0).exp() * cdf(drift / width + width) - cdf(drift / width));
    assert!((actual - expected).abs() < 1e-11);
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    assert!(matches!(
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&arithmetic, &context)
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let expired = context
        .replace(PricingContextChanges {
            valuation_time: Some(end.into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&arithmetic.replace(110.0).unwrap(), &expired)
            .unwrap(),
        10.0
    );
}

#[test]
fn arithmetic_solver_prices_only_the_requested_volatility_bracket() {
    let (start, end) = dates();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    let engine = TurnbullWakemanArithmeticAveragePriceEngine;
    let quote = engine.price(&option, &market(start)).unwrap();
    let context = market(start)
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.04, 0.01, 100.0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let sigma = implied_volatility(
        &engine,
        &option,
        &context,
        quote,
        ImpliedVolatilitySettings {
            lower_bound: 0.05,
            upper_bound: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-7);
}

#[test]
fn tiny_variances_and_scaled_normal_tails_retain_representable_prices() {
    let (start, end) = dates();
    for direction in [OptionType::Call, OptionType::Put] {
        for (spot, sigma) in [(100.0, 1e-170), (1e160, 1e-160), (1e300, 1e-200)] {
            for window in 0..3 {
                let at = if window == 2 {
                    Date::new(2025, 5, 1).unwrap()
                } else {
                    start
                };
                let averaging = if window == 1 {
                    Date::new(2025, 5, 1).unwrap()
                } else {
                    start
                };
                let future: f64 = if window == 0 { 1.0 } else { 245.0 / 365.0 };
                let lead = if window == 1 { 120.0 / 365.0 } else { 0.0 };
                let weight = if window == 2 { future } else { 1.0 };
                let expected = weight
                    * spot
                    * sigma
                    * ((lead + future / 3.0) / (2.0 * std::f64::consts::PI)).sqrt();
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap(),
                    spot,
                    at.into(),
                )
                .unwrap();
                let realized = if window == 2 { spot } else { 0.0 };
                let geometric = GeometricAveragePriceOption::new(
                    direction, spot, start, averaging, end, realized,
                )
                .unwrap();
                let arithmetic = ArithmeticAveragePriceOption::new(
                    direction, spot, start, averaging, end, realized,
                )
                .unwrap();
                for price in [
                    AnalyticGeometricAveragePriceEngine
                        .price(&geometric, &context)
                        .unwrap(),
                    TurnbullWakemanArithmeticAveragePriceEngine
                        .price(&arithmetic, &context)
                        .unwrap(),
                ] {
                    assert!(
                        (price / expected - 1.0).abs() < 1e-12,
                        "{price} vs {expected}"
                    );
                }
            }
        }
        let call = direction == OptionType::Call;
        for scale in [1.0, 1e-100] {
            let spot = if call { 1e300 } else { 1e304 } * scale;
            let strike = if call { 1e304 } else { 1e300 } * scale;
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.04, 0.01, 0.4).unwrap(),
                spot,
                start.into(),
            )
            .unwrap();
            let geometric =
                GeometricAveragePriceOption::new(direction, strike, start, start, end, 0.0)
                    .unwrap();
            let arithmetic =
                ArithmeticAveragePriceOption::new(direction, strike, start, start, end, 0.0)
                    .unwrap();
            // Independent 90-digit normal-tail references.
            let expected_geo = if call {
                3.016612743105038e-48
            } else {
                1.6951408750594615e-48
            } * scale;
            let expected_arith = if call {
                3.859991844527151e-40
            } else {
                2.369019512512863e-42
            } * scale;
            assert!(
                (AnalyticGeometricAveragePriceEngine
                    .price(&geometric, &context)
                    .unwrap()
                    / expected_geo
                    - 1.0)
                    .abs()
                    < 1e-8
            );
            assert!(
                (TurnbullWakemanArithmeticAveragePriceEngine
                    .price(&arithmetic, &context)
                    .unwrap()
                    / expected_arith
                    - 1.0)
                    .abs()
                    < 1e-8
            );
        }
    }
}

#[test]
fn arithmetic_affine_cashflows_and_adjusted_strike_overflow_keep_original_payoff_units() {
    let (start, _) = dates();
    let engine = TurnbullWakemanArithmeticAveragePriceEngine;
    // Independent high-precision discounted affine payoffs.
    for (elapsed, amount, rate, dividend, strike_fraction, expected) in [
        (2, 1e308, 0.0, 0.0, 0.25, 7.5e307),
        (2, 1e308, 0.05, 0.05, 0.25, 7.134220683755355e307),
        (2, 1e308, 2.0, 0.0, 0.25, 2.0050048747581985e307),
        (1, 1e300, 750.0, 750.0, 0.25, 1.4262637226062548e-26),
        (1, 1e-300, -750.0, -750.0, 0.25, 3.943870906091103e25),
        (1, 100.0, 0.04, 0.01, 0.5, 48.76732432807474),
    ] {
        let at = Date::new(2025 + elapsed, 1, 1).unwrap();
        let end = Date::new(2026 + elapsed, 1, 1).unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
            amount,
            at.into(),
        )
        .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let option = ArithmeticAveragePriceOption::new(
                direction,
                amount * strike_fraction,
                start,
                start,
                end,
                amount,
            )
            .unwrap();
            let actual = engine.price(&option, &context).unwrap();
            if direction == OptionType::Call {
                assert!((actual / expected - 1.0).abs() < 1e-12);
            } else {
                assert_eq!(actual, 0.0);
            }
        }
    }
    let end = Date::new(2026, 1, 1).unwrap();
    let effective = Date::new(2023, 1, 1).unwrap();
    for at in [start, Date::new(2025, 12, 31).unwrap()] {
        for sigma in [f64::from_bits(1), 1e-200, 1e-6, 0.2, 1.0] {
            for rate in [-0.05, 0.0, 0.05] {
                for direction in [OptionType::Call, OptionType::Put] {
                    let price = |scale| {
                        let option = ArithmeticAveragePriceOption::new(
                            direction,
                            100.0 * scale,
                            effective,
                            effective,
                            end,
                            50.0 * scale,
                        )
                        .unwrap();
                        let context = PricingContext::new(
                            BlackScholesMertonParameters::new(rate, rate, sigma).unwrap(),
                            50.0 * scale,
                            at.into(),
                        )
                        .unwrap();
                        engine.price(&option, &context).unwrap() / scale
                    };
                    let reference = price(1.0);
                    let scaled = price(1e306);
                    if reference == 0.0 {
                        assert_eq!(scaled, 0.0);
                    } else {
                        assert!((scaled / reference - 1.0).abs() < 1e-10);
                    }
                }
            }
        }
    }
}

#[test]
fn single_fixing_preserves_independent_asset_discount_at_extreme_rates() {
    let (start, end) = dates();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(1e16, 0.02, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, 0.0).unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, 0.0).unwrap();
    let expected = 100.0 * (-0.02_f64).exp();
    for price in [
        AnalyticEuropeanEngine.price(&vanilla, &context).unwrap(),
        AnalyticGeometricAveragePriceEngine
            .price(&geometric, &context)
            .unwrap(),
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&arithmetic, &context)
            .unwrap(),
    ] {
        assert!((price - expected).abs() < 1e-12);
    }
}

#[test]
fn non_finite_small_volatility_moments_report_computation_failure() {
    let (start, end) = dates();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, 0.0).unwrap();
    for sigma in [1e-6, 0.001] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(400.0, 0.0, sigma).unwrap(),
            100.0,
            start.into(),
        )
        .unwrap();
        assert!(matches!(
            TurnbullWakemanArithmeticAveragePriceEngine
                .price(&option, &context)
                .unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
}
