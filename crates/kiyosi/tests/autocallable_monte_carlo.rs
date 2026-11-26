#[path = "support/autocallable.rs"]
mod autocallable_support;

use kiyosi::*;

#[test]
fn portfolio_prices_replay_scalar_paths() -> Result<()> {
    let (mut terms, _, _) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let first = SnowballOption::new(terms.clone())?;
    terms.maturity_coupon_rate = 0.09;
    terms.knock_in_level = 90.0;
    let second = SnowballOption::new(terms.clone())?;
    let early = SnowballOption::new(SnowballTerms {
        autocall_observations: autocallable_observations(
            &[date(2, 3), date(3, 3), date(4, 1)],
            &[85.0; 3],
            &[0.08; 3],
        )?,
        ..terms.clone()
    })?;
    terms.knock_in_observation = KnockInObservation::AtExpiry;
    let different_grid = SnowballOption::new(terms)?;
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 2048,
            seed: Some(42),
            sampling,
            antithetic: true,
            max_active_workers: 2,
        })?;
        for notes in [
            vec![&first, &second, &first],
            std::iter::once(&first)
                .chain(std::iter::repeat_n(&early, 32))
                .collect(),
            vec![&first, &different_grid, &second],
            (0..41)
                .map(|index| [&first, &different_grid, &second][index % 3])
                .collect(),
        ] {
            let expected = notes
                .iter()
                .map(|note| engine.price(note, &context).map(f64::to_bits));
            let actual = engine.price_many(&notes, &context)?;
            assert_eq!(actual.len(), notes.len());
            for (price, scalar) in actual.into_iter().zip(expected) {
                assert_eq!(price.to_bits(), scalar?);
            }
        }
    }
    Ok(())
}

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}

fn market(spot: f64, valuation: impl Into<Timestamp>) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation.into(),
    )
    .unwrap()
}

#[test]
fn binary_price_and_uncertainty_match_an_independent_terminal_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        date(1, 1),
        end,
        autocallable_observations(&[end], &[100.0], &[0.1]).unwrap(),
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 0.1, date(1, 1), end).unwrap();
    let context = market(100.0, date(1, 1));
    let expected = (-0.04_f64).exp() + AnalyticDigitalEngine.price(&digital, &context).unwrap();
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let estimate = engine.price_with_statistics(&note, &context).unwrap();
    assert_eq!(engine.price(&note, &context).unwrap(), estimate.price());
    let error = estimate.standard_error().unwrap();
    assert!((estimate.price() - expected).abs() < 4.0 * error);
    let (lower, upper) = estimate.confidence_interval().unwrap();
    assert!((upper - lower - 2.0 * 1.959_963_984_540_054 * error).abs() < 1e-14);
}

#[test]
fn shared_coupon_endpoints_replay_scalar_paths_and_detect_different_grids() -> Result<()> {
    let start = date(1, 1);
    let end = date(4, 1);
    let context = market(100.0, start);
    let note = SnowballOption::new(SnowballTerms::new(
        0.08,
        100.0,
        90.0,
        100.0,
        100.0,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[105.0, 110.0], &[0.08, 0.09])?,
        KnockInObservation::EveryTradingDay {
            observation_clock_time: start.time_of_day(),
        },
        TradingCalendar::weekdays(),
    ))?;
    let lower = note.with_coupon(-0.05, CouponQuoteConvention::PreserveMaturityCoupon)?;
    let upper = note.with_coupon(0.23, CouponQuoteConvention::PreserveMaturityCoupon)?;
    let collapsed = note.with_coupon(0.0, CouponQuoteConvention::ShiftMaturityCoupon)?;
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 1024,
            seed: Some(31),
            sampling,
            antithetic: true,
            max_active_workers: 2,
        })?;
        assert_eq!(
            engine.price_coupon_bounds(&lower, &upper, &context)?,
            Some([
                engine.price(&lower, &context)?,
                engine.price(&upper, &context)?
            ]),
        );
        assert_eq!(
            engine.price_coupon_bounds(&collapsed, &upper, &context)?,
            None
        );
    }
    Ok(())
}

#[test]
fn scalar_price_survives_confidence_interval_overflow() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let mut terms = BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[100.0], &[1.5e308]).unwrap(),
        TradingCalendar::weekdays(),
    );
    terms.principal_redemption_ratio = 0.0;
    let note = BinarySnowballOption::new(terms).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let settings = MonteCarloSettings {
        path_count: 2,
        seed: Some(0),
        max_active_workers: 1,
        ..Default::default()
    };
    let engine = MonteCarloAutocallableEngine::new(settings).unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 1.5e308, start, end).unwrap();
    let expected = MonteCarloDigitalEngine::new(settings)
        .unwrap()
        .price(&digital, &context)
        .unwrap();
    assert!((expected / 7.5e307 - 1.0).abs() < 1e-12);
    assert!((engine.price(&note, &context).unwrap() / expected - 1.0).abs() < 1e-12);
    assert!(matches!(
        engine.price_with_statistics(&note, &context),
        Err(Error::ComputationFailure(_))
    ));
}

#[test]
fn discounted_cashflows_preserve_representable_values_beyond_discount_range() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(1),
        ..Default::default()
    })
    .unwrap();
    for (principal, rate) in [(1e308_f64, 1000.0), (1e-308, -710.0)] {
        let mut terms = BinarySnowballTerms::new(
            0.0,
            start,
            end,
            autocallable_observations(&[end], &[100.0], &[0.0]).unwrap(),
            TradingCalendar::weekdays(),
        );
        terms.principal_redemption_ratio = principal;
        let note = BinarySnowballOption::new(terms).unwrap();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, rate, 0.2).unwrap(),
            100.0,
            start,
        )
        .unwrap();
        let expected = (principal.ln() - rate).exp();
        let actual = engine.price(&note, &context).unwrap();
        assert!(
            (actual / expected - 1.0).abs() < 1e-12,
            "{actual} != {expected}"
        );
    }
    let mut terms = PhoenixTerms::new(
        -1e308,
        100.0,
        80.0,
        100.0,
        100.0,
        start,
        end,
        [end]
            .iter()
            .zip([1.0].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        [end]
            .iter()
            .zip([0.0].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.principal_redemption_ratio = 1e308;
    let note = PhoenixOption::new(terms).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(-1.0, -1.0, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    assert_eq!(engine.price(&note, &context).unwrap(), 0.0);
}

#[test]
fn sampling_settings_counts_and_threads_preserve_prices_and_uncertainty() {
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.02,
        date(1, 1),
        date(4, 1),
        autocallable_observations(&[date(2, 3), date(4, 1)], &[101.0, 105.0], &[0.08; 2]).unwrap(),
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        for antithetic in [false, true] {
            for path_count in [1, 2, 32, 1024] {
                let settings = MonteCarloSettings {
                    path_count,
                    seed: Some(u64::MAX),
                    sampling,
                    antithetic,
                    max_active_workers: 1,
                };
                let serial = MonteCarloAutocallableEngine::new(settings)
                    .unwrap()
                    .price_with_statistics(&note, &context)
                    .unwrap();
                let parallel = MonteCarloAutocallableEngine::new(MonteCarloSettings {
                    max_active_workers: 3,
                    ..settings
                })
                .unwrap()
                .price_with_statistics(&note, &context)
                .unwrap();
                assert_eq!(serial, parallel);
                assert_eq!(
                    serial.standard_error().is_some(),
                    sampling == MonteCarloSampling::PseudoRandom
                        && path_count > if antithetic { 2 } else { 1 }
                );
            }
        }
    }
    let odd = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 31,
        seed: Some(42),
        antithetic: true,
        ..Default::default()
    })
    .unwrap();
    assert!(
        odd.price_with_statistics(&note, &context)
            .unwrap()
            .standard_error()
            .is_none()
    );
    for settings in [
        MonteCarloSettings {
            path_count: 0,
            ..Default::default()
        },
        MonteCarloSettings {
            max_active_workers: -1,
            ..Default::default()
        },
        MonteCarloSettings {
            path_count: 31,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            MonteCarloAutocallableEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn shared_greeks_and_solvers_leave_engine_and_inputs_unchanged() {
    let end = date(4, 1);
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        95.0,
        100.0,
        60.0,
        date(1, 1),
        end,
        autocallable_observations(&[date(2, 3), end], &[103.0, 105.0], &[0.08; 2]).unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);

    let note = SnowballOption::new(terms).unwrap();
    let original = note.clone();
    let context = market(100.0, date(1, 1));
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 4096,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let shifts = NumericalShiftSettings {
        spot_shift: Some(0.25),
        ..Default::default()
    };
    let result =
        kiyosi::numerical_greeks(&engine, &note, &context, GreeksRequest::all(), shifts).unwrap();
    let bump = |spot| {
        engine
            .price(
                &note,
                &context
                    .replace(PricingContextChanges {
                        spot_price: Some(spot),
                        ..Default::default()
                    })
                    .unwrap(),
            )
            .unwrap()
    };
    let (up, down) = (bump(100.25), bump(99.75));
    assert_eq!(result.require(Greek::Delta).unwrap(), (up - down) / 0.5);
    assert_eq!(
        result.require(Greek::Gamma).unwrap(),
        (up - 2.0 * result.price() + down) / 0.25 / 0.25
    );
    let volatility = implied_volatility(
        &engine,
        &note,
        &context,
        result.price(),
        ImpliedVolatilitySettings::new(0.2, 0.4),
    )
    .unwrap();
    assert!((volatility - 0.3).abs() < 1e-6);
    for convention in [
        CouponQuoteConvention::ShiftMaturityCoupon,
        CouponQuoteConvention::PreserveMaturityCoupon,
    ] {
        let quote = engine
            .price(&note.with_coupon(0.12, convention).unwrap(), &context)
            .unwrap();
        let coupon = implied_snowball_coupon(
            &engine,
            &note,
            &context,
            quote,
            convention,
            ImpliedCouponSettings::default(),
        )
        .unwrap()
        .first_observation_coupon;
        assert!((coupon - 0.12).abs() < 1e-8);
    }
    let unseeded = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 1024,
        ..Default::default()
    })
    .unwrap();
    let original_settings = unseeded.sampling_settings();
    let prepared =
        <MonteCarloAutocallableEngine as PricingEngine<SnowballOption>>::prepare_request(&unseeded)
            .unwrap()
            .unwrap();
    // A continuous terminal put spread avoids unidentified roots on sampled barrier jumps.
    let mut smooth_terms = SnowballTerms::new(
        0.0,
        100.0,
        100.0,
        100.0,
        60.0,
        date(1, 1),
        end,
        autocallable_observations(&[end], &[100.0], &[0.0]).unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    smooth_terms.knock_in_observation = KnockInObservation::AtExpiry;
    let smooth = SnowballOption::new(smooth_terms).unwrap();
    let price = prepared.price(&smooth, &context).unwrap();
    assert_eq!(price, prepared.price(&smooth, &context).unwrap());
    assert!(prepared.prepare_request().unwrap().is_none());
    let sigma = implied_volatility(
        prepared.as_ref(),
        &smooth,
        &context,
        price,
        ImpliedVolatilitySettings::new(0.2, 0.4),
    )
    .unwrap();
    assert!((sigma - 0.3).abs() < 1e-6);
    assert_eq!(unseeded.sampling_settings(), original_settings);
    assert_eq!(note, original);
    assert_eq!(engine.sampling_settings().seed, Some(42));
}

#[test]
fn known_cashflows_and_history_validate_before_shortcuts() {
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        autocallable_observations(&[date(2, 3), date(4, 1)], &[110.0; 2], &[0.08; 2]).unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (date(4, 1)).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 1,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let missing = SnowballOption::new(terms.clone()).unwrap();
    assert!(matches!(
        engine
            .price(&missing, &market(100.0, date(1, 2)))
            .unwrap_err(),
        Error::MissingHistory(_)
    ));
    terms.barrier_state = Some(AutocallableBarrierState::KnockedOut);

    let out = SnowballOption::new(terms.clone()).unwrap();
    assert!(matches!(
        engine.price(&out, &market(100.0, date(1, 1))).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let estimate = engine
        .price_with_statistics(&out, &market(100.0, date(3, 3)))
        .unwrap();
    assert_eq!(estimate.price(), 0.0);
    assert_eq!(estimate.standard_error(), Some(0.0));
    assert!(matches!(
        implied_volatility(
            &engine,
            &out,
            &market(100.0, date(3, 3)),
            0.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);

    let note = SnowballOption::new(terms).unwrap();
    for (spot, expected) in [
        (70.0, 0.7),
        (80.0, 1.0 + 0.06 * 90.0 / 365.0),
        (110.0, 1.0 + 0.08 * 90.0 / 365.0),
    ] {
        let context = market(spot, date(4, 1));
        assert!((engine.price(&note, &context).unwrap() - expected).abs() < 1e-14);
        let result = engine
            .price_with_greeks(&note, &context, GreeksRequest::all())
            .unwrap();
        for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
            assert_eq!(result.get(greek), None);
        }
        for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
            if result.supported().has(greek) {
                assert_eq!(result.require(greek).unwrap(), 0.0);
            }
        }
    }
    let context = market(100.0, date(1, 1));
    let node = RateTermStructure::new(
        date(1, 1),
        vec![date(4, 1), (date(4, 1)).add_days(1).unwrap()],
        vec![0.99, (0.99) * 0.99],
    )
    .unwrap();
    let curved = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                context
                    .model_parameters()
                    .as_bsm()
                    .unwrap()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(Rate::Curve(node)),
                        ..Default::default()
                    })
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&note, &curved).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn sobol_dimension_limit_applies_only_to_remaining_simulation_dates() {
    let start = date(1, 1);
    let end = Date::new(2110, 1, 1).unwrap().start_of_day();
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        1e-100,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(&[end], &[1e100], &[0.08]).unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    let note = SnowballOption::new(terms.clone()).unwrap();
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine.price(&note, &market(100.0, start)).unwrap_err(),
        Error::InvalidInput(message) if message.contains("Sobol dimension")
    ));
    terms.knock_in_observation = KnockInObservation::AtExpiry;
    let expiry_only = SnowballOption::new(terms).unwrap();
    let actual = engine.price(&expiry_only, &market(100.0, start)).unwrap();

    // Both seeded paths finish between the deliberately distant barriers.
    let years = start.years_until(end);
    let expected = (1.0 + 0.06 * years) * (-0.04 * years).exp();
    assert!(
        (actual - expected).abs() < 1e-12,
        "price {actual}, expected {expected}"
    );
}

#[test]
fn daily_monitoring_and_all_variants_agree_with_quadrature() {
    fn check<T: AutocallableTerms>(note: &AutocallableNote<T>, context: &PricingContext) {
        let reference = QuadratureAutocallableEngine::new(QuadratureSettings {
            asset_step_count: 1024,
            ..Default::default()
        })
        .unwrap()
        .price(note, context)
        .unwrap();
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 65536,
            seed: Some(42),
            antithetic: true,
            ..Default::default()
        })
        .unwrap();
        let actual = engine.price_with_statistics(note, context).unwrap();
        assert!(
            (actual.price() - reference).abs() < 4.0 * actual.standard_error().unwrap() + 1e-5,
            "{} != {reference}",
            actual.price()
        );
    }

    let start = date(1, 1);
    let end = date(4, 1);
    let observations = autocallable_observations(
        &[date(2, 3), date(3, 3), end],
        &[110.0, 108.0, 105.0],
        &[0.08; 3],
    )
    .unwrap();
    let context = market(100.0, start);
    for mode in [
        KnockInObservation::AtExpiry,
        KnockInObservation::EveryTradingDay {
            observation_clock_time: kiyosi::TimeOfDay::new(0, 0, 0, 0).unwrap(),
        },
    ] {
        let (snow, ternary, phoenix) = autocallable_support::multi_event_terms(mode);

        check(&SnowballOption::new(snow).unwrap(), &context);
        check(&TernarySnowballOption::new(ternary).unwrap(), &context);
        check(&PhoenixOption::new(phoenix).unwrap(), &context);
        check(
            &BinarySnowballOption::new(BinarySnowballTerms::new(
                0.06,
                start,
                end,
                observations.clone(),
                TradingCalendar::weekdays(),
            ))
            .unwrap(),
            &context,
        );
    }
}

#[test]
fn batched_phoenix_spot_and_market_greeks_replay_scalar_stencils() -> Result<()> {
    let start = date(1, 1);
    let end = date(4, 1);
    let first = date(2, 3);
    let note = PhoenixOption::new(PhoenixTerms::new(
        0.02,
        100.0,
        90.0,
        100.0,
        60.0,
        start,
        end,
        vec![
            KnockOutObservation::new(first, 101.0)?,
            KnockOutObservation::new(end, 105.0)?,
        ],
        vec![
            CouponObservation::new(first, 99.0)?,
            CouponObservation::new(end, 102.0)?,
        ],
        KnockInObservation::EveryTradingDay {
            observation_clock_time: start.time_of_day(),
        },
        TradingCalendar::weekdays(),
    ))?;
    let context = market(100.0, start);
    let shift = NumericalShiftSettings {
        spot_shift: Some(4.0),
        ..Default::default()
    };
    let request = GreeksRequest::new([
        Greek::Delta,
        Greek::Gamma,
        Greek::Vega,
        Greek::Rho,
        Greek::DividendRho,
    ]);
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 2048,
            seed: Some(31),
            sampling,
            antithetic: true,
            max_active_workers: 2,
        })?;
        let batch = engine.price_with_greeks_with_settings(&note, &context, request, shift)?;
        let scalar = numerical_greeks(&engine, &note, &context, request, shift)?;
        assert_eq!(batch, scalar, "{sampling:?} must replay the scalar paths");
        for greek in [
            Greek::Delta,
            Greek::Gamma,
            Greek::Vega,
            Greek::Rho,
            Greek::DividendRho,
        ] {
            assert!(batch.require(greek)?.is_finite());
        }
    }
    Ok(())
}
