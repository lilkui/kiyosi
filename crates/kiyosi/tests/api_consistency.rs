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
                .price_with_greeks(
                    &settled,
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    Default::default(),
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
        1000.0, 0.12, 90.0, 110.0, start, end,
    ))?
    .replace_history(Some(0))?;
    let at_expiry = context.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    let price = AnalyticRangeAccrualEngine.price(&range, &at_expiry)?;
    assert!((price - 360.0 / 365.0).abs() < 1e-12);
    let daily = range
        .clone()
        .with_calendar(TradingCalendar::all_days())
        .unwrap();
    assert!((AnalyticRangeAccrualEngine.price(&daily, &at_expiry)? - price / 3.0).abs() < 1e-12);
    assert_eq!(daily.replace_history(None)?.calendar(), daily.calendar());
    assert_eq!(daily.replace(Default::default())?, daily);

    let accumulator = Accumulator::new(AccumulatorTerms::new(90.0, 200.0, 1.0, 1.0, start, end))?;
    let sampling = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        thread_count: 1,
        ..Default::default()
    };
    let engine = MonteCarloAccumulatorEngine::new(sampling)?;
    let daily = accumulator
        .with_calendar(TradingCalendar::all_days())
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
    ))?;
    let daily = note.with_calendar(TradingCalendar::all_days()).unwrap();
    assert_eq!(
        daily
            .replace_history(Some(AutocallableBarrierState::NoEvent))?
            .calendar(),
        daily.calendar()
    );
    assert_eq!(
        daily
            .with_coupon(0.2, CouponQuoteConvention::ShiftMaturityCoupon)?
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
    assert_eq!(curve.flat_rate(), Some(0.0));
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
                engine.price_with_greeks(
                    &option,
                    &context,
                    request,
                    NumericalShiftSettings {
                        time_shift_days: 0,
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
            for (at, valid) in [(before, false), (start, true), (end, true), (after, false)] {
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
                    100.0,
                    at,
                )?;
                let price = engine.price(&option, &context);
                let greeks = engine.price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta]),
                    Default::default(),
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
                100.0, coupon, 90.0, 110.0, start, end
            ))?
        );
    }
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
    assert_eq!(records[0].coupon_rate, -0.01);
    assert_eq!(records[0].coupon_barrier_level, 0.0);
    let mut edited = AutocallableObservation::new(time, 110.0)?;
    edited.coupon_rate = f64::NAN;
    assert!(matches!(edited.validate(), Err(Error::InvalidInput(_))));
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
                    if let Some(expected) = curve.flat_rate() {
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
fn terminated_accumulators_have_no_remaining_cashflows() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2025, 1, 10)?.start_of_day();
    let option = Accumulator::new(AccumulatorTerms::new(90.0, 110.0, 1.0, 2.0, start, end))?;
    let settled = option.replace(AccumulatorChanges {
        terminated: Some(true),
        ..Default::default()
    })?;
    assert!(!option.terminated());
    assert!(settled.replace_history(3.0)?.terminated());
    assert!(
        settled
            .replace(AccumulatorChanges {
                strike: Some(95.0),
                ..Default::default()
            })?
            .terminated()
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
            terminated: Some(false),
            accumulated_quantity: Some(Some(0.0)),
            ..Default::default()
        })?;
        assert!(engine.price(&active, &context)? > 0.0);
        let unknown = settled.replace(AccumulatorChanges {
            terminated: Some(false),
            ..Default::default()
        })?;
        assert!(matches!(
            engine.price(&unknown, &context),
            Err(Error::InvalidInput(_))
        ));
        let result = engine.price_with_greeks(
            &settled,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )?;
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
    let result =
        AnalyticEuropeanEngine.price_with_greeks(&option, &context, request, Default::default())?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
    assert_eq!(result.status(Greek::Gamma), GreekStatus::NotRequested);
    let terminal = context.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    assert_eq!(
        AnalyticEuropeanEngine
            .price_with_greeks(&option, &terminal, request, Default::default())?
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
    assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
    let heston = context.replace(PricingContextChanges {
        model_parameters: Some(
            HestonParameters::new(0.05, 0.02, 0.04, 0.04, 1.5, 0.3, -0.7)?.into(),
        ),
        ..Default::default()
    })?;
    let result = AnalyticHestonEuropeanEngine::default().price_with_greeks(
        &option,
        &heston,
        request,
        Default::default(),
    )?;
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
        Default::default(),
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
        assert_eq!(curve.flat_rate(), Some(0.05));
        assert!((curve.shifted(0.01)?.flat_rate().unwrap() - 0.06).abs() < 1e-15);
        assert_eq!(curve.reference_time(), reference);
    }
    let nodes = RateTermStructure::new(start, vec![end], vec![(-0.05_f64).exp()])?;
    assert!((nodes.flat_rate().unwrap() - 0.05).abs() < 1e-15);
    Ok(())
}

#[test]
fn native_greek_methods_accept_shared_shift_settings() -> Result<()> {
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
    let shifts = NumericalShiftSettings {
        spot_shift: Some(0.2),
        ..Default::default()
    };
    let request = GreeksRequest::new([Greek::Delta]);
    macro_rules! check {
        ($engine:expr, $option:expr) => {{
            let engine = $engine;
            let direct = engine.price_with_greeks($option, &context, request, shifts)?;
            let generic =
                PricingEngine::price_with_greeks(&engine, $option, &context, request, shifts)?;
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
                    .price_with_greeks(option, &context, request, Default::default())?
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
                        engine.price_with_greeks(option, &context, request, shifts),
                        Err(Error::InvalidInput(_))
                    ));
                }
            }
            for days in [0, -1] {
                assert!(matches!(
                    engine.price_with_greeks(
                        option,
                        &context,
                        request,
                        NumericalShiftSettings {
                            time_shift_days: days,
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
    let heston = HestonParameters::new(0.05, 0.02, 0.04, 0.04, 0.0, 0.0, 0.0)?;
    check(
        &AnalyticHestonEuropeanEngine::default(),
        &vanilla,
        heston.clone(),
    )?;
    check(
        &FiniteDifferenceHestonVanillaEngine::default(),
        &vanilla,
        heston,
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
        option.clone().with_calendar(TradingCalendar::weekdays()),
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
    let mut terms = AccumulatorTerms::new(90.0, 110.0, 1.0, 2.0, start, end);
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
        note.with_coupon(0.2, CouponQuoteConvention::PreserveMaturityCoupon)?
            .calendar(),
        note.calendar()
    );
    let mut terms = RangeAccrualTerms::new(100.0, 0.1, 80.0, 120.0, start, closed);
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
            volatility: Some(0.0),
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
