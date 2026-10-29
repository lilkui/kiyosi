use kiyosi_core::{Date, Error, RateTermStructure, Timestamp};

fn at(year: i32, month: u32, day: u32) -> Timestamp {
    Date::new(year, month, day).unwrap().into()
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
    assert!(!curve.is_flat());
    assert_eq!(curve.reference_time(), Some(reference));
    assert_eq!(curve.max_time(), Some(end));
    assert_eq!(curve.nodes(), &[middle, end]);
    assert_eq!(curve.factors()[0], first);
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
fn pricing_consumes_both_curves_and_replacement_is_transactional() {
    use kiyosi_core::{
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
        model.risk_free_rate().unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let context = PricingContext::new(model.clone(), 100.0, start).unwrap();
    let call = EuropeanOption::new(OptionType::Call, 103.0, start.date(), end.date()).unwrap();
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
    assert_eq!(rolled.model_parameters(), &model);
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
    use kiyosi_core::Error;
    let reference = at(2025, 1, 1);
    let end = at(2026, 1, 1);
    let before = at(2024, 1, 2);
    let flat = RateTermStructure::flat(-0.03, None).unwrap();
    assert!(flat.is_flat());
    assert_eq!(flat.reference_time(), None);
    assert_eq!(flat.max_time(), None);
    assert!(flat.nodes().is_empty());
    assert!(flat.factors().is_empty());
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
    assert!((changed.factors()[0] - 0.95 * (-0.01_f64).exp()).abs() < 1e-14);
    assert!((flat.shifted(0.01).unwrap().flat_rate().unwrap() + 0.02).abs() < 1e-15);
    assert_eq!(curve.factors(), vec![0.95]);
    let mut factors = curve.factors();
    factors.clear();
    assert_eq!(curve.factors(), vec![0.95]);
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
    use kiyosi_core::{
        AnalyticEuropeanEngine, BlackScholesMertonParameters, Error, EuropeanOption, OptionType,
        PricingContext,
    };
    let origin = at(1, 1, 1);
    let first = Timestamp::new(Date::new(9997, 12, 31).unwrap(), 23, 59, 59, 999999).unwrap();
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
    let call = EuropeanOption::new(OptionType::Call, 100.0, first.date(), second.date()).unwrap();
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
    let call = EuropeanOption::new(
        OptionType::Call,
        100_f64.exp(),
        origin.date(),
        second.date(),
    )
    .unwrap();
    assert!(matches!(
        AnalyticEuropeanEngine.price(&call, &context).unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn context_validation_timing_replacement_and_concurrent_owned_values() {
    use kiyosi_core::{
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
        assert!(matches!(
            model
                .replace(Changes {
                    volatility: Some(value),
                    ..Default::default()
                })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
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
    assert_eq!(zero.risk_free_rate().unwrap(), 0.0);
    assert_eq!(zero.dividend_yield().unwrap(), 0.0);
    let short = RateTermStructure::new(start, vec![at(2025, 6, 30)], vec![0.98]).unwrap();
    let short_model = Model::new(short, 0.0, 0.2).unwrap();
    let short_context = context
        .replace(PricingContextChanges {
            model_parameters: Some(short_model),
            ..Default::default()
        })
        .unwrap();
    let call = EuropeanOption::new(OptionType::Call, 100.0, start.date(), end.date()).unwrap();
    assert!(matches!(
        AnalyticEuropeanEngine
            .price(&call, &short_context)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let changed = context
        .replace(PricingContextChanges {
            spot_price: Some(105.0),
            valuation_time: Some(Timestamp::new(start.date(), 12, 30, 1, 123456).unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.calendar(), context.calendar());
    assert_eq!(changed.model_parameters(), &model);
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
    assert_eq!(changed.model_parameters().risk_free_curve(), &risk);
}
