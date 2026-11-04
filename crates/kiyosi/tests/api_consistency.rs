use kiyosi::*;

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
