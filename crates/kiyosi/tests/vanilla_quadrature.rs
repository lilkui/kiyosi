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
fn european_quadrature_and_sampling_preserve_quoted_forward_at_tiny_volatility() -> Result<()> {
    let (start, end) = dates();
    let risk = RateTermStructure::new(start, vec![end], vec![0.75])?;
    let dividend = RateTermStructure::new(start, vec![end], vec![0.625])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(risk, dividend, 1e-17)?,
        1.2e17,
        start,
    )?;
    let quadrature = QuadratureVanillaEngine::default();
    let monte_carlo = MonteCarloVanillaEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count: 16_384,
            max_active_workers: 1,
            seed: Some(42),
            sampling: MonteCarloSampling::Sobol,
            ..Default::default()
        },
        ..Default::default()
    })?;
    for kind in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(kind, 1e17, start, end)?;
        let expected = AnalyticEuropeanEngine.price(&option, &context)?;
        let integrated = quadrature.price(&option, &context)?;
        let sampled = monte_carlo.price(&option, &context)?;
        assert!(
            (integrated - expected).abs() < 1e-8,
            "{kind:?}: {integrated} != {expected}"
        );
        assert!(
            (sampled - expected).abs() < 1e-3,
            "{kind:?}: {sampled} != {expected}"
        );
    }
    Ok(())
}

#[test]
fn european_quadrature_preserves_finite_large_prices() -> Result<()> {
    let (start, end) = dates();
    let context = market(1e308, 0.05, 0.02, 0.2);
    for kind in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(kind, 1e308, start, end)?;
        let expected = AnalyticEuropeanEngine.price(&option, &context)?;
        let actual = QuadratureVanillaEngine::default().price(&option, &context)?;
        assert!(
            (actual / expected - 1.0).abs() < 1e-7,
            "{kind:?}: {actual} != {expected}"
        );
    }
    Ok(())
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
fn american_quadrature_rejects_an_oversized_transition() -> Result<()> {
    let (start, end) = dates();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let context = market(100.0, 0.05, 0.02, 0.2);
    let engine = QuadratureVanillaEngine::new(QuadratureVanillaSettings {
        asset_step_count: 8_000,
        time_step_count: 3,
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &context),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn american_quadrature_rejects_an_oversized_grid_before_rollback() -> Result<()> {
    let (start, end) = dates();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let context = market(100.0, 0.05, 0.02, 0.2);
    let engine = QuadratureVanillaEngine::new(QuadratureVanillaSettings {
        asset_step_count: 2_000_000,
        time_step_count: 2,
        ..Default::default()
    })?;
    assert!(
        matches!(engine.price(&option, &context), Err(Error::InvalidInput(message)) if message.contains("workspace"))
    );
    Ok(())
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
fn american_call_without_early_exercise_preserves_deep_tail() -> Result<()> {
    let (start, end) = dates();
    let context = market(1e20, 0.05, 0.0, 0.1);
    let option = AmericanOption::new(OptionType::Call, 2.5e20, start, end)?;
    let price = QuadratureVanillaEngine::default().price(&option, &context)?;
    assert!((price - 3.985_906_455_712_04).abs() < 1e-10, "{price}");
    Ok(())
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
        .price_with_greeks(&option, &context, GreeksRequest::all())
        .unwrap();
    let expected = AnalyticEuropeanEngine
        .price_with_greeks(&option, &context, GreeksRequest::all())
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
        ImpliedVolatilitySettings::new(0.1, 0.3),
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
        .price_with_greeks(&option, &expiry, GreeksRequest::all())
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

#[test]
fn european_quadrature_counts_panels_across_sections() -> Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = QuadratureVanillaEngine::new(QuadratureVanillaSettings {
        asset_step_count: 125_000_001,
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&option, &market(100.0, 0.05, 0.02, 0.2)),
        Err(Error::InvalidInput(message)) if message.contains("panels")
    ));
    assert!(engine.price(&option, &market(100.0, 0.05, 0.02, 0.0))? > 0.0);
    Ok(())
}
