use kiyosi::*;

fn context(spot: f64) -> Result<PricingContext> {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        spot,
        Date::new(2026, 1, 1)?.start_of_day(),
    )
}

#[test]
fn terminal_vanilla_greeks_match_payoff_derivatives_across_engines() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let context = context(120.0)?;
    let sampling = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        thread_count: 1,
        ..Default::default()
    };
    let european: Vec<Box<dyn PricingEngine<EuropeanOption>>> = vec![
        Box::new(AnalyticEuropeanEngine),
        Box::new(QuadratureVanillaEngine::default()),
        Box::new(CoxRossRubinsteinVanillaEngine::default()),
        Box::new(FiniteDifferenceVanillaEngine::default()),
        Box::new(MonteCarloVanillaEngine::new(sampling)?),
    ];
    let american: Vec<Box<dyn PricingEngine<AmericanOption>>> = vec![
        Box::new(BjerksundStenslandVanillaEngine),
        Box::new(QuadratureVanillaEngine::default()),
        Box::new(CoxRossRubinsteinVanillaEngine::default()),
        Box::new(FiniteDifferenceVanillaEngine::default()),
        Box::new(MonteCarloVanillaEngine::new(sampling)?),
    ];
    macro_rules! check {
        ($contract:ident, $engines:expr) => {
            for direction in [OptionType::Call, OptionType::Put] {
                let option = $contract::new(direction, 100.0, start, context.valuation_time())?;
                for engine in &$engines {
                    let result = engine.price_with_greeks(
                        &option,
                        &context,
                        GreeksRequest::all(),
                        Default::default(),
                    )?;
                    assert!(
                        (result.require(Greek::Delta)?
                            - if direction == OptionType::Call {
                                1.0
                            } else {
                                0.0
                            })
                        .abs()
                            < 1e-10
                    );
                    for greek in [
                        Greek::Gamma,
                        Greek::Speed,
                        Greek::Vega,
                        Greek::Volga,
                        Greek::Vanna,
                        Greek::Zomma,
                        Greek::Rho,
                        Greek::DividendRho,
                    ] {
                        assert!(result.require(greek)?.abs() < 1e-8, "{greek:?}: {result:?}");
                    }
                    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
                        assert_eq!(result.status(greek), GreekStatus::Undefined);
                    }
                    let bumped = numerical_greeks(
                        engine.as_ref(),
                        &option,
                        &context,
                        GreeksRequest::new([Greek::Delta]),
                        Default::default(),
                    )?;
                    assert!(
                        (bumped.require(Greek::Delta)? - result.require(Greek::Delta)?).abs()
                            < 1e-10
                    );
                }
            }
        };
    }
    check!(EuropeanOption, european);
    check!(AmericanOption, american);
    Ok(())
}

#[test]
fn terminal_spot_stencils_respect_payoff_kinks_and_request_metadata() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let context = context(100.0)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, context.valuation_time())?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega]);
    let result =
        AnalyticEuropeanEngine.price_with_greeks(&option, &context, request, Default::default())?;
    assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
    assert_eq!(result.status(Greek::Gamma), GreekStatus::Undefined);
    assert_eq!(result.require(Greek::Vega)?, 0.0);
    assert_eq!(result.status(Greek::Rho), GreekStatus::NotRequested);
    let away = context.replace(PricingContextChanges {
        spot_price: Some(120.0),
        ..Default::default()
    })?;
    let crossed = numerical_greeks(
        &AnalyticEuropeanEngine,
        &option,
        &away,
        request,
        NumericalShiftSettings {
            spot_shift: Some(30.0),
            ..Default::default()
        },
    )?;
    assert_eq!(crossed.status(Greek::Delta), GreekStatus::Undefined);
    assert_eq!(crossed.require(Greek::Vega)?, 0.0);
    Ok(())
}

#[test]
fn terminal_autocallable_stencils_preserve_knock_in_and_redemption_regimes() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = context(100.0)?.valuation_time();
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(&[end], &[120.0], Some(&[0.08]), None)?,
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms)?;
    for (spot, delta) in [
        (60.0, None),
        (70.0, Some(0.01)),
        (80.0, None),
        (100.0, Some(0.0)),
        (120.0, None),
    ] {
        let result = QuadratureAutocallableEngine::default().price_with_greeks(
            &note,
            &context(spot)?,
            GreeksRequest::all(),
            Default::default(),
        )?;
        if let Some(delta) = delta {
            assert!((result.require(Greek::Delta)? - delta).abs() < 1e-10);
        } else {
            assert_eq!(result.get(Greek::Delta), None);
        }
        assert_eq!(result.require(Greek::Vega)?, 0.0);
    }
    Ok(())
}
