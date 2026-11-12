use kiyosi::*;

fn day(month: u32, date: u32) -> Timestamp {
    Date::new(2025, month, date).unwrap().start_of_day()
}
fn terms(end: Timestamp) -> AccumulatorTerms {
    AccumulatorTerms {
        knock_out_settled: Some(false),
        accumulated_quantity: Some(0.0),
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, day(1, 1), end, (end).time_of_day())
    }
}

#[test]
fn termination_history_is_required_after_prior_knock_out_observations() -> Result<()> {
    let start = day(1, 6);
    let end = day(1, 10);
    let mut terms = AccumulatorTerms::new(100.0, 110.0, 1.0, 1.0, start, end, start.time_of_day());
    terms.accumulated_quantity = Some(2.0);

    let option = Accumulator::new(terms)?;
    assert_eq!(option.knock_out_settled(), None);
    let active = option.replace(AccumulatorChanges {
        knock_out_settled: Some(Some(false)),
        ..Default::default()
    })?;
    let settled = option.replace(AccumulatorChanges {
        knock_out_settled: Some(Some(true)),
        ..Default::default()
    })?;
    assert_eq!(
        active.replace(AccumulatorChanges {
            knock_out_settled: Some(None),
            ..Default::default()
        })?,
        option
    );
    assert_eq!(active.knock_out_settled(), Some(false));
    assert_eq!(settled.knock_out_settled(), Some(true));
    let context = market(105.0, day(1, 8), 0.0, 0.0, 0.0);
    for engine in [
        &QuadratureAccumulatorEngine::default() as &dyn PricingEngine<Accumulator>,
        &FiniteDifferenceAccumulatorEngine::default(),
        &MonteCarloAccumulatorEngine::default(),
    ] {
        assert!(matches!(
            engine.price(&option, &context),
            Err(Error::MissingHistory(_))
        ));
        assert_eq!(engine.price(&active, &context)?, 25.0);
        assert_eq!(engine.price(&settled, &context)?, 0.0);
        let initial = context.replace(PricingContextChanges {
            valuation_time: Some(start),
            ..Default::default()
        })?;
        assert!(
            engine
                .price(
                    &option.replace(AccumulatorChanges {
                        accumulated_quantity: Some(None),
                        ..Default::default()
                    })?,
                    &initial
                )
                .is_ok()
        );
        assert!(matches!(
            engine.price(&settled, &initial),
            Err(Error::InvalidInput(_))
        ));
        let no_knock_out = option.replace(AccumulatorChanges {
            knock_out_observation_times: Some(Some(vec![])),
            accumulated_quantity: Some(Some(2.0)),
            knock_out_settled: Some(None),
            ..Default::default()
        })?;
        assert_eq!(engine.price(&no_knock_out, &context)?, 25.0);
    }
    Ok(())
}
fn market(spot: f64, at: Timestamp, sigma: f64, rate: f64, dividend: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        at,
    )
    .unwrap()
}

struct TerminalReference;
impl PricingEngine<Accumulator> for TerminalReference {
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        let lower = |level| -> Result<f64> {
            let cash = DigitalOption::cash_or_nothing(
                OptionType::Put,
                level,
                option.strike(),
                option.effective_time(),
                option.expiry_time(),
            )?;
            let asset = DigitalOption::asset_or_nothing(
                OptionType::Put,
                level,
                option.effective_time(),
                option.expiry_time(),
            )?;
            Ok(AnalyticDigitalEngine.price(&asset, context)?
                - AnalyticDigitalEngine.price(&cash, context)?)
        };
        let model = context.model_parameters().as_bsm().unwrap();
        let fixed = option.accumulated_quantity().unwrap()
            * (context.spot_price()
                * model
                    .dividend_curve()
                    .factor(context.valuation_time(), option.expiry_time())?
                - option.strike()
                    * model
                        .risk_free_curve()
                        .factor(context.valuation_time(), option.expiry_time())?);
        let purchase = if option.knock_out_level() > option.strike() {
            lower(option.knock_out_level())?
                + (option.acceleration_factor() - 1.0) * lower(option.strike())?
        } else {
            option.acceleration_factor() * lower(option.knock_out_level())?
        };
        Ok(fixed + option.daily_quantity() * purchase)
    }
}

#[test]
fn single_observation_matches_terminal_digital_portfolio_without_grid_error() {
    let minimal = QuadratureAccumulatorEngine::new(QuadratureSettings {
        asset_step_count: 4,
        integration_standard_deviations: 0.1,
    })
    .unwrap();
    for knock_out in [90.0, 100.0, 110.0] {
        for acceleration in [0.0, 1.0, 2.0] {
            let option = Accumulator::new(AccumulatorTerms {
                knock_out_level: knock_out,
                acceleration_factor: acceleration,
                accumulated_quantity: Some(3.0),
                ..terms(day(1, 6))
            })
            .unwrap();
            for spot in [80.0, 100.0, 120.0] {
                let context = market(
                    spot,
                    Timestamp::new(day(1, 3).date(), 12, 0, 0, 0).unwrap(),
                    0.6,
                    -0.02,
                    0.03,
                );
                let value = QuadratureAccumulatorEngine::default()
                    .price(&option, &context)
                    .unwrap();
                let expected = TerminalReference.price(&option, &context).unwrap();
                assert!(
                    (value - expected).abs() < 1e-9,
                    "ko={knock_out} acceleration={acceleration} spot={spot}: {value} != {expected}"
                );
                assert_eq!(minimal.price(&option, &context).unwrap(), value);
            }
        }
    }
}

#[test]
fn history_replacement_owns_validated_values_and_preserves_terms() {
    let option = Accumulator::new(terms(day(2, 3))).unwrap();
    assert_eq!(option.accumulated_quantity().unwrap(), 0.0);
    let seasoned = option
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(3.0)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(seasoned.accumulated_quantity().unwrap(), 3.0);
    assert_eq!(
        seasoned
            .replace(AccumulatorChanges {
                accumulated_quantity: Some(Some(0.0)),
                ..Default::default()
            })
            .unwrap(),
        option
    );
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            seasoned
                .replace(AccumulatorChanges {
                    accumulated_quantity: Some(Some(invalid)),
                    ..Default::default()
                })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert_eq!(seasoned.accumulated_quantity().unwrap(), 3.0);
    }
    assert_eq!(option.strike(), 100.0);
    assert_eq!(option.knock_out_level(), 110.0);
    assert_eq!(option.daily_quantity(), 1.0);
    assert_eq!(option.acceleration_factor(), 2.0);
    assert_eq!(option.effective_time(), day(1, 1));
    assert_eq!(option.expiry_time(), day(2, 3));
}

#[test]
fn validation_categories_and_order_preserve_accepted_degenerate_terms() {
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        for (field, name) in [
            "strike",
            "knock_out_level",
            "daily_quantity",
            "acceleration_factor",
            "accumulated_quantity",
        ]
        .into_iter()
        .enumerate()
        {
            let mut t = terms(day(1, 1));
            match field {
                0 => t.strike = invalid,
                1 => t.knock_out_level = invalid,
                2 => t.daily_quantity = invalid,
                3 => t.acceleration_factor = invalid,
                _ => t.accumulated_quantity = Some(invalid),
            }
            let error = Accumulator::new(t).unwrap_err();
            assert!(matches!(error, Error::InvalidInput(_)));
            assert!(error.to_string().contains(name), "{name}: {error}");
        }
    }
    let mut t = terms(day(1, 1));
    t.daily_quantity = 0.0;
    t.acceleration_factor = 0.0;
    assert!(Accumulator::new(t.clone()).is_ok());
    t.expiry_time = Date::new(2024, 12, 31).unwrap().start_of_day();
    assert!(matches!(
        Accumulator::new(t.clone()).unwrap_err(),
        Error::InvalidInput(_)
    ));
    t.strike = 0.0;
    assert!(matches!(
        Accumulator::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn settlement_and_current_purchase_preserve_exact_thresholds_and_timing() {
    let option = Accumulator::new(terms(day(1, 6)))
        .unwrap()
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(3.0)),
            ..Default::default()
        })
        .unwrap();
    let engine = QuadratureAccumulatorEngine::default();
    for (spot, quantity) in [
        (90.0, 5.0),
        (100.0, 4.0),
        (109.0, 4.0),
        (110.0, 3.0),
        (111.0, 3.0),
        (f64::from_bits(100.0_f64.to_bits() - 1), 5.0),
        (f64::from_bits(100.0_f64.to_bits() + 1), 4.0),
        (f64::from_bits(110.0_f64.to_bits() - 1), 4.0),
        (f64::from_bits(110.0_f64.to_bits() + 1), 3.0),
    ] {
        let context = market(spot, day(1, 6), 0.2, 0.04, 0.01);
        assert_eq!(
            engine.price(&option, &context).unwrap(),
            quantity * (spot - 100.0)
        );
        let result = engine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        assert_eq!(result.get(Greek::Theta), None);
        assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
        if (spot - 100.0).abs() <= 0.01 || (spot - 110.0).abs() <= 0.01 {
            assert_eq!(result.get(Greek::Delta), None);
        } else {
            assert!((result.require(Greek::Delta).unwrap() - quantity).abs() < 1e-10);
        }
    }
    for spot in [90.0, 100.0, 109.0] {
        let context = market(spot, day(1, 3), 0.2, 0.04, 0.01);
        let updated = option
            .replace(AccumulatorChanges {
                accumulated_quantity: Some(Some(if spot < 100.0 { 5.0 } else { 4.0 })),
                ..Default::default()
            })
            .unwrap();
        assert!(
            (engine.price(&option, &context).unwrap()
                - TerminalReference.price(&updated, &context).unwrap())
            .abs()
                < 1e-9
        );
    }
    assert_eq!(
        engine
            .price(&option, &market(110.0, day(1, 2), 0.2, 0.04, 0.01))
            .unwrap(),
        30.0
    );
    for at in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        Timestamp::new(day(1, 6).date(), 0, 0, 0, 1).unwrap(),
    ] {
        assert!(matches!(
            engine
                .price(&option, &market(100.0, at, 0.2, 0.04, 0.01))
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let huge = option
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(f64::MAX)),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price(&huge, &market(111.0, day(1, 6), 0.2, 0.04, 0.01))
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert!(matches!(
        engine
            .price(
                &option,
                &market(
                    100.0,
                    Timestamp::new(day(1, 3).date(), 12, 0, 0, 0).unwrap(),
                    1e308,
                    0.04,
                    0.01
                )
            )
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn two_observations_match_independent_conditional_terminal_portfolio() {
    let engine = QuadratureAccumulatorEngine::default();
    let fine = QuadratureAccumulatorEngine::new(QuadratureSettings {
        asset_step_count: 1024,
        integration_standard_deviations: 10.0,
    })
    .unwrap();
    for ko in [95.0, 100.0, 110.0] {
        let option = Accumulator::new(AccumulatorTerms {
            knock_out_level: ko,
            accumulated_quantity: Some(3.0),
            ..terms(day(1, 3))
        })
        .unwrap();
        let context = market(
            100.25,
            Timestamp::new(day(1, 1).date(), 12, 0, 0, 0).unwrap(),
            0.6,
            -0.02,
            0.03,
        );
        let time: f64 = 0.5 / 365.0;
        let width = 0.6 * time.sqrt();
        let drift = (-0.02 - 0.03 - 0.5 * 0.6 * 0.6) * time;
        let z = |level: f64| ((level / 100.25).ln() - drift) / width;
        let mut breaks = vec![-12.0, z(100.0), z(ko), 12.0];
        breaks.retain(|v| (-12.0..=12.0).contains(v));
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let integrand = |z: f64, side: f64| {
            let spot = 100.25 * (drift + width * z).exp();
            let value = if side >= ko {
                3.0 * (spot - 100.0)
            } else {
                let quantity = 3.0 + if side < 100.0 { 2.0 } else { 1.0 };
                TerminalReference
                    .price(
                        &option
                            .replace(AccumulatorChanges {
                                accumulated_quantity: Some(Some(quantity)),
                                ..Default::default()
                            })
                            .unwrap(),
                        &market(spot, day(1, 2), 0.6, -0.02, 0.03),
                    )
                    .unwrap()
            };
            value * (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
        };
        let mut reference = 0.0;
        for edges in breaks.windows(2) {
            let step = (edges[1] - edges[0]) / 4096.0;
            let side = 100.25 * (drift + width * edges[0].midpoint(edges[1])).exp();
            let mut sum = integrand(edges[0], side) + integrand(edges[1], side);
            for i in 1..4096 {
                sum += if i % 2 == 0 { 2.0 } else { 4.0 }
                    * integrand(edges[0] + f64::from(i) * step, side);
            }
            reference += sum * step / 3.0;
        }
        reference *= (0.02 * time).exp();
        let value = engine.price(&option, &context).unwrap();
        let refined = fine.price(&option, &context).unwrap();
        assert!(
            (value - reference).abs() < 2e-5,
            "{ko}: {value} != {reference}"
        );
        assert!(
            (refined - reference).abs() < 2e-6,
            "{ko}: {refined} != {reference}"
        );
    }
}

#[test]
fn affine_history_zero_limits_and_tiny_volatility_tails() {
    let engine = QuadratureAccumulatorEngine::default();
    let option = Accumulator::new(terms(day(2, 3))).unwrap();
    let at = Timestamp::new(day(1, 1).date(), 12, 0, 0, 0).unwrap();
    let context = market(90.0, at, 0.2, 0.04, 0.01);
    let values = [0.0, 3.0, 6.0].map(|q| {
        engine
            .price(
                &option
                    .replace(AccumulatorChanges {
                        accumulated_quantity: Some(Some(q)),
                        ..Default::default()
                    })
                    .unwrap(),
                &context,
            )
            .unwrap()
    });
    assert!((values[2] - values[1] - (values[1] - values[0])).abs() < 1e-10);
    let inactive = Accumulator::new(AccumulatorTerms {
        daily_quantity: 0.0,
        ..terms(day(2, 3))
    })
    .unwrap();
    assert_eq!(engine.price(&inactive, &context).unwrap(), 0.0);
    let zero_carry = market(90.0, at, 0.2, 0.0, 0.0);
    assert_eq!(
        engine
            .price(
                &inactive
                    .replace(AccumulatorChanges {
                        accumulated_quantity: Some(Some(3.0)),
                        ..Default::default()
                    })
                    .unwrap(),
                &zero_carry
            )
            .unwrap(),
        -30.0
    );
    for (spot, expected) in [(90.0, -490.0), (120.0, 60.0)] {
        let tiny = market(spot, at, 1e-8, 0.0, 0.0);
        assert!(
            (engine
                .price(
                    &option
                        .replace(AccumulatorChanges {
                            accumulated_quantity: Some(Some(3.0)),
                            ..Default::default()
                        })
                        .unwrap(),
                    &tiny
                )
                .unwrap()
                - expected)
                .abs()
                < 1e-9
        );
    }
    std::thread::scope(|scope| {
        let other = scope.spawn(|| engine.price(&option, &context).unwrap());
        assert_eq!(
            engine.price(&option, &context).unwrap(),
            other.join().unwrap()
        );
    });
}

#[test]
fn discounted_low_tail_preserves_large_finite_quantities() {
    let engine = QuadratureAccumulatorEngine::default();
    let context = market(
        90.0,
        Timestamp::new(day(1, 1).date(), 12, 0, 0, 0).unwrap(),
        0.2,
        1000.0,
        1000.0,
    );
    let option = Accumulator::new(AccumulatorTerms {
        daily_quantity: 1e306,
        acceleration_factor: 1.0,
        ..terms(day(1, 6))
    })
    .unwrap();
    let actual = engine.price(&option, &context).unwrap();
    let reference = -1.326_800_799_078_903e302;
    assert!((actual / reference - 1.0).abs() < 1e-12);
    let smaller = Accumulator::new(AccumulatorTerms {
        daily_quantity: 1e305,
        acceleration_factor: 1.0,
        ..terms(day(1, 6))
    })
    .unwrap();
    assert!((actual / engine.price(&smaller, &context).unwrap() - 10.0).abs() < 1e-12);
}

#[test]
#[expect(
    clippy::cast_precision_loss,
    reason = "test counts are within exact f64 integer precision."
)]
fn spatial_and_domain_convergence_meet_accuracy_targets() {
    let engines = [
        QuadratureAccumulatorEngine::default(),
        QuadratureAccumulatorEngine::new(QuadratureSettings {
            asset_step_count: 1024,
            integration_standard_deviations: 8.0,
        })
        .unwrap(),
        QuadratureAccumulatorEngine::new(QuadratureSettings {
            asset_step_count: 1024,
            integration_standard_deviations: 10.0,
        })
        .unwrap(),
    ];
    let year = Date::new(2026, 1, 1).unwrap().start_of_day();
    for (end, spot, sigma, ko, acceleration, rate, daily) in [
        (day(2, 3), 80.0, 0.05, 105.0, 2.0, 0.04, false),
        (day(2, 3), 100.0, 0.2, 110.0, 2.0, 0.04, false),
        (day(2, 3), 120.0, 0.6, 150.0, 1.0, -0.02, true),
        (day(2, 3), 100.0, 0.05, 150.0, 0.0, 0.04, false),
        (day(4, 1), 80.0, 0.2, 110.0, 2.0, 0.04, true),
        (year, 100.0, 0.2, 110.0, 2.0, 0.04, false),
    ] {
        let option = Accumulator::new(AccumulatorTerms {
            knock_out_level: ko,
            acceleration_factor: acceleration,
            accumulated_quantity: Some(3.0),
            ..terms(end)
        })
        .unwrap();
        let context = market(
            spot,
            Timestamp::new(day(1, 1).date(), 12, 0, 0, 0).unwrap(),
            sigma,
            rate,
            0.01,
        );
        let calendar = if daily {
            TradingCalendar::all_days()
        } else {
            TradingCalendar::default()
        };
        let option = option
            .replace(AccumulatorChanges {
                calendar: Some(calendar),
                accumulated_quantity: Some(option.accumulated_quantity()),
                knock_out_settled: Some(option.knock_out_settled()),
                ..Default::default()
            })
            .unwrap();
        let observations = ObservationSchedule::fixed_interval(
            day(1, 2),
            end,
            1,
            option.calendar(),
            (day(1, 2)).time_of_day(),
        )
        .unwrap()
        .len();
        let exposure = 100.0 * (3.0 + observations as f64 * acceleration.max(1.0));
        let values = engines.map(|e| e.price(&option, &context).unwrap());
        assert!(
            (values[0] - values[1]).abs() / exposure <= 1e-6,
            "{end:?} {spot} {sigma}"
        );
        assert!(
            (values[1] - values[2]).abs() / exposure <= 1e-6,
            "{end:?} {spot} {sigma}"
        );
    }
}

struct Scalar(QuadratureAccumulatorEngine);
impl PricingEngine<Accumulator> for Scalar {
    fn price(&self, option: &Accumulator, context: &PricingContext) -> Result<f64> {
        self.0.price(option, context)
    }
}

#[test]
fn greeks_match_terminal_reference_and_cached_prices_match_scalar_repricing() {
    let option = Accumulator::new(terms(day(1, 6)))
        .unwrap()
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(3.0)),
            ..Default::default()
        })
        .unwrap();
    let context = market(
        101.5,
        Timestamp::new(day(1, 4).date(), 12, 0, 0, 0).unwrap(),
        0.6,
        0.04,
        0.01,
    );
    let engine = QuadratureAccumulatorEngine::default();
    let shifts = NumericalShiftSettings {
        spot_shift: Some(0.2),
        volatility_shift: Some(0.001),
        rate_shift: 0.001,
        ..Default::default()
    };
    let actual =
        kiyosi::numerical_greeks(&engine, &option, &context, GreeksRequest::all(), shifts).unwrap();
    let reference = numerical_greeks(
        &TerminalReference,
        &option,
        &context,
        GreeksRequest::all(),
        shifts,
    )
    .unwrap();
    for greek in Greek::ALL {
        let expected = reference.require(greek).unwrap();
        assert!(
            (actual.require(greek).unwrap() - expected).abs() < 1e-7 + 1e-5 * expected.abs(),
            "{greek:?}"
        );
    }
    let option = Accumulator::new(terms(day(1, 20)))
        .unwrap()
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(3.0)),
            ..Default::default()
        })
        .unwrap();
    let engine = QuadratureAccumulatorEngine::new(QuadratureSettings {
        asset_step_count: 128,
        ..Default::default()
    })
    .unwrap();
    for at in [
        Timestamp::new(day(1, 4).date(), 12, 0, 0, 0).unwrap(),
        Timestamp::new(day(1, 5).date(), 23, 59, 59, 0).unwrap(),
        day(1, 6),
    ] {
        for spot in [90.0, 100.0, 100.25, 109.9, 110.0] {
            let context = market(spot, at, 0.3, -0.02, 0.01);
            let actual =
                kiyosi::numerical_greeks(&engine, &option, &context, GreeksRequest::all(), shifts)
                    .unwrap();
            let scalar = numerical_greeks(
                &Scalar(engine),
                &option,
                &context,
                GreeksRequest::all(),
                shifts,
            )
            .unwrap();
            assert_eq!(actual, scalar);
        }
    }
}

#[test]
fn analytic_availability_and_implied_volatility_follow_shared_contract() {
    let engine = QuadratureAccumulatorEngine::default();
    let option = Accumulator::new(terms(day(2, 3)))
        .unwrap()
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(3.0)),
            ..Default::default()
        })
        .unwrap();
    let boundary = market(100.0, day(1, 2), 0.2, 0.04, 0.01);
    let result = engine
        .price_with_greeks(&option, &boundary, GreeksRequest::all())
        .unwrap();
    for greek in [
        Greek::Delta,
        Greek::Gamma,
        Greek::Speed,
        Greek::Theta,
        Greek::Charm,
        Greek::Color,
        Greek::Vanna,
        Greek::Zomma,
    ] {
        assert!(!result.has(greek));
    }
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let option = Accumulator::new(terms(day(1, 6)))
        .unwrap()
        .replace(AccumulatorChanges {
            accumulated_quantity: Some(Some(3.0)),
            ..Default::default()
        })
        .unwrap();
    let context = market(
        100.0,
        Timestamp::new(day(1, 3).date(), 12, 0, 0, 0).unwrap(),
        0.4,
        0.04,
        0.01,
    );
    let quote = engine.price(&option, &context).unwrap();
    let settings = ImpliedVolatilitySettings::new(0.2, 0.6);
    assert!(
        (implied_volatility(&engine, &option, &context, quote, settings).unwrap() - 0.4).abs()
            < 1e-6
    );
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
    for (option, context) in [
        (option.clone(), market(110.0, day(1, 3), 0.2, 0.04, 0.01)),
        (option.clone(), market(100.0, day(1, 6), 0.2, 0.04, 0.01)),
        (
            Accumulator::new(AccumulatorTerms {
                daily_quantity: 0.0,
                ..terms(day(1, 6))
            })
            .unwrap()
            .replace(AccumulatorChanges {
                accumulated_quantity: Some(Some(3.0)),
                ..Default::default()
            })
            .unwrap(),
            market(90.0, day(1, 3), 0.2, 0.0, 0.0),
        ),
    ] {
        let quote = engine.price(&option, &context).unwrap();
        assert!(matches!(
            implied_volatility(&engine, &option, &context, quote, settings).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let curve =
        RateTermStructure::new(day(1, 1), vec![day(1, 6), day(2, 3)], vec![0.995, 0.99]).unwrap();
    let curved = boundary
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(curve, 0.01, 0.2)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price_with_greeks(&option, &curved, GreeksRequest::new([]))
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));

    for settings in [
        QuadratureSettings {
            asset_step_count: 3,
            ..Default::default()
        },
        QuadratureSettings {
            integration_standard_deviations: 0.0,
            ..Default::default()
        },
        QuadratureSettings {
            integration_standard_deviations: f64::INFINITY,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            QuadratureAccumulatorEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
#[expect(
    clippy::cast_precision_loss,
    reason = "test counts are within exact f64 integer precision."
)]
fn greeks_converge_under_spatial_refinement_and_smaller_bumps() {
    let normal = QuadratureAccumulatorEngine::default();
    let refined = QuadratureAccumulatorEngine::new(QuadratureSettings {
        asset_step_count: 1024,
        ..Default::default()
    })
    .unwrap();
    let at = Timestamp::new(day(1, 4).date(), 12, 0, 0, 0).unwrap();
    for (end, ko, acceleration, spot, sigma, rate) in [
        (day(2, 3), 110.0, 2.0, 100.25, 0.2, 0.04),
        (day(2, 3), 150.0, 1.0, 120.0, 0.6, -0.02),
    ] {
        let option = Accumulator::new(AccumulatorTerms {
            knock_out_level: ko,
            acceleration_factor: acceleration,
            accumulated_quantity: Some(3.0),
            ..terms(end)
        })
        .unwrap();
        let context = market(spot, at, sigma, rate, 0.01);
        let base = normal
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        let fine = refined
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        let smaller = kiyosi::numerical_greeks(
            &refined,
            &option,
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings {
                spot_shift: Some(0.005),
                volatility_shift: Some(0.00005),
                rate_shift: 0.00005,
                ..Default::default()
            },
        )
        .unwrap();
        let observations = ObservationSchedule::fixed_interval(
            day(1, 5),
            end,
            1,
            option.calendar(),
            (day(1, 5)).time_of_day(),
        )
        .unwrap()
        .len();
        let exposure = 100.0 * (3.0 + observations as f64 * acceleration.max(1.0));
        for (greek, order, absolute, relative) in [
            (Greek::Delta, 1, 1e-6, 0.005),
            (Greek::Gamma, 2, 1e-5, 0.01),
            (Greek::Vega, 0, 1e-8, 0.005),
            (Greek::Rho, 0, 1e-8, 0.005),
            (Greek::Theta, 0, 1e-8, 0.02),
            (Greek::Speed, 3, 1e-4, 0.05),
            (Greek::Vanna, 1, 1e-7, 0.05),
            (Greek::Charm, 1, 1e-7, 0.05),
            (Greek::Zomma, 2, 1e-6, 0.05),
            (Greek::Color, 2, 1e-6, 0.05),
        ] {
            let scale = 100.0_f64.powi(order) / exposure;
            let expected = fine.require(greek).unwrap() * scale;
            let limit = absolute + relative * expected.abs();
            for result in [&base, &smaller] {
                assert!(
                    (result.require(greek).unwrap() * scale - expected).abs() <= limit,
                    "{end:?} {greek:?}"
                );
            }
        }
    }
}

#[test]
fn unknown_history_requires_an_explicit_quantity_after_a_prior_observation() {
    let engine = QuadratureAccumulatorEngine::default();
    for start in [day(1, 2), day(1, 4)] {
        let mut terms = AccumulatorTerms::new(
            90.0,
            150.0,
            1.0,
            2.0,
            start,
            day(1, 7),
            (day(1, 7)).time_of_day(),
        );
        terms.knock_out_settled = Some(false);

        let option = Accumulator::new(terms).unwrap();
        assert_eq!(option.accumulated_quantity(), None);
        let first = if start == day(1, 4) { day(1, 6) } else { start };
        let current = market(100.0, first, 0.2, 0.0, 0.0);
        assert!(engine.price(&option, &current).is_ok());
        let positive = option
            .replace(AccumulatorChanges {
                accumulated_quantity: Some(Some(3.0)),
                ..Default::default()
            })
            .unwrap();
        for valuation in [start, first] {
            assert!(matches!(
                engine.price(&positive, &market(100.0, valuation, 0.2, 0.0, 0.0)),
                Err(Error::InvalidInput(_))
            ));
        }
        let prior = current
            .replace(PricingContextChanges {
                valuation_time: Some(Timestamp::new(first.date(), 12, 0, 0, 0).unwrap()),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &prior),
            Err(Error::MissingHistory(_))
        ));
        assert!(
            engine
                .price(
                    &option
                        .replace(AccumulatorChanges {
                            accumulated_quantity: Some(Some(0.0)),
                            ..Default::default()
                        })
                        .unwrap(),
                    &prior
                )
                .is_ok()
        );
        let expiry = market(100.0, day(1, 7), 0.2, 0.0, 0.0);
        assert!(matches!(
            engine.price(&option, &expiry),
            Err(Error::MissingHistory(_))
        ));
        assert_eq!(
            engine
                .price(
                    &option
                        .replace(AccumulatorChanges {
                            accumulated_quantity: Some(Some(0.0)),
                            ..Default::default()
                        })
                        .unwrap(),
                    &expiry
                )
                .unwrap(),
            10.0
        );
        assert_eq!(
            engine
                .price(
                    &option
                        .replace(AccumulatorChanges {
                            accumulated_quantity: Some(Some(10.0)),
                            ..Default::default()
                        })
                        .unwrap(),
                    &expiry
                )
                .unwrap(),
            110.0
        );
        assert_eq!(
            option
                .replace(AccumulatorChanges {
                    accumulated_quantity: Some(Some(10.0)),
                    ..Default::default()
                })
                .unwrap()
                .replace(AccumulatorChanges {
                    accumulated_quantity: Some(None),
                    ..Default::default()
                })
                .unwrap(),
            option
        );
    }
}
