mod support;
use support::dated;

use kiyosi::*;

#[test]
fn greek_requests_reject_disjoint_support_and_preserve_partial_results() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(365)?;
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        105.0,
        start,
    )?;
    let heston = PricingContext::new(
        HestonParameters::new(HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.5,
            vol_of_vol: 0.3,
            correlation: -0.7,
        })?,
        105.0,
        start,
    )?;
    let local = PricingContext::new(
        LocalVolatilityParameters::new(
            0.05,
            0.02,
            LocalVolatilitySurface::new(
                vec![start, end],
                vec![80.0, 120.0],
                vec![vec![0.2; 2]; 2],
            )?,
        )?,
        105.0,
        start,
    )?;
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))?
    .replace_history(Some(BarrierTouchState::Untouched), Some(start))?;
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))?
    .replace_history(Some(BarrierTouchState::Untouched), Some(start))?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    macro_rules! check {
        ($engine:expr, $option:expr, $context:expr, $partial:expr) => {{
            let engine = $engine;
            for at in [start, end] {
                let context = $context.replace(PricingContextChanges {
                    valuation_time: Some(at),
                    ..Default::default()
                })?;
                let option = dated(&$option, &context);
                let request = GreeksRequest::new([Greek::Vega]);
                assert!(matches!(
                    engine.price_with_greeks(&option, &context, request),
                    Err(Error::UnsupportedOperation(_))
                ));
                assert!(matches!(
                    PricingEngine::price_with_greeks_with_settings(
                        &engine,
                        &option,
                        &context,
                        request,
                        Default::default()
                    ),
                    Err(Error::UnsupportedOperation(_))
                ));
                assert_eq!(
                    engine
                        .price_with_greeks(&option, &context, GreeksRequest::new([]))?
                        .price(),
                    engine.price(&option, &context)?
                );
                if $partial {
                    let result = engine.price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Delta, Greek::Vega]),
                    )?;
                    assert_eq!(result.status(Greek::Vega), GreekStatus::Unsupported);
                    assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
                }
            }
        }};
    }
    check!(
        AnalyticHestonEuropeanEngine::default(),
        vanilla,
        heston,
        false
    );
    check!(
        FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
            asset_step_count: 12,
            variance_step_count: 8,
            time_step_count: 8,
            ..Default::default()
        })?,
        vanilla,
        heston,
        false
    );
    check!(
        FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_step_count: 40,
            time_step_count: 40,
            ..Default::default()
        })?,
        vanilla,
        local,
        true
    );
    check!(
        CoxRossRubinsteinDigitalEngine::default(),
        digital,
        bsm,
        true
    );
    check!(
        CoxRossRubinsteinBarrierEngine::default(),
        barrier,
        bsm,
        true
    );
    let sampling = MonteCarloSettings {
        path_count: 128,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    };
    check!(MonteCarloBarrierEngine::new(sampling)?, barrier, bsm, true);
    check!(
        MonteCarloDoubleBarrierEngine::new(sampling)?,
        double,
        bsm,
        true
    );
    Ok(())
}

#[test]
fn observation_coupon_choices_are_explicit_and_control_cashflows() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(365)?;
    for choices in [(None, None), (Some(&[0.0][..]), Some(&[0.0][..]))] {
        assert!(matches!(
            autocallable_observations(&[end], &[110.0], choices.0, choices.1),
            Err(Error::InvalidInput(_))
        ));
    }
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        110.0,
        end,
    )?;
    let engine = QuadratureAutocallableEngine::default();
    for (coupon, expected) in [(0.0, 1.0), (0.12, 1.12)] {
        let note = BinarySnowballOption::new(BinarySnowballTerms::new(
            0.12,
            start,
            end,
            vec![AutocallableObservation::snowball(end, 110.0, coupon)?],
        ))?
        .replace_history(Some(AutocallableBarrierState::NoEvent), Some(end))?;
        assert!((engine.price(&note, &context)? - expected).abs() < 1e-12);
    }
    for (barrier, expected) in [(0.0, 1.03), (90.0, 1.0)] {
        let mut terms = PhoenixTerms::new(
            PhoenixCoupon::PerPeriod(0.03),
            100.0,
            80.0,
            100.0,
            100.0,
            start,
            end,
            vec![AutocallableObservation::phoenix(end, 200.0, barrier)?],
            None,
        );
        terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
        let note = PhoenixOption::new(terms)?
            .replace_history(Some(AutocallableBarrierState::NoEvent), Some(end))?;
        let context = context.replace(PricingContextChanges {
            spot_price: Some(50.0),
            ..Default::default()
        })?;
        assert!((engine.price(&note, &context)? - expected).abs() < 1e-12);
    }
    Ok(())
}

#[test]
fn current_barrier_touches_retain_cashflow_parameter_greeks() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    for volatility in [0.0, 0.2] {
        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
            let mut terms = BarrierOptionTerms::new(
                OptionType::Call,
                100.0,
                start,
                start.add_days(365)?,
                120.0,
                BarrierType::UpAndOut,
            );
            terms.rebate = 10.0;
            terms.rebate_timing = timing;
            let option = BarrierOption::new(terms)?;
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.05, 0.02, volatility)?,
                120.0,
                start,
            )?;
            let result = AnalyticBarrierEngine.price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Rho, Greek::DividendRho, Greek::Vega]),
            )?;
            let expected = if timing == RebateTiming::AtExpiry {
                -10.0 * (-0.05_f64).exp()
            } else {
                0.0
            };
            assert!((result.require(Greek::Rho)? - expected).abs() < 2e-8);
            assert_eq!(result.require(Greek::DividendRho)?, 0.0);
            assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
            if volatility > 0.0 {
                assert_eq!(result.require(Greek::Vega)?, 0.0);
            } else {
                assert_eq!(result.status(Greek::Vega), GreekStatus::Undefined);
            }
        }
    }
    Ok(())
}

#[test]
fn bounded_barrier_grids_keep_resolution_when_the_unused_upper_bound_changes() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let expiry = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.02, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    for direction in [OptionType::Call, OptionType::Put] {
        let single = BarrierOption::new(BarrierOptionTerms::new(
            direction,
            100.0,
            start,
            expiry,
            120.0,
            BarrierType::UpAndOut,
        ))?;
        let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
            direction,
            100.0,
            start,
            expiry,
            80.0,
            120.0,
            DoubleBarrierType::KnockOut,
        ))?;
        let expected = AnalyticBarrierEngine.price(&dated(&single, &context), &context)?;
        let mut previous: Option<[f64; 2]> = None;
        for upper in [121.0, 400.0, 10_000.0] {
            let settings = FiniteDifferenceSettings {
                asset_step_count: 200,
                time_step_count: 200,
                asset_upper_boundary: Some(upper),
                ..Default::default()
            };
            let actual = [
                FiniteDifferenceBarrierEngine::new(settings)?
                    .price(&dated(&single, &context), &context)?,
                FiniteDifferenceDoubleBarrierEngine::new(settings)?
                    .price(&dated(&double, &context), &context)?,
            ];
            assert!(
                (actual[0] - expected).abs() < 0.003,
                "{direction:?}: {actual:?} != {expected}"
            );
            if let Some(previous) = previous {
                assert_eq!(actual, previous);
            }
            previous = Some(actual);
        }
    }
    Ok(())
}

#[test]
fn barrier_engines_require_curve_coverage_only_for_remaining_cashflows() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let expiry = start.add_days(365)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(31)?], vec![1.0])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve.clone(), curve, 0.2)?,
        100.0,
        start.add_days(1)?,
    )?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.touch_state = Some(BarrierTouchState::Touched);
    terms.history_time = Some(terms.effective_time);
    terms.rebate_timing = RebateTiming::AtHit;
    let settled = BarrierOption::new(terms)?;
    for engine in [
        &AnalyticBarrierEngine as &dyn PricingEngine<BarrierOption>,
        &CoxRossRubinsteinBarrierEngine::default(),
        &QuadratureBarrierEngine::default(),
        &FiniteDifferenceBarrierEngine::default(),
        &MonteCarloBarrierEngine::default(),
    ] {
        assert_eq!(engine.price(&dated(&settled, &context), &context)?, 0.0);
        assert_eq!(
            engine
                .price_with_greeks_with_settings(
                    &dated(&settled, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    engine.greek_settings(&dated(&settled, &context))
                )?
                .require(Greek::Delta)?,
            0.0
        );
        for changes in [
            BarrierOptionChanges {
                touch_state: Some(Some(BarrierTouchState::Untouched)),
                ..Default::default()
            },
            BarrierOptionChanges {
                rebate: Some(10.0),
                rebate_timing: Some(RebateTiming::AtExpiry),
                ..Default::default()
            },
            BarrierOptionChanges {
                barrier_type: Some(BarrierType::UpAndIn),
                rebate_timing: Some(RebateTiming::AtExpiry),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                engine.price(&dated(&settled.replace(changes)?, &context), &context),
                Err(Error::InvalidInput(_))
            ));
        }
        let missing = settled.replace_history(None, None)?;
        assert!(matches!(
            engine.price(&dated(&missing, &context), &context),
            Err(Error::MissingHistory(_))
        ));
    }
    Ok(())
}

#[test]
fn structured_contract_calendars_control_observations_and_survive_replacement() -> Result<()> {
    let start = Date::new(2025, 1, 3)?.start_of_day();
    let end = start.add_days(3)?;
    let model = BlackScholesMertonParameters::new(0.0, 0.0, 1e-12)?;
    let context = PricingContext::new(model, 100.0, start)?;
    let range = RangeAccrualOption::new(RangeAccrualTerms::new(
        1000.0,
        0.12,
        90.0,
        110.0,
        start,
        end,
        (end).time_of_day(),
    ))?
    .replace_history(Some(0), Some(start))?;
    let at_expiry = context.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    let price = AnalyticRangeAccrualEngine.price(&dated(&range, &at_expiry), &at_expiry)?;
    assert!((price - 360.0 / 365.0).abs() < 1e-12);
    let daily = range
        .replace(RangeAccrualOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            realized_hit_count: Some(range.realized_hit_count()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (AnalyticRangeAccrualEngine.price(&dated(&daily, &at_expiry), &at_expiry)? - price / 3.0)
            .abs()
            < 1e-12
    );
    assert_eq!(
        daily.replace_history(None, None)?.calendar(),
        daily.calendar()
    );
    assert_eq!(daily.replace(Default::default())?, daily);

    let accumulator = Accumulator::new(AccumulatorTerms::new(
        90.0,
        200.0,
        1.0,
        1.0,
        start,
        end,
        (end).time_of_day(),
    ))?;
    let sampling = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    };
    let engine = MonteCarloAccumulatorEngine::new(sampling)?;
    let daily = accumulator
        .replace_calendar(TradingCalendar::all_days())
        .unwrap();
    assert!((engine.price(&dated(&daily, &context), &context)? - 40.0).abs() < 1e-7);
    assert_eq!(
        daily
            .replace_history(
                0.0,
                Some(daily.history_time().unwrap_or(daily.effective_time()))
            )?
            .calendar(),
        daily.calendar()
    );

    let note = SnowballOption::new(SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        vec![AutocallableObservation::snowball(end, 110.0, 0.0)?],
        Some((end).time_of_day()),
    ))?;
    let daily = note.replace_calendar(TradingCalendar::all_days()).unwrap();
    assert_eq!(
        daily
            .replace_history(
                Some(AutocallableBarrierState::NoEvent),
                Some(daily.history_time().unwrap_or(daily.effective_time()))
            )?
            .calendar(),
        daily.calendar()
    );
    assert_eq!(
        daily
            .with_quoted_coupon(0.2, CouponQuoteConvention::ShiftMaturityCoupon)?
            .calendar(),
        daily.calendar()
    );
    assert_eq!(daily.replace(Default::default())?, daily);
    Ok(())
}

#[test]
fn constant_node_curves_price_like_scalars_and_keep_coverage() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let curve = RateTermStructure::new(start, vec![end, end.add_days(365)?], vec![1.0, 1.0])?;
    assert_eq!(curve.exact_flat_rate(), Some(0.0));
    let short = RateTermStructure::new(start, vec![start.add_days(180)?], vec![1.0])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        start,
    )?;
    let nodes = context.replace(PricingContextChanges {
        model_parameters: Some(
            BlackScholesMertonParameters::new(curve.clone(), curve, 0.2)?.into(),
        ),
        ..Default::default()
    })?;
    let short = context.replace(PricingContextChanges {
        model_parameters: Some(BlackScholesMertonParameters::new(short, 0.0, 0.2)?.into()),
        ..Default::default()
    })?;
    let sampling = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 512,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    })?;
    for engine in [
        &AnalyticEuropeanEngine as &dyn PricingEngine,
        &CoxRossRubinsteinVanillaEngine::default(),
        &QuadratureVanillaEngine::default(),
        &FiniteDifferenceVanillaEngine::default(),
        &sampling,
    ] {
        assert!(
            (engine.price(&dated(&option, &context), &context)?
                - engine.price(&dated(&option, &nodes), &nodes)?)
            .abs()
                < 1e-9
        );
        assert!(matches!(
            engine.price(&dated(&option, &short), &short),
            Err(Error::InvalidInput(_))
        ));
    }
    Ok(())
}

#[test]
fn heston_greeks_check_model_before_request_controls() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, start.add_days(365)?)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    for engine in [
        &AnalyticHestonEuropeanEngine::default() as &dyn PricingEngine,
        &FiniteDifferenceHestonVanillaEngine::default(),
    ] {
        for request in [GreeksRequest::new([]), GreeksRequest::all()] {
            assert!(matches!(
                kiyosi::numerical_greeks(
                    engine,
                    &dated(&option, &context),
                    &context,
                    request,
                    NumericalShiftSettings {
                        time_shift: Some(std::time::Duration::ZERO),
                        ..Default::default()
                    }
                ),
                Err(Error::UnsupportedOperation(_))
            ));
        }
    }
    Ok(())
}

#[test]
fn built_in_pricing_requires_valuation_within_inclusive_contract_life() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(1)?;
    let before = Timestamp::new(Date::new(2024, 12, 31)?, 23, 59, 59, 999_999)?;
    let after = Timestamp::new(end.date(), 0, 0, 0, 1)?;
    macro_rules! check {
        ($engine:expr, $option:expr) => {{
            let engine = $engine;
            let option = $option;
            for (at, valid) in [
                (before, option.valuation_start().is_none()),
                (start, true),
                (end, true),
                (after, false),
            ] {
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
                    100.0,
                    at,
                )?;
                let price = engine.price(&dated(&option, &context), &context);
                let greeks = engine.price_with_greeks(
                    &dated(&option, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                );
                if valid {
                    assert!(price?.is_finite());
                    assert!(greeks?.price().is_finite());
                } else {
                    assert!(matches!(price, Err(Error::InvalidInput(_))));
                    assert!(matches!(greeks, Err(Error::InvalidInput(_))));
                }
            }
        }};
    }
    check!(
        AnalyticEuropeanEngine,
        EuropeanOption::new(OptionType::Call, 100.0, start, end)?
    );
    for coupon in [0.0, 0.1] {
        check!(
            AnalyticRangeAccrualEngine,
            RangeAccrualOption::new(RangeAccrualTerms::new(
                100.0,
                coupon,
                90.0,
                110.0,
                start,
                end,
                (end).time_of_day()
            ))?
        );
    }
    Ok(())
}

#[test]
fn european_digitals_allow_pre_effective_pricing_greeks_and_solves() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let effective = start.add_days(30)?;
    let expiry = start.add_days(365)?;
    let mc = MonteCarloDigitalEngine::new(MonteCarloSettings {
        path_count: 128,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    })?;
    let fd = FiniteDifferenceDigitalEngine::new(FiniteDifferenceSettings {
        asset_step_count: 200,
        time_step_count: 100,
        ..Default::default()
    })?;
    let engines: [&dyn PricingEngine<DigitalOption>; 5] = [
        &AnalyticDigitalEngine,
        &CoxRossRubinsteinDigitalEngine::default(),
        &fd,
        &QuadratureDigitalEngine::default(),
        &mc,
    ];
    for direction in [OptionType::Call, OptionType::Put] {
        for future in [
            DigitalOption::cash_or_nothing(direction, 100.0, 10.0, effective, expiry)?,
            DigitalOption::asset_or_nothing(direction, 100.0, effective, expiry)?,
        ] {
            let existing = future.replace(DigitalOptionChanges {
                effective_time: Some(start),
                ..Default::default()
            })?;
            assert_eq!(future.valuation_start(), None);
            for volatility in [0.0, 0.2] {
                let model = BlackScholesMertonParameters::new(0.05, 0.02, volatility)?;
                for at in [
                    start,
                    Timestamp::new(effective.add_days(-1)?.date(), 23, 59, 59, 999_999)?,
                    effective,
                ] {
                    let context = PricingContext::new(model.clone(), 105.0, at)?;
                    for engine in engines {
                        let price = engine.price(&dated(&future, &context), &context)?;
                        assert_eq!(price, engine.price(&dated(&existing, &context), &context)?);
                        let request = GreeksRequest::new([Greek::Delta]);
                        let native = engine.price_with_greeks_with_settings(
                            &dated(&future, &context),
                            &context,
                            request,
                            engine.greek_settings(&dated(&future, &context)),
                        )?;
                        assert_eq!(
                            native,
                            engine.price_with_greeks_with_settings(
                                &dated(&existing, &context),
                                &context,
                                request,
                                engine.greek_settings(&dated(&existing, &context))
                            )?
                        );
                        assert!(native.require(Greek::Delta)?.is_finite());
                        let numerical = numerical_greeks(
                            engine,
                            &dated(&future, &context),
                            &context,
                            GreeksRequest::new([Greek::Theta]),
                            Default::default(),
                        )?;
                        assert!(numerical.require(Greek::Theta)?.is_finite());
                        assert_eq!(
                            numerical,
                            numerical_greeks(
                                engine,
                                &dated(&existing, &context),
                                &context,
                                GreeksRequest::new([Greek::Theta]),
                                Default::default()
                            )?
                        );
                        if volatility > 0.0 {
                            let solved = implied_volatility(
                                engine,
                                &dated(&future, &context),
                                &context,
                                price,
                                ImpliedVolatilitySettings::new(0.2, 0.3),
                            )?;
                            assert!((solved - 0.2).abs() < 1e-12);
                        }
                    }
                    assert_eq!(
                        mc.price_with_statistics(&dated(&future, &context), &context)?,
                        mc.price_with_statistics(&dated(&existing, &context), &context)?
                    );
                }
                let short = RateTermStructure::bounded_flat(0.05, start, expiry.add_days(-1)?)?;
                let uncovered = PricingContext::new(
                    model.replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(short.into()),
                        ..Default::default()
                    })?,
                    105.0,
                    start,
                )?;
                let expired =
                    PricingContext::new(model, 105.0, Timestamp::new(expiry.date(), 0, 0, 0, 1)?)?;
                for engine in engines {
                    for invalid in [&uncovered, &expired] {
                        assert!(matches!(
                            engine.price(&dated(&future, invalid), invalid),
                            Err(Error::InvalidInput(_))
                        ));
                    }
                }
            }
            let surface = LocalVolatilitySurface::new(
                vec![start, expiry],
                vec![50.0, 150.0],
                vec![vec![0.2; 2]; 2],
            )?;
            let local = PricingContext::new(
                LocalVolatilityParameters::new(0.05, 0.02, surface)?,
                105.0,
                start,
            )?;
            assert_eq!(
                fd.price(&dated(&future, &local), &local)?,
                fd.price(&dated(&existing, &local), &local)?
            );
            assert_eq!(future.effective_time(), effective);
        }
    }
    Ok(())
}

#[test]
fn european_vanilla_pre_effective_prices_greeks_and_solves_preserve_contract_dates() -> Result<()> {
    let valuation = Date::new(2025, 1, 1)?.start_of_day();
    let effective = valuation.add_days(30)?;
    let expiry = valuation.add_days(365)?;
    let future = EuropeanOption::new(OptionType::Call, 100.0, effective, expiry)?;
    let existing = future.replace(EuropeanOptionChanges {
        effective_time: Some(valuation),
        ..Default::default()
    })?;
    let monte_carlo = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 128,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    })?;
    for volatility in [0.0, 0.2] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.05, 0.02, volatility)?,
            100.0,
            valuation,
        )?;
        for engine in [
            &AnalyticEuropeanEngine as &dyn PricingEngine<EuropeanOption>,
            &CoxRossRubinsteinVanillaEngine::default(),
            &FiniteDifferenceVanillaEngine::default(),
            &QuadratureVanillaEngine::default(),
            &monte_carlo,
        ] {
            assert_eq!(
                engine.price(&dated(&future, &context), &context)?,
                engine.price(&dated(&existing, &context), &context)?
            );
            assert!(
                engine
                    .price_with_greeks_with_settings(
                        &dated(&future, &context),
                        &context,
                        GreeksRequest::new([Greek::Delta]),
                        engine.greek_settings(&dated(&future, &context))
                    )?
                    .require(Greek::Delta)?
                    .is_finite()
            );
        }
        if volatility > 0.0 {
            let quote = AnalyticEuropeanEngine.price(&dated(&future, &context), &context)?;
            assert!(
                (implied_volatility(
                    &AnalyticEuropeanEngine,
                    &dated(&future, &context),
                    &context,
                    quote,
                    kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
                )? - volatility)
                    .abs()
                    < 1e-7
            );
        }
    }
    let heston = PricingContext::new(
        HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.5,
            vol_of_vol: 0.0,
            correlation: -0.7,
        })?,
        100.0,
        valuation,
    )?;
    for engine in [
        &AnalyticHestonEuropeanEngine::default() as &dyn PricingEngine<EuropeanOption>,
        &FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
            asset_step_count: 40,
            variance_step_count: 10,
            time_step_count: 20,
            ..Default::default()
        })?,
    ] {
        assert_eq!(
            engine.price(&dated(&future, &heston), &heston)?,
            engine.price(&dated(&existing, &heston), &heston)?
        );
    }
    let surface = LocalVolatilitySurface::new(
        vec![valuation, expiry],
        vec![50.0, 150.0],
        vec![vec![0.2; 2]; 2],
    )?;
    let local = PricingContext::new(
        LocalVolatilityParameters::new(0.05, 0.02, surface)?,
        100.0,
        valuation,
    )?;
    let fd = FiniteDifferenceVanillaEngine::default();
    assert_eq!(
        fd.price(&dated(&future, &local), &local)?,
        fd.price(&dated(&existing, &local), &local)?
    );
    assert!(matches!(
        PricingContext::new(
            BlackScholesMertonParameters::new(
                RateTermStructure::bounded_flat(0.05, effective, expiry)?,
                0.02,
                0.2
            )?,
            100.0,
            valuation
        ),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(future.effective_time(), effective);
    Ok(())
}

#[test]
fn observation_construction_validates_numeric_terms_before_note_creation() -> Result<()> {
    let time = Date::new(2026, 1, 1)?.start_of_day();
    for level in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            AutocallableObservation::snowball(time, level, 0.0),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            autocallable_observations(&[time], &[level], Some(&[0.0]), None),
            Err(Error::InvalidInput(_))
        ));
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            autocallable_observations(&[time], &[110.0], Some(&[invalid]), None),
            Err(Error::InvalidInput(_))
        ));
    }
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            autocallable_observations(&[time], &[110.0], None, Some(&[invalid])),
            Err(Error::InvalidInput(_))
        ));
    }
    let records = autocallable_observations(&[time], &[110.0], Some(&[-0.01]), None)?;
    assert_eq!(records[0].coupon_rate(), Some(-0.01));
    assert_eq!(records[0].coupon_barrier_level(), None);
    let edited = AutocallableObservation::snowball(time, 110.0, 0.0)?;
    assert!(matches!(
        edited.replace(AutocallableObservationChanges {
            coupon_rate: Some(f64::NAN),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(edited.coupon_rate(), Some(0.0));
    Ok(())
}

#[test]
fn asian_history_uses_optional_positive_averages() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(10)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start.add_days(1)?,
    )?;
    macro_rules! check {
        ($contract:ident, $engine:expr) => {{
            let original = $contract::new(OptionType::Call, 100.0, start, start, end, None, None)?;
            assert_eq!(original.realized_average(), None);
            assert!(matches!(
                $engine.price(&dated(&original, &context), &context),
                Err(Error::MissingHistory(_))
            ));
            let seasoned = original.replace_history(110.0, Some(context.valuation_time()))?;
            assert_eq!(seasoned.realized_average(), Some(110.0));
            assert!($engine.price(&dated(&seasoned, &context), &context)? > 0.0);
            assert_eq!(seasoned.replace_history(None, None)?, original);
            assert_eq!(
                seasoned.replace(AveragePriceOptionChanges {
                    realized_average: Some(None),
                    ..Default::default()
                })?,
                original
            );
            for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                assert!(matches!(
                    seasoned.replace_history(
                        invalid,
                        Some(seasoned.history_time().unwrap_or(seasoned.effective_time()))
                    ),
                    Err(Error::InvalidInput(_))
                ));
            }
            assert_eq!(seasoned.realized_average(), Some(110.0));
        }};
    }
    check!(
        GeometricAveragePriceOption,
        AnalyticGeometricAveragePriceEngine
    );
    check!(
        ArithmeticAveragePriceOption,
        TurnbullWakemanArithmeticAveragePriceEngine
    );
    Ok(())
}

#[test]
fn model_scalar_rate_queries_match_curve_representations() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let surface =
        LocalVolatilitySurface::new(vec![start, end], vec![80.0, 120.0], vec![vec![0.2; 2]; 2])?;
    for curve in [
        RateTermStructure::flat(0.03, None)?,
        RateTermStructure::flat(0.03, Some(start))?,
        RateTermStructure::new(start, vec![end], vec![0.97])?,
    ] {
        macro_rules! check {
            ($model:expr) => {{
                let model = $model;
                assert_eq!(model.risk_free_curve(), &curve);
                assert_eq!(model.dividend_curve(), &curve);
                for rate in [model.flat_risk_free_rate(), model.flat_dividend_yield()] {
                    if let Some(expected) = curve.exact_flat_rate() {
                        assert_eq!(rate?, expected);
                    } else {
                        assert!(matches!(rate, Err(Error::UnsupportedOperation(_))));
                    }
                }
            }};
        }
        check!(BlackScholesMertonParameters::new(
            curve.clone(),
            curve.clone(),
            0.2
        )?);
        check!(HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: curve.clone().into(),
            dividend_yield: curve.clone().into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.0,
            vol_of_vol: 0.2,
            correlation: -0.5
        })?);
        check!(LocalVolatilityParameters::new(
            curve.clone(),
            curve.clone(),
            surface.clone()
        )?);
    }
    Ok(())
}

#[test]
fn settled_accumulators_have_no_remaining_cashflows() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2025, 1, 10)?.start_of_day();
    let option = Accumulator::new(AccumulatorTerms::new(
        90.0,
        110.0,
        1.0,
        2.0,
        start,
        end,
        (end).time_of_day(),
    ))?;
    let settled = option.replace(AccumulatorChanges {
        history_time: Some(Some(
            option.history_time().unwrap_or(option.effective_time()),
        )),
        knock_out_settled: Some(Some(true)),
        ..Default::default()
    })?;
    assert_eq!(option.knock_out_settled(), None);
    assert_eq!(
        settled
            .replace_history(
                3.0,
                Some(settled.history_time().unwrap_or(settled.effective_time()))
            )?
            .knock_out_settled(),
        Some(true)
    );
    assert_eq!(
        settled
            .replace(AccumulatorChanges {
                strike: Some(95.0),
                ..Default::default()
            })?
            .knock_out_settled(),
        Some(true)
    );
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        Date::new(2025, 1, 3)?.start_of_day(),
    )?;
    let engines: Vec<Box<dyn PricingEngine<Accumulator>>> = vec![
        Box::new(QuadratureAccumulatorEngine::default()),
        Box::new(FiniteDifferenceAccumulatorEngine::default()),
        Box::new(MonteCarloAccumulatorEngine::default().with_resolved_seed()?),
    ];
    for engine in engines {
        assert_eq!(engine.price(&dated(&settled, &context), &context)?, 0.0);
        assert_eq!(
            engine.price(
                &dated(
                    &settled.replace_history(
                        3.0,
                        Some(settled.history_time().unwrap_or(settled.effective_time()))
                    )?,
                    &context
                ),
                &context
            )?,
            0.0
        );
        let active = settled.replace(AccumulatorChanges {
            history_time: Some(Some(
                settled.history_time().unwrap_or(settled.effective_time()),
            )),
            knock_out_settled: Some(Some(false)),
            accumulated_quantity: Some(Some(0.0)),
            ..Default::default()
        })?;
        assert!(engine.price(&dated(&active, &context), &context)? > 0.0);
        let unknown = settled.replace(AccumulatorChanges {
            history_time: Some(Some(
                settled.history_time().unwrap_or(settled.effective_time()),
            )),
            knock_out_settled: Some(Some(false)),
            ..Default::default()
        })?;
        assert!(matches!(
            engine.price(&dated(&unknown, &context), &context),
            Err(Error::MissingHistory(_))
        ));
        let result = engine.price_with_greeks_with_settings(
            &dated(&settled, &context),
            &context,
            GreeksRequest::all(),
            engine.greek_settings(&dated(&settled, &context)),
        )?;
        for greek in Greek::ALL {
            assert_eq!(result.require(greek)?, 0.0);
        }
        assert!(matches!(
            implied_volatility(
                engine.as_ref(),
                &dated(&settled, &context),
                &context,
                0.0,
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
            ),
            Err(Error::UnsupportedOperation(_))
        ));
    }
    Ok(())
}

#[test]
fn greek_status_distinguishes_requests_support_and_stencils() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let request = GreeksRequest::new([Greek::Delta]);
    let result =
        AnalyticEuropeanEngine.price_with_greeks(&dated(&option, &context), &context, request)?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
    assert_eq!(result.status(Greek::Gamma), GreekStatus::NotRequested);
    let terminal = context.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    assert_eq!(
        AnalyticEuropeanEngine
            .price_with_greeks(&dated(&option, &terminal), &terminal, request)?
            .status(Greek::Delta),
        GreekStatus::Undefined
    );
    let result = numerical_greeks(
        &AnalyticEuropeanEngine,
        &dated(&option, &context),
        &context,
        request,
        NumericalShiftSettings {
            spot_shift: Some(400.0),
            ..Default::default()
        },
    )?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::StencilUnavailable);
    let heston = context.replace(PricingContextChanges {
        model_parameters: Some(
            HestonParameters::new(kiyosi::HestonParameterInputs {
                risk_free_rate: 0.05.into(),
                dividend_yield: 0.02.into(),
                initial_variance: 0.04,
                long_run_variance: 0.04,
                mean_reversion_speed: 1.5,
                vol_of_vol: 0.3,
                correlation: -0.7,
            })?
            .into(),
        ),
        ..Default::default()
    })?;
    assert!(matches!(
        AnalyticHestonEuropeanEngine::default().price_with_greeks(&option, &heston, request),
        Err(Error::UnsupportedOperation(_))
    ));
    let custom = PricingResult::new(10.0)?
        .with_greek(Greek::Delta, 0.5)?
        .with_request(
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            GreeksRequest::new([Greek::Gamma]),
        );
    assert_eq!(custom.get(Greek::Delta), None);
    assert_eq!(custom.status(Greek::Delta), GreekStatus::Unsupported);
    assert_eq!(custom.status(Greek::Gamma), GreekStatus::Undefined);
    assert_eq!(custom.status(Greek::Theta), GreekStatus::NotRequested);
    let local = context.replace(PricingContextChanges {
        model_parameters: Some(
            LocalVolatilityParameters::new(
                0.05,
                0.02,
                LocalVolatilitySurface::new(
                    vec![start, end],
                    vec![80.0, 120.0],
                    vec![vec![0.2; 2]; 2],
                )?,
            )?
            .into(),
        ),
        ..Default::default()
    })?;
    let result = FiniteDifferenceVanillaEngine::default().price_with_greeks(
        &dated(&option, &local),
        &local,
        GreeksRequest::new([Greek::Delta, Greek::Vega]),
    )?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
    assert_eq!(result.status(Greek::Vega), GreekStatus::Unsupported);
    assert_eq!(
        result.requested(),
        GreeksRequest::new([Greek::Delta, Greek::Vega])
    );
    Ok(())
}

#[test]
fn flat_rate_query_is_independent_of_reference_time() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    for reference in [None, Some(start)] {
        let curve = RateTermStructure::flat(0.05, reference)?;
        assert_eq!(curve.exact_flat_rate(), Some(0.05));
        assert!((curve.shifted(0.01)?.exact_flat_rate().unwrap() - 0.06).abs() < 1e-15);
        assert_eq!(curve.reference_time(), reference);
    }
    let nodes = RateTermStructure::new(start, vec![end], vec![(-0.05_f64).exp()])?;
    assert!((nodes.exact_flat_rate().unwrap() - 0.05).abs() < 1e-15);
    Ok(())
}

#[test]
fn native_greek_methods_match_trait_dispatch() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let asian =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None, None)?;
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        BarrierType::DownAndOut,
    ))?;
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))?;
    let settings = MonteCarloSettings {
        path_count: 1024,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    };
    let request = GreeksRequest::new([Greek::Delta]);
    macro_rules! check {
        ($engine:expr, $option:expr) => {{
            let engine = $engine;
            let direct = engine.price_with_greeks(&dated($option, &context), &context, request)?;
            let generic = PricingEngine::price_with_greeks_with_settings(
                &engine,
                $option,
                &context,
                request,
                engine.greek_settings($option),
            )?;
            assert_eq!(direct, generic);
            assert_eq!(direct.status(Greek::Delta), GreekStatus::Available);
            let price_only = PricingEngine::price_with_greeks_with_settings(
                &engine,
                $option,
                &context,
                GreeksRequest::new([]),
                engine.greek_settings($option),
            )?;
            assert_eq!(price_only.price(), direct.price());
            for greek in Greek::ALL {
                assert_eq!(price_only.status(greek), GreekStatus::NotRequested);
                assert_eq!(price_only.get(greek), None);
            }
        }};
    }
    check!(AnalyticEuropeanEngine, &vanilla);
    check!(CoxRossRubinsteinDigitalEngine::default(), &digital);
    check!(CoxRossRubinsteinBarrierEngine::default(), &barrier);
    check!(MonteCarloDigitalEngine::new(settings)?, &digital);
    check!(MonteCarloAsianEngine::new(settings)?, &asian);
    check!(MonteCarloBarrierEngine::new(settings)?, &barrier);
    check!(MonteCarloDoubleBarrierEngine::new(settings)?, &double);
    Ok(())
}

#[test]
fn double_touch_barrier_type_roundtrips_through_replacement() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let option = DoubleTouchOption::cash(
            DoubleTouchOptionTerms::new(start, end, 80.0, 120.0, kind),
            10.0,
        )?;
        assert_eq!(option.barrier_type(), kind);
        assert_eq!(
            option.replace(DoubleTouchOptionChanges {
                barrier_type: Some(option.barrier_type()),
                ..Default::default()
            })?,
            option
        );
        let other = if kind == DoubleBarrierType::KnockIn {
            DoubleBarrierType::KnockOut
        } else {
            DoubleBarrierType::KnockIn
        };
        assert_eq!(
            option
                .replace(DoubleTouchOptionChanges {
                    barrier_type: Some(other),
                    ..Default::default()
                })?
                .barrier_type(),
            other
        );
    }
    Ok(())
}

#[test]
fn greek_shift_validation_is_independent_of_estimator_and_valuation_time() -> Result<()> {
    fn check<I: support::DatedHistory>(
        engine: &impl PricingEngine<I>,
        option: &I,
        model: impl Into<ModelParameters>,
    ) -> Result<()> {
        let context = PricingContext::new(model, 110.0, option.effective_time())?;
        let request = GreeksRequest::new([Greek::Delta]);
        for at in [option.effective_time(), option.expiry_time()] {
            let context = context.replace(PricingContextChanges {
                valuation_time: Some(at),
                ..Default::default()
            })?;
            assert!(
                engine
                    .price_with_greeks_with_settings(
                        &dated(option, &context),
                        &context,
                        request,
                        engine.greek_settings(&dated(option, &context))
                    )?
                    .price()
                    .is_finite()
            );
            for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                for shifts in [
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
                        kiyosi::numerical_greeks(
                            engine,
                            &dated(option, &context),
                            &context,
                            request,
                            shifts
                        ),
                        Err(Error::InvalidInput(_))
                    ));
                }
            }
            for shift in [
                std::time::Duration::ZERO,
                std::time::Duration::from_nanos(1),
            ] {
                assert!(matches!(
                    kiyosi::numerical_greeks(
                        engine,
                        &dated(option, &context),
                        &context,
                        request,
                        NumericalShiftSettings {
                            time_shift: Some(shift),
                            ..Default::default()
                        }
                    ),
                    Err(Error::InvalidInput(_))
                ));
            }
        }
        Ok(())
    }
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let bsm = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    check(&AnalyticEuropeanEngine, &vanilla, bsm.clone())?;
    check(&QuadratureVanillaEngine::default(), &vanilla, bsm.clone())?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    check(
        &CoxRossRubinsteinDigitalEngine::default(),
        &digital,
        bsm.clone(),
    )?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.touch_state = Some(BarrierTouchState::Untouched);
    terms.history_time = Some(terms.effective_time);
    check(
        &CoxRossRubinsteinBarrierEngine::default(),
        &BarrierOption::new(terms.clone())?,
        bsm.clone(),
    )?;
    let sampling = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    };
    check(
        &MonteCarloBarrierEngine::new(sampling)?,
        &BarrierOption::new(terms)?,
        bsm.clone(),
    )?;
    let mut double = DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    );
    double.touch_state = Some(BarrierTouchState::Untouched);
    double.history_time = Some(double.effective_time);
    check(
        &MonteCarloDoubleBarrierEngine::new(sampling)?,
        &DoubleBarrierOption::new(double)?,
        bsm.clone(),
    )?;
    check(
        &MonteCarloDigitalEngine::new(sampling)?,
        &digital,
        bsm.clone(),
    )?;
    check(
        &MonteCarloAsianEngine::new(sampling)?,
        &GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None, None)?,
        bsm.clone(),
    )?;
    check(
        &MonteCarloAsianEngine::new(sampling)?,
        &ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None, None)?,
        bsm,
    )?;
    Ok(())
}

#[test]
fn settings_validate_without_constructing_engines_or_pricing() -> Result<()> {
    BinomialSettings::default().validate()?;
    MonteCarloSettings::default().validate()?;
    FiniteDifferenceSettings::default().validate()?;
    NumericalShiftSettings::default().validate()?;
    assert!(matches!(
        BinomialSettings { step_count: 0 }.validate(),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        MonteCarloSettings {
            path_count: 3,
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        }
        .validate(),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(f64::INFINITY),
            ..Default::default()
        }
        .validate(),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        NumericalShiftSettings {
            rate_shift: -1.0,
            ..Default::default()
        }
        .validate(),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn contractual_calendars_are_validated_before_pricing() -> Result<()> {
    let start = Date::new(2025, 1, 3)?.start_of_day();
    let closed = start.add_days(1)?;
    let end = start.add_days(3)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.observation_times = Some(vec![closed]);
    assert!(matches!(
        BarrierOption::new(terms.clone()),
        Err(Error::InvalidInput(_))
    ));
    terms.calendar = TradingCalendar::all_days();
    let option = BarrierOption::new(terms)?;
    assert!(matches!(
        option.replace(BarrierOptionChanges {
            calendar: Some(TradingCalendar::weekdays()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        option.replace_calendar(TradingCalendar::weekdays()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(option.calendar(), &TradingCalendar::all_days());
    assert_eq!(
        option
            .replace(BarrierOptionChanges {
                strike: Some(105.0),
                ..Default::default()
            })?
            .observation_times(),
        Some(&[closed][..])
    );
    let mut terms = AccumulatorTerms::new(90.0, 110.0, 1.0, 2.0, start, end, (end).time_of_day());
    terms.knock_out_observation_times = Some(vec![closed]);
    assert!(matches!(
        Accumulator::new(terms.clone()),
        Err(Error::InvalidInput(_))
    ));
    terms.calendar = TradingCalendar::all_days();
    let accumulator = Accumulator::new(terms)?;
    assert!(matches!(
        accumulator.replace(AccumulatorChanges {
            calendar: Some(TradingCalendar::weekdays()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    let mut terms = BinarySnowballTerms::new(
        0.1,
        start,
        end,
        autocallable_observations(&[closed], &[110.0], Some(&[0.0]), None)?,
    );
    assert!(matches!(
        BinarySnowballOption::new(terms.clone()),
        Err(Error::InvalidInput(_))
    ));
    terms.calendar = TradingCalendar::all_days();
    let note = BinarySnowballOption::new(terms)?;
    assert!(matches!(
        note.replace(BinarySnowballOptionChanges {
            calendar: Some(TradingCalendar::weekdays()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        note.with_quoted_coupon(0.2, CouponQuoteConvention::PreserveMaturityCoupon)?
            .calendar(),
        note.calendar()
    );
    let mut terms = RangeAccrualTerms::new(
        100.0,
        0.1,
        80.0,
        120.0,
        start,
        closed,
        (closed).time_of_day(),
    );
    assert!(matches!(
        RangeAccrualOption::new(terms.clone()),
        Err(Error::InvalidInput(_))
    ));
    terms.calendar = TradingCalendar::all_days();
    let range = RangeAccrualOption::new(terms)?;
    assert!(matches!(
        range.replace(RangeAccrualOptionChanges {
            calendar: Some(TradingCalendar::weekdays()),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    macro_rules! unchanged {
        ($option:ident) => {
            assert!(matches!(
                $option.replace_calendar(TradingCalendar::weekdays()),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(
                $option.replace_calendar(TradingCalendar::all_days())?,
                $option
            );
            assert_eq!($option.calendar(), &TradingCalendar::all_days());
        };
    }
    unchanged!(accumulator);
    unchanged!(note);
    unchanged!(range);
    Ok(())
}

#[test]
fn numeric_rejections_identify_the_failing_field() -> Result<()> {
    for (field, inputs) in [
        ("risk_free_rate", [f64::NAN, 0.0, 0.2]),
        ("dividend_yield", [0.02, f64::INFINITY, 0.2]),
        ("volatility", [0.02, 0.0, -0.2]),
    ] {
        let error = BlackScholesMertonParameters::new(inputs[0], inputs[1], inputs[2]).unwrap_err();
        assert!(matches!(&error, Error::InvalidInput(_)));
        assert!(error.to_string().contains(field));
    }
    let model = BlackScholesMertonParameters::new(0.02, 0.0, 0.2)?;
    let error = model
        .replace(BlackScholesMertonParameterChanges {
            volatility: Some(-0.1),
            ..Default::default()
        })
        .unwrap_err();
    assert!(error.to_string().contains("volatility"));
    assert_eq!(model.volatility(), 0.2);
    let start = Date::new(2025, 1, 3)?.start_of_day();
    let error = EuropeanOption::new(OptionType::Call, 0.0, start, start).unwrap_err();
    assert!(error.to_string().contains("strike"));
    let error =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, -1.0, start, start).unwrap_err();
    assert!(error.to_string().contains("payout"));
    let error = PricingContext::new(model, 0.0, start).unwrap_err();
    assert!(error.to_string().contains("spot_price"));
    Ok(())
}

#[test]
fn structured_engines_require_curve_coverage_only_for_remaining_cashflows() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let expiry = start.add_days(365)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(31)?], vec![1.0])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve.clone(), curve, 0.2)?,
        125.0,
        start.add_days(2)?,
    )?;
    let mut terms = AccumulatorTerms::new(
        100.0,
        120.0,
        1.0,
        2.0,
        start,
        expiry,
        (expiry).time_of_day(),
    );
    terms.accumulated_quantity = Some(3.0);
    terms.history_time = Some(terms.effective_time);
    terms.knock_out_settled = Some(false);
    terms.history_time = Some(terms.effective_time);
    let accumulator = Accumulator::new(terms)?;
    let settled = accumulator.replace(AccumulatorChanges {
        history_time: Some(Some(
            accumulator
                .history_time()
                .unwrap_or(accumulator.effective_time()),
        )),
        knock_out_settled: Some(Some(true)),
        ..Default::default()
    })?;
    let active = accumulator.replace(AccumulatorChanges {
        knock_out_level: Some(150.0),
        accumulated_quantity: Some(accumulator.accumulated_quantity()),
        knock_out_settled: Some(accumulator.knock_out_settled()),
        ..Default::default()
    })?;
    let mc = MonteCarloAccumulatorEngine::default();
    for engine in [
        &FiniteDifferenceAccumulatorEngine::default() as &dyn PricingEngine<Accumulator>,
        &QuadratureAccumulatorEngine::default(),
        &mc,
    ] {
        assert_eq!(engine.price(&dated(&settled, &context), &context)?, 0.0);
        assert_eq!(
            engine.price(&dated(&accumulator, &context), &context)?,
            75.0
        );
        assert_eq!(
            engine
                .price_with_greeks_with_settings(
                    &dated(&settled, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    engine.greek_settings(&dated(&settled, &context))
                )?
                .require(Greek::Delta)?,
            0.0
        );
        assert!(matches!(
            engine.price(&dated(&active, &context), &context),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(
        mc.price_with_statistics(&dated(&settled, &context), &context)?
            .standard_error(),
        Some(0.0)
    );
    let mut terms = BinarySnowballTerms::new(
        0.1,
        start,
        expiry,
        autocallable_observations(
            &[start.add_days(1)?, start.add_days(2)?, expiry],
            &[120.0; 3],
            Some(&[0.0; 3]),
            None,
        )?,
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    terms.history_time = Some(terms.effective_time);
    let note = BinarySnowballOption::new(terms)?;
    let settled = note.replace_history(
        Some(AutocallableBarrierState::KnockedOut),
        Some(note.history_time().unwrap_or(note.effective_time())),
    )?;
    let active_context = context.replace(PricingContextChanges {
        spot_price: Some(100.0),
        ..Default::default()
    })?;
    let mc = MonteCarloAutocallableEngine::default();
    for engine in [
        &FiniteDifferenceAutocallableEngine::default() as &dyn PricingEngine<BinarySnowballOption>,
        &QuadratureAutocallableEngine::default(),
        &mc,
    ] {
        assert_eq!(engine.price(&dated(&settled, &context), &context)?, 0.0);
        assert_eq!(engine.price(&dated(&note, &context), &context)?, 1.0);
        assert_eq!(
            engine
                .price_with_greeks_with_settings(
                    &dated(&settled, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    engine.greek_settings(&dated(&settled, &context))
                )?
                .require(Greek::Delta)?,
            0.0
        );
        assert!(matches!(
            engine.price(&dated(&note, &active_context), &active_context),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(
        mc.price_with_statistics(&dated(&settled, &context), &context)?
            .standard_error(),
        Some(0.0)
    );
    Ok(())
}

#[test]
fn accumulator_settlement_requires_a_strictly_prior_knock_out_observation() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let expiry = start.add_days(4)?;
    let model = BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?;
    let mc = MonteCarloAccumulatorEngine::default();
    for times in [None, Some(vec![start.add_days(2)?]), Some(vec![])] {
        let mut terms = AccumulatorTerms::new(
            100.0,
            120.0,
            1.0,
            2.0,
            start,
            expiry,
            (expiry).time_of_day(),
        );
        terms.knock_out_observation_times = times.clone();
        let original = Accumulator::new(terms)?;
        let settled = original.replace(AccumulatorChanges {
            history_time: Some(Some(
                original.history_time().unwrap_or(original.effective_time()),
            )),
            knock_out_settled: Some(Some(true)),
            ..Default::default()
        })?;
        let first = times.as_ref().map_or(Some(start), |t| t.first().copied());
        for valuation in [
            start,
            start.add_days(2)?,
            Timestamp::new(start.add_days(2)?.date(), 0, 0, 0, 1)?,
            expiry,
        ] {
            let context = PricingContext::new(model.clone(), 100.0, valuation)?;
            let valid = first.is_some_and(|first| first < valuation);
            for engine in [
                &FiniteDifferenceAccumulatorEngine::default() as &dyn PricingEngine<Accumulator>,
                &QuadratureAccumulatorEngine::default(),
                &mc,
            ] {
                let price = engine.price(&dated(&settled, &context), &context);
                let greeks = engine.price_with_greeks_with_settings(
                    &dated(&settled, &context),
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    engine.greek_settings(&dated(&settled, &context)),
                );
                if valid {
                    assert_eq!(price?, 0.0);
                    assert_eq!(greeks?.require(Greek::Delta)?, 0.0);
                } else {
                    assert!(matches!(price, Err(Error::InvalidInput(_))));
                    assert!(matches!(greeks, Err(Error::InvalidInput(_))));
                }
            }
            let statistics = mc.price_with_statistics(&dated(&settled, &context), &context);
            if valid {
                assert_eq!(statistics?.price(), 0.0);
            } else {
                assert!(matches!(statistics, Err(Error::InvalidInput(_))));
            }
        }
        assert_eq!(original.knock_out_settled(), None);
        assert_eq!(original.accumulated_quantity(), None);
        assert_eq!(original.knock_out_observation_times(), times.as_deref());
    }
    Ok(())
}

#[test]
fn snowball_coupon_diagnostics_identify_the_supplied_field() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let observations = autocallable_observations(&[end], &[110.0], Some(&[0.0]), None)?;
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let binary = BinarySnowballOption::new(BinarySnowballTerms::new(
            invalid,
            start,
            end,
            observations.clone(),
        ));
        let ternary = TernarySnowballOption::new(TernarySnowballTerms::new(
            invalid,
            0.0,
            80.0,
            start,
            end,
            observations.clone(),
            Some((end).time_of_day()),
        ));
        let minimum = TernarySnowballOption::new(TernarySnowballTerms::new(
            0.1,
            invalid,
            80.0,
            start,
            end,
            observations.clone(),
            Some((end).time_of_day()),
        ));
        for (error, field) in [
            (binary.unwrap_err(), "maturity_coupon_rate"),
            (ternary.unwrap_err(), "maturity_coupon_rate"),
            (minimum.unwrap_err(), "minimum_coupon_rate"),
        ] {
            assert!(matches!(error, Error::InvalidInput(_)));
            assert!(error.to_string().contains(field), "{error}");
        }
    }
    Ok(())
}

#[test]
fn spot_shift_fallback_is_distinct_from_engine_resolution() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let settings = NumericalShiftSettings::default();
    assert_eq!(settings.fallback_spot_shift(), 0.01);
    assert_eq!(settings.spot_shift, None);
    let european = EuropeanOption::new(OptionType::Put, 100.0, start, start.add_days(365)?)?;
    let american = AmericanOption::new(OptionType::Put, 100.0, start, start.add_days(365)?)?;
    let engine = MonteCarloVanillaEngine::default();
    assert_eq!(engine.default_spot_shift(&european, &context), 0.01);
    assert_eq!(engine.default_spot_shift(&american, &context), 8.0);
    let fd = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        spot_shift: Some(0.5),
        ..Default::default()
    })?;
    assert_eq!(fd.default_spot_shift(&european, &context), 0.01);
    assert_eq!(fd.greek_settings(&european).spot_shift, Some(0.5));
    assert_eq!(fd.default_spot_shift(&american, &context), 0.01);
    assert_eq!(
        NumericalShiftSettings {
            spot_shift: Some(0.5),
            ..settings
        }
        .fallback_spot_shift(),
        0.5
    );
    Ok(())
}

#[test]
fn settings_diagnostics_identify_invalid_fields() {
    let cases = [
        ("step_count", BinomialSettings { step_count: 0 }.validate()),
        (
            "step_count",
            BinomialSettings {
                step_count: 1_000_001,
            }
            .validate(),
        ),
        (
            "path_count",
            MonteCarloSettings {
                path_count: 0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "path_count",
            MonteCarloSettings {
                path_count: 10_000_001,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "max_active_workers",
            MonteCarloSettings {
                max_active_workers: -1,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "asset_step_count",
            FiniteDifferenceSettings {
                asset_step_count: 2,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "time_step_count",
            FiniteDifferenceSettings {
                time_step_count: 0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "asset_upper_boundary",
            FiniteDifferenceSettings {
                asset_upper_boundary: Some(0.0),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "asset_upper_boundary",
            FiniteDifferenceSettings {
                asset_upper_boundary: Some(f64::NAN),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "variance_step_count",
            HestonFiniteDifferenceSettings {
                variance_step_count: 2,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "asset_step_count",
            HestonFiniteDifferenceSettings {
                asset_step_count: 2,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "time_step_count",
            HestonFiniteDifferenceSettings {
                time_step_count: 0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "variance_upper_boundary",
            HestonFiniteDifferenceSettings {
                variance_upper_boundary: Some(-1.0),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "variance_upper_boundary",
            HestonFiniteDifferenceSettings {
                variance_upper_boundary: Some(f64::INFINITY),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "asset_upper_boundary",
            HestonFiniteDifferenceSettings {
                asset_upper_boundary: Some(0.0),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "asset_step_count",
            QuadratureSettings {
                asset_step_count: 3,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "integration_standard_deviations",
            QuadratureSettings {
                integration_standard_deviations: 0.0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "integration_standard_deviations",
            QuadratureSettings {
                integration_standard_deviations: f64::NAN,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "time_step_count",
            QuadratureVanillaSettings {
                time_step_count: 0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "absolute_tolerance",
            HestonIntegrationSettings {
                absolute_tolerance: -1.0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "absolute_tolerance",
            HestonIntegrationSettings {
                absolute_tolerance: f64::NAN,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "relative_tolerance",
            HestonIntegrationSettings {
                relative_tolerance: -1.0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "relative_tolerance",
            HestonIntegrationSettings {
                relative_tolerance: f64::INFINITY,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "max_evaluations",
            HestonIntegrationSettings {
                max_evaluations: 0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "spot_shift",
            NumericalShiftSettings {
                spot_shift: Some(0.0),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "spot_shift",
            NumericalShiftSettings {
                spot_shift: Some(f64::NAN),
                ..Default::default()
            }
            .validate(),
        ),
        (
            "volatility_shift",
            NumericalShiftSettings {
                volatility_shift: 0.0,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "rate_shift",
            NumericalShiftSettings {
                rate_shift: f64::INFINITY,
                ..Default::default()
            }
            .validate(),
        ),
        (
            "time_shift",
            NumericalShiftSettings {
                time_shift: Some(std::time::Duration::ZERO),
                ..Default::default()
            }
            .validate(),
        ),
    ];
    for (field, result) in cases {
        let error = result.unwrap_err();
        assert!(matches!(&error, Error::InvalidInput(_)));
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
}

#[test]
fn greek_methods_report_estimators_and_active_controls() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let expiry = start.add_days(365)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, expiry)?;
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    ))?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let cases = [
        (
            AnalyticEuropeanEngine.greek_method(&option, &context)?,
            GreekMethod::Native,
            &[][..],
        ),
        (
            QuadratureVanillaEngine::default().greek_method(&option, &context)?,
            GreekMethod::Numerical,
            &["spot_shift", "volatility_shift", "rate_shift", "time_shift"][..],
        ),
        (
            CoxRossRubinsteinVanillaEngine::default().greek_method(&option, &context)?,
            GreekMethod::Mixed,
            &["spot_shift", "volatility_shift", "rate_shift", "time_shift"][..],
        ),
        (
            MonteCarloDigitalEngine::default().greek_method(&digital, &context)?,
            GreekMethod::Native,
            &[][..],
        ),
        (
            MonteCarloBarrierEngine::default().greek_method(&barrier, &context)?,
            GreekMethod::NativeWithSpotControl,
            &["spot_shift"][..],
        ),
    ];
    for (method, expected, controls) in cases {
        assert_eq!(method, expected);
        assert_eq!(method.active_controls(), controls);
    }
    let terminal = context.replace(PricingContextChanges {
        valuation_time: Some(expiry),
        ..Default::default()
    })?;
    assert_eq!(
        AnalyticEuropeanEngine.greek_method(&option, &terminal)?,
        GreekMethod::Native
    );
    assert_eq!(GreekMethod::Terminal.active_controls(), &["spot_shift"]);
    let zero = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.0)?,
        100.0,
        start,
    )?;
    assert_eq!(
        AnalyticEuropeanEngine.greek_method(&option, &zero)?,
        GreekMethod::Numerical
    );
    let local = PricingContext::new(
        LocalVolatilityParameters::new(
            0.05,
            0.02,
            LocalVolatilitySurface::new(
                vec![start, expiry],
                vec![80.0, 120.0],
                vec![vec![0.2; 2]; 2],
            )?,
        )?,
        100.0,
        start,
    )?;
    assert_eq!(
        FiniteDifferenceVanillaEngine::default().greek_method(&option, &local)?,
        GreekMethod::NativeWithSpotControl
    );
    assert!(matches!(
        AnalyticEuropeanEngine.greek_method(&option, &local),
        Err(Error::UnsupportedOperation(_))
    ));
    let heston = PricingContext::new(
        HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.5,
            vol_of_vol: 0.3,
            correlation: -0.7,
        })?,
        100.0,
        start,
    )?;
    assert_eq!(
        AnalyticHestonEuropeanEngine::default().greek_method(&option, &heston)?,
        GreekMethod::Unsupported
    );
    assert_eq!(GreekMethod::Unsupported.active_controls(), [] as [&str; 0]);
    Ok(())
}

#[test]
fn greek_methods_check_curve_support_without_rejecting_asian_settlement() -> Result<()> {
    fn check<I: support::DatedHistory>(
        engine: &impl PricingEngine<I>,
        option: &I,
        context: &PricingContext,
        curve: &RateTermStructure,
        method: GreekMethod,
    ) -> Result<()> {
        assert_eq!(engine.greek_method(option, context)?, method);
        assert!(engine.price(&dated(option, context), context)?.is_finite());
        for dividend in [false, true] {
            for volatility in [0.0, 0.2] {
                let model = BlackScholesMertonParameters::new(
                    if dividend {
                        Rate::Flat(0.05)
                    } else {
                        curve.clone().into()
                    },
                    if dividend {
                        curve.clone().into()
                    } else {
                        Rate::Flat(0.02)
                    },
                    volatility,
                )?;
                let unsupported = context.replace(PricingContextChanges {
                    model_parameters: Some(model.into()),
                    ..Default::default()
                })?;
                assert!(matches!(
                    engine.price(&dated(option, &unsupported), &unsupported),
                    Err(Error::UnsupportedOperation(_))
                ));
                assert!(matches!(
                    engine.greek_method(option, &unsupported),
                    Err(Error::UnsupportedOperation(_))
                ));
            }
        }
        Ok(())
    }
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(180)?, end], vec![0.98, 0.94])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let european = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let american = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None, None)?;
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None, None)?;
    let mc = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 16,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    })?;
    check(
        &CoxRossRubinsteinVanillaEngine::default(),
        &european,
        &context,
        &curve,
        GreekMethod::Mixed,
    )?;
    check(
        &CoxRossRubinsteinVanillaEngine::default(),
        &american,
        &context,
        &curve,
        GreekMethod::Mixed,
    )?;
    check(
        &CoxRossRubinsteinDigitalEngine::default(),
        &digital,
        &context,
        &curve,
        GreekMethod::Native,
    )?;
    check(
        &BjerksundStenslandVanillaEngine,
        &american,
        &context,
        &curve,
        GreekMethod::Numerical,
    )?;
    check(&mc, &geometric, &context, &curve, GreekMethod::Native)?;
    check(
        &TurnbullWakemanArithmeticAveragePriceEngine,
        &arithmetic,
        &context,
        &curve,
        GreekMethod::Numerical,
    )?;

    let curved = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    assert_eq!(mc.greek_method(&arithmetic, &curved)?, GreekMethod::Native);
    assert!(mc.price(&dated(&arithmetic, &curved), &curved)?.is_finite());
    assert_eq!(
        AnalyticGeometricAveragePriceEngine.greek_method(&geometric, &curved)?,
        GreekMethod::Numerical
    );
    assert!(
        AnalyticGeometricAveragePriceEngine
            .price(&dated(&geometric, &curved), &curved)?
            .is_finite()
    );
    let terminal = curved.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    let geometric = geometric.replace_history(110.0, Some(end))?;
    let arithmetic = arithmetic.replace_history(110.0, Some(end))?;
    assert_eq!(
        mc.greek_method(&geometric, &terminal)?,
        GreekMethod::Terminal
    );
    assert_eq!(mc.price(&dated(&geometric, &terminal), &terminal)?, 10.0);
    assert_eq!(
        TurnbullWakemanArithmeticAveragePriceEngine.greek_method(&arithmetic, &terminal)?,
        GreekMethod::Terminal
    );
    assert_eq!(
        TurnbullWakemanArithmeticAveragePriceEngine
            .price(&dated(&arithmetic, &terminal), &terminal)?,
        10.0
    );
    Ok(())
}

#[test]
fn observation_replacement_preserves_validity_and_original_values() -> Result<()> {
    let time = Date::new(2026, 1, 1)?.start_of_day();
    let original = AutocallableObservation::snowball(time, 110.0, -0.01)?;
    let phoenix = AutocallableObservation::phoenix(time, 110.0, 90.0)?;
    assert_eq!(phoenix.coupon_rate(), None);
    assert!(matches!(
        phoenix.replace(AutocallableObservationChanges {
            coupon_rate: Some(0.12),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        autocallable_observations(&[time], &[110.0], Some(&[0.12]), Some(&[90.0])),
        Err(Error::InvalidInput(_))
    ));
    let changed = original.replace(AutocallableObservationChanges {
        time: Some(time.add_days(1)?),
        knock_out_level: Some(120.0),
        coupon_rate: Some(0.08),
        ..Default::default()
    })?;
    assert_eq!(changed.time(), time.add_days(1)?);
    assert_eq!(changed.knock_out_level(), 120.0);
    assert_eq!(changed.coupon_rate(), Some(0.08));
    assert_eq!(changed.coupon_barrier_level(), None);
    assert_eq!(original.replace(Default::default())?, original);
    for changes in [
        AutocallableObservationChanges {
            knock_out_level: Some(0.0),
            ..Default::default()
        },
        AutocallableObservationChanges {
            knock_out_level: Some(f64::INFINITY),
            ..Default::default()
        },
        AutocallableObservationChanges {
            coupon_rate: Some(f64::NAN),
            ..Default::default()
        },
        AutocallableObservationChanges {
            coupon_barrier_level: Some(-1.0),
            ..Default::default()
        },
        AutocallableObservationChanges {
            coupon_barrier_level: Some(f64::INFINITY),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            original.replace(changes),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(original.knock_out_level(), 110.0);
        assert_eq!(original.coupon_rate(), Some(-0.01));
        assert_eq!(original.coupon_barrier_level(), None);
    }
    Ok(())
}

#[test]
fn zero_coupon_observations_preserve_product_identity() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = Date::new(2026, 1, 2)?.start_of_day();
    let snowball = AutocallableObservation::snowball(end, 110.0, 0.0)?;
    let phoenix = AutocallableObservation::phoenix(end, 110.0, 0.0)?;
    for record in [snowball, phoenix] {
        assert_eq!(
            record.replace(AutocallableObservationChanges {
                coupon_rate: record.coupon_rate(),
                coupon_barrier_level: record.coupon_barrier_level(),
                ..Default::default()
            })?,
            record
        );
    }
    assert_eq!(snowball.coupon_rate(), Some(0.0));
    assert_eq!(snowball.coupon_barrier_level(), None);
    assert_eq!(phoenix.coupon_rate(), None);
    assert_eq!(phoenix.coupon_barrier_level(), Some(0.0));
    assert_eq!(
        autocallable_observations(&[end], &[110.0], None, Some(&[0.0]))?,
        vec![phoenix]
    );
    assert_ne!(snowball, phoenix);
    assert_eq!(snowball.kind(), AutocallableObservationKind::Snowball);
    assert_eq!(
        phoenix.replace(Default::default())?.kind(),
        AutocallableObservationKind::Phoenix
    );
    assert!(matches!(
        snowball.replace(AutocallableObservationChanges {
            coupon_barrier_level: Some(0.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        phoenix.replace(AutocallableObservationChanges {
            coupon_rate: Some(0.0),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        BinarySnowballOption::new(BinarySnowballTerms::new(0.1, start, end, vec![phoenix])),
        Err(Error::InvalidInput(_))
    ));
    let terms = PhoenixTerms::new(
        kiyosi::PhoenixCoupon::Annual(0.1),
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        vec![snowball],
        Some(end.time_of_day()),
    );
    assert!(matches!(
        PhoenixOption::new(terms),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn accumulator_history_replacement_updates_quantity_and_settlement() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let original = Accumulator::new(AccumulatorTerms::new(
        90.0,
        110.0,
        1.0,
        2.0,
        start,
        start.add_days(9)?,
        (start.add_days(9)?).time_of_day(),
    ))?;
    let settled = original.replace_history(
        AccumulatorHistoryChanges {
            accumulated_quantity: Some(Some(3.0)),
            knock_out_settled: Some(Some(true)),
        },
        Some(original.history_time().unwrap_or(original.effective_time())),
    )?;
    assert_eq!(settled.accumulated_quantity(), Some(3.0));
    assert_eq!(settled.knock_out_settled(), Some(true));
    assert_eq!(original.accumulated_quantity(), None);
    assert_eq!(original.knock_out_settled(), None);
    assert_eq!(
        settled
            .replace_history(
                4.0,
                Some(settled.history_time().unwrap_or(settled.effective_time()))
            )?
            .knock_out_settled(),
        Some(true)
    );
    assert_eq!(
        settled
            .replace_history(
                AccumulatorHistoryChanges {
                    knock_out_settled: Some(Some(false)),
                    ..Default::default()
                },
                Some(settled.history_time().unwrap_or(settled.effective_time()))
            )?
            .accumulated_quantity(),
        Some(3.0)
    );
    let cleared = settled.replace_history(None, settled.history_time())?;
    assert_eq!(cleared.accumulated_quantity(), None);
    assert_eq!(cleared.knock_out_settled(), Some(true));
    assert_eq!(
        settled.replace_history(
            AccumulatorHistoryChanges::default(),
            Some(settled.history_time().unwrap_or(settled.effective_time()))
        )?,
        settled
    );
    assert!(matches!(
        settled.replace_history(
            AccumulatorHistoryChanges {
                accumulated_quantity: Some(Some(-1.0)),
                knock_out_settled: Some(Some(false))
            },
            Some(settled.history_time().unwrap_or(settled.effective_time()))
        ),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(settled.knock_out_settled(), Some(true));
    assert_eq!(settled.accumulated_quantity(), Some(3.0));
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        start.add_days(2)?,
    )?;
    assert_eq!(
        QuadratureAccumulatorEngine::default().price(&dated(&settled, &context), &context)?,
        0.0
    );
    let active = settled.replace_history(
        AccumulatorHistoryChanges {
            accumulated_quantity: Some(Some(0.0)),
            knock_out_settled: Some(Some(false)),
        },
        Some(settled.history_time().unwrap_or(settled.effective_time())),
    )?;
    assert!(
        QuadratureAccumulatorEngine::default().price(&dated(&active, &context), &context)? > 0.0
    );
    Ok(())
}

#[test]
fn smaller_volatility_bumps_recover_unavailable_stencils() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.000_05)?,
        100.0,
        start,
    )?;
    let request = GreeksRequest::new([Greek::Vega]);
    let unavailable = numerical_greeks(
        &AnalyticEuropeanEngine,
        &dated(&option, &context),
        &context,
        request,
        Default::default(),
    )?;
    assert_eq!(
        unavailable.status(Greek::Vega),
        GreekStatus::StencilUnavailable
    );
    assert_eq!(unavailable.get(Greek::Vega), None);
    let recovered = numerical_greeks(
        &AnalyticEuropeanEngine,
        &dated(&option, &context),
        &context,
        request,
        NumericalShiftSettings {
            volatility_shift: 0.000_01,
            ..Default::default()
        },
    )?;
    let exact =
        AnalyticEuropeanEngine.price_with_greeks(&dated(&option, &context), &context, request)?;
    assert_eq!(recovered.status(Greek::Vega), GreekStatus::Available);
    assert!((recovered.require(Greek::Vega)? - exact.require(Greek::Vega)?).abs() < 1e-7);
    let zero = context.replace(PricingContextChanges {
        model_parameters: Some(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?.into()),
        ..Default::default()
    })?;
    assert_eq!(
        numerical_greeks(
            &AnalyticEuropeanEngine,
            &dated(&option, &zero),
            &zero,
            request,
            Default::default()
        )?
        .status(Greek::Vega),
        GreekStatus::Undefined
    );
    Ok(())
}

#[test]
fn fd_greek_shifts_recover_vega_and_preserve_native_spot_estimates() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.000_05)?,
        100.0,
        start,
    )?;
    let settings = FiniteDifferenceSettings {
        asset_step_count: 200,
        time_step_count: 100,
        ..Default::default()
    };
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega]);
    let base = FiniteDifferenceVanillaEngine::new(settings)?
        .price_with_greeks(&option, &context, request)?;
    assert_eq!(base.status(Greek::Vega), GreekStatus::StencilUnavailable);
    let configured = FiniteDifferenceSettings {
        volatility_shift: 0.000_01,
        rate_shift: 0.000_2,
        time_shift: Some(std::time::Duration::from_secs(60)),
        ..settings
    };
    let engine = FiniteDifferenceVanillaEngine::new(configured)?;
    assert_eq!(engine.settings(), configured);
    let tuned = engine.price_with_greeks(&option, &context, request)?;
    let original = FiniteDifferenceVanillaEngine::new(settings)?;
    assert_eq!(
        original.price_with_greeks_with_settings(
            &option,
            &context,
            request,
            engine.greek_settings(&option)
        )?,
        tuned
    );
    assert_eq!(original.settings(), settings);
    assert_eq!(tuned.price(), base.price());
    assert_eq!(tuned.require(Greek::Delta)?, base.require(Greek::Delta)?);
    assert_eq!(tuned.require(Greek::Gamma)?, base.require(Greek::Gamma)?);
    let exact = AnalyticEuropeanEngine.price_with_greeks(&option, &context, request)?;
    assert!((tuned.require(Greek::Vega)? - exact.require(Greek::Vega)?).abs() < 0.1);
    let regular = context.replace(PricingContextChanges {
        model_parameters: Some(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?.into()),
        ..Default::default()
    })?;
    let measures = [Greek::Rho, Greek::DividendRho, Greek::Theta];
    let request = GreeksRequest::new(measures);
    let configured_result = engine.price_with_greeks(&option, &regular, request)?;
    let scalar = numerical_greeks(
        &engine,
        &option,
        &regular,
        request,
        NumericalShiftSettings {
            volatility_shift: configured.volatility_shift,
            rate_shift: configured.rate_shift,
            time_shift: configured.time_shift,
            ..Default::default()
        },
    )?;
    for greek in measures {
        assert!((configured_result.require(greek)? - scalar.require(greek)?).abs() < 1e-10);
    }
    for invalid in [
        FiniteDifferenceSettings {
            volatility_shift: 0.0,
            ..settings
        },
        FiniteDifferenceSettings {
            rate_shift: f64::NAN,
            ..settings
        },
        FiniteDifferenceSettings {
            time_shift: Some(std::time::Duration::ZERO),
            ..settings
        },
        FiniteDifferenceSettings {
            time_shift: Some(std::time::Duration::from_nanos(1)),
            ..settings
        },
    ] {
        assert!(matches!(
            FiniteDifferenceVanillaEngine::new(invalid),
            Err(Error::InvalidInput(_))
        ));
    }
    Ok(())
}
