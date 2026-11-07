mod support;
use support::dated;

use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}
fn market(at: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        at,
    )
    .unwrap()
}

#[test]
fn continuous_asian_prices_match_existing_independent_references() {
    let (start, end) = dates();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    assert!(
        (AnalyticGeometricAveragePriceEngine
            .price(&dated(&geometric, &market(start)), &market(start))
            .unwrap()
            - 5.035_867_546_515_384)
            .abs()
            < 1e-12
    );
    assert!(
        (TurnbullWakemanArithmeticAveragePriceEngine
            .price(&dated(&arithmetic, &market(start)), &market(start))
            .unwrap()
            - 5.246_501_860_362_777)
            .abs()
            < 1e-12
    );
}

macro_rules! contract_tests {
    ($module:ident, $option:ident, $engine:ident, $refreshed_price:expr) => {
        mod $module {
            use super::*;
            #[test]
            fn dated_history_refresh_updates_average_and_time_atomically() -> Result<()> {
                let start = Date::new(2025, 1, 2)?.start_of_day();
                let at = start.add_days(4)?;
                let end = start.add_days(10)?;
                let original =
                    $option::new(OptionType::Call, 100.0, start, start, end, Some(100.0))?
                        .with_history_time(Some(at))?;
                let market = PricingContext::new(
                    BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
                    100.0,
                    at,
                )?;
                let later = market.replace(PricingContextChanges {
                    valuation_time: Some(at.add_days(1)?),
                    ..Default::default()
                })?;
                let refreshed = original.replace(AveragePriceOptionChanges {
                    realized_average: Some(Some(104.0)),
                    history_time: Some(Some(later.valuation_time())),
                    ..Default::default()
                })?;
                assert_eq!(refreshed.realized_average(), Some(104.0));
                assert_eq!(refreshed.history_time(), Some(later.valuation_time()));
                assert!(($engine.price(&refreshed, &later)? - $refreshed_price).abs() < 1e-12);
                assert!(matches!(
                    original.replace(AveragePriceOptionChanges {
                        realized_average: Some(Some(104.0)),
                        history_time: Some(Some(end.add_days(1)?)),
                        ..Default::default()
                    }),
                    Err(Error::InvalidInput(_))
                ));
                assert_eq!(original.realized_average(), Some(100.0));
                assert_eq!(original.history_time(), Some(at));
                assert_eq!($engine.price(&original, &market)?, 0.0);
                Ok(())
            }
            #[test]
            fn construction_replacement_and_validation() {
                let (start, end) = dates();
                for strike in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                    assert!(matches!(
                        $option::new(OptionType::Call, strike, start, start, end, None)
                            .unwrap_err(),
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
                        Date::new(2026, 1, 2).unwrap().start_of_day(),
                        end,
                        None
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
                let updated = original.replace_history(110.0).unwrap();
                assert_eq!(updated.option_type(), OptionType::Put);
                assert_eq!(updated.strike(), 100.0);
                assert_eq!(updated.effective_time(), start);
                assert_eq!(updated.averaging_start_time(), start);
                assert_eq!(updated.expiry_time(), end);
                assert_eq!(updated.realized_average(), Some(110.0));
                assert!(matches!(
                    original.replace_history(-1.0).unwrap_err(),
                    Error::InvalidInput(_)
                ));
                assert_eq!(original.realized_average(), DEFAULT_REALIZED_AVERAGE);
            }
            #[test]
            fn history_life_single_fixing_and_expiry_boundaries() {
                let (start, end) = dates();
                let averaging = Date::new(2025, 2, 1).unwrap().start_of_day();
                let original =
                    $option::new(OptionType::Call, 100.0, start, averaging, end, None).unwrap();
                let seasoned = original.replace_history(110.0).unwrap();
                for at in [start, averaging] {
                    assert!(
                        $engine
                            .price(&dated(&original, &market(at)), &market(at))
                            .is_ok()
                    );
                    assert!(matches!(
                        $engine
                            .price(&dated(&seasoned, &market(at)), &market(at))
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                for at in [Date::new(2025, 6, 1).unwrap().start_of_day(), end] {
                    assert!(
                        $engine
                            .price(&dated(&seasoned, &market(at)), &market(at))
                            .is_ok()
                    );
                    assert!(matches!(
                        $engine
                            .price(&dated(&original, &market(at)), &market(at))
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                for at in [
                    Date::new(2024, 12, 31).unwrap().start_of_day(),
                    Date::new(2026, 1, 2).unwrap().start_of_day(),
                ] {
                    assert!(matches!(
                        $engine
                            .price(&dated(&seasoned, &market(at)), &market(at))
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
                let after = market(end)
                    .replace(PricingContextChanges {
                        valuation_time: Some(Timestamp::new(end.date(), 0, 0, 0, 1).unwrap()),
                        ..Default::default()
                    })
                    .unwrap();
                assert!(matches!(
                    $engine
                        .price(&dated(&seasoned, &after), &after)
                        .unwrap_err(),
                    Error::InvalidInput(_)
                ));
                assert_eq!(
                    $engine
                        .price(&dated(&seasoned, &market(end)), &market(end))
                        .unwrap(),
                    10.0
                );
                let settled = $engine
                    .price_with_greeks(
                        &dated(&seasoned, &market(end)),
                        &market(end),
                        GreeksRequest::all(),
                    )
                    .unwrap();
                assert_eq!(settled.get(Greek::Theta), None);
                for greek in [Greek::Delta, Greek::Gamma, Greek::Vega, Greek::Rho] {
                    assert_eq!(settled.require(greek).unwrap(), 0.0);
                }
                assert!(matches!(
                    implied_volatility(
                        &$engine,
                        &dated(&seasoned, &market(end)),
                        &market(end),
                        10.0,
                        Default::default()
                    )
                    .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
                for direction in [OptionType::Call, OptionType::Put] {
                    let single = $option::new(direction, 100.0, start, end, end, None).unwrap();
                    let vanilla = EuropeanOption::new(direction, 100.0, start, end).unwrap();
                    for at in [start, Date::new(2025, 12, 31).unwrap().start_of_day(), end] {
                        assert_eq!(
                            $engine
                                .price(&dated(&single, &market(at)), &market(at))
                                .unwrap(),
                            AnalyticEuropeanEngine
                                .price(&dated(&vanilla, &market(at)), &market(at))
                                .unwrap()
                        );
                    }
                    assert!(matches!(
                        $engine
                            .price(
                                &dated(&single.replace_history(110.0).unwrap(), &market(end)),
                                &market(end)
                            )
                            .unwrap_err(),
                        Error::InvalidInput(_)
                    ));
                }
            }
            #[test]
            fn greeks_solver_selection_errors_and_history_stencils() {
                let (start, end) = dates();
                let option =
                    $option::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
                let context = market(start);
                let full = $engine
                    .price_with_greeks(&dated(&option, &context), &context, GreeksRequest::all())
                    .unwrap();
                assert!(full.require(Greek::Delta).unwrap() > 0.0);
                assert!(full.require(Greek::Vega).unwrap() > 0.0);
                for g in [Greek::Theta, Greek::Charm, Greek::Color] {
                    assert_eq!(full.get(g), None);
                }
                let scalar = numerical_greeks(
                    &$engine,
                    &dated(&option, &context),
                    &context,
                    GreeksRequest::all(),
                    Default::default(),
                )
                .unwrap();
                assert_eq!(full, scalar);
                for greek in Greek::ALL {
                    let selected = $engine
                        .price_with_greeks(
                            &dated(&option, &context),
                            &context,
                            GreeksRequest::new([greek, greek]),
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
                        &dated(&option, &context),
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
                            &dated(&option, &context),
                            &context,
                            GreeksRequest::new([])
                        )
                        .unwrap_err(),
                    Error::InvalidInput(_)
                ));
                assert!(matches!(
                    kiyosi::numerical_greeks(
                        &$engine,
                        &dated(&option, &context),
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
                assert!(matches!(
                    implied_volatility(
                        &$engine,
                        &dated(&option, &context),
                        &context,
                        -1.0,
                        Default::default()
                    )
                    .unwrap_err(),
                    Error::RootNotBracketed(_)
                ));
                let seasoned = option.replace_history(100.0).unwrap();
                let middle = market(Date::new(2025, 7, 1).unwrap().start_of_day());
                let result = $engine
                    .price_with_greeks(&dated(&seasoned, &middle), &middle, GreeksRequest::all())
                    .unwrap();
                assert!(result.values().iter().all(Option::is_some));
                assert_eq!(option.realized_average(), None);
                assert_eq!(
                    context.model_parameters().as_bsm().unwrap().volatility(),
                    0.2
                );
            }
        }
    };
}
contract_tests!(
    geometric,
    GeometricAveragePriceOption,
    AnalyticGeometricAveragePriceEngine,
    (104.0_f64 * 100.0).sqrt() - 100.0
);
contract_tests!(
    arithmetic,
    ArithmeticAveragePriceOption,
    TurnbullWakemanArithmeticAveragePriceEngine,
    2.0
);

#[test]
fn seasoned_and_forward_geometric_prices_match_independent_regressions() {
    let (start, end) = dates();
    let context = market(Date::new(2025, 7, 1).unwrap().start_of_day())
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.05, 0.02, 0.2)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    for (averaging, realized, expected) in [
        (start, 80.0, 0.005_141_652_127_146_822),
        (start, 120.0, 9.472_410_353_379_193),
        (
            Date::new(2025, 10, 1).unwrap().start_of_day(),
            0.0,
            5.064_920_706_779_442,
        ),
    ] {
        let option = GeometricAveragePriceOption::new(
            OptionType::Call,
            100.0,
            start,
            averaging,
            end,
            (realized != 0.0).then_some(realized),
        )
        .unwrap();
        assert!(
            (AnalyticGeometricAveragePriceEngine
                .price(&dated(&option, &context), &context)
                .unwrap()
                - expected)
                .abs()
                < 1e-10
        );
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "Exact comparisons select fixed test cases and payoff boundaries."
)]
fn arithmetic_solver_rejects_locked_payoffs_and_moments_handle_carry_singularities() {
    let (start, end) = dates();
    let engine = TurnbullWakemanArithmeticAveragePriceEngine;
    let context = market(Date::new(2025, 7, 2).unwrap().start_of_day());
    for direction in [OptionType::Call, OptionType::Put] {
        for realized in [100.0, 201.0, 220.0] {
            let option = ArithmeticAveragePriceOption::new(
                direction,
                100.0,
                start,
                start,
                end,
                (realized != 0.0).then_some(realized),
            )
            .unwrap();
            let quote = engine.price(&dated(&option, &context), &context).unwrap();
            let implied = implied_volatility(
                &engine,
                &dated(&option, &context),
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
        (0.02, 4.097_769_938_360),
        (0.04, 3.624_512_942_693),
        (0.04 - 1e-6, 3.624_535_605_482),
        (0.04 + 1e-6, 3.624_490_280_002),
    ] {
        let option =
            ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
                .unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, dividend, 0.2).unwrap(),
            100.0,
            start,
        )
        .unwrap();
        assert!(
            (engine.price(&dated(&option, &context), &context).unwrap() - expected).abs() < 1e-9
        );
    }
}

#[test]
fn node_curves_use_weighted_geometric_carry_and_arithmetic_expiry_bypasses_applicability() {
    let (start, end) = dates();
    let knot = Date::new(2025, 7, 2).unwrap().start_of_day();
    let rates = RateTermStructure::new(
        start,
        vec![knot, end],
        vec![(-0.02_f64 * 182.0 / 365.0).exp(), (-0.05_f64).exp()],
    )
    .unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rates, 0.0, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let option =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let actual = AnalyticGeometricAveragePriceEngine
        .price(&dated(&option, &context), &context)
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
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    assert!(matches!(
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&dated(&arithmetic, &context), &context)
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let expired = context
        .replace(PricingContextChanges {
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(
                &dated(&arithmetic.replace_history(110.0).unwrap(), &expired),
                &expired
            )
            .unwrap(),
        10.0
    );
}

#[test]
fn arithmetic_solver_prices_only_the_requested_volatility_bracket() {
    let (start, end) = dates();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    let engine = TurnbullWakemanArithmeticAveragePriceEngine;
    let quote = engine
        .price(&dated(&option, &market(start)), &market(start))
        .unwrap();
    let context = market(start)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.04, 0.01, 100.0)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let sigma = implied_volatility(
        &engine,
        &dated(&option, &context),
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
                    Date::new(2025, 5, 1).unwrap().start_of_day()
                } else {
                    start
                };
                let averaging = if window == 1 {
                    Date::new(2025, 5, 1).unwrap().start_of_day()
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
                    at,
                )
                .unwrap();
                let realized = if window == 2 { spot } else { 0.0 };
                let geometric = GeometricAveragePriceOption::new(
                    direction,
                    spot,
                    start,
                    averaging,
                    end,
                    (realized != 0.0).then_some(realized),
                )
                .unwrap();
                let arithmetic = ArithmeticAveragePriceOption::new(
                    direction,
                    spot,
                    start,
                    averaging,
                    end,
                    (realized != 0.0).then_some(realized),
                )
                .unwrap();
                for price in [
                    AnalyticGeometricAveragePriceEngine
                        .price(&dated(&geometric, &context), &context)
                        .unwrap(),
                    TurnbullWakemanArithmeticAveragePriceEngine
                        .price(&dated(&arithmetic, &context), &context)
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
                start,
            )
            .unwrap();
            let geometric =
                GeometricAveragePriceOption::new(direction, strike, start, start, end, None)
                    .unwrap();
            let arithmetic =
                ArithmeticAveragePriceOption::new(direction, strike, start, start, end, None)
                    .unwrap();
            // Independent 90-digit normal-tail references.
            let expected_geo = if call {
                3.016_612_743_105_038e-48
            } else {
                1.695_140_875_059_461_5e-48
            } * scale;
            let expected_arith = if call {
                3.859_991_844_527_151e-40
            } else {
                2.369_019_512_512_863e-42
            } * scale;
            assert!(
                (AnalyticGeometricAveragePriceEngine
                    .price(&dated(&geometric, &context), &context)
                    .unwrap()
                    / expected_geo
                    - 1.0)
                    .abs()
                    < 1e-8
            );
            assert!(
                (TurnbullWakemanArithmeticAveragePriceEngine
                    .price(&dated(&arithmetic, &context), &context)
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
        (2, 1e308, 0.05, 0.05, 0.25, 7.134_220_683_755_355e307),
        (2, 1e308, 2.0, 0.0, 0.25, 2.005_004_874_758_198_5e307),
        (1, 1e300, 750.0, 750.0, 0.25, 1.426_263_722_606_254_8e-26),
        (1, 1e-300, -750.0, -750.0, 0.25, 3.943_870_906_091_103e25),
        (1, 100.0, 0.04, 0.01, 0.5, 48.767_324_328_074_74),
    ] {
        let at = Date::new(2025 + elapsed, 1, 1).unwrap().start_of_day();
        let end = Date::new(2026 + elapsed, 1, 1).unwrap().start_of_day();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
            amount,
            at,
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
            let actual = engine.price(&dated(&option, &context), &context).unwrap();
            if direction == OptionType::Call {
                assert!((actual / expected - 1.0).abs() < 1e-12);
            } else {
                assert_eq!(actual, 0.0);
            }
        }
    }
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let effective = Date::new(2023, 1, 1).unwrap().start_of_day();
    for at in [start, Date::new(2025, 12, 31).unwrap().start_of_day()] {
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
                            at,
                        )
                        .unwrap();
                        engine.price(&dated(&option, &context), &context).unwrap() / scale
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
        start,
    )
    .unwrap();
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None).unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None).unwrap();
    let expected = 100.0 * (-0.02_f64).exp();
    for price in [
        AnalyticEuropeanEngine
            .price(&dated(&vanilla, &context), &context)
            .unwrap(),
        AnalyticGeometricAveragePriceEngine
            .price(&dated(&geometric, &context), &context)
            .unwrap(),
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&dated(&arithmetic, &context), &context)
            .unwrap(),
    ] {
        assert!((price - expected).abs() < 1e-12);
    }
}

#[test]
fn non_finite_small_volatility_moments_report_computation_failure() {
    let (start, end) = dates();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    for sigma in [1e-6, 0.001] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(400.0, 0.0, sigma).unwrap(),
            100.0,
            start,
        )
        .unwrap();
        assert!(matches!(
            TurnbullWakemanArithmeticAveragePriceEngine
                .price(&dated(&option, &context), &context)
                .unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
}
