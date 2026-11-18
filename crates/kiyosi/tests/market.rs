use kiyosi::{Date, Error, RateTermStructure, Timestamp, TradingCalendar};

fn at(year: i32, month: u32, day: u32) -> Timestamp {
    Date::new(year, month, day).unwrap().start_of_day()
}

#[test]
fn rounded_flat_nodes_require_explicit_constant_curve_representation() {
    let start = at(2025, 1, 1);
    let end = start.add_days(365).unwrap();
    let curve = RateTermStructure::new(
        start,
        vec![start.add_days(180).unwrap(), end],
        vec![(-0.05_f64 * 180.0 / 365.0).exp(), (-0.05_f64).exp()],
    )
    .unwrap();
    assert_eq!(curve.exact_flat_rate(), None);
    let model = kiyosi::BlackScholesMertonParameters::new(curve, 0.0, 0.2).unwrap();
    let error = model.flat_risk_free_rate().unwrap_err();
    assert!(matches!(error, Error::UnsupportedOperation(_)));
    assert!(error.to_string().contains("bounded_flat"));
    let explicit = RateTermStructure::bounded_flat(0.05, start, end).unwrap();
    assert_eq!(explicit.exact_flat_rate(), Some(0.05));
    assert!(matches!(
        explicit.forward_rate(end.add_days(1).unwrap()),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn log_interpolation_and_right_hand_forwards() {
    let reference = at(2025, 1, 1);
    let middle = at(2025, 6, 30);
    let end = at(2026, 1, 1);
    let first = (-0.02_f64 * 180.0 / 365.0).exp();
    let curve = RateTermStructure::new(
        reference,
        vec![middle, end],
        vec![first, first * (-0.08_f64 * 185.0 / 365.0).exp()],
    )
    .unwrap();
    assert_eq!(curve.reference_time(), Some(reference));
    assert_eq!(curve.max_time(), Some(end));
    assert_eq!(curve.nodes(), &[middle, end]);
    assert_eq!(curve.factors().unwrap()[0], first);
    assert!(
        (curve.log_factor(at(2025, 4, 1), at(2025, 7, 30)).unwrap()
            + (0.02 * 90.0 + 0.08 * 30.0) / 365.0)
            .abs()
            < 1e-14
    );
    assert!((curve.forward_rate(reference).unwrap() - 0.02).abs() < 1e-13);
    assert!((curve.forward_rate(middle).unwrap() - 0.08).abs() < 1e-13);
    assert_eq!(
        curve.forward_rate(middle).unwrap(),
        curve.forward_rate(end).unwrap()
    );
    assert_eq!(curve.factor(end, end).unwrap(), 1.0);
}

#[test]
fn calendar_bounded_quotes_pause_on_closed_dates() -> kiyosi::Result<()> {
    let saturday = at(2025, 1, 4);
    let monday = saturday.add_days(2)?;
    let friday_noon = saturday.add_micros(-43_200_000_000)?;
    let monday_noon = friday_noon.add_days(3)?;
    let calendar = TradingCalendar::weekdays();
    let curve = RateTermStructure::bounded_flat_with_calendar(
        0.05,
        friday_noon,
        monday_noon,
        calendar.clone(),
    )?;
    assert_eq!(curve.nodes(), &[saturday, monday]);
    assert_eq!(curve.exact_flat_rate(), None);
    assert_eq!(curve.factor(saturday, monday)?, 1.0);
    assert_eq!(curve.forward_rate(saturday)?, 0.0);
    assert!((curve.forward_rate(monday)? - 0.05 * 365.0 / 261.0).abs() < 1e-15);
    assert!((curve.log_factor(friday_noon, monday_noon)? + 0.05 / 261.0).abs() < 1e-15);
    for (factor, &node) in curve.factors()?.iter().zip(curve.nodes()) {
        assert_eq!(*factor, curve.factor(friday_noon, node)?);
    }
    let shifted = curve.shifted(0.01)?;
    assert_eq!(shifted.factor(saturday, monday)?, 1.0);
    assert!((shifted.log_factor(friday_noon, monday_noon)? + 0.06 / 261.0).abs() < 1e-15);
    let zero = RateTermStructure::bounded_flat_with_calendar(
        0.0,
        friday_noon,
        monday_noon,
        calendar.clone(),
    )?;
    assert_eq!(zero.exact_flat_rate(), Some(0.0));
    assert_eq!(zero.shifted(0.05)?, curve);
    assert_eq!(
        RateTermStructure::bounded_flat_with_calendar(
            0.05,
            friday_noon,
            monday_noon,
            TradingCalendar::all_days()
        )?,
        RateTermStructure::bounded_flat(0.05, friday_noon, monday_noon)?
    );
    assert!(matches!(
        shifted.factor(friday_noon, monday_noon.add_micros(1)?),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        RateTermStructure::bounded_flat_with_calendar(0.05, monday_noon, friday_noon, calendar),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn calendar_rate_greeks_use_each_quotes_accrual_time() -> kiyosi::Result<()> {
    use kiyosi::{
        AnalyticEuropeanEngine, BlackScholesMertonParameters, EuropeanOption,
        FiniteDifferenceSettings, FiniteDifferenceVanillaEngine, Greek, GreeksRequest, OptionType,
        PricingContext, PricingEngine, Rate, numerical_greeks,
    };
    let start = at(2025, 1, 3).add_micros(43_200_000_000)?;
    let end = start.add_days(3)?;
    let risk = RateTermStructure::bounded_flat_with_calendar(
        0.05,
        start,
        end,
        TradingCalendar::weekdays(),
    )?;
    let custom = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![end.date()],
        vec![start.add_days(1)?.date()],
        Some(243),
    )?;
    let dividend = RateTermStructure::bounded_flat_with_calendar(-0.02, start, end, custom)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let context = |r: Rate, q: Rate| {
        PricingContext::new(BlackScholesMertonParameters::new(r, q, 0.2)?, 100.0, start)
    };
    let market = context(risk.into(), dividend.into())?;
    let risk_time = 1.0 / 261.0;
    let dividend_time = 1.5 / 243.0;
    let natural_time = start.years_until(end);
    let equivalent = context(
        (0.05 * risk_time / natural_time).into(),
        (-0.02 * dividend_time / natural_time).into(),
    )?;
    let request = GreeksRequest::new([Greek::Rho, Greek::DividendRho]);
    let engine = AnalyticEuropeanEngine;
    let base = engine.price_with_greeks(&option, &equivalent, request)?;
    let actual = engine.price_with_greeks(&option, &market, request)?;
    let numerical = numerical_greeks(&engine, &option, &market, request, Default::default())?;
    assert!((actual.price() - base.price()).abs() < 1e-12);
    let pde = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 600,
        time_step_count: 200,
        ..Default::default()
    })?;
    assert!((pde.price(&option, &market)? - base.price()).abs() < 1e-4);
    for (greek, accrual_time) in [(Greek::Rho, risk_time), (Greek::DividendRho, dividend_time)] {
        let expected = base.require(greek)? * accrual_time / natural_time;
        assert!((actual.require(greek)? - expected).abs() < 1e-12);
        assert!((numerical.require(greek)? - expected).abs() < 1e-8);
    }
    Ok(())
}

#[test]
fn pricing_consumes_both_curves_and_replacement_is_transactional() {
    use kiyosi::{
        AnalyticEuropeanEngine, BlackScholesMertonParameterChanges as ModelChanges,
        BlackScholesMertonParameters as Model, Error, EuropeanOption, OptionType, PricingContext,
        PricingContextChanges, Rate,
    };
    let start = at(2025, 1, 1);
    let middle = at(2025, 6, 30);
    let end = at(2026, 1, 1);
    let risk = RateTermStructure::new(
        start,
        vec![middle, end],
        vec![
            (-0.02_f64 * 180.0 / 365.0).exp(),
            (-(0.02_f64 * 180.0 + 0.08 * 185.0) / 365.0).exp(),
        ],
    )
    .unwrap();
    let dividend = RateTermStructure::new(
        at(2024, 12, 2),
        vec![at(2025, 4, 1), end],
        vec![
            (-0.01_f64 * 120.0 / 365.0).exp(),
            (-(0.01_f64 * 120.0 + 0.03 * 275.0) / 365.0).exp(),
        ],
    )
    .unwrap();
    let model = Model::new(risk.clone(), dividend.clone(), 0.2).unwrap();
    assert_eq!(model.risk_free_curve(), &risk);
    assert!(matches!(
        model.flat_risk_free_rate().unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let context = PricingContext::new(model.clone(), 100.0, start).unwrap();
    let call = EuropeanOption::new(OptionType::Call, 103.0, start, end).unwrap();
    let equivalent = PricingContext::new(
        Model::new(
            (0.02 * 180.0 + 0.08 * 185.0) / 365.0,
            (0.01 * 90.0 + 0.03 * 275.0) / 365.0,
            0.2,
        )
        .unwrap(),
        100.0,
        start,
    )
    .unwrap();
    assert!(
        (AnalyticEuropeanEngine.price(&call, &context).unwrap()
            - AnalyticEuropeanEngine.price(&call, &equivalent).unwrap())
        .abs()
            < 1e-11
    );
    let changed = model
        .replace(ModelChanges {
            volatility: Some(0.25),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.risk_free_curve(), &risk);
    assert_eq!(changed.dividend_curve(), &dividend);
    assert_eq!(changed.volatility(), 0.25);
    assert_eq!(model.volatility(), 0.2);
    assert!(matches!(
        model
            .replace(ModelChanges {
                risk_free_rate: Some(Rate::Flat(f64::NAN)),
                ..Default::default()
            })
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        context
            .replace(PricingContextChanges {
                valuation_time: Some(at(2026, 1, 2)),
                ..Default::default()
            })
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(context.valuation_time(), start);
    let rolled = context
        .replace(PricingContextChanges {
            valuation_time: Some(middle),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rolled.model_parameters().as_bsm().unwrap(), &model);
    assert_eq!(rolled.valuation_time(), middle);
}

#[test]
fn malformed_nodes_coverage_and_numerical_limits_have_stable_categories() {
    let reference = at(2025, 1, 1);
    let middle = at(2025, 6, 30);
    let end = at(2026, 1, 1);
    for bad in [0.0, -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            RateTermStructure::new(reference, vec![middle], vec![bad]).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for nodes in [
        vec![],
        vec![reference],
        vec![at(2024, 12, 31)],
        vec![middle, middle],
        vec![end, middle],
    ] {
        assert!(matches!(
            RateTermStructure::new(reference, nodes.clone(), vec![0.95; nodes.len()]).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(matches!(
        RateTermStructure::new(reference, vec![middle, end], vec![0.95]).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let curve = RateTermStructure::new(reference, vec![middle, end], vec![1.02, 0.99]).unwrap();
    assert!(curve.forward_rate(reference).unwrap() < 0.0);
    assert!(curve.forward_rate(middle).unwrap() > 0.0);
    assert!((curve.factor(middle, end).unwrap() - 0.99 / 1.02).abs() < 1e-14);
    for (start, end) in [
        (at(2024, 12, 31), middle),
        (reference, at(2026, 1, 2)),
        (middle, reference),
    ] {
        assert!(matches!(
            curve.log_factor(start, end).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            curve.factor(start, end).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for time in [at(2024, 12, 31), at(2026, 1, 2)] {
        assert!(matches!(
            curve.forward_rate(time).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            RateTermStructure::flat(bad, None).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            curve.shifted(bad).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let huge = RateTermStructure::flat(f64::MAX, None).unwrap();
    assert!(matches!(
        huge.shifted(f64::MAX).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        huge.log_factor(reference, at(2027, 1, 1)).unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert!(matches!(
        RateTermStructure::flat(-1000.0, None)
            .unwrap()
            .factor(reference, end)
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(
        RateTermStructure::flat(1000.0, None)
            .unwrap()
            .factor(reference, end)
            .unwrap(),
        0.0
    );
}

#[test]
fn flat_and_parallel_shifted_curves_preserve_coverage_and_ownership() {
    use kiyosi::Error;
    let reference = at(2025, 1, 1);
    let end = at(2026, 1, 1);
    let before = at(2024, 1, 2);
    let flat = RateTermStructure::flat(-0.03, None).unwrap();
    assert_eq!(flat.reference_time(), None);
    assert_eq!(flat.max_time(), None);
    assert_eq!(flat.nodes(), []);
    assert_eq!(flat.factors().unwrap(), [] as [f64; 0]);
    assert!((flat.factor(before, reference).unwrap() - 0.03_f64.exp()).abs() < 1e-14);
    let dated = RateTermStructure::flat(-0.03, Some(reference)).unwrap();
    assert!(matches!(
        dated.factor(before, reference).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(dated.max_time(), None);
    let curve = RateTermStructure::new(reference, vec![end], vec![0.95]).unwrap();
    let changed = curve.shifted(0.01).unwrap();
    assert_eq!(curve.shifted(0.0).unwrap(), curve);
    assert_eq!(changed.nodes(), curve.nodes());
    assert_eq!(changed.reference_time(), curve.reference_time());
    assert_eq!(changed.max_time(), curve.max_time());
    assert!((changed.factor(reference, end).unwrap() - 0.95 * (-0.01_f64).exp()).abs() < 1e-14);
    assert!((changed.factors().unwrap()[0] - 0.95 * (-0.01_f64).exp()).abs() < 1e-14);
    assert!((flat.shifted(0.01).unwrap().exact_flat_rate().unwrap() + 0.02).abs() < 1e-15);
    assert_eq!(curve.factors().unwrap(), vec![0.95]);
    let mut factors = curve.factors().unwrap();
    factors.clear();
    assert_eq!(curve.factors().unwrap(), vec![0.95]);
    assert!(matches!(
        curve
            .shifted(f64::MAX)
            .unwrap()
            .shifted(f64::MAX)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn distant_microsecond_nodes_remain_distinct_and_pricing_rejects_unresolved_horizons() {
    use kiyosi::{
        AnalyticEuropeanEngine, BlackScholesMertonParameters, Error, EuropeanOption, OptionType,
        PricingContext,
    };
    let origin = at(1, 1, 1);
    let first = Timestamp::new(Date::new(9997, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap();
    let second = at(9998, 1, 1);
    let curve = RateTermStructure::new(
        origin,
        vec![first, second],
        vec![(-100_f64).exp(), (-101_f64).exp()],
    )
    .unwrap();
    assert!((curve.log_factor(first, second).unwrap() + 1.0).abs() < 1e-14);
    assert!((curve.factor(first, second).unwrap() - (-1_f64).exp()).abs() < 1e-14);
    let rate = 365.0 * 86400.0 * 1e6;
    assert!((curve.forward_rate(first).unwrap() / rate - 1.0).abs() < 1e-13);
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve.clone(), 0.0, 0.2).unwrap(),
        100.0,
        first,
    )
    .unwrap();
    let call = EuropeanOption::new(OptionType::Call, 100.0, first, second).unwrap();
    assert!(
        (AnalyticEuropeanEngine.price(&call, &context).unwrap() - 100.0 * (1.0 - (-1_f64).exp()))
            .abs()
            < 1e-11
    );
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 1e-8).unwrap(),
        100.0,
        origin,
    )
    .unwrap();
    let call = EuropeanOption::new(OptionType::Call, 100_f64.exp(), origin, second).unwrap();
    assert!(matches!(
        AnalyticEuropeanEngine.price(&call, &context).unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn deterministic_vanilla_uses_exact_width_between_curve_knots() -> kiyosi::Result<()> {
    use kiyosi::{
        AnalyticEuropeanEngine, BlackScholesMertonParameters, EuropeanOption,
        FiniteDifferenceVanillaEngine, OptionType, PricingContext, PricingEngine,
    };

    let start = at(2025, 1, 1);
    let knot = start.add_days(365)?;
    let expiry = knot.add_micros(1)?;
    let curve = RateTermStructure::new(start, vec![knot, expiry], vec![1.0, 0.5])?;
    let expected = 100.0 - 100.0 * curve.factor(start, expiry)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
    let model = BlackScholesMertonParameters::new(curve, 0.0, 0.0)?;
    let context = PricingContext::new(model, 100.0, start)?;

    for price in [
        AnalyticEuropeanEngine.price(&option, &context)?,
        FiniteDifferenceVanillaEngine::default().price(&option, &context)?,
    ] {
        assert!((price - expected).abs() < 1e-12, "{price} vs {expected}");
    }
    Ok(())
}

#[test]
fn context_validation_timing_replacement_and_concurrent_owned_values() {
    use kiyosi::{
        AnalyticEuropeanEngine, BlackScholesMertonParameterChanges as Changes,
        BlackScholesMertonParameters as Model, Error, EuropeanOption, OptionType, PricingContext,
        PricingContextChanges,
    };
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RateTermStructure>();
    let start = at(2025, 1, 1);
    let end = at(2026, 1, 1);
    let risk = RateTermStructure::new(start, vec![end], vec![0.95]).unwrap();
    let dividend = RateTermStructure::flat(0.01, Some(start)).unwrap();
    let model = Model::new(risk.clone(), dividend, 0.2).unwrap();
    for time in [at(2024, 12, 31), at(2026, 1, 2)] {
        assert!(matches!(
            PricingContext::new(model.clone(), 100.0, time).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let context = PricingContext::new(model.clone(), 100.0, start).unwrap();
    assert_eq!(model.replace(Changes::default()).unwrap(), model);
    assert_eq!(
        context.replace(PricingContextChanges::default()).unwrap(),
        context
    );
    for value in [0.0, f64::NAN, f64::INFINITY] {
        if value != 0.0 {
            assert!(matches!(
                model
                    .replace(Changes {
                        volatility: Some(value),
                        ..Default::default()
                    })
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
        assert!(matches!(
            context
                .replace(PricingContextChanges {
                    spot_price: Some(value),
                    ..Default::default()
                })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let zero = model
        .replace(Changes {
            risk_free_rate: Some(0.0.into()),
            dividend_yield: Some(0.0.into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(zero.flat_risk_free_rate().unwrap(), 0.0);
    assert_eq!(zero.flat_dividend_yield().unwrap(), 0.0);
    let short = RateTermStructure::new(start, vec![at(2025, 6, 30)], vec![0.98]).unwrap();
    let short_model = Model::new(short, 0.0, 0.2).unwrap();
    let short_context = context
        .replace(PricingContextChanges {
            model_parameters: Some(short_model.into()),
            ..Default::default()
        })
        .unwrap();
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    assert!(matches!(
        AnalyticEuropeanEngine
            .price(&call, &short_context)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let changed = context
        .replace(PricingContextChanges {
            spot_price: Some(105.0),
            valuation_time: Some(Timestamp::new(start.date(), 12, 30, 1, 123_456).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.model_parameters().as_bsm().unwrap(), &model);
    assert_eq!(context.spot_price(), 100.0);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| AnalyticEuropeanEngine.price(&call, &context).unwrap());
        let owned_context = context.clone();
        let owned_call = call.clone();
        let second = scope.spawn(move || {
            AnalyticEuropeanEngine
                .price(&owned_call, &owned_context)
                .unwrap()
        });
        assert_eq!(first.join().unwrap(), second.join().unwrap());
    });
    drop(context);
    drop(model);
    assert_eq!(
        changed
            .model_parameters()
            .as_bsm()
            .unwrap()
            .risk_free_curve(),
        &risk
    );
}

#[test]
fn shifted_curve_reconstruction_preserves_the_original_representation() {
    let start = Date::new(2025, 1, 2).unwrap().start_of_day();
    let end = Date::new(2026, 1, 2).unwrap().start_of_day();
    let curve = RateTermStructure::new(start, vec![end], vec![0.95])
        .unwrap()
        .shifted(0.01)
        .unwrap();
    assert_eq!(curve.base_factors(), &[0.95]);
    assert_eq!(curve.parallel_shift(), 0.01);
    let restored =
        RateTermStructure::new(start, curve.nodes().to_vec(), curve.base_factors().to_vec())
            .unwrap()
            .shifted(curve.parallel_shift())
            .unwrap();
    assert_eq!(restored, curve);
    assert_eq!(
        restored.factor(start, end).unwrap(),
        curve.factor(start, end).unwrap()
    );
}

#[test]
fn bulk_curve_factors_follow_scalar_overflow_and_underflow_rules() {
    let start = Date::new(2025, 1, 1).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let original = RateTermStructure::new(start, vec![end], vec![0.95]).unwrap();
    let overflowing = original.shifted(-1000.0).unwrap();
    assert!(matches!(
        overflowing.factors(),
        Err(Error::ComputationFailure(_))
    ));
    assert!(matches!(
        overflowing.factor(start, end),
        Err(Error::ComputationFailure(_))
    ));
    let underflowing = original.shifted(1000.0).unwrap();
    assert_eq!(underflowing.factors().unwrap(), vec![0.0]);
    assert_eq!(underflowing.factor(start, end).unwrap(), 0.0);
    assert_eq!(original.factors().unwrap(), vec![0.95]);
    assert!(matches!(
        RateTermStructure::new(start, vec![end], underflowing.factors().unwrap()),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn bounded_flat_curves_preserve_exact_rates_coverage_and_engine_support() -> kiyosi::Result<()> {
    use kiyosi::{
        BlackScholesMertonParameters, CoxRossRubinsteinVanillaEngine, EuropeanOption, OptionType,
        PricingContext,
    };
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let curve = RateTermStructure::bounded_flat(0.05, start, end)?;
    assert_eq!(curve.exact_flat_rate(), Some(0.05));
    assert_eq!(curve.reference_time(), Some(start));
    assert_eq!(curve.max_time(), Some(end));
    assert_eq!(curve.forward_rate(start.add_days(180)?)?, 0.05);
    assert_eq!(curve.factor(start, end)?, (-0.05_f64).exp());
    for outside in [start.add_days(-1)?, end.add_days(1)?] {
        assert!(matches!(
            curve.forward_rate(outside),
            Err(Error::InvalidInput(_))
        ));
    }
    let shifted = curve.shifted(0.01)?;
    assert_eq!(shifted.exact_flat_rate(), Some(0.05 + 0.01));
    assert_eq!(shifted.max_time(), Some(end));
    assert_eq!(curve.exact_flat_rate(), Some(0.05));
    assert!(matches!(
        RateTermStructure::bounded_flat(0.05, end, start),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        RateTermStructure::bounded_flat(f64::NAN, start, end),
        Err(Error::InvalidInput(_))
    ));
    let point = RateTermStructure::bounded_flat(0.05, start, start)?;
    assert_eq!(point.factor(start, start)?, 1.0);
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let bounded = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let scalar = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        start,
    )?;
    let engine = CoxRossRubinsteinVanillaEngine::default();
    assert_eq!(
        engine.price(&option, &bounded)?,
        engine.price(&option, &scalar)?
    );
    Ok(())
}
