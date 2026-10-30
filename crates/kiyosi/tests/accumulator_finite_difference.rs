use kiyosi::*;

fn day(month: u32, date: u32) -> Date {
    Date::new(2025, month, date).unwrap()
}
fn option(end: Date) -> Accumulator {
    Accumulator::new(AccumulatorTerms {
        accumulated_quantity: 3.0,
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, day(1, 1), end)
    })
    .unwrap()
}
fn market(spot: f64, at: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        at,
    )
    .unwrap()
}

fn terminal_reference(option: &Accumulator, context: &PricingContext) -> f64 {
    let below = |level| {
        let cash = DigitalOption::cash_or_nothing(
            OptionType::Put,
            level,
            option.strike(),
            option.effective_date(),
            option.expiry_date(),
        )
        .unwrap();
        let asset = DigitalOption::asset_or_nothing(
            OptionType::Put,
            level,
            option.effective_date(),
            option.expiry_date(),
        )
        .unwrap();
        AnalyticDigitalEngine.price(&asset, context).unwrap()
            - AnalyticDigitalEngine.price(&cash, context).unwrap()
    };
    let model = context.model_parameters();
    let end = option.expiry_date().into();
    let holding = option.accumulated_quantity()
        * (context.spot_price()
            * model
                .dividend_curve()
                .factor(context.valuation_time(), end)
                .unwrap()
            - option.strike()
                * model
                    .risk_free_curve()
                    .factor(context.valuation_time(), end)
                    .unwrap());
    holding
        + option.daily_quantity()
            * if option.knock_out_level() > option.strike() {
                below(option.knock_out_level())
                    + (option.acceleration_factor() - 1.0) * below(option.strike())
            } else {
                option.acceleration_factor() * below(option.knock_out_level())
            }
}

#[test]
fn terminal_event_converges_to_independent_digital_portfolio() {
    let context = market(100.25, Timestamp::new(day(1, 3), 12, 0, 0, 0).unwrap());
    let option = option(day(1, 6));
    let expected = terminal_reference(&option, &context);
    let mut errors = Vec::new();
    for nodes in [400, 1600] {
        let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
            asset_step_count: nodes,
            time_step_count: 2000,
            asset_upper_boundary: Some(440.0),
            ..Default::default()
        })
        .unwrap();
        errors.push((engine.price(&option, &context).unwrap() - expected).abs());
    }
    assert!(errors[1] < 0.001, "{errors:?}");
    assert!(errors[1] < errors[0], "{errors:?}");
}

#[test]
fn trading_midnight_is_observed_once_and_intraday_uses_caller_history() {
    let option = option(day(1, 6));
    let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_step_count: 3200,
        asset_upper_boundary: Some(440.0),
        ..Default::default()
    })
    .unwrap();
    for (spot, quantity) in [(90.0, 5.0), (100.0, 4.0), (109.0, 4.0)] {
        let context = market(spot, day(1, 3).into());
        let expected = terminal_reference(&option.replace(quantity).unwrap(), &context);
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 0.001);
    }
    for at in [
        Timestamp::new(day(1, 3), 0, 0, 0, 1).unwrap(),
        Timestamp::new(day(1, 3), 12, 0, 0, 0).unwrap(),
        day(1, 4).into(),
    ] {
        let context = market(90.0, at);
        assert!(
            (engine.price(&option, &context).unwrap() - terminal_reference(&option, &context))
                .abs()
                < 0.001
        );
    }
    let calendar =
        TradingCalendar::custom(TradingCalendar::weekdays(), vec![day(1, 3)], vec![], None)
            .unwrap();
    let context = market(90.0, day(1, 3).into())
        .replace(PricingContextChanges {
            calendar: Some(calendar),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&option, &context).unwrap() - terminal_reference(&option, &context)).abs()
            < 0.001
    );
}

#[test]
fn settlement_thresholds_history_replacement_and_expiry_greeks_are_exact() {
    let option = option(day(1, 6));
    let engine = FiniteDifferenceAccumulatorEngine::default();
    for (spot, quantity) in [
        (90.0, 5.0),
        (100.0, 4.0),
        (109.0, 4.0),
        (110.0, 3.0),
        (111.0, 3.0),
        (f64::from_bits(100_f64.to_bits() - 1), 5.0),
        (f64::from_bits(110_f64.to_bits() - 1), 4.0),
    ] {
        let result = engine
            .price_with_greeks(
                &option,
                &market(spot, day(1, 6).into()),
                GreeksRequest::all(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(result.price(), quantity * (spot - 100.0));
        assert!(result.values().iter().all(Option::is_none));
    }
    assert_eq!(
        engine
            .price(&option, &market(110.0, day(1, 1).into()))
            .unwrap(),
        30.0
    );
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            option.replace(value).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert_eq!(option.accumulated_quantity(), 3.0);
    assert_eq!(option.replace(0.25).unwrap().accumulated_quantity(), 0.25);
}

#[test]
fn validation_order_domains_and_failures_preserve_engine_and_inputs() {
    let option = option(day(1, 6));
    let engine = FiniteDifferenceAccumulatorEngine::default();
    assert_eq!(engine.settings(), FiniteDifferenceSettings::default());
    for settings in [
        FiniteDifferenceSettings {
            asset_step_count: 2,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(f64::NAN),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceAccumulatorEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for at in [
        Date::new(2024, 12, 31).unwrap().into(),
        Timestamp::new(day(1, 6), 0, 0, 0, 1).unwrap(),
    ] {
        assert!(matches!(
            engine.price(&option, &market(100.0, at)).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let weekend = Accumulator::new(AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        2.0,
        day(1, 1),
        day(1, 5),
    ))
    .unwrap();
    assert!(matches!(
        engine
            .price(&weekend, &market(100.0, day(1, 1).into()))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &weekend,
                &market(100.0, day(1, 1).into()),
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let clipped = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(110.0),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        clipped
            .price(&option, &market(100.0, day(1, 1).into()))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(
        clipped
            .price(&option, &market(110.0, day(1, 1).into()))
            .unwrap(),
        30.0
    );
    let explicit = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        time_step_count: 1,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        explicit
            .price(&option, &market(100.0, day(1, 1).into()))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price(
                &option.replace(f64::MAX).unwrap(),
                &market(111.0, day(1, 6).into())
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(
        engine
            .price(&option, &market(110.0, day(1, 6).into()))
            .unwrap(),
        30.0
    );
}

#[test]
fn daily_layers_converge_to_quadrature_and_quantity_is_affine() {
    let option = option(day(4, 1));
    let context = market(100.25, Timestamp::new(day(1, 1), 12, 0, 0, 0).unwrap());
    let reference = QuadratureAccumulatorEngine::new(QuadratureSettings {
        asset_step_count: 1024,
        ..Default::default()
    })
    .unwrap()
    .price(&option, &context)
    .unwrap();
    let mut errors = Vec::new();
    for nodes in [800, 3200] {
        let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
            asset_step_count: nodes,
            time_step_count: 2000,
            asset_upper_boundary: Some(440.0),
            ..Default::default()
        })
        .unwrap();
        errors.push((engine.price(&option, &context).unwrap() - reference).abs());
    }
    assert!(errors[1] < 0.014, "{errors:?}");
    assert!(errors[1] < errors[0], "{errors:?}");
    let engine = FiniteDifferenceAccumulatorEngine::default();
    let prices =
        [0.0, 3.0, 6.0].map(|q| engine.price(&option.replace(q).unwrap(), &context).unwrap());
    assert!((prices[2] - 2.0 * prices[1] + prices[0]).abs() < 1e-10);
    std::thread::scope(|scope| {
        let task = scope.spawn(|| engine.price(&option, &context).unwrap());
        assert_eq!(task.join().unwrap(), prices[1]);
    });
}

#[test]
fn spline_and_mixed_greeks_match_independent_public_repricing() {
    let option = option(day(2, 3));
    let context = market(100.25, Timestamp::new(day(1, 4), 12, 0, 0, 0).unwrap());
    let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(440.0),
        ..Default::default()
    })
    .unwrap();
    let result = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    assert_eq!(result.price(), engine.price(&option, &context).unwrap());
    let scalar = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([
            Greek::Delta,
            Greek::Gamma,
            Greek::Vega,
            Greek::Rho,
            Greek::Theta,
        ]),
        NumericalShiftSettings {
            spot_shift: 0.002,
            ..Default::default()
        },
    )
    .unwrap();
    for greek in [
        Greek::Delta,
        Greek::Gamma,
        Greek::Vega,
        Greek::Rho,
        Greek::Theta,
    ] {
        assert!(
            (result.require(greek).unwrap() - scalar.require(greek).unwrap()).abs() < 1e-6,
            "{greek:?}: native={:?} scalar={:?}",
            result.get(greek),
            scalar.get(greek)
        );
    }
    for greek in Greek::ALL {
        let single = engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([greek]),
                Default::default(),
            )
            .unwrap();
        assert_eq!(single.get(greek), result.get(greek));
    }
    for (greek, dependency, bump, scale) in [
        (Greek::Speed, Greek::Gamma, 0.01, 1.0),
        (Greek::Vanna, Greek::Delta, 0.0001, 100.0),
        (Greek::Zomma, Greek::Gamma, 0.0001, 100.0),
    ] {
        let center = if greek == Greek::Speed {
            context.spot_price()
        } else {
            0.2
        };
        let bump = center + bump - center;
        let values = [1.0, -1.0].map(|sign| {
            let shifted = if greek == Greek::Speed {
                context
                    .replace(PricingContextChanges {
                        spot_price: Some(center + sign * bump),
                        ..Default::default()
                    })
                    .unwrap()
            } else {
                context
                    .replace(PricingContextChanges {
                        model_parameters: Some(
                            context
                                .model_parameters()
                                .replace(BlackScholesMertonParameterChanges {
                                    volatility: Some(center + sign * bump),
                                    ..Default::default()
                                })
                                .unwrap(),
                        ),
                        ..Default::default()
                    })
                    .unwrap()
            };
            engine
                .price_with_greeks(
                    &option,
                    &shifted,
                    GreeksRequest::new([dependency]),
                    Default::default(),
                )
                .unwrap()
                .require(dependency)
                .unwrap()
        });
        assert!(
            (result.require(greek).unwrap() - (values[0] - values[1]) / (2.0 * bump * scale)).abs()
                < 1e-10,
            "{greek:?}"
        );
    }
    let boundary = engine
        .price_with_greeks(
            &option,
            &market(100.0, day(1, 1).into()),
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
    for greek in [
        Greek::Delta,
        Greek::Gamma,
        Greek::Speed,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Theta,
        Greek::Charm,
        Greek::Color,
    ] {
        assert!(!boundary.has(greek), "{greek:?}");
    }
    assert!(boundary.has(Greek::Vega) && boundary.has(Greek::Rho));
}

#[test]
fn node_curves_price_fixed_holdings_and_only_future_cashflows_require_coverage() {
    let end = Date::new(2026, 1, 1).unwrap();
    let risk = RateTermStructure::new(
        day(1, 1).into(),
        vec![day(7, 1).into(), end.into()],
        vec![0.97, 0.94],
    )
    .unwrap();
    let dividend = RateTermStructure::new(
        day(1, 1).into(),
        vec![day(7, 1).into(), end.into()],
        vec![0.99, 0.98],
    )
    .unwrap();
    let model = BlackScholesMertonParameters::new(risk, dividend, 0.2).unwrap();
    let context = PricingContext::new(model, 100.0, day(1, 1).into()).unwrap();
    let option = Accumulator::new(AccumulatorTerms {
        daily_quantity: 0.0,
        accumulated_quantity: 2.0,
        ..AccumulatorTerms::new(100.0, 399.0, 0.0, 1.0, day(1, 1), end)
    })
    .unwrap();
    let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        time_step_count: 800,
        ..Default::default()
    })
    .unwrap();
    assert!((engine.price(&option, &context).unwrap() - 8.0).abs() < 0.002);
    let short =
        RateTermStructure::new(day(1, 1).into(), vec![day(2, 3).into()], vec![0.99]).unwrap();
    let context = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                context
                    .model_parameters()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(short.into()),
                        ..Default::default()
                    })
                    .unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(
        engine
            .price(
                &option,
                &context
                    .replace(PricingContextChanges {
                        spot_price: Some(399.0),
                        ..Default::default()
                    })
                    .unwrap()
            )
            .unwrap(),
        598.0
    );
}

#[test]
fn speed_propagates_nonfinite_shifted_prices_without_changing_inputs() {
    let option = Accumulator::new(AccumulatorTerms {
        accumulated_quantity: f64::MAX,
        ..AccumulatorTerms::new(100.0, 110.0, 0.0, 2.0, day(1, 1), day(1, 6))
    })
    .unwrap();
    let context = market(100.0, Timestamp::new(day(1, 3), 12, 0, 0, 0).unwrap());
    let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(440.0),
        ..Default::default()
    })
    .unwrap();
    let quote = engine.price(&option, &context).unwrap();
    for spot in [98.0, 102.0] {
        let shifted = context
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &shifted).unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Speed]),
                NumericalShiftSettings {
                    spot_shift: 2.0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
    assert_eq!(option.accumulated_quantity(), f64::MAX);
    assert_eq!(context.spot_price(), 100.0);
}

#[test]
fn implied_volatility_recovers_quotes_and_preserves_solver_failures_and_identifiability() {
    let option = option(day(1, 6));
    let context = market(100.25, Timestamp::new(day(1, 3), 12, 0, 0, 0).unwrap());
    let engine = FiniteDifferenceAccumulatorEngine::default();
    let quote = engine.price(&option, &context).unwrap();
    let controls = ImpliedVolatilitySettings {
        lower_bound: 0.15,
        upper_bound: 0.25,
        ..Default::default()
    };
    assert!(
        (implied_volatility(&engine, &option, &context, quote, controls).unwrap() - 0.2).abs()
            < 1e-6
    );
    assert!(matches!(
        implied_volatility(&engine, &option, &context, 1e10, controls).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert!(matches!(
        implied_volatility(&engine, &option, &context, f64::NAN, controls).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            quote,
            ImpliedVolatilitySettings {
                max_iterations: 1,
                price_tolerance: 1e-30,
                parameter_tolerance: 1e-30,
                ..controls
            }
        )
        .unwrap_err(),
        Error::NonConvergence(_)
    ));
    for context in [
        market(110.0, day(1, 1).into()),
        market(90.0, day(1, 6).into()),
    ] {
        assert!(matches!(
            implied_volatility(
                &engine,
                &option,
                &context,
                engine.price(&option, &context).unwrap(),
                controls
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    for quantity in [0.0, 3.0] {
        let option = Accumulator::new(AccumulatorTerms {
            daily_quantity: 0.0,
            accumulated_quantity: quantity,
            ..AccumulatorTerms::new(100.0, 110.0, 0.0, 2.0, day(1, 1), day(1, 6))
        })
        .unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
            90.0,
            day(1, 1).into(),
        )
        .unwrap();
        assert!(matches!(
            implied_volatility(
                &engine,
                &option,
                &context,
                engine.price(&option, &context).unwrap(),
                controls
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
}

#[test]
fn native_greeks_reuse_central_calendar_validation() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2025, 1, 3).unwrap();
    let validations = Arc::new(AtomicUsize::new(0));
    let counted = validations.clone();
    let calendar = TradingCalendar::new(
        move |day| {
            if day == end {
                counted.fetch_add(1, Ordering::Relaxed);
            }
            true
        },
        365,
    )
    .unwrap();
    let context = PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        101.0,
        start.into(),
        calendar,
    )
    .unwrap();
    let option =
        Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, start, end)).unwrap();
    let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_step_count: 32,
        time_step_count: 10,
        ..Default::default()
    })
    .unwrap();
    let scalar = engine.price(&option, &context).unwrap();
    let expected_validations = validations.swap(0, Ordering::Relaxed);
    assert!(expected_validations > 0);
    let result = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta]),
            Default::default(),
        )
        .unwrap();
    assert_eq!(result.price(), scalar);
    assert!(result.require(Greek::Delta).unwrap().is_finite());
    assert_eq!(validations.load(Ordering::Relaxed), expected_validations);
}
