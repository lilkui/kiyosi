use kiyosi::*;

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
        let expected = AnalyticBarrierEngine.price(&single, &context)?;
        let mut previous: Option<[f64; 2]> = None;
        for upper in [121.0, 400.0, 10_000.0] {
            let settings = FiniteDifferenceSettings {
                asset_step_count: 200,
                time_step_count: 200,
                asset_upper_boundary: Some(upper),
                ..Default::default()
            };
            let actual = [
                FiniteDifferenceBarrierEngine::new(settings)?.price(&single, &context)?,
                FiniteDifferenceDoubleBarrierEngine::new(settings)?.price(&double, &context)?,
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
    terms.rebate_timing = RebateTiming::AtHit;
    let settled = BarrierOption::new(terms)?;
    for engine in [
        &AnalyticBarrierEngine as &dyn PricingEngine<BarrierOption>,
        &CoxRossRubinsteinBarrierEngine::default(),
        &QuadratureBarrierEngine::default(),
        &FiniteDifferenceBarrierEngine::default(),
        &MonteCarloBarrierEngine::default(),
    ] {
        assert_eq!(engine.price(&settled, &context)?, 0.0);
        assert_eq!(
            engine
                .price_with_greeks(&settled, &context, GreeksRequest::new([Greek::Delta]))?
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
                engine.price(&settled.replace(changes)?, &context),
                Err(Error::InvalidInput(_))
            ));
        }
        let missing = settled.replace_history(None);
        assert!(matches!(
            engine.price(&missing, &context),
            Err(Error::InvalidInput(_))
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
    .replace_history(Some(0))?;
    let at_expiry = context.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    let price = AnalyticRangeAccrualEngine.price(&range, &at_expiry)?;
    assert!((price - 360.0 / 365.0).abs() < 1e-12);
    let daily = range
        .replace(RangeAccrualOptionChanges {
            calendar: Some(TradingCalendar::all_days()),
            realized_hit_count: Some(range.realized_hit_count()),
            ..Default::default()
        })
        .unwrap();
    assert!((AnalyticRangeAccrualEngine.price(&daily, &at_expiry)? - price / 3.0).abs() < 1e-12);
    assert_eq!(daily.replace_history(None)?.calendar(), daily.calendar());
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
        thread_count: 1,
        ..Default::default()
    };
    let engine = MonteCarloAccumulatorEngine::new(sampling)?;
    let daily = accumulator
        .replace_calendar(TradingCalendar::all_days())
        .unwrap();
    assert!((engine.price(&daily, &context)? - 40.0).abs() < 1e-7);
    assert_eq!(daily.replace_history(0.0)?.calendar(), daily.calendar());

    let note = SnowballOption::new(SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        vec![AutocallableObservation::new(end, 110.0)?],
        Some((end).time_of_day()),
    ))?;
    let daily = note.replace_calendar(TradingCalendar::all_days()).unwrap();
    assert_eq!(
        daily
            .replace_history(Some(AutocallableBarrierState::NoEvent))?
            .calendar(),
        daily.calendar()
    );
    assert_eq!(
        daily
            .with_shifted_coupons(0.2, CouponQuoteConvention::ShiftMaturityCoupon)?
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
        thread_count: 1,
        ..Default::default()
    })?;
    for engine in [
        &AnalyticEuropeanEngine as &dyn PricingEngine,
        &CoxRossRubinsteinVanillaEngine::default(),
        &QuadratureVanillaEngine::default(),
        &FiniteDifferenceVanillaEngine::default(),
        &sampling,
    ] {
        assert!((engine.price(&option, &context)? - engine.price(&option, &nodes)?).abs() < 1e-9);
        assert!(matches!(
            engine.price(&option, &short),
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
                    &option,
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
                let price = engine.price(&option, &context);
                let greeks =
                    engine.price_with_greeks(&option, &context, GreeksRequest::new([Greek::Delta]));
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
        thread_count: 1,
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
                        let price = engine.price(&future, &context)?;
                        assert_eq!(price, engine.price(&existing, &context)?);
                        let request = GreeksRequest::new([Greek::Delta]);
                        let native = engine.price_with_greeks(&future, &context, request)?;
                        assert_eq!(
                            native,
                            engine.price_with_greeks(&existing, &context, request)?
                        );
                        assert!(native.require(Greek::Delta)?.is_finite());
                        let numerical = numerical_greeks(
                            engine,
                            &future,
                            &context,
                            GreeksRequest::new([Greek::Theta]),
                            Default::default(),
                        )?;
                        assert!(numerical.require(Greek::Theta)?.is_finite());
                        assert_eq!(
                            numerical,
                            numerical_greeks(
                                engine,
                                &existing,
                                &context,
                                GreeksRequest::new([Greek::Theta]),
                                Default::default()
                            )?
                        );
                        if volatility > 0.0 {
                            let solved = implied_volatility(
                                engine,
                                &future,
                                &context,
                                price,
                                ImpliedVolatilitySettings {
                                    lower_bound: 0.2,
                                    upper_bound: 0.3,
                                    ..Default::default()
                                },
                            )?;
                            assert!((solved - 0.2).abs() < 1e-12);
                        }
                    }
                    assert_eq!(
                        mc.price_with_statistics(&future, &context)?,
                        mc.price_with_statistics(&existing, &context)?
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
                            engine.price(&future, invalid),
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
            assert_eq!(fd.price(&future, &local)?, fd.price(&existing, &local)?);
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
        thread_count: 1,
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
                engine.price(&future, &context)?,
                engine.price(&existing, &context)?
            );
            assert!(
                engine
                    .price_with_greeks(&future, &context, GreeksRequest::new([Greek::Delta]))?
                    .require(Greek::Delta)?
                    .is_finite()
            );
        }
        if volatility > 0.0 {
            let quote = AnalyticEuropeanEngine.price(&future, &context)?;
            assert!(
                (implied_volatility(
                    &AnalyticEuropeanEngine,
                    &future,
                    &context,
                    quote,
                    Default::default()
                )? - volatility)
                    .abs()
                    < 1e-7
            );
        }
    }
    let heston = PricingContext::new(
        HestonParameters::new(0.05, 0.02, 0.04, 0.04, 1.5, 0.0, -0.7)?,
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
            engine.price(&future, &heston)?,
            engine.price(&existing, &heston)?
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
    assert_eq!(fd.price(&future, &local)?, fd.price(&existing, &local)?);
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
            AutocallableObservation::new(time, level),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            autocallable_observations(&[time], &[level], None, None),
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
    let records = autocallable_observations(&[time], &[110.0], Some(&[-0.01]), Some(&[0.0]))?;
    assert_eq!(records[0].coupon_rate(), -0.01);
    assert_eq!(records[0].coupon_barrier_level(), 0.0);
    let edited = AutocallableObservation::new(time, 110.0)?;
    assert!(matches!(
        edited.replace(AutocallableObservationChanges {
            coupon_rate: Some(f64::NAN),
            ..Default::default()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(edited.coupon_rate(), 0.0);
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
            let original = $contract::new(OptionType::Call, 100.0, start, start, end, None)?;
            assert_eq!(original.realized_average(), None);
            assert!(matches!(
                $engine.price(&original, &context),
                Err(Error::InvalidInput(_))
            ));
            let seasoned = original.replace_history(110.0)?;
            assert_eq!(seasoned.realized_average(), Some(110.0));
            assert!($engine.price(&seasoned, &context)? > 0.0);
            assert_eq!(seasoned.replace_history(None)?, original);
            assert_eq!(
                seasoned.replace(AveragePriceOptionChanges {
                    realized_average: Some(None),
                    ..Default::default()
                })?,
                original
            );
            for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                assert!(matches!(
                    seasoned.replace_history(invalid),
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
        check!(HestonParameters::new(
            curve.clone(),
            curve.clone(),
            0.04,
            0.04,
            1.0,
            0.2,
            -0.5
        )?);
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
        knock_out_settled: Some(Some(true)),
        ..Default::default()
    })?;
    assert_eq!(option.knock_out_settled(), None);
    assert_eq!(
        settled.replace_history(3.0)?.knock_out_settled(),
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
        Box::new(MonteCarloAccumulatorEngine::default()),
    ];
    for engine in engines {
        assert_eq!(engine.price(&settled, &context)?, 0.0);
        assert_eq!(engine.price(&settled.replace_history(3.0)?, &context)?, 0.0);
        let active = settled.replace(AccumulatorChanges {
            knock_out_settled: Some(Some(false)),
            accumulated_quantity: Some(Some(0.0)),
            ..Default::default()
        })?;
        assert!(engine.price(&active, &context)? > 0.0);
        let unknown = settled.replace(AccumulatorChanges {
            knock_out_settled: Some(Some(false)),
            ..Default::default()
        })?;
        assert!(matches!(
            engine.price(&unknown, &context),
            Err(Error::InvalidInput(_))
        ));
        let result = engine.price_with_greeks(&settled, &context, GreeksRequest::all())?;
        for greek in Greek::ALL {
            assert_eq!(result.require(greek)?, 0.0);
        }
        assert!(matches!(
            implied_volatility(engine.as_ref(), &settled, &context, 0.0, Default::default()),
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
    let result = AnalyticEuropeanEngine.price_with_greeks(&option, &context, request)?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
    assert_eq!(result.status(Greek::Gamma), GreekStatus::NotRequested);
    let terminal = context.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    assert_eq!(
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &terminal, request)?
            .status(Greek::Delta),
        GreekStatus::Undefined
    );
    let result = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
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
            HestonParameters::new(0.05, 0.02, 0.04, 0.04, 1.5, 0.3, -0.7)?.into(),
        ),
        ..Default::default()
    })?;
    let result =
        AnalyticHestonEuropeanEngine::default().price_with_greeks(&option, &heston, request)?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Unsupported);
    assert_eq!(result.status(Greek::Gamma), GreekStatus::NotRequested);
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
        &option,
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
    let asian = GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)?;
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
        thread_count: 1,
        ..Default::default()
    };
    let request = GreeksRequest::new([Greek::Delta]);
    macro_rules! check {
        ($engine:expr, $option:expr) => {{
            let engine = $engine;
            let direct = engine.price_with_greeks($option, &context, request)?;
            let generic = PricingEngine::price_with_greeks(&engine, $option, &context, request)?;
            assert_eq!(direct, generic);
            assert_eq!(direct.status(Greek::Delta), GreekStatus::Available);
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
    fn check<I: OptionContract>(
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
                    .price_with_greeks(option, &context, request)?
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
                        kiyosi::numerical_greeks(engine, option, &context, request, shifts),
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
                        option,
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
    check(
        &CoxRossRubinsteinBarrierEngine::default(),
        &BarrierOption::new(terms.clone())?,
        bsm.clone(),
    )?;
    let sampling = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        thread_count: 1,
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
        &GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None)?,
        bsm.clone(),
    )?;
    check(
        &MonteCarloAsianEngine::new(sampling)?,
        &ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None)?,
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
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_times = vec![closed];
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
        &[closed]
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
        autocallable_observations(&[closed], &[110.0], None, None)?,
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
        note.with_shifted_coupons(0.2, CouponQuoteConvention::PreserveMaturityCoupon)?
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
    terms.knock_out_settled = Some(false);
    let accumulator = Accumulator::new(terms)?;
    let settled = accumulator.replace(AccumulatorChanges {
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
        assert_eq!(engine.price(&settled, &context)?, 0.0);
        assert_eq!(engine.price(&accumulator, &context)?, 75.0);
        assert_eq!(
            engine
                .price_with_greeks(&settled, &context, GreeksRequest::new([Greek::Delta]))?
                .require(Greek::Delta)?,
            0.0
        );
        assert!(matches!(
            engine.price(&active, &context),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(
        mc.price_with_statistics(&settled, &context)?
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
            None,
            None,
        )?,
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = BinarySnowballOption::new(terms)?;
    let settled = note.replace_history(Some(AutocallableBarrierState::KnockedOut))?;
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
        assert_eq!(engine.price(&settled, &context)?, 0.0);
        assert_eq!(engine.price(&note, &context)?, 1.0);
        assert_eq!(
            engine
                .price_with_greeks(&settled, &context, GreeksRequest::new([Greek::Delta]))?
                .require(Greek::Delta)?,
            0.0
        );
        assert!(matches!(
            engine.price(&note, &active_context),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(
        mc.price_with_statistics(&settled, &context)?
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
                let price = engine.price(&settled, &context);
                let greeks = engine.price_with_greeks(
                    &settled,
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                );
                if valid {
                    assert_eq!(price?, 0.0);
                    assert_eq!(greeks?.require(Greek::Delta)?, 0.0);
                } else {
                    assert!(matches!(price, Err(Error::InvalidInput(_))));
                    assert!(matches!(greeks, Err(Error::InvalidInput(_))));
                }
            }
            let statistics = mc.price_with_statistics(&settled, &context);
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
    let observations = autocallable_observations(&[end], &[110.0], None, None)?;
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
    assert_eq!(
        <MonteCarloVanillaEngine as PricingEngine<AmericanOption>>::default_spot_shift(
            &MonteCarloVanillaEngine::default(),
            &context
        ),
        8.0
    );
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
            "thread_count",
            MonteCarloSettings {
                thread_count: -1,
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
        HestonParameters::new(0.05, 0.02, 0.04, 0.04, 1.5, 0.3, -0.7)?,
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
    fn check<I: OptionContract>(
        engine: &impl PricingEngine<I>,
        option: &I,
        context: &PricingContext,
        curve: &RateTermStructure,
        method: GreekMethod,
    ) -> Result<()> {
        assert_eq!(engine.greek_method(option, context)?, method);
        assert!(engine.price(option, context)?.is_finite());
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
                    engine.price(option, &unsupported),
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
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)?;
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)?;
    let mc = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 16,
        seed: Some(42),
        thread_count: 1,
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
    assert!(mc.price(&arithmetic, &curved)?.is_finite());
    assert_eq!(
        AnalyticGeometricAveragePriceEngine.greek_method(&geometric, &curved)?,
        GreekMethod::Numerical
    );
    assert!(
        AnalyticGeometricAveragePriceEngine
            .price(&geometric, &curved)?
            .is_finite()
    );
    let terminal = curved.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    let geometric = geometric.replace_history(110.0)?;
    let arithmetic = arithmetic.replace_history(110.0)?;
    assert_eq!(
        mc.greek_method(&geometric, &terminal)?,
        GreekMethod::Terminal
    );
    assert_eq!(mc.price(&geometric, &terminal)?, 10.0);
    assert_eq!(
        TurnbullWakemanArithmeticAveragePriceEngine.greek_method(&arithmetic, &terminal)?,
        GreekMethod::Terminal
    );
    assert_eq!(
        TurnbullWakemanArithmeticAveragePriceEngine.price(&arithmetic, &terminal)?,
        10.0
    );
    Ok(())
}

#[test]
fn observation_replacement_preserves_validity_and_original_values() -> Result<()> {
    let time = Date::new(2026, 1, 1)?.start_of_day();
    let original = AutocallableObservation::snowball(time, 110.0, -0.01)?;
    let phoenix = AutocallableObservation::phoenix(time, 110.0, 90.0)?;
    assert_eq!(phoenix.coupon_rate(), 0.0);
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
    assert_eq!(changed.coupon_rate(), 0.08);
    assert_eq!(changed.coupon_barrier_level(), 0.0);
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
        assert_eq!(original.coupon_rate(), -0.01);
        assert_eq!(original.coupon_barrier_level(), 0.0);
    }
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
    let settled = original.replace_history(AccumulatorHistoryChanges {
        accumulated_quantity: Some(Some(3.0)),
        knock_out_settled: Some(Some(true)),
    })?;
    assert_eq!(settled.accumulated_quantity(), Some(3.0));
    assert_eq!(settled.knock_out_settled(), Some(true));
    assert_eq!(original.accumulated_quantity(), None);
    assert_eq!(original.knock_out_settled(), None);
    assert_eq!(
        settled.replace_history(4.0)?.knock_out_settled(),
        Some(true)
    );
    assert_eq!(
        settled
            .replace_history(AccumulatorHistoryChanges {
                knock_out_settled: Some(Some(false)),
                ..Default::default()
            })?
            .accumulated_quantity(),
        Some(3.0)
    );
    let cleared = settled.replace_history(None)?;
    assert_eq!(cleared.accumulated_quantity(), None);
    assert_eq!(cleared.knock_out_settled(), Some(true));
    assert_eq!(
        settled.replace_history(AccumulatorHistoryChanges::default())?,
        settled
    );
    assert!(matches!(
        settled.replace_history(AccumulatorHistoryChanges {
            accumulated_quantity: Some(Some(-1.0)),
            knock_out_settled: Some(Some(false))
        }),
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
        QuadratureAccumulatorEngine::default().price(&settled, &context)?,
        0.0
    );
    let active = settled.replace_history(AccumulatorHistoryChanges {
        accumulated_quantity: Some(Some(0.0)),
        knock_out_settled: Some(Some(false)),
    })?;
    assert!(QuadratureAccumulatorEngine::default().price(&active, &context)? > 0.0);
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
        &option,
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
        &option,
        &context,
        request,
        NumericalShiftSettings {
            volatility_shift: 0.000_01,
            ..Default::default()
        },
    )?;
    let exact = AnalyticEuropeanEngine.price_with_greeks(&option, &context, request)?;
    assert_eq!(recovered.status(Greek::Vega), GreekStatus::Available);
    assert!((recovered.require(Greek::Vega)? - exact.require(Greek::Vega)?).abs() < 1e-7);
    let zero = context.replace(PricingContextChanges {
        model_parameters: Some(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?.into()),
        ..Default::default()
    })?;
    assert_eq!(
        numerical_greeks(
            &AnalyticEuropeanEngine,
            &option,
            &zero,
            request,
            Default::default()
        )?
        .status(Greek::Vega),
        GreekStatus::Undefined
    );
    Ok(())
}
