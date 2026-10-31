use kiyosi::*;

fn day(date: u32) -> Date {
    Date::new(2025, 1, date).unwrap()
}

fn terms(direction: OptionType, dates: Option<Vec<Date>>) -> AccumulatorTerms {
    AccumulatorTerms {
        option_type: direction,
        accumulated_quantity: 3.0,
        knock_out_observation_dates: dates,
        ..AccumulatorTerms::new(
            100.0,
            if direction == OptionType::Call {
                110.0
            } else {
                90.0
            },
            1.0,
            2.0,
            day(1),
            day(6),
        )
    }
}

fn market(spot: f64, at: Timestamp, volatility: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, volatility).unwrap(),
        spot,
        at,
    )
    .unwrap()
}

fn engines() -> [Box<dyn PricingEngine<Accumulator>>; 3] {
    [
        Box::new(QuadratureAccumulatorEngine::default()),
        Box::new(
            FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
                asset_step_count: 1600,
                time_step_count: 400,
                asset_upper_boundary: Some(440.0),
                ..Default::default()
            })
            .unwrap(),
        ),
        Box::new(
            MonteCarloAccumulatorEngine::new(MonteCarloSettings {
                path_count: 32768,
                seed: Some(42),
                thread_count: 1,
                ..Default::default()
            })
            .unwrap(),
        ),
    ]
}

fn terminal_reference(option: &Accumulator, context: &PricingContext) -> f64 {
    let digital = |direction, level| {
        let cash = DigitalOption::cash_or_nothing(
            direction,
            level,
            option.strike(),
            option.effective_date(),
            option.expiry_date(),
        )
        .unwrap();
        let asset = DigitalOption::asset_or_nothing(
            direction,
            level,
            option.effective_date(),
            option.expiry_date(),
        )
        .unwrap();
        AnalyticDigitalEngine.price(&asset, context).unwrap()
            - AnalyticDigitalEngine.price(&cash, context).unwrap()
    };
    let model = context.model_parameters();
    let expiry = option.expiry_date().into();
    let forward = context.spot_price()
        * model
            .dividend_curve()
            .factor(context.valuation_time(), expiry)
            .unwrap()
        - option.strike()
            * model
                .risk_free_curve()
                .factor(context.valuation_time(), expiry)
                .unwrap();
    let observed = option
        .knock_out_observation_dates()
        .is_none_or(|dates| dates.contains(&option.expiry_date()));
    let accelerated = if option.option_type() == OptionType::Call {
        OptionType::Put
    } else {
        OptionType::Call
    };
    let sign = if option.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let purchase = if !observed {
        forward + (option.acceleration_factor() - 1.0) * digital(accelerated, option.strike())
    } else {
        let barrier = digital(accelerated, option.knock_out_level());
        let conventional_order = if option.option_type() == OptionType::Call {
            option.knock_out_level() > option.strike()
        } else {
            option.knock_out_level() < option.strike()
        };
        if conventional_order {
            barrier + (option.acceleration_factor() - 1.0) * digital(accelerated, option.strike())
        } else {
            option.acceleration_factor() * barrier
        }
    };
    sign * (option.accumulated_quantity() * forward + option.daily_quantity() * purchase)
}

#[test]
fn terminal_events_respect_direction_schedule_and_inclusive_knock_out() {
    for (direction, spot, monitored, expected) in [
        (OptionType::Call, 90.0, true, -50.0),
        (OptionType::Call, 100.0, true, 0.0),
        (OptionType::Call, 109.0, true, 36.0),
        (OptionType::Call, 110.0, true, 30.0),
        (OptionType::Call, 111.0, true, 33.0),
        (OptionType::Call, 110.0, false, 40.0),
        (OptionType::Put, 110.0, true, -50.0),
        (OptionType::Put, 100.0, true, 0.0),
        (OptionType::Put, 91.0, true, 36.0),
        (OptionType::Put, 90.0, true, 30.0),
        (OptionType::Put, 89.0, true, 33.0),
        (OptionType::Put, 90.0, false, 40.0),
    ] {
        let dates = Some(if monitored { vec![day(6)] } else { Vec::new() });
        let option = Accumulator::new(terms(direction, dates)).unwrap();
        let context = market(spot, day(6).into(), 0.2);
        for engine in engines() {
            assert_eq!(engine.price(&option, &context).unwrap(), expected);
            let result = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            assert_eq!(result.price(), expected);
            assert!(result.values().iter().all(Option::is_none));
        }
    }
    for direction in [OptionType::Call, OptionType::Put] {
        let mut t = terms(direction, Some(Vec::new()));
        t.effective_date = day(6);
        t.daily_quantity = 0.25;
        t.acceleration_factor = 0.5;
        t.accumulated_quantity = 0.5;
        let option = Accumulator::new(t).unwrap();
        let spot = if direction == OptionType::Call {
            90.0
        } else {
            110.0
        };
        for engine in engines() {
            assert_eq!(
                engine
                    .price(&option, &market(spot, day(6).into(), 0.2))
                    .unwrap(),
                -6.25
            );
        }
    }
}

#[test]
fn sparse_dates_preserve_daily_accumulation_and_stop_only_on_selected_dates() {
    let at = Timestamp::new(day(1), 12, 0, 0, 0).unwrap();
    for (direction, spot) in [(OptionType::Call, 120.0), (OptionType::Put, 80.0)] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 1e-8).unwrap(),
            spot,
            at,
        )
        .unwrap();
        for (dates, expected) in [
            (None, 60.0),
            (Some(vec![day(3)]), 80.0),
            (Some(vec![day(6)]), 100.0),
            (Some(Vec::new()), 120.0),
        ] {
            let option = Accumulator::new(terms(direction, dates)).unwrap();
            for engine in engines() {
                let actual = engine.price(&option, &context).unwrap();
                assert!(
                    (actual - expected).abs() < 2e-7,
                    "{direction:?}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn quadrature_tails_discount_to_the_next_knock_out_or_expiry() {
    let valuation = Timestamp::new(day(1), 12, 0, 0, 0).unwrap();
    let engine = QuadratureAccumulatorEngine::default();
    for (direction, spot, dates, quantity, settlement_date) in [
        (OptionType::Call, 120.0, vec![day(3)], 4.0, day(3)),
        (OptionType::Put, 80.0, vec![day(3)], 4.0, day(3)),
        (OptionType::Call, 120.0, Vec::new(), 6.0, day(6)),
        (OptionType::Put, 80.0, Vec::new(), 6.0, day(6)),
        (OptionType::Call, 80.0, Vec::new(), 9.0, day(6)),
        (OptionType::Put, 120.0, Vec::new(), 9.0, day(6)),
    ] {
        let option = Accumulator::new(terms(direction, Some(dates))).unwrap();
        let context = market(spot, valuation, 1e-8);
        let time = (f64::from(settlement_date.day()) - 1.5) / 365.0;
        let sign = if direction == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let expected =
            sign * quantity * (spot * (-0.01 * time).exp() - 100.0 * (-0.04 * time).exp());
        let actual = engine.price(&option, &context).unwrap();
        assert!(
            (actual - expected).abs() < 1e-7,
            "{direction:?}, spot {spot}, settlement {settlement_date:?}: {actual} != {expected}"
        );
    }
}

#[test]
fn sparse_knock_out_grid_covers_drift_back_inside_the_barrier() {
    let expiry = Date::new(2025, 2, 3).unwrap();
    let time: f64 = 32.5 / 365.0;
    for (direction, spot, barrier, rate, dividend, sign) in [
        (OptionType::Call, 110.05, 110.0, 0.04, 0.06, 1.0),
        (OptionType::Put, 89.95, 90.0, 0.06, 0.04, -1.0),
    ] {
        let option = Accumulator::new(AccumulatorTerms {
            option_type: direction,
            accumulated_quantity: 3.0,
            knock_out_observation_dates: Some(vec![day(15)]),
            ..AccumulatorTerms::new(100.0, barrier, 1.0, 1.0, day(1), expiry)
        })
        .unwrap();
        let alive = if direction == OptionType::Call {
            OptionType::Put
        } else {
            OptionType::Call
        };
        let asset = DigitalOption::asset_or_nothing(alive, barrier, day(1), day(15)).unwrap();
        let cash = DigitalOption::cash_or_nothing(alive, barrier, 100.0, day(1), day(15)).unwrap();
        let observation: f64 = 13.5 / 365.0;
        let remaining = time - observation;
        for sigma in [1e-8, 0.0005, 0.0008] {
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
                spot,
                Timestamp::new(day(1), 12, 0, 0, 0).unwrap(),
            )
            .unwrap();
            let alive_asset = AnalyticDigitalEngine.price(&asset, &context).unwrap();
            let alive_cash = AnalyticDigitalEngine.price(&cash, &context).unwrap();
            let forward =
                spot * (-dividend * observation).exp() - 100.0 * (-rate * observation).exp();
            // Twelve units settle at knock-out; otherwise all 26 settle at expiry.
            let expected = sign
                * (12.0 * (forward - alive_asset + alive_cash)
                    + 26.0
                        * (alive_asset * (-dividend * remaining).exp()
                            - alive_cash * (-rate * remaining).exp()));
            if sigma == 1e-8 {
                let limit =
                    sign * 26.0 * (spot * (-dividend * time).exp() - 100.0 * (-rate * time).exp());
                assert!((expected - limit).abs() < 1e-9);
            }
            for (nodes, tolerance) in [(512, 0.002), (2048, 0.00002)] {
                let engine = QuadratureAccumulatorEngine::new(QuadratureSettings {
                    asset_step_count: nodes,
                    ..Default::default()
                })
                .unwrap();
                let actual = engine.price(&option, &context).unwrap();
                assert!(
                    (actual - expected).abs() < tolerance,
                    "{direction:?}, sigma {sigma}, nodes {nodes}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn explicit_daily_zero_payoffs_keep_known_statistics_and_skip_grid_validation() {
    let monte_carlo = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let clipped = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(50.0),
        ..Default::default()
    })
    .unwrap();
    let context = market(100.0, Timestamp::new(day(1), 12, 0, 0, 0).unwrap(), 0.2);
    for (direction, barrier) in [(OptionType::Call, 90.0), (OptionType::Put, 110.0)] {
        for dates in [
            None,
            Some(vec![day(1), day(2), day(3)]),
            Some(vec![day(2), day(3)]),
        ] {
            let option = Accumulator::new(AccumulatorTerms {
                option_type: direction,
                knock_out_observation_dates: dates,
                ..AccumulatorTerms::new(100.0, barrier, 1.0, 0.0, day(1), day(3))
            })
            .unwrap();
            let result = monte_carlo
                .price_with_statistics(&option, &context)
                .unwrap();
            assert_eq!(result.price(), 0.0);
            assert_eq!(result.standard_error(), Some(0.0));
            assert_eq!(result.confidence_interval(), Some((0.0, 0.0)));
            assert_eq!(clipped.price(&option, &context).unwrap(), 0.0);
            let holding_context = PricingContext::new(
                BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
                if direction == OptionType::Call {
                    85.0
                } else {
                    115.0
                },
                context.valuation_time(),
            )
            .unwrap();
            let holding = option.replace(3.0).unwrap();
            assert_eq!(clipped.price(&holding, &holding_context).unwrap(), -45.0);
            assert_eq!(
                monte_carlo
                    .price_with_statistics(&holding, &holding_context)
                    .unwrap()
                    .standard_error(),
                Some(0.0)
            );
        }
        let option = Accumulator::new(AccumulatorTerms {
            option_type: direction,
            knock_out_observation_dates: Some(vec![day(3)]),
            ..AccumulatorTerms::new(100.0, barrier, 1.0, 0.0, day(1), day(3))
        })
        .unwrap();
        let spot = if direction == OptionType::Call {
            120.0
        } else {
            80.0
        };
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 1e-8).unwrap(),
            spot,
            context.valuation_time(),
        )
        .unwrap();
        let holidays = context
            .replace(PricingContextChanges {
                calendar: Some(
                    TradingCalendar::custom(
                        TradingCalendar::weekdays(),
                        vec![day(2)],
                        vec![],
                        None,
                    )
                    .unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        let result = monte_carlo
            .price_with_statistics(&option, &holidays)
            .unwrap();
        assert_eq!(result.standard_error(), Some(0.0));
        assert_eq!(clipped.price(&option, &holidays).unwrap(), 0.0);
        let engine = QuadratureAccumulatorEngine::default();
        assert!((engine.price(&option, &context).unwrap() - 20.0).abs() < 1e-7);
        let at_current_event = context
            .replace(PricingContextChanges {
                valuation_time: Some(day(2).into()),
                ..Default::default()
            })
            .unwrap();
        assert!((engine.price(&option, &at_current_event).unwrap() - 20.0).abs() < 1e-7);
    }
}

#[test]
fn zero_multiplier_with_inverted_barrier_still_accrues_on_non_knock_out_dates() {
    let at = Timestamp::new(day(1), 12, 0, 0, 0).unwrap();
    for (direction, barrier, spot) in [
        (OptionType::Call, 95.0, 120.0),
        (OptionType::Put, 105.0, 80.0),
    ] {
        let mut t = terms(direction, Some(Vec::new()));
        t.knock_out_level = barrier;
        t.acceleration_factor = 0.0;
        t.accumulated_quantity = 0.0;
        let option = Accumulator::new(t).unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, 1e-8).unwrap(),
            spot,
            at,
        )
        .unwrap();
        for engine in engines() {
            let actual = engine.price(&option, &context).unwrap();
            assert!((actual - 60.0).abs() < 2e-7, "{direction:?}: {actual}");
        }
    }
}

#[test]
fn explicit_daily_dates_preserve_default_prices_for_both_directions() {
    let context = market(100.25, Timestamp::new(day(1), 12, 0, 0, 0).unwrap(), 0.4);
    for direction in [OptionType::Call, OptionType::Put] {
        let default = Accumulator::new(terms(direction, None)).unwrap();
        let explicit =
            Accumulator::new(terms(direction, Some(vec![day(1), day(2), day(3), day(6)]))).unwrap();
        assert_eq!(default.option_type(), direction);
        assert_eq!(default.knock_out_observation_dates(), None);
        for engine in engines() {
            assert_eq!(
                engine.price(&default, &context).unwrap(),
                engine.price(&explicit, &context).unwrap()
            );
        }
    }
}

#[test]
fn date_validation_and_history_replacement_preserve_owned_terms() {
    for dates in [
        vec![day(3), day(2)],
        vec![day(2), day(2)],
        vec![Date::new(2024, 12, 31).unwrap()],
        vec![day(7)],
    ] {
        assert!(matches!(
            Accumulator::new(terms(OptionType::Put, Some(dates))),
            Err(Error::InvalidInput(_))
        ));
    }
    let mut dates = vec![day(2), day(6)];
    let option = Accumulator::new(terms(OptionType::Put, Some(dates.clone()))).unwrap();
    dates.clear();
    let updated = option.replace(0.25).unwrap();
    assert_eq!(updated.option_type(), OptionType::Put);
    assert_eq!(
        updated.knock_out_observation_dates(),
        Some([day(2), day(6)].as_slice())
    );
    assert_eq!(updated.accumulated_quantity(), 0.25);
    assert_eq!(option.accumulated_quantity(), 3.0);
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            updated.replace(invalid),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(updated.accumulated_quantity(), 0.25);
    }
    let weekend = Accumulator::new(terms(OptionType::Put, Some(vec![day(4)]))).unwrap();
    let weekday_context = market(100.25, Timestamp::new(day(1), 12, 0, 0, 0).unwrap(), 0.2);
    let all_days_context = weekday_context
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        })
        .unwrap();
    for engine in engines() {
        assert!(matches!(
            engine.price(&weekend, &weekday_context),
            Err(Error::InvalidInput(_))
        ));
        assert!(
            engine
                .price(&weekend, &all_days_context)
                .unwrap()
                .is_finite()
        );
        assert!(engine.price(&option, &weekday_context).unwrap().is_finite());
    }
}

#[test]
fn midnight_purchases_use_strict_strike_comparisons_and_intraday_history_once() {
    let below = f64::from_bits(100_f64.to_bits() - 1);
    let above = f64::from_bits(100_f64.to_bits() + 1);
    for (direction, spot, quantity) in [
        (OptionType::Call, below, 5.0),
        (OptionType::Call, 100.0, 4.0),
        (OptionType::Call, above, 4.0),
        (OptionType::Call, 120.0, 4.0),
        (OptionType::Put, below, 4.0),
        (OptionType::Put, 100.0, 4.0),
        (OptionType::Put, above, 5.0),
        (OptionType::Put, 80.0, 4.0),
    ] {
        let option = Accumulator::new(terms(direction, Some(Vec::new()))).unwrap();
        let at_event = market(spot, day(3).into(), 0.4);
        let after_event = market(spot, Timestamp::new(day(3), 0, 0, 0, 1).unwrap(), 0.4);
        let history = option.replace(quantity).unwrap();
        for engine in engines() {
            let at_price = engine.price(&option, &at_event).unwrap();
            let after_price = engine.price(&history, &after_event).unwrap();
            assert!(
                (at_price - after_price).abs() < 1e-7,
                "{direction:?}, spot {spot}: {at_price} != {after_price}"
            );
        }
        let intraday = market(spot, Timestamp::new(day(3), 12, 0, 0, 0).unwrap(), 0.4);
        let expected = terminal_reference(&history, &intraday);
        let actual = QuadratureAccumulatorEngine::default()
            .price(&history, &intraday)
            .unwrap();
        assert!((actual - expected).abs() < 1e-9);
    }
}

#[test]
fn terminal_digital_portfolio_covers_directions_barrier_orderings_and_empty_dates() {
    let engine = QuadratureAccumulatorEngine::new(QuadratureSettings {
        asset_step_count: 4,
        integration_standard_deviations: 0.1,
    })
    .unwrap();
    let context = market(100.25, Timestamp::new(day(3), 12, 0, 0, 0).unwrap(), 0.6);
    for direction in [OptionType::Call, OptionType::Put] {
        for barrier in [95.0, 100.0, 105.0] {
            for multiplier in [0.0, 0.5, 2.0] {
                for monitored in [false, true] {
                    let mut t = terms(
                        direction,
                        Some(if monitored { vec![day(6)] } else { Vec::new() }),
                    );
                    t.knock_out_level = barrier;
                    t.acceleration_factor = multiplier;
                    let option = Accumulator::new(t).unwrap();
                    let expected = terminal_reference(&option, &context);
                    let actual = engine.price(&option, &context).unwrap();
                    assert!(
                        (actual - expected).abs() < 1e-9,
                        "{direction:?}, barrier {barrier}, multiplier {multiplier}, monitored {monitored}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn finite_difference_converges_and_monte_carlo_matches_terminal_portfolio() {
    let context = market(100.25, Timestamp::new(day(3), 12, 0, 0, 0).unwrap(), 0.4);
    let monte_carlo = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        thread_count: 1,
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for monitored in [false, true] {
            let option = Accumulator::new(terms(
                direction,
                Some(if monitored { vec![day(6)] } else { Vec::new() }),
            ))
            .unwrap();
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
            assert!(
                errors[1] < 0.001,
                "{direction:?}, monitored {monitored}: {errors:?}"
            );
            assert!(
                errors[1] < errors[0],
                "{direction:?}, monitored {monitored}: {errors:?}"
            );
            let estimate = monte_carlo
                .price_with_statistics(&option, &context)
                .unwrap();
            assert!(
                (estimate.price() - expected).abs()
                    < 4.0 * estimate.standard_error().unwrap() + 0.02
            );
        }
    }
}

#[test]
fn sparse_multi_day_prices_agree_with_sampling_and_grid_refinement() {
    let context = market(100.25, Timestamp::new(day(1), 12, 0, 0, 0).unwrap(), 0.6);
    let monte_carlo = MonteCarloAccumulatorEngine::new(MonteCarloSettings {
        path_count: 65536,
        seed: Some(42),
        thread_count: 1,
        ..Default::default()
    })
    .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        for dates in [vec![day(3)], vec![day(2), day(6)], Vec::new()] {
            let option = Accumulator::new(terms(direction, Some(dates))).unwrap();
            let reference = QuadratureAccumulatorEngine::new(QuadratureSettings {
                asset_step_count: 1024,
                ..Default::default()
            })
            .unwrap()
            .price(&option, &context)
            .unwrap();
            let coarse = QuadratureAccumulatorEngine::new(QuadratureSettings {
                asset_step_count: 512,
                ..Default::default()
            })
            .unwrap()
            .price(&option, &context)
            .unwrap();
            assert!(
                (coarse - reference).abs() < 0.002,
                "{direction:?}: {coarse} != {reference}"
            );
            let finite_difference =
                FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
                    asset_step_count: 1600,
                    time_step_count: 2000,
                    asset_upper_boundary: Some(440.0),
                    ..Default::default()
                })
                .unwrap()
                .price(&option, &context)
                .unwrap();
            assert!(
                (finite_difference - reference).abs() < 0.01,
                "{direction:?}: {finite_difference} != {reference}"
            );
            let estimate = monte_carlo
                .price_with_statistics(&option, &context)
                .unwrap();
            assert!(
                (estimate.price() - reference).abs()
                    < 4.0 * estimate.standard_error().unwrap() + 0.02
            );
        }
    }
}

#[test]
fn scheduled_decumulator_supports_greeks_and_identifiable_volatility() {
    let option = Accumulator::new(terms(OptionType::Put, Some(vec![day(6)]))).unwrap();
    let original = option.clone();
    let context = market(100.25, Timestamp::new(day(3), 12, 0, 0, 0).unwrap(), 0.4);
    let h = NumericalShiftSettings::default().spot_shift;
    let shifted = |spot| {
        context
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap()
    };
    let up = terminal_reference(&option, &shifted(context.spot_price() + h));
    let down = terminal_reference(&option, &shifted(context.spot_price() - h));
    let expected_delta = (up - down) / (2.0 * h);
    let expected_gamma = (up - 2.0 * terminal_reference(&option, &context) + down) / h.powi(2);
    for (index, engine) in engines().into_iter().enumerate() {
        let result = engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega]),
                Default::default(),
            )
            .unwrap();
        assert_eq!(result.price(), engine.price(&option, &context).unwrap());
        for greek in [Greek::Delta, Greek::Gamma, Greek::Vega] {
            assert!(result.has(greek), "missing {greek:?}");
        }
        if index < 2 {
            let tolerance = if index == 0 { 1e-6 } else { 0.002 };
            assert!((result.require(Greek::Delta).unwrap() - expected_delta).abs() < tolerance);
            assert!((result.require(Greek::Gamma).unwrap() - expected_gamma).abs() < tolerance);
        }
        let volatility = implied_volatility(
            engine.as_ref(),
            &option,
            &context,
            result.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.2,
                upper_bound: 0.6,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((volatility - 0.4).abs() < 1e-6, "{volatility}");
        assert_eq!(option, original);
    }
    for daily in [0.0, 1.0] {
        let mut t = terms(OptionType::Put, Some(Vec::new()));
        t.daily_quantity = daily;
        t.acceleration_factor = 1.0;
        let linear = Accumulator::new(t).unwrap();
        for engine in engines() {
            let quote = engine.price(&linear, &context).unwrap();
            assert!(matches!(
                implied_volatility(
                    engine.as_ref(),
                    &linear,
                    &context,
                    quote,
                    Default::default()
                ),
                Err(Error::UnsupportedOperation(_))
            ));
        }
    }
}

#[test]
fn adjacent_barrier_levels_preserve_forward_prices_and_greeks() {
    let time: f64 = 4.5 / 365.0;
    let levels = [
        f64::from_bits(100_f64.to_bits() - 1),
        100.0,
        f64::from_bits(100_f64.to_bits() + 1),
        100.0 + 1e-11,
        100.0 + 1e-8,
    ];
    for direction in [OptionType::Call, OptionType::Put] {
        let sign = if direction == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let expected = sign * 6.0 * (100.001 * (-0.06 * time).exp() - 100.0 * (-0.04 * time).exp());
        let delta = sign * 6.0 * (-0.06 * time).exp();
        for sigma in [1e-8, 0.0001, 0.2] {
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.04, 0.06, sigma).unwrap(),
                100.001,
                Timestamp::new(day(1), 12, 0, 0, 0).unwrap(),
            )
            .unwrap();
            for barrier in levels {
                let option = Accumulator::new(AccumulatorTerms {
                    option_type: direction,
                    acceleration_factor: 1.0,
                    knock_out_level: barrier,
                    ..terms(direction, Some(Vec::new()))
                })
                .unwrap();
                for nodes in [512, 1024] {
                    let engine = QuadratureAccumulatorEngine::new(QuadratureSettings {
                        asset_step_count: nodes,
                        ..Default::default()
                    })
                    .unwrap();
                    let result = engine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                            Default::default(),
                        )
                        .unwrap();
                    assert!(
                        (result.price() - expected).abs() < 1e-8,
                        "{direction:?}, sigma {sigma}, level {barrier:?}, nodes {nodes}: {} != {expected}",
                        result.price()
                    );
                    assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-7);
                    assert!(result.require(Greek::Gamma).unwrap().abs() < 1e-6);
                }
            }
        }
    }
}

#[test]
fn current_greek_discontinuities_follow_the_knock_out_schedule() {
    for (direction, barrier) in [(OptionType::Call, 110.0), (OptionType::Put, 90.0)] {
        for monitored in [false, true] {
            let option = Accumulator::new(terms(
                direction,
                Some(if monitored { vec![day(3)] } else { Vec::new() }),
            ))
            .unwrap();
            let context = market(barrier, day(3).into(), 0.4);
            for engine in engines() {
                let result = engine
                    .price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega]),
                        Default::default(),
                    )
                    .unwrap();
                assert_eq!(result.has(Greek::Delta), !monitored);
                assert_eq!(result.has(Greek::Gamma), !monitored);
                assert!(result.has(Greek::Vega));
            }
        }
        let option = Accumulator::new(terms(direction, Some(Vec::new()))).unwrap();
        for engine in engines() {
            let result = engine
                .price_with_greeks(
                    &option,
                    &market(100.0, day(3).into(), 0.4),
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            assert!(!result.has(Greek::Delta));
            assert!(!result.has(Greek::Gamma));
        }
    }
}
