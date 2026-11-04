use kiyosi::*;

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
                    if curve.is_explicitly_flat() {
                        assert_eq!(rate?, 0.03);
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
    let context = PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        Date::new(2025, 1, 3)?.start_of_day(),
        TradingCalendar::all_days(),
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
    assert_eq!(nodes.flat_rate(), None);
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
