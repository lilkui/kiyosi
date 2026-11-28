#[path = "support/autocallable.rs"]
mod autocallable_support;

use kiyosi::engines::{MonteCarloSampling, MonteCarloSettings, QuadratureSettings};
use kiyosi::instruments::{AutocallableNote, AutocallableTerms};
use kiyosi::*;

#[test]
fn autocallable_batches_reject_an_excessive_contract_count() -> Result<()> {
    let (terms, _, _) = autocallable_support::multi_event_terms(KnockInObservation::AtExpiry);
    let note = SnowballOption::new(terms)?;
    let notes = vec![&note; 1_048_577];
    let engine = MonteCarloAutocallableEngine::default();
    let context = market(100.0, date(1, 1));
    for result in [
        engine.price_many(&notes, &context),
        engine.price_many_on_shared_grid(&notes, &context),
    ] {
        assert!(matches!(
            result,
            Err(Error::InvalidInput(message)) if message.contains("contract batch exceeds")
        ));
    }
    Ok(())
}

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
        autocall_observations: SnowballObservation::from_columns(
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

#[test]
fn portfolio_reduction_replays_across_waves() -> Result<()> {
    let (terms, _, _) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let first = SnowballOption::new(terms)?;
    let second = first.replace(SnowballOptionChanges {
        knock_in_level: Some(85.0),
        ..Default::default()
    })?;
    let notes = [&first, &second];
    let context = market(100.0, date(1, 1));
    let settings = MonteCarloSettings {
        path_count: 32_768,
        seed: Some(42),
        antithetic: false,
        max_active_workers: 1,
        ..Default::default()
    };
    let serial = MonteCarloAutocallableEngine::new(settings)?;
    let expected = [
        serial.price(&first, &context)?,
        serial.price(&second, &context)?,
    ];
    assert_eq!(serial.price_many(&notes, &context)?, expected);
    let parallel = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        max_active_workers: 2,
        ..settings
    })?;
    assert_eq!(parallel.price_many(&notes, &context)?, expected);
    Ok(())
}

#[test]
fn shared_portfolio_coupon_strips_match_two_lane_quotes() -> Result<()> {
    fn check<T: AutocallableTerms>(
        engine: &MonteCarloAutocallableEngine,
        notes: &[AutocallableNote<T>],
        context: &PricingContext,
    ) -> Result<()> {
        let batch = notes.iter().collect::<Vec<_>>();
        let prices = engine.price_many_on_shared_grid(&batch, context)?;
        assert_eq!(prices.len(), batch.len(), "shared note quote count");
        for (&actual, &note) in prices.iter().zip(&batch) {
            let expected = engine.price_many_on_shared_grid(&[note, note], context)?[0];
            assert!((actual - expected).abs() < 2e-14, "{actual} vs {expected}");
        }
        assert_eq!(prices[0].to_bits(), prices[3].to_bits());
        assert!((prices[1] - prices[2]).abs() > 1e-4);
        let serial = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            max_active_workers: 1,
            ..engine.settings()
        })?;
        assert_eq!(prices, serial.price_many_on_shared_grid(&batch, context)?);
        Ok(())
    }
    let clock = date(1, 1).time_of_day();
    let (snow, _, phoenix) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: clock,
        });
    let mut snows = Vec::new();
    let mut phoenixes = Vec::new();
    for coupon in [0.02, 0.08, -0.01, 0.02, 0.04] {
        snows.push(SnowballOption::new(SnowballTerms {
            maturity_coupon_rate: coupon,
            ..snow.clone()
        })?);
        phoenixes.push(PhoenixOption::new(PhoenixTerms {
            coupon_per_period: coupon,
            ..phoenix.clone()
        })?);
    }
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 4096,
            seed: Some(42),
            antithetic: true,
            sampling,
            max_active_workers: 2,
        })?;
        check(&engine, &snows, &context)?;
        check(&engine, &phoenixes, &context)?;
    }
    let outlier = snows[0].replace(SnowballOptionChanges {
        knock_in_level: Some(85.0),
        ..Default::default()
    })?;
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 2048,
        seed: Some(42),
        ..Default::default()
    })?;
    let notes = [&snows[0], &snows[1], &outlier, &snows[2], &snows[4]];
    let mixed = engine.price_many_on_shared_grid(&notes, &context)?;
    assert_eq!(mixed.len(), notes.len(), "mixed note quote count");
    for (&note, value) in notes.iter().zip(mixed) {
        let expected = if note == &outlier {
            engine.price_many_on_shared_grid(&[note, &snows[0]], &context)?[0]
        } else {
            engine.price_many_on_shared_grid(&[note, &outlier], &context)?[0]
        };
        assert!((value - expected).abs() < 2e-14, "{value} vs {expected}");
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
        SnowballObservation::from_columns(&[end], &[100.0], &[0.1]).unwrap(),
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
        SnowballObservation::from_columns(&[date(2, 3), end], &[105.0, 110.0], &[0.08, 0.09])?,
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
        SnowballObservation::from_columns(&[end], &[100.0], &[1.5e308]).unwrap(),
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
            SnowballObservation::from_columns(&[end], &[100.0], &[0.0]).unwrap(),
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
        SnowballObservation::from_columns(&[date(2, 3), date(4, 1)], &[101.0, 105.0], &[0.08; 2])
            .unwrap(),
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
        SnowballObservation::from_columns(&[date(2, 3), end], &[103.0, 105.0], &[0.08; 2]).unwrap(),
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
    let original_settings = unseeded.settings();
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
        SnowballObservation::from_columns(&[end], &[100.0], &[0.0]).unwrap(),
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
    assert_eq!(unseeded.settings(), original_settings);
    assert_eq!(note, original);
    assert_eq!(engine.settings().seed, Some(42));
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
        SnowballObservation::from_columns(&[date(2, 3), date(4, 1)], &[110.0; 2], &[0.08; 2])
            .unwrap(),
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
            assert_eq!(result.get(greek), Some(0.0), "{greek:?} at expiry");
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
        SnowballObservation::from_columns(&[end], &[1e100], &[0.08]).unwrap(),
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
    let observations = SnowballObservation::from_columns(
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
    let measures = [
        Greek::Delta,
        Greek::Gamma,
        Greek::Vega,
        Greek::Rho,
        Greek::DividendRho,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Speed,
    ];
    let request = GreeksRequest::new(measures);
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
        for greek in measures {
            assert!(batch.require(greek)?.is_finite());
        }
    }
    Ok(())
}

#[test]
fn shared_daily_grid_prices_staggered_cash_flows() -> Result<()> {
    let (terms, _, _) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let first = SnowballOption::new(terms.clone())?;
    let second = SnowballOption::new(SnowballTerms {
        knock_in_level: 95.0,
        autocall_observations: SnowballObservation::from_columns(
            &[date(2, 4), date(3, 4), date(4, 1)],
            &[106.0, 103.0, 99.0],
            &[0.12, 0.09, 0.06],
        )?,
        ..terms
    })?;
    let notes = [&second, &first, &second];
    let context = market(100.0, date(1, 1));
    let settings = MonteCarloSettings {
        path_count: 16384,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    };
    let engine = MonteCarloAutocallableEngine::new(settings)?;
    let actual = engine.price_many_on_shared_grid(&notes, &context)?;
    assert_eq!(actual.len(), notes.len(), "shared note quote count");
    let unshared = engine.price_many(&notes, &context)?;
    assert_eq!(unshared.len(), notes.len(), "independent note quote count");
    assert_ne!(
        actual, unshared,
        "the common grid uses a distinct path ordering"
    );
    assert_eq!(actual[0].to_bits(), actual[2].to_bits());
    let parallel = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        max_active_workers: 2,
        ..settings
    })?;
    assert_eq!(
        actual,
        parallel.price_many_on_shared_grid(&notes, &context)?
    );
    let off_clock = SnowballOption::new(SnowballTerms {
        autocall_observations: SnowballObservation::from_columns(
            &[date(2, 3).add_micros(1)?, date(3, 3), date(4, 1)],
            &[103.0, 108.0, 105.0],
            &[0.08; 3],
        )?,
        ..first.terms().clone()
    })?;
    for sampler in [
        engine,
        MonteCarloAutocallableEngine::new(MonteCarloSettings {
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            ..settings
        })?,
    ] {
        let dense = sampler.price_many_on_shared_grid(&[&off_clock, &off_clock], &context)?;
        let sparse = sampler.price_many_on_shared_grid(&notes, &context)?;
        assert_eq!(
            sampler.price_many_on_shared_grid(
                &[&second, &off_clock, &first, &off_clock, &second],
                &context,
            )?,
            [sparse[0], dense[0], sparse[1], dense[1], sparse[2]],
        );
        assert_eq!(
            sampler.price_many_on_shared_grid(&[&first], &context)?,
            [sampler.price(&first, &context)?],
        );
    }
    for (&price, note) in actual.iter().zip(notes) {
        let reference = QuadratureAutocallableEngine::default().price(note, &context)?;
        let estimate = engine.price_with_statistics(note, &context)?;
        assert!((price - reference).abs() < 5.0 * estimate.standard_error().unwrap());
    }
    Ok(())
}

#[test]
fn shared_daily_grid_settles_notes_at_their_own_maturities() -> Result<()> {
    let (terms, _, _) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let long = SnowballOption::new(terms.clone())?;
    let short = SnowballOption::new(SnowballTerms {
        expiry_time: date(3, 4),
        autocall_observations: SnowballObservation::from_columns(
            &[date(2, 3), date(3, 4)],
            &[110.0, 105.0],
            &[0.08; 2],
        )?,
        ..terms
    })?;
    let context = market(100.0, date(1, 1));
    let notes = [&short, &long, &short];
    let oracle = QuadratureAutocallableEngine::default();
    let mut settings = MonteCarloSettings {
        path_count: 16384,
        seed: Some(42),
        max_active_workers: 1,
        antithetic: true,
        ..Default::default()
    };
    let scalar = MonteCarloAutocallableEngine::new(settings)?;
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        settings.sampling = sampling;
        let engine = MonteCarloAutocallableEngine::new(settings)?;
        let shared = engine.price_many_on_shared_grid(&notes, &context)?;
        assert_eq!(shared.len(), notes.len(), "shared note quote count");
        assert_ne!(
            [shared[1], shared[0]],
            engine.price_many(&[&long, &short], &context)?[..],
            "the two maturities share draws"
        );
        assert_eq!(shared[0].to_bits(), shared[2].to_bits());
        let parallel = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            max_active_workers: 2,
            ..settings
        })?;
        assert_eq!(
            parallel.price_many_on_shared_grid(&[&long, &short], &context)?,
            [shared[1], shared[0]],
        );
        for (price, note) in shared.into_iter().zip(notes) {
            let error = scalar
                .price_with_statistics(note, &context)?
                .standard_error()
                .unwrap();
            assert!((price - oracle.price(note, &context)?).abs() < 5.0 * error);
        }
    }
    Ok(())
}

#[test]
fn shared_knock_in_strip_matches_individual_thresholds_in_a_mixed_book() -> Result<()> {
    let (terms, _, _) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let first = SnowballOption::new(terms)?;
    let mut notes = [90.0, 75.0, 95.0, 80.0, 90.0]
        .into_iter()
        .map(|level| {
            first.replace(SnowballOptionChanges {
                knock_in_level: Some(level),
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    notes.insert(
        2,
        first.replace(SnowballOptionChanges {
            maturity_coupon_rate: Some(0.11),
            ..Default::default()
        })?,
    );
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let settings = MonteCarloSettings {
            path_count: 32768,
            seed: Some(42),
            max_active_workers: 2,
            antithetic: true,
            sampling,
        };
        let engine = MonteCarloAutocallableEngine::new(settings)?;
        let batch = notes.iter().collect::<Vec<_>>();
        let prices = engine.price_many_on_shared_grid(&batch, &context)?;
        assert_eq!(prices.len(), notes.len(), "shared note quote count");
        assert_eq!(prices[0].to_bits(), prices[5].to_bits());
        for (&actual, note) in prices.iter().zip(&notes) {
            let expected = engine.price_many_on_shared_grid(&[note, note], &context)?[0];
            assert!((actual - expected).abs() < 2e-14, "{actual} vs {expected}");
        }
        let serial = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            max_active_workers: 1,
            ..settings
        })?;
        assert_eq!(prices, serial.price_many_on_shared_grid(&batch, &context)?);
    }
    Ok(())
}

#[test]
fn mixed_knock_out_strips_match_individual_first_passages() -> Result<()> {
    let (terms, _, _) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let notes = [
        (110.0, 100.0),
        (70.0, 100.0),
        (110.0, 95.0),
        (135.0, 100.0),
        (93.0, 95.0),
        (110.0, 100.0),
        (120.0, 95.0),
        (105.0, 99.0),
    ]
    .map(|(level, upper_strike)| {
        SnowballOption::new(SnowballTerms {
            autocall_observations: SnowballObservation::from_columns(
                &[date(2, 3), date(3, 3), date(4, 1)],
                &[level; 3],
                &[0.08, 0.09, 0.10],
            )?,
            upper_strike,
            ..terms.clone()
        })
    })
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    let batch = notes.iter().collect::<Vec<_>>();
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let settings = MonteCarloSettings {
            path_count: 8192,
            seed: Some(42),
            max_active_workers: 2,
            antithetic: true,
            sampling,
        };
        let engine = MonteCarloAutocallableEngine::new(settings)?;
        let prices = engine.price_many_on_shared_grid(&batch, &context)?;
        assert_eq!(prices.len(), notes.len(), "shared note quote count");
        assert_eq!(prices[0].to_bits(), prices[5].to_bits());
        for (&actual, note) in prices.iter().zip(&notes) {
            let expected = engine.price_many_on_shared_grid(&[note, note], &context)?[0];
            assert!((actual - expected).abs() < 2e-14, "{actual} vs {expected}");
        }
        let serial = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            max_active_workers: 1,
            ..settings
        })?;
        assert_eq!(prices, serial.price_many_on_shared_grid(&batch, &context)?);
    }
    Ok(())
}

#[test]
fn dense_phoenix_events_skip_called_notes_and_preserve_live_cash_flows() -> Result<()> {
    let first = date(1, 2);
    let later = date(2, 3);
    let end = date(4, 1);
    let (_, _, terms) =
        autocallable_support::multi_event_terms(KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        });
    let make = |index: i32, compact: bool| -> Result<PhoenixOption> {
        let survives = index % 3 == 0;
        let dates = if compact && !survives {
            &[first][..]
        } else {
            &[first, later, end]
        };
        PhoenixOption::new(PhoenixTerms {
            knock_out_observations: vec![KnockOutObservation::new(
                first,
                if survives { 500.0 } else { 50.0 },
            )?],
            coupon_observations: dates
                .iter()
                .map(|&date| CouponObservation::new(date, 70.0 + f64::from(index) / 2.0))
                .collect::<Result<_>>()?,
            ..terms.clone()
        })
    };
    let full = (0..70)
        .map(|i| make(i, false))
        .collect::<Result<Vec<_>>>()?;
    let compact = (0..70).map(|i| make(i, true)).collect::<Result<Vec<_>>>()?;
    let context = market(100.0, date(1, 1));
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 2048,
            seed: Some(42),
            sampling,
            antithetic: true,
            max_active_workers: 2,
        })?;
        let price = |book: &[PhoenixOption]| {
            engine.price_many_on_shared_grid(&book.iter().collect::<Vec<_>>(), &context)
        };
        assert_eq!(price(&full)?, price(&compact)?);
        // Two notes take the sparse-event path on the same union of dates.
        assert_eq!(price(&full)?[0].to_bits(), price(&full[..2])?[0].to_bits());
    }
    Ok(())
}
