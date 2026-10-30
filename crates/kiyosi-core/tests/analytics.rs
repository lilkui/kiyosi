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
fn numerical_greeks_validate_before_preparation_and_prepare_only_once() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone)]
    struct Engine {
        preparations: Arc<AtomicUsize>,
        ready: bool,
        reject_prepared: bool,
    }
    impl PricingEngine for Engine {
        fn validate_applicability(&self, _: &PricingContext) -> Result<()> {
            if self.ready && self.reject_prepared {
                return Err(Error::UnsupportedOperation(
                    "prepared engine is unavailable".into(),
                ));
            }
            Ok(())
        }
        fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine + Send + Sync + '_>>> {
            self.preparations.fetch_add(1, Ordering::Relaxed);
            Ok((!self.ready).then(|| {
                Box::new(Self {
                    ready: true,
                    ..self.clone()
                }) as Box<dyn PricingEngine + Send + Sync>
            }))
        }
        fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            assert!(self.ready, "pricing must use the prepared engine");
            AnalyticEuropeanEngine.price(option, context)
        }
    }
    let (option, context) = market();
    let engine = Engine {
        preparations: Arc::new(AtomicUsize::new(0)),
        ready: false,
        reject_prepared: false,
    };
    let request = GreeksRequest::new([Greek::Delta]);
    for (request, settings) in [
        (GreeksRequest::default(), NumericalShiftSettings::default()),
        (
            request,
            NumericalShiftSettings {
                spot_shift: 0.0,
                ..Default::default()
            },
        ),
    ] {
        assert!(matches!(
            numerical_greeks(&engine, &option, &context, request, settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert_eq!(engine.preparations.load(Ordering::Relaxed), 0);
    }
    let expected = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
        &context,
        request,
        Default::default(),
    )
    .unwrap();
    let actual = numerical_greeks(&engine, &option, &context, request, Default::default()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(engine.preparations.load(Ordering::Relaxed), 1);
    let unavailable = Engine {
        reject_prepared: true,
        ..engine
    };
    assert!(matches!(
        numerical_greeks(&unavailable, &option, &context, request, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn analytic_greeks_and_volatility_recovery_use_public_api() {
    let (option, context) = market();
    let result = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, GreeksRequest::all())
        .unwrap();
    assert!((result.require(Greek::Delta).unwrap() - 0.586851146134764).abs() < 1e-12);
    assert!((result.require(Greek::Gamma).unwrap() - 0.0189505787550087).abs() < 1e-12);
    assert!((result.require(Greek::Vega).unwrap() - 0.379011575100175).abs() < 1e-12);
    let recovered = implied_volatility(
        &AnalyticEuropeanEngine,
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
    let exact = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, GreeksRequest::all())
        .unwrap();
    for bump in [0.05, 0.1, 0.2] {
        let settings = NumericalShiftSettings {
            spot_shift: bump,
            ..Default::default()
        };
        let actual = numerical_greeks(
            &AnalyticEuropeanEngine,
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
    let result = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, request)
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
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &expiry, GreeksRequest::all())
            .unwrap(),
        numerical_greeks(
            &AnalyticEuropeanEngine,
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
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &outside, GreeksRequest::default())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &outside, request)
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
                numerical_greeks(
                    &AnalyticEuropeanEngine,
                    &option,
                    &context,
                    request,
                    settings
                )
                .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
    }
    for days in [0, -1] {
        assert!(matches!(
            numerical_greeks(
                &AnalyticEuropeanEngine,
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
fn native_greeks_ignore_numerical_settings_in_shared_trait_adapters() {
    fn check<I: OptionContract>(
        engine: &impl PricingEngine<I>,
        option: &I,
        context: &PricingContext,
        expected: PricingResult,
    ) {
        let settings = NumericalShiftSettings {
            spot_shift: 0.0,
            volatility_shift: f64::NAN,
            rate_shift: f64::INFINITY,
            time_shift_days: 0,
        };
        assert_eq!(
            engine
                .price_with_greeks(option, context, GreeksRequest::all(), settings)
                .unwrap(),
            expected,
        );
    }
    let (option, context) = market();
    let request = GreeksRequest::all();
    check(
        &AnalyticEuropeanEngine,
        &option,
        &context,
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &context, request)
            .unwrap(),
    );
    let settings = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        ..Default::default()
    };
    let digital = DigitalOption::cash_or_nothing(
        OptionType::Call,
        100.0,
        10.0,
        option.effective_date(),
        option.expiry_date(),
    )
    .unwrap();
    let engine = MonteCarloDigitalEngine::new(settings).unwrap();
    check(
        &engine,
        &digital,
        &context,
        engine
            .price_with_greeks(&digital, &context, request)
            .unwrap(),
    );
    let engine = MonteCarloAsianEngine::new(settings).unwrap();
    let geometric = GeometricAveragePriceOption::new(
        OptionType::Call,
        100.0,
        option.effective_date(),
        option.effective_date(),
        option.expiry_date(),
        0.0,
    )
    .unwrap();
    check(
        &engine,
        &geometric,
        &context,
        engine
            .price_with_greeks(&geometric, &context, request)
            .unwrap(),
    );
    let arithmetic = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        100.0,
        option.effective_date(),
        option.effective_date(),
        option.expiry_date(),
        0.0,
    )
    .unwrap();
    check(
        &engine,
        &arithmetic,
        &context,
        engine
            .price_with_greeks(&arithmetic, &context, request)
            .unwrap(),
    );
}

#[test]
fn solver_categories_bounds_and_failure_state_are_stable() {
    let (option, context) = market();
    let saved = context.clone();
    let engine = AnalyticEuropeanEngine;
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
            &AnalyticEuropeanEngine,
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
        &AnalyticEuropeanEngine,
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
        &AnalyticEuropeanEngine,
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
        &AnalyticEuropeanEngine,
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
            AnalyticEuropeanEngine.price(option, context)
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
        &AnalyticEuropeanEngine,
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
    .unwrap()
    .shifted(0.01)
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
    let exact = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, GreeksRequest::all())
        .unwrap();
    let after = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(middle.date(), 0, 0, 1, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let next = AnalyticEuropeanEngine
        .price_with_greeks(
            &option,
            &after,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
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
    let saved = context.clone();
    let numeric = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
        &context,
        GreeksRequest::all(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(context, saved);
    for greek in Greek::ALL {
        let tolerance = match greek {
            Greek::Rho => 1e-8,
            Greek::Theta | Greek::Charm | Greek::Color => 1e-4,
            _ => 1e-5,
        };
        assert!(
            (numeric.require(greek).unwrap() - exact.require(greek).unwrap()).abs() < tolerance,
            "{greek:?}: {:?} != {:?}",
            numeric.get(greek),
            exact.get(greek)
        );
    }
    assert!(
        (implied_volatility(
            &AnalyticEuropeanEngine,
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
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &invalid, GreeksRequest::new([Greek::Delta]))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn analytic_speed_preserves_subnormal_volatility_widths() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
    let near_expiry =
        Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap();
    // Independent 80-digit references for ATM, zero-rate speed = -3 * gamma / (2 * spot).
    for (volatility, valuation, expected) in [
        (f64::from_bits(1), start.into(), -1_211.202_247_392_103_8),
        (f64::from_bits(3), start.into(), -403.734_082_464_034_6),
        (f64::from_bits(1), near_expiry, -6_801_739_133.529_686),
    ] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, volatility).unwrap(),
            1e160,
            valuation,
        )
        .unwrap();
        for direction in [OptionType::Call, OptionType::Put] {
            let option = EuropeanOption::new(direction, 1e160, start, end).unwrap();
            let result = AnalyticEuropeanEngine
                .price_with_greeks(&option, &context, GreeksRequest::new([Greek::Speed]))
                .unwrap();
            assert!((result.require(Greek::Speed).unwrap() / expected - 1.0).abs() < 1e-12);
        }
    }
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
    let result = AnalyticEuropeanEngine
        .price_with_greeks(&far, &huge, GreeksRequest::new([Greek::Vega]))
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
    let delta = AnalyticEuropeanEngine
        .price_with_greeks(&tiny_option, &tiny, GreeksRequest::new([Greek::Delta]))
        .unwrap();
    assert!((delta.require(Greek::Delta).unwrap() - 0.539827837277029).abs() < 1e-12);
    assert!(matches!(
        AnalyticEuropeanEngine
            .price_with_greeks(&tiny_option, &tiny, GreeksRequest::all())
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}
