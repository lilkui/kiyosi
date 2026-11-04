use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}

fn market(spot: f64, rate: f64, dividend: f64, sigma: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, sigma).unwrap(),
        spot,
        dates().0,
    )
    .unwrap()
}

#[test]
fn european_prices_match_analytic_across_moneyness_carry_and_scale() {
    let (start, end) = dates();
    for kind in [OptionType::Call, OptionType::Put] {
        for (spot, rate, dividend, sigma) in [
            (80.0, 0.04, 0.01, 0.3),
            (100.0, 0.05, 0.02, 0.2),
            (120.0, -0.02, -0.03, 0.4),
            (100.0, 0.0, 0.0, 0.01),
        ] {
            for scale in [1e-8, 1.0, 1e8] {
                let option = EuropeanOption::new(kind, 100.0 * scale, start, end).unwrap();
                let context = market(spot * scale, rate, dividend, sigma);
                let actual = QuadratureVanillaEngine::default()
                    .price(&option, &context)
                    .unwrap();
                let expected = AnalyticEuropeanEngine.price(&option, &context).unwrap();
                assert!(
                    (actual - expected).abs() < 1e-6 * scale,
                    "{kind:?} {spot} {rate} {dividend} {sigma} {scale}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn american_one_interval_is_exercise_now_or_at_expiry() {
    let (start, end) = dates();
    let engine = QuadratureVanillaEngine::new(QuadratureVanillaSettings {
        time_step_count: 1,
        ..Default::default()
    })
    .unwrap();
    for kind in [OptionType::Call, OptionType::Put] {
        for spot in [50.0, 100.0, 150.0] {
            let american = AmericanOption::new(kind, 100.0, start, end).unwrap();
            let european = EuropeanOption::new(kind, 100.0, start, end).unwrap();
            let context = market(spot, 0.05, 0.08, 0.2);
            let sign = if kind == OptionType::Call { 1.0 } else { -1.0 };
            let expected = AnalyticEuropeanEngine
                .price(&european, &context)
                .unwrap()
                .max((sign * (spot - 100.0)).max(0.0));
            assert!((engine.price(&american, &context).unwrap() - expected).abs() < 1e-10);
        }
    }
}

#[test]
fn american_prices_refine_toward_independent_tree_with_negative_carry() {
    let (start, end) = dates();
    let reference =
        CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 4000 }).unwrap();
    let engine = QuadratureVanillaEngine::new(QuadratureVanillaSettings {
        asset_step_count: 768,
        time_step_count: 800,
        integration_standard_deviations: 10.0,
    })
    .unwrap();
    for (kind, spot, rate, dividend) in [
        (OptionType::Put, 100.0, 0.05, 0.02),
        (OptionType::Call, 100.0, 0.04, 0.08),
        (OptionType::Put, 80.0, -0.02, -0.05),
        (OptionType::Call, 120.0, -0.04, -0.01),
    ] {
        let option = AmericanOption::new(kind, 100.0, start, end).unwrap();
        let context = market(spot, rate, dividend, 0.3);
        let actual = engine.price(&option, &context).unwrap();
        let expected = reference.price(&option, &context).unwrap();
        assert!(
            (actual - expected).abs() < 0.01,
            "{kind:?} {spot} {rate} {dividend}: {actual} != {expected}"
        );
    }
}

#[test]
fn european_greeks_and_implied_volatility_use_shared_contracts() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let context = market(100.0, 0.05, 0.02, 0.2);
    let engine = QuadratureVanillaEngine::default();
    let actual = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    let expected = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    for greek in Greek::ALL {
        assert!(
            (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs() < 1e-4,
            "{greek:?}: {:?} != {:?}",
            actual.get(greek),
            expected.get(greek)
        );
    }
    let sigma = implied_volatility(
        &engine,
        &option,
        &context,
        actual.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.1,
            upper_bound: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-7);
}

#[test]
fn settings_and_life_validation_preserve_expiry_and_failure_state() {
    let (start, end) = dates();
    for settings in [
        QuadratureVanillaSettings {
            asset_step_count: 3,
            ..Default::default()
        },
        QuadratureVanillaSettings {
            time_step_count: 0,
            ..Default::default()
        },
        QuadratureVanillaSettings {
            integration_standard_deviations: f64::NAN,
            ..Default::default()
        },
        QuadratureVanillaSettings {
            integration_standard_deviations: 0.0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            QuadratureVanillaEngine::new(settings),
            Err(Error::InvalidInput(_))
        ));
    }
    let engine = QuadratureVanillaEngine::default();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    let context = market(80.0, 0.05, 0.02, 0.2);
    let expiry = context
        .replace(PricingContextChanges {
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&option, &expiry, GreeksRequest::all(), Default::default())
        .unwrap();
    assert_eq!(result.price(), 20.0);
    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
        assert_eq!(result.get(greek), None);
    }
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        if result.supported().has(greek) {
            assert_eq!(result.require(greek).unwrap(), 0.0);
        }
    }
    assert!((result.require(Greek::Delta).unwrap() + 1.0).abs() < 1e-10);
    let invalid = context
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2024, 12, 31).unwrap().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &invalid),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(engine.settings(), QuadratureVanillaSettings::default());
    assert!(engine.price(&option, &context).unwrap() >= 20.0);
}
