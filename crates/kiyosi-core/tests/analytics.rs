use kiyosi_core::*;

fn market() -> (EuropeanOption, PricingContext) {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    (
        EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap(),
        PricingContext::new(
            BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
            100.0,
            start.into(),
        )
        .unwrap(),
    )
}

#[test]
fn analytic_greeks_and_volatility_recovery_use_public_api() {
    let (option, context) = market();
    let result = AnalyticVanillaEngine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings::default(),
        )
        .unwrap();
    assert!((result.require(Greek::Delta).unwrap() - 0.586851146134764).abs() < 1e-12);
    assert!((result.require(Greek::Gamma).unwrap() - 0.0189505787550087).abs() < 1e-12);
    assert!((result.require(Greek::Vega).unwrap() - 0.379011575100175).abs() < 1e-12);
    let recovered = implied_volatility(
        &AnalyticVanillaEngine,
        &option,
        &context,
        result.price(),
        ImpliedVolatilitySettings::default(),
    )
    .unwrap();
    assert!((recovered - 0.2).abs() < 1e-8);
}

#[test]
fn numerical_greeks_are_stable_across_absolute_bumps() {
    let (option, context) = market();
    let exact = AnalyticVanillaEngine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings::default(),
        )
        .unwrap();
    for bump in [0.05, 0.1, 0.2] {
        let settings = NumericalShiftSettings {
            spot_shift: bump,
            ..Default::default()
        };
        let actual = numerical_greeks(
            &AnalyticVanillaEngine,
            &option,
            &context,
            GreeksRequest::all(),
            settings,
        )
        .unwrap();
        for greek in Greek::ALL {
            let tolerance = match greek {
                Greek::Theta | Greek::Charm | Greek::Color => 1e-4,
                _ => 1e-5,
            };
            assert!(
                (actual.require(greek).unwrap() - exact.require(greek).unwrap()).abs() < tolerance,
                "{greek:?}: {:?} != {:?}",
                actual.get(greek),
                exact.get(greek)
            );
        }
    }
}

#[test]
fn selections_expiry_and_validation_timing_preserve_availability() {
    let (option, context) = market();
    let request = GreeksRequest::new([Greek::Delta, Greek::Vega, Greek::Delta]);
    let result = AnalyticVanillaEngine
        .price_with_greeks(&option, &context, request, Default::default())
        .unwrap();
    assert!(result.has(Greek::Delta) && result.has(Greek::Vega));
    assert_eq!(result.get(Greek::Gamma), None);
    assert!(matches!(
        result.require(Greek::Gamma).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(option.expiry_date().into()),
            spot_price: Some(110.0),
            ..Default::default()
        })
        .unwrap();
    for result in [
        AnalyticVanillaEngine
            .price_with_greeks(&option, &expiry, GreeksRequest::all(), Default::default())
            .unwrap(),
        numerical_greeks(
            &AnalyticVanillaEngine,
            &option,
            &expiry,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap(),
    ] {
        assert_eq!(result.price(), 10.0);
        assert!(result.values().iter().all(Option::is_none));
        assert!(matches!(
            result.require(Greek::Delta).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2024, 12, 31).unwrap().into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        AnalyticVanillaEngine
            .price_with_greeks(
                &option,
                &outside,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        AnalyticVanillaEngine
            .price_with_greeks(&option, &outside, request, Default::default())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
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
                AnalyticVanillaEngine
                    .price_with_greeks(&option, &context, request, settings)
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            assert!(matches!(
                numerical_greeks(&AnalyticVanillaEngine, &option, &context, request, settings)
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
    }
    for days in [0, -1] {
        assert!(matches!(
            AnalyticVanillaEngine
                .price_with_greeks(
                    &option,
                    &context,
                    request,
                    NumericalShiftSettings {
                        time_shift_days: days,
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn solver_categories_bounds_and_failure_state_are_stable() {
    let (option, context) = market();
    let saved = context.clone();
    let engine = AnalyticVanillaEngine;
    let defaults = ImpliedVolatilitySettings::default();
    assert_eq!(
        (
            defaults.lower_bound,
            defaults.upper_bound,
            defaults.price_tolerance,
            defaults.parameter_tolerance,
            defaults.max_iterations
        ),
        (0.0001, 4.0, 1e-8, 1e-8, 100)
    );
    let price = engine.price(&option, &context).unwrap();
    for vol in [
        defaults.lower_bound,
        defaults.upper_bound,
        0.03,
        0.2,
        1.5,
        3.5,
    ] {
        let model = context
            .model_parameters()
            .replace(BlackScholesMertonParameterChanges {
                volatility: Some(vol),
                ..Default::default()
            })
            .unwrap();
        let market = context
            .replace(PricingContextChanges {
                model_parameters: Some(model),
                ..Default::default()
            })
            .unwrap();
        let quote = engine.price(&option, &market).unwrap();
        assert!(
            (implied_volatility(&engine, &option, &context, quote, defaults).unwrap() - vol).abs()
                < 1e-8
        );
    }
    for settings in [
        ImpliedVolatilitySettings {
            lower_bound: 0.0,
            ..defaults
        },
        ImpliedVolatilitySettings {
            upper_bound: 0.0001,
            ..defaults
        },
        ImpliedVolatilitySettings {
            upper_bound: f64::INFINITY,
            ..defaults
        },
        ImpliedVolatilitySettings {
            price_tolerance: 0.0,
            ..defaults
        },
        ImpliedVolatilitySettings {
            parameter_tolerance: f64::NAN,
            ..defaults
        },
        ImpliedVolatilitySettings {
            max_iterations: 0,
            ..defaults
        },
    ] {
        assert!(matches!(
            implied_volatility(&engine, &option, &context, price, settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for quote in [f64::NAN, f64::INFINITY] {
        assert!(matches!(
            implied_volatility(&engine, &option, &context, quote, defaults).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for quote in [-1.0, 101.0] {
        assert!(matches!(
            implied_volatility(&engine, &option, &context, quote, defaults).unwrap_err(),
            Error::RootNotBracketed(_)
        ));
    }
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            price,
            ImpliedVolatilitySettings {
                max_iterations: 1,
                price_tolerance: 1e-15,
                parameter_tolerance: 1e-15,
                ..defaults
            }
        )
        .unwrap_err(),
        Error::NonConvergence(_)
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(option.expiry_date().into()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        implied_volatility(&engine, &option, &expiry, 0.0, defaults).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(option.expiry_date(), 0, 0, 0, 1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        implied_volatility(&engine, &option, &outside, 0.0, defaults).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(context, saved);
}

#[test]
fn non_finite_solver_values_and_pricing_errors_report_computation_failure() {
    struct NonFinite;
    impl PricingEngine for NonFinite {
        fn price(&self, _: &EuropeanOption, _: &PricingContext) -> Result<f64> {
            Ok(f64::INFINITY)
        }
    }
    let (option, context) = market();
    assert!(matches!(
        implied_volatility(&NonFinite, &option, &context, 1.0, Default::default()).unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert!(matches!(
        numerical_greeks(
            &NonFinite,
            &option,
            &context,
            GreeksRequest::all(),
            Default::default()
        )
        .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    let model = BlackScholesMertonParameters::new(-1000.0, 0.0, 0.2).unwrap();
    let market = context
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    let put = EuropeanOption::new(
        OptionType::Put,
        option.strike(),
        option.effective_date(),
        option.expiry_date(),
    )
    .unwrap();
    assert!(matches!(
        implied_volatility(
            &AnalyticVanillaEngine,
            &put,
            &market,
            1.0,
            Default::default()
        )
        .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn custom_engine_io_failures_propagate_through_solvers_and_greeks() {
    struct Unavailable;
    impl PricingEngine for Unavailable {
        fn price(&self, _: &EuropeanOption, _: &PricingContext) -> Result<f64> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "custom pricing data unavailable",
            )
            .into())
        }
    }

    let (option, context) = market();
    let original = context.clone();
    for error in [
        implied_volatility(&Unavailable, &option, &context, 1.0, Default::default()).unwrap_err(),
        numerical_greeks(
            &Unavailable,
            &option,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap_err(),
    ] {
        let Error::Io(cause) = error else {
            panic!("expected native I/O error, got {error:?}");
        };
        assert_eq!(cause.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(cause.to_string(), "custom pricing data unavailable");
    }
    assert_eq!(context, original);
}

#[test]
fn numerical_stencils_keep_feasible_measures_and_fail_on_feasible_pricing_errors() {
    let (option, context) = market();
    let settings = NumericalShiftSettings {
        spot_shift: 200.0,
        volatility_shift: 0.3,
        ..Default::default()
    };
    let result = numerical_greeks(
        &AnalyticVanillaEngine,
        &option,
        &context,
        GreeksRequest::all(),
        settings,
    )
    .unwrap();
    for greek in [
        Greek::Delta,
        Greek::Gamma,
        Greek::Speed,
        Greek::Vega,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Charm,
        Greek::Color,
    ] {
        assert!(!result.has(greek), "{greek:?}");
    }
    assert!(result.has(Greek::Theta) && result.has(Greek::Rho));
    let collapsed = numerical_greeks(
        &AnalyticVanillaEngine,
        &option,
        &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]),
        NumericalShiftSettings {
            spot_shift: f64::MIN_POSITIVE,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!collapsed.has(Greek::Delta));
    assert!(collapsed.has(Greek::Vega));
    let huge_time = numerical_greeks(
        &AnalyticVanillaEngine,
        &option,
        &context,
        GreeksRequest::new([Greek::Theta]),
        NumericalShiftSettings {
            time_shift_days: i32::MAX,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(huge_time.require(Greek::Theta).unwrap().is_finite());
    struct RejectBump;
    impl PricingEngine for RejectBump {
        fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            if context.spot_price() > 100.0 {
                // A domain failure from a valid bumped context must propagate.
                return EuropeanOption::new(
                    OptionType::Call,
                    -1.0,
                    option.effective_date(),
                    option.expiry_date(),
                )
                .map(|_| 0.0);
            }
            AnalyticVanillaEngine.price(option, context)
        }
    }
    assert!(matches!(
        numerical_greeks(
            &RejectBump,
            &option,
            &context,
            GreeksRequest::new([Greek::Delta]),
            Default::default()
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(context.spot_price(), 100.0);
    let extreme = context
        .replace(PricingContextChanges {
            spot_price: Some(1.0),
            model_parameters: Some(BlackScholesMertonParameters::new(0.0, -709.5, 0.2).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let speed = numerical_greeks(
        &AnalyticVanillaEngine,
        &option,
        &extreme,
        GreeksRequest::new([Greek::Speed]),
        NumericalShiftSettings {
            spot_shift: 0.6,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!speed.has(Greek::Speed));
}

#[test]
fn curve_knots_use_right_hand_time_sensitivities_and_parallel_rho() {
    let (option, original) = market();
    let start = option.effective_date().start_of_day();
    let middle = Date::new(2025, 6, 30).unwrap().start_of_day();
    let end = option.expiry_date().start_of_day();
    let curve = RateTermStructure::new(
        start,
        vec![middle, end],
        vec![
            (-0.02_f64 * 180.0 / 365.0).exp(),
            (-0.02_f64 * 180.0 / 365.0 - 0.08 * 185.0 / 365.0).exp(),
        ],
    )
    .unwrap();
    let model = original
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let context = original
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            valuation_time: Some(middle),
            ..Default::default()
        })
        .unwrap();
    let exact = AnalyticVanillaEngine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    let after = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(middle.date(), 0, 0, 1, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let next = AnalyticVanillaEngine
        .price_with_greeks(
            &option,
            &after,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            Default::default(),
        )
        .unwrap();
    assert!(
        ((next.price() - exact.price()) * 86400.0 - exact.require(Greek::Theta).unwrap()).abs()
            < 1e-7
    );
    assert!(
        ((next.require(Greek::Delta).unwrap() - exact.require(Greek::Delta).unwrap()) * 86400.0
            - exact.require(Greek::Charm).unwrap())
        .abs()
            < 1e-9
    );
    assert!(
        ((next.require(Greek::Gamma).unwrap() - exact.require(Greek::Gamma).unwrap()) * 86400.0
            - exact.require(Greek::Color).unwrap())
        .abs()
            < 1e-10
    );
    let numeric = numerical_greeks(
        &AnalyticVanillaEngine,
        &option,
        &context,
        GreeksRequest::all(),
        Default::default(),
    )
    .unwrap();
    assert!(
        (numeric.require(Greek::Rho).unwrap() - exact.require(Greek::Rho).unwrap()).abs() < 1e-8
    );
    assert!(
        (implied_volatility(
            &AnalyticVanillaEngine,
            &option,
            &context,
            exact.price(),
            Default::default()
        )
        .unwrap()
            - 0.2)
            .abs()
            < 1e-8
    );
    // An incomplete horizon is a pricing failure, even for a delta-only request.
    let short = RateTermStructure::new(start, vec![middle], vec![0.99]).unwrap();
    let model = original
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            dividend_yield: Some(short.into()),
            ..Default::default()
        })
        .unwrap();
    let invalid = original
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        AnalyticVanillaEngine
            .price_with_greeks(
                &option,
                &invalid,
                GreeksRequest::new([Greek::Delta]),
                Default::default()
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn scaled_tail_greeks_and_selected_extreme_results_remain_representable() {
    let (option, context) = market();
    let huge = context
        .replace(PricingContextChanges {
            spot_price: Some(1e300),
            model_parameters: Some(BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let far = EuropeanOption::new(
        OptionType::Call,
        1e304,
        option.effective_date(),
        option.expiry_date(),
    )
    .unwrap();
    let result = AnalyticVanillaEngine
        .price_with_greeks(
            &far,
            &huge,
            GreeksRequest::new([Greek::Vega]),
            Default::default(),
        )
        .unwrap();
    // Independently evaluated log-density at d1 = log(1e-4)/0.2 + 0.1.
    let expected = 1.207_035_364_719_334_4e-161;
    assert!((result.require(Greek::Vega).unwrap() / expected - 1.0).abs() < 1e-10);
    let tiny = context
        .replace(PricingContextChanges {
            spot_price: Some(1e-300),
            model_parameters: Some(BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let tiny_option = EuropeanOption::new(
        OptionType::Call,
        1e-300,
        option.effective_date(),
        option.expiry_date(),
    )
    .unwrap();
    let delta = AnalyticVanillaEngine
        .price_with_greeks(
            &tiny_option,
            &tiny,
            GreeksRequest::new([Greek::Delta]),
            Default::default(),
        )
        .unwrap();
    assert!((delta.require(Greek::Delta).unwrap() - 0.539827837277029).abs() < 1e-12);
    assert!(matches!(
        AnalyticVanillaEngine
            .price_with_greeks(
                &tiny_option,
                &tiny,
                GreeksRequest::all(),
                Default::default()
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}
