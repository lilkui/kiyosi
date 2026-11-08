use kiyosi::*;

fn market() -> (EuropeanOption, PricingContext) {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    (
        EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap(),
        PricingContext::new(
            BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
            100.0,
            start,
        )
        .unwrap(),
    )
}

#[test]
fn unavailable_greek_errors_preserve_measure_and_status() -> Result<()> {
    let request = GreeksRequest::new([Greek::Vega]);
    let absent = PricingResult::new(10.0)?;
    for (result, expected) in [
        (absent.clone(), GreekStatus::NotRequested),
        (
            absent.clone().with_request(request, GreeksRequest::new([])),
            GreekStatus::Unsupported,
        ),
        (
            absent.clone().with_request(request, request),
            GreekStatus::Undefined,
        ),
        (
            absent
                .with_request(request, request)
                .with_unavailable_stencils(request),
            GreekStatus::StencilUnavailable,
        ),
    ] {
        let error = result.require(Greek::Vega).unwrap_err();
        assert!(
            matches!(error, Error::GreekUnavailable { greek: Greek::Vega, status } if status == expected)
        );
        assert!(error.to_string().contains(expected.name()));
        assert!(error.to_string().contains("vega"));
    }
    Ok(())
}

#[test]
fn numerical_greeks_validate_before_preparation_and_prepare_only_once() {
    use std::{cell::Cell, rc::Rc};

    #[derive(Clone)]
    struct Engine {
        preparations: Rc<Cell<usize>>,
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
        fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine + '_>>> {
            self.preparations.set(self.preparations.get() + 1);
            Ok((!self.ready).then(|| {
                Box::new(Self {
                    ready: true,
                    ..self.clone()
                }) as Box<dyn PricingEngine>
            }))
        }
        fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            assert!(self.ready, "pricing must use the prepared engine");
            AnalyticEuropeanEngine.price(option, context)
        }
    }
    let (option, context) = market();
    let engine = Engine {
        preparations: Rc::new(Cell::new(0)),
        ready: false,
        reject_prepared: false,
    };
    let request = GreeksRequest::new([Greek::Delta]);
    for (request, settings) in [
        (
            request,
            NumericalShiftSettings {
                rate_shift: 0.0,
                ..Default::default()
            },
        ),
        (
            request,
            NumericalShiftSettings {
                spot_shift: Some(0.0),
                ..Default::default()
            },
        ),
    ] {
        assert!(matches!(
            numerical_greeks(&engine, &option, &context, request, settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert_eq!(engine.preparations.get(), 0);
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
    assert_eq!(engine.preparations.get(), 1);
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
    assert!((result.require(Greek::Delta).unwrap() - 0.586_851_146_134_764).abs() < 1e-12);
    assert!((result.require(Greek::Gamma).unwrap() - 0.018_950_578_755_008_7).abs() < 1e-12);
    assert!((result.require(Greek::Vega).unwrap() - 37.901_157_510_017_5).abs() < 1e-12);
    assert!((result.require(Greek::Volga).unwrap() - 2.368_822_344_376_09).abs() < 1e-14);
    assert!((result.require(Greek::DividendRho).unwrap() + 58.685_114_613_476_4).abs() < 1e-12);
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
            spot_shift: Some(bump),
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
                Greek::Theta | Greek::Charm | Greek::Color => 0.0365,
                Greek::Volga => 0.1,
                Greek::Vega | Greek::Vanna | Greek::Zomma | Greek::Rho | Greek::DividendRho => {
                    0.001
                }
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
fn volga_and_dividend_rho_preserve_units_curves_and_selectivity() {
    let (call, original) = market();
    let start = call.effective_time();
    let end = call.expiry_time();
    let middle = Date::new(2025, 7, 1).unwrap().start_of_day();
    let curve = RateTermStructure::new(
        start,
        vec![middle, end],
        vec![(-0.01_f64 * 181.0 / 365.0).exp(), (-0.03_f64).exp()],
    )
    .unwrap();
    for dividend in [Rate::from(-0.02), Rate::from(curve)] {
        let context = original
            .replace(PricingContextChanges {
                model_parameters: Some(
                    original
                        .model_parameters()
                        .as_bsm()
                        .unwrap()
                        .replace(BlackScholesMertonParameterChanges {
                            dividend_yield: Some(dividend),
                            ..Default::default()
                        })
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let saved = context.clone();
        for direction in [OptionType::Call, OptionType::Put] {
            let option =
                EuropeanOption::new(direction, 100.0, call.effective_time(), call.expiry_time())
                    .unwrap();
            let exact = AnalyticEuropeanEngine
                .price_with_greeks(&option, &context, GreeksRequest::all())
                .unwrap();
            // A full year gives dividend rho = -spot * delta.
            assert!(
                (exact.require(Greek::DividendRho).unwrap()
                    + context.spot_price() * exact.require(Greek::Delta).unwrap())
                .abs()
                    < 1e-12
            );
            for (greek, tolerance) in [(Greek::Volga, 1e-4), (Greek::DividendRho, 1e-6)] {
                for bump in [0.00005, 0.0001] {
                    let numeric = numerical_greeks(
                        &AnalyticEuropeanEngine,
                        &option,
                        &context,
                        GreeksRequest::new([greek]),
                        NumericalShiftSettings {
                            volatility_shift: bump,
                            rate_shift: bump,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    assert!(
                        (numeric.require(greek).unwrap() - exact.require(greek).unwrap()).abs()
                            < tolerance,
                        "{direction:?} {greek:?}"
                    );
                    assert!(
                        Greek::ALL
                            .into_iter()
                            .filter(|&g| g != greek)
                            .all(|g| !numeric.has(g))
                    );
                }
            }
        }
        assert_eq!(context, saved);
    }
}

#[test]
fn volga_preserves_tiny_volatility_curvature() {
    let (original, context) = market();
    for (spot, sigma) in [(1e300, 1e-200), (1e160, f64::from_bits(1))] {
        let option = EuropeanOption::new(
            OptionType::Call,
            spot,
            original.effective_time(),
            original.expiry_time(),
        )
        .unwrap();
        let context = context
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                model_parameters: Some(
                    BlackScholesMertonParameters::new(0.0, 0.0, sigma)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let result = AnalyticEuropeanEngine
            .price_with_greeks(&option, &context, GreeksRequest::new([Greek::Volga]))
            .unwrap();
        // At zero carry the small-volatility limit is -S*sigma*phi(0)/4.
        let expected = -(spot * sigma) / (4.0 * (2.0 * std::f64::consts::PI).sqrt());
        assert!((result.require(Greek::Volga).unwrap() / expected - 1.0).abs() < 1e-12);
    }
}

#[test]
fn new_parameter_greeks_keep_unavailable_stencils_and_bump_errors() {
    struct RejectDividendBump;
    impl PricingEngine for RejectDividendBump {
        fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            if context
                .model_parameters()
                .as_bsm()
                .unwrap()
                .flat_dividend_yield()?
                > 0.02
            {
                return Err(Error::UnsupportedOperation(
                    "bumped dividend is unsupported".into(),
                ));
            }
            AnalyticEuropeanEngine.price(option, context)
        }
    }

    let (option, context) = market();
    let request = GreeksRequest::new([Greek::Volga, Greek::DividendRho]);
    let result = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
        &context,
        request,
        NumericalShiftSettings {
            volatility_shift: 0.2,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!result.has(Greek::Volga));
    assert!(result.has(Greek::DividendRho));
    let collapsed = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
        &context,
        request,
        NumericalShiftSettings {
            rate_shift: f64::MIN_POSITIVE,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(collapsed.has(Greek::Volga));
    assert!(!collapsed.has(Greek::DividendRho));

    assert!(matches!(
        numerical_greeks(
            &RejectDividendBump,
            &option,
            &context,
            request,
            Default::default()
        ),
        Err(Error::UnsupportedOperation(_))
    ));
    assert_eq!(
        context
            .model_parameters()
            .as_bsm()
            .unwrap()
            .flat_dividend_yield()
            .unwrap(),
        0.02
    );
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
        Error::GreekUnavailable {
            greek: Greek::Gamma,
            status: GreekStatus::NotRequested
        }
    ));
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(option.expiry_time()),
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
        assert_eq!(result.get(Greek::Theta), None);
        assert!((result.require(Greek::Delta).unwrap() - 1.0).abs() < 1e-10);
        assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
        assert!(matches!(
            result.require(Greek::Theta).unwrap_err(),
            Error::GreekUnavailable {
                greek: Greek::Theta,
                status: GreekStatus::Undefined
            }
        ));
    }
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(option.expiry_time().add_days(1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &outside, GreeksRequest::new([]))
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
                spot_shift: Some(bad),
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
    for shift in [
        std::time::Duration::ZERO,
        std::time::Duration::from_nanos(1),
        std::time::Duration::from_nanos(1001),
    ] {
        assert!(matches!(
            numerical_greeks(
                &AnalyticEuropeanEngine,
                &option,
                &context,
                request,
                NumericalShiftSettings {
                    time_shift: Some(shift),
                    ..Default::default()
                }
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn custom_native_greeks_preserve_estimator_across_controls() -> Result<()> {
    struct AnalyticProxy;
    impl PricingEngine for AnalyticProxy {
        fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            AnalyticEuropeanEngine.price(option, context)
        }
        fn price_with_greeks_with_settings(
            &self,
            option: &EuropeanOption,
            context: &PricingContext,
            request: GreeksRequest,
            settings: NumericalShiftSettings,
        ) -> Result<PricingResult> {
            AnalyticEuropeanEngine
                .price_with_greeks_with_settings(option, context, request, settings)
        }
    }
    let (option, context) = market();
    let request = GreeksRequest::new([Greek::Delta]);
    let expected = AnalyticEuropeanEngine.price_with_greeks(&option, &context, request)?;
    let engine: &dyn PricingEngine = &AnalyticProxy;
    assert_eq!(
        engine.price_with_greeks_with_settings(
            &option,
            &context,
            request,
            engine.greek_settings(&option)
        )?,
        expected
    );
    assert_eq!(
        engine.price_with_greeks_with_settings(
            &option,
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(10.0),
                ..Default::default()
            },
        )?,
        expected,
    );
    assert!(matches!(
        engine.price_with_greeks_with_settings(
            &option,
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(0.0),
                ..Default::default()
            },
        ),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn custom_engine_greeks_use_configured_shifts_and_allow_overrides() -> Result<()> {
    struct Cubic;
    impl PricingEngine for Cubic {
        fn price(&self, _: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            Ok(context.spot_price().powi(3))
        }
        fn greek_settings(&self, _: &EuropeanOption) -> NumericalShiftSettings {
            NumericalShiftSettings {
                spot_shift: Some(2.0),
                ..Default::default()
            }
        }
    }
    let (option, context) = market();
    let request = GreeksRequest::new([Greek::Delta]);
    let configured = Cubic.price_with_greeks_with_settings(
        &option,
        &context,
        request,
        Cubic.greek_settings(&option),
    )?;
    assert!((configured.require(Greek::Delta)? - 30_004.0).abs() < 1e-8);
    let overridden = Cubic.price_with_greeks_with_settings(
        &option,
        &context,
        request,
        NumericalShiftSettings {
            spot_shift: Some(1.0),
            ..Default::default()
        },
    )?;
    assert!((overridden.require(Greek::Delta)? - 30_001.0).abs() < 1e-8);
    Ok(())
}

#[test]
fn native_greeks_use_defaults_in_shared_trait_adapters() {
    fn check<I: OptionContract>(
        engine: &impl PricingEngine<I>,
        option: &I,
        context: &PricingContext,
        expected: &PricingResult,
    ) {
        assert_eq!(
            engine
                .price_with_greeks_with_settings(
                    option,
                    context,
                    GreeksRequest::all(),
                    engine.greek_settings(option)
                )
                .unwrap(),
            *expected,
        );
        assert_eq!(
            engine
                .price_with_greeks_with_settings(
                    option,
                    context,
                    GreeksRequest::all(),
                    engine.greek_settings(option)
                )
                .unwrap(),
            *expected,
        );
    }
    let (option, context) = market();
    let request = GreeksRequest::all();
    check(
        &AnalyticEuropeanEngine,
        &option,
        &context,
        &AnalyticEuropeanEngine
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
        option.effective_time(),
        option.expiry_time(),
    )
    .unwrap();
    let engine = CoxRossRubinsteinDigitalEngine::default();
    check(
        &engine,
        &digital,
        &context,
        &engine
            .price_with_greeks(&digital, &context, request)
            .unwrap(),
    );
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        option.effective_time(),
        option.expiry_time(),
        120.0,
        BarrierType::UpAndOut,
    ))
    .unwrap();
    let engine = CoxRossRubinsteinBarrierEngine::default();
    check(
        &engine,
        &barrier,
        &context,
        &engine
            .price_with_greeks(&barrier, &context, request)
            .unwrap(),
    );
    let engine = MonteCarloDigitalEngine::new(settings).unwrap();
    check(
        &engine,
        &digital,
        &context,
        &engine
            .price_with_greeks(&digital, &context, request)
            .unwrap(),
    );
    let engine = MonteCarloAsianEngine::new(settings).unwrap();
    let geometric = GeometricAveragePriceOption::new(
        OptionType::Call,
        100.0,
        option.effective_time(),
        option.effective_time(),
        option.expiry_time(),
        None,
        None,
    )
    .unwrap();
    check(
        &engine,
        &geometric,
        &context,
        &engine
            .price_with_greeks(&geometric, &context, request)
            .unwrap(),
    );
    let arithmetic = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        100.0,
        option.effective_time(),
        option.effective_time(),
        option.expiry_time(),
        None,
        None,
    )
    .unwrap();
    check(
        &engine,
        &arithmetic,
        &context,
        &engine
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
        (0.0, 4.0, 1e-8, 1e-8, 100)
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
            .as_bsm()
            .unwrap()
            .replace(BlackScholesMertonParameterChanges {
                volatility: Some(vol),
                ..Default::default()
            })
            .unwrap();
        let market = context
            .replace(PricingContextChanges {
                model_parameters: Some(model.into()),
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
            lower_bound: -0.1,
            ..defaults
        },
        ImpliedVolatilitySettings {
            upper_bound: 0.0,
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
            valuation_time: Some(option.expiry_time()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        implied_volatility(&engine, &option, &expiry, 0.0, defaults).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let outside = context
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(option.expiry_time().date(), 0, 0, 0, 1).unwrap()),
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
            model_parameters: Some(model.into()),
            ..Default::default()
        })
        .unwrap();
    let put = EuropeanOption::new(
        OptionType::Put,
        option.strike(),
        option.effective_time(),
        option.expiry_time(),
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
    struct RejectBump;
    impl PricingEngine for RejectBump {
        fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
            if context.spot_price() > 100.0 {
                // A domain failure from a valid bumped context must propagate.
                return EuropeanOption::new(
                    OptionType::Call,
                    -1.0,
                    option.effective_time(),
                    option.expiry_time(),
                )
                .map(|_| 0.0);
            }
            AnalyticEuropeanEngine.price(option, context)
        }
    }

    let (option, context) = market();
    let settings = NumericalShiftSettings {
        spot_shift: Some(200.0),
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
            spot_shift: Some(f64::MIN_POSITIVE),
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
            time_shift: Some(std::time::Duration::from_secs(
                86_400 * u64::from(i32::MAX.unsigned_abs()),
            )),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(huge_time.require(Greek::Theta).unwrap().is_finite());

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
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.0, -709.5, 0.2)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let speed = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
        &extreme,
        GreeksRequest::new([Greek::Speed]),
        NumericalShiftSettings {
            spot_shift: Some(0.6),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!speed.has(Greek::Speed));
}

#[test]
fn curve_knots_use_right_hand_time_sensitivities_and_parallel_rho() {
    let (option, original) = market();
    let start = option.effective_time();
    let middle = Date::new(2025, 6, 30).unwrap().start_of_day();
    let end = option.expiry_time();
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
        .as_bsm()
        .unwrap()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let context = original
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
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
        ((next.price() - exact.price()) * (365.0 * 86400.0) - exact.require(Greek::Theta).unwrap())
            .abs()
            < 3.65e-5
    );
    assert!(
        ((next.require(Greek::Delta).unwrap() - exact.require(Greek::Delta).unwrap())
            * (365.0 * 86400.0)
            - exact.require(Greek::Charm).unwrap())
        .abs()
            < 3.65e-7
    );
    assert!(
        ((next.require(Greek::Gamma).unwrap() - exact.require(Greek::Gamma).unwrap())
            * (365.0 * 86400.0)
            - exact.require(Greek::Color).unwrap())
        .abs()
            < 3.65e-8
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
            Greek::Rho => 1e-6,
            Greek::Theta | Greek::Charm | Greek::Color => 0.0365,
            Greek::Volga => 0.1,
            Greek::Vega | Greek::Vanna | Greek::Zomma | Greek::DividendRho => 0.001,
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
        .as_bsm()
        .unwrap()
        .replace(BlackScholesMertonParameterChanges {
            dividend_yield: Some(short.into()),
            ..Default::default()
        })
        .unwrap();
    let invalid = original
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
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
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let near_expiry =
        Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap();
    // Independent 80-digit references for ATM, zero-rate speed = -3 * gamma / (2 * spot).
    for (volatility, valuation, expected) in [
        (f64::from_bits(1), start, -1_211.202_247_392_103_8),
        (f64::from_bits(3), start, -403.734_082_464_034_6),
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
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.0, 0.0, 0.2)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let far = EuropeanOption::new(
        OptionType::Call,
        1e304,
        option.effective_time(),
        option.expiry_time(),
    )
    .unwrap();
    let result = AnalyticEuropeanEngine
        .price_with_greeks(&far, &huge, GreeksRequest::new([Greek::Vega]))
        .unwrap();
    // Independently evaluated log-density at d1 = log(1e-4)/0.2 + 0.1.
    let expected = 1.207_035_364_719_334_4e-159;
    assert!((result.require(Greek::Vega).unwrap() / expected - 1.0).abs() < 1e-10);
    let tiny = context
        .replace(PricingContextChanges {
            spot_price: Some(1e-300),
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.0, 0.0, 0.2)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let tiny_option = EuropeanOption::new(
        OptionType::Call,
        1e-300,
        option.effective_time(),
        option.expiry_time(),
    )
    .unwrap();
    let delta = AnalyticEuropeanEngine
        .price_with_greeks(&tiny_option, &tiny, GreeksRequest::new([Greek::Delta]))
        .unwrap();
    assert!((delta.require(Greek::Delta).unwrap() - 0.539_827_837_277_029).abs() < 1e-12);
    assert!(matches!(
        AnalyticEuropeanEngine
            .price_with_greeks(&tiny_option, &tiny, GreeksRequest::all())
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn custom_engines_can_return_validated_analytic_results() {
    struct Engine;
    impl PricingEngine for Engine {
        fn price(&self, _: &EuropeanOption, _: &PricingContext) -> Result<f64> {
            Ok(10.0)
        }
        fn price_with_greeks_with_settings(
            &self,
            option: &EuropeanOption,
            context: &PricingContext,
            request: GreeksRequest,
            settings: NumericalShiftSettings,
        ) -> Result<PricingResult> {
            settings.validate()?;
            let result = PricingResult::new(self.price(option, context)?)?;
            if request.has(Greek::Delta) {
                result.with_greek(Greek::Delta, 0.5)
            } else {
                Ok(result)
            }
        }
    }
    let (option, context) = market();
    let result = Engine
        .price_with_greeks_with_settings(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta]),
            Engine.greek_settings(&option),
        )
        .unwrap();
    assert_eq!(result.price(), 10.0);
    assert_eq!(result.require(Greek::Delta).unwrap(), 0.5);
    assert_eq!(result.get(Greek::Vega), None);
    assert_eq!(
        result
            .with_greek(Greek::Delta, 0.6)
            .unwrap()
            .get(Greek::Delta),
        Some(0.6)
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            PricingResult::new(value),
            Err(Error::ComputationFailure(_))
        ));
        assert!(matches!(
            PricingResult::new(10.0)
                .unwrap()
                .with_greek(Greek::Delta, value),
            Err(Error::ComputationFailure(_))
        ));
    }
}
