mod support;
use support::dated;

use kiyosi::*;

fn context(spot: f64) -> Result<PricingContext> {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        spot,
        Date::new(2026, 1, 1)?.start_of_day(),
    )
}

#[test]
fn resolved_barriers_have_zero_spot_greeks_at_inactive_strikes() -> Result<()> {
    fn check<I: OptionContract + Clone>(
        engine: &impl PricingEngine<I>,
        option: &I,
        context: &PricingContext,
        inactive: bool,
    ) -> Result<()> {
        let result = engine.price_with_greeks(
            &dated(option, context),
            context,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
        )?;
        for greek in [Greek::Delta, Greek::Gamma] {
            assert_eq!(result.get(greek), inactive.then_some(0.0));
            assert_eq!(
                result.status(greek),
                if inactive {
                    GreekStatus::Available
                } else {
                    GreekStatus::Undefined
                }
            );
        }
        Ok(())
    }
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    for (at, volatility) in [(end, 0.2), (start.add_days(180)?, 0.0)] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, 0.0, volatility)?,
            100.0,
            at,
        )?;
        for (kind, history, inactive) in [
            (BarrierType::UpAndOut, BarrierTouchState::Touched, true),
            (BarrierType::UpAndIn, BarrierTouchState::Touched, false),
        ] {
            let mut terms =
                BarrierOptionTerms::new(OptionType::Call, 100.0, start, end, 120.0, kind);
            terms.touch_state = Some(history);
            check(
                &AnalyticBarrierEngine,
                &BarrierOption::new(terms)?,
                &context,
                inactive,
            )?;
            let mut terms = DoubleBarrierOptionTerms::new(
                OptionType::Call,
                100.0,
                start,
                end,
                80.0,
                120.0,
                if kind == BarrierType::UpAndIn {
                    DoubleBarrierType::KnockIn
                } else {
                    DoubleBarrierType::KnockOut
                },
            );
            terms.touch_state = Some(history);
            check(
                &AnalyticDoubleBarrierEngine,
                &DoubleBarrierOption::new(terms)?,
                &context,
                inactive,
            )?;
            let mut terms =
                BinaryBarrierTerms::new(OptionType::Call, 100.0, start, end, 120.0, kind);
            terms.touch_state = Some(history);
            check(
                &AnalyticBinaryBarrierEngine,
                &BinaryBarrierOption::cash(terms, 10.0)?,
                &context,
                inactive,
            )?;
        }
    }
    Ok(())
}

#[test]
fn finite_difference_terminal_greeks_are_exact_for_bsm_and_local_volatility() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let local = LocalVolatilityParameters::new(
        0.05,
        0.02,
        LocalVolatilitySurface::new(
            vec![start, end],
            vec![80.0, 120.0],
            vec![vec![0.2, 0.2], vec![0.2, 0.2]],
        )?,
    )?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let models: [ModelParameters; 2] = [
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?.into(),
        local.into(),
    ];
    for model in models {
        for (direction, deltas) in [
            (OptionType::Call, [Some(0.0), None, Some(1.0)]),
            (OptionType::Put, [Some(-1.0), None, Some(0.0)]),
        ] {
            for (spot, delta) in [
                f64::from_bits(100_f64.to_bits() - 1),
                100.0,
                f64::from_bits(100_f64.to_bits() + 1),
            ]
            .into_iter()
            .zip(deltas)
            {
                let context = PricingContext::new(model.clone(), spot, end)?;
                macro_rules! check {
                    ($kind:ident) => {{
                        let option = $kind::new(direction, 100.0, start, end)?;
                        let result = engine.price_with_greeks(
                            &dated(&option, &context),
                            &context,
                            GreeksRequest::all(),
                        )?;
                        let generic: &dyn PricingEngine<$kind> = &engine;
                        assert_eq!(
                            result,
                            generic.price_with_greeks(
                                &dated(&option, &context),
                                &context,
                                GreeksRequest::all()
                            )?
                        );
                        assert_eq!(
                            generic.greek_method(&option, &context)?,
                            GreekMethod::Native
                        );
                        for greek in Greek::ALL {
                            let expected = if !result.supported().has(greek)
                                || matches!(greek, Greek::Theta | Greek::Charm | Greek::Color)
                            {
                                None
                            } else if greek == Greek::Delta {
                                delta
                            } else if matches!(
                                greek,
                                Greek::Gamma | Greek::Speed | Greek::Vanna | Greek::Zomma
                            ) {
                                delta.map(|_| 0.0)
                            } else {
                                Some(0.0)
                            };
                            assert_eq!(
                                result.get(greek),
                                expected,
                                "{direction:?}, {spot}, {greek:?}"
                            );
                        }
                        let delta_only = engine.price_with_greeks(
                            &dated(&option, &context),
                            &context,
                            GreeksRequest::new([Greek::Delta]),
                        )?;
                        assert_eq!(delta_only.status(Greek::Gamma), GreekStatus::NotRequested);
                    }};
                }
                check!(EuropeanOption);
                check!(AmericanOption);
            }
        }
    }
    Ok(())
}

#[test]
fn analytic_terminal_greeks_are_exact_next_to_the_strike() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let strike = 100.0_f64;
    for volatility in [0.0, 0.2] {
        for (direction, deltas) in [
            (OptionType::Call, [Some(0.0), None, Some(1.0)]),
            (OptionType::Put, [Some(-1.0), None, Some(0.0)]),
        ] {
            for (spot, delta) in [
                f64::from_bits(strike.to_bits() - 1),
                strike,
                f64::from_bits(strike.to_bits() + 1),
            ]
            .into_iter()
            .zip(deltas)
            {
                let context = context(spot)?.replace(PricingContextChanges {
                    model_parameters: Some(
                        BlackScholesMertonParameters::new(0.05, 0.02, volatility)?.into(),
                    ),
                    ..Default::default()
                })?;
                let option =
                    EuropeanOption::new(direction, strike, start, context.valuation_time())?;
                let result = AnalyticEuropeanEngine.price_with_greeks(
                    &dated(&option, &context),
                    &context,
                    GreeksRequest::all(),
                )?;
                let generic: &dyn PricingEngine = &AnalyticEuropeanEngine;
                assert_eq!(
                    result,
                    generic.price_with_greeks(
                        &dated(&option, &context),
                        &context,
                        GreeksRequest::all()
                    )?
                );
                assert_eq!(
                    generic.greek_method(&option, &context)?,
                    GreekMethod::Native
                );
                assert_eq!(
                    generic.greek_method(&option, &context)?.active_controls(),
                    &[] as &[&str]
                );
                for greek in Greek::ALL {
                    let expected = match greek {
                        Greek::Theta | Greek::Charm | Greek::Color => None,
                        Greek::Delta => delta,
                        Greek::Gamma | Greek::Speed | Greek::Vanna | Greek::Zomma => {
                            delta.map(|_| 0.0)
                        }
                        _ => Some(0.0),
                    };
                    assert_eq!(
                        result.get(greek),
                        expected,
                        "{direction:?}, {spot}, {greek:?}"
                    );
                    assert_eq!(
                        result.status(greek),
                        if expected.is_some() {
                            GreekStatus::Available
                        } else {
                            GreekStatus::Undefined
                        }
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn automatic_spot_bumps_fit_small_price_units() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let engines: [&dyn PricingEngine; 5] = [
        &AnalyticEuropeanEngine,
        &CoxRossRubinsteinVanillaEngine::default(),
        &FiniteDifferenceVanillaEngine::default(),
        &QuadratureVanillaEngine::default(),
        &MonteCarloVanillaEngine::default(),
    ];
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for spot in [0.005, 0.5, 50.0] {
        let context = context(spot)?;
        let option = EuropeanOption::new(
            OptionType::Call,
            0.2 * spot,
            start,
            context.valuation_time(),
        )?;
        for engine in engines {
            let result = engine.price_with_greeks(&dated(&option, &context), &context, request)?;
            assert!((result.require(Greek::Delta)? - 1.0).abs() < 1e-10);
            assert!((result.require(Greek::Gamma)? * spot).abs() < 1e-8);
        }
        let explicit = numerical_greeks(
            &AnalyticEuropeanEngine,
            &dated(&option, &context),
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(2.0 * spot),
                ..Default::default()
            },
        )?;
        assert_eq!(
            explicit.status(Greek::Delta),
            GreekStatus::StencilUnavailable
        );
        let active = context.replace(PricingContextChanges {
            valuation_time: Some(start),
            ..Default::default()
        })?;
        let atm = option.replace(EuropeanOptionChanges {
            strike: Some(spot),
            ..Default::default()
        })?;
        let exact =
            AnalyticEuropeanEngine.price_with_greeks(&dated(&atm, &active), &active, request)?;
        let numerical = numerical_greeks(
            &AnalyticEuropeanEngine,
            &dated(&atm, &active),
            &active,
            request,
            Default::default(),
        )?;
        assert!((numerical.require(Greek::Delta)? - exact.require(Greek::Delta)?).abs() < 1e-4);
        assert!(
            ((numerical.require(Greek::Gamma)? - exact.require(Greek::Gamma)?) * spot).abs() < 1e-3
        );
    }
    Ok(())
}

#[test]
fn native_terminal_greeks_honor_spot_bumps_through_trait_and_direct_calls() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let context = context(100.005)?;
    let end = context.valuation_time();
    let request = GreeksRequest::new([Greek::Delta]);
    let sampling = MonteCarloSettings {
        path_count: 32,
        seed: Some(42),
        thread_count: 1,
        ..Default::default()
    };
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.touch_state = Some(BarrierTouchState::Untouched);
    let barrier = BarrierOption::new(terms)?;
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None)?;
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, end, end, None)?;
    macro_rules! check {
        ($engine:expr, $option:expr, $delta:expr) => {{
            let engine = $engine;
            for (bump, expected) in [(0.01, None), (0.001, Some($delta))] {
                let shifts = NumericalShiftSettings {
                    spot_shift: Some(bump),
                    ..Default::default()
                };
                let direct = kiyosi::numerical_greeks(
                    &engine,
                    &dated($option, &context),
                    &context,
                    request,
                    shifts,
                )?;
                let generic = kiyosi::numerical_greeks(
                    &engine,
                    &dated($option, &context),
                    &context,
                    request,
                    shifts,
                )?;
                assert_eq!(direct, generic);
                match expected {
                    Some(delta) => assert!((direct.require(Greek::Delta)? - delta).abs() < 1e-10),
                    None => {
                        assert_eq!(direct.status(Greek::Delta), GreekStatus::StencilUnavailable)
                    }
                }
            }
            assert!(matches!(
                kiyosi::numerical_greeks(
                    &engine,
                    &dated($option, &context),
                    &context,
                    request,
                    NumericalShiftSettings {
                        spot_shift: Some(0.0),
                        ..Default::default()
                    }
                ),
                Err(Error::InvalidInput(_))
            ));
        }};
    }
    check!(AnalyticEuropeanEngine, &vanilla, 1.0);
    check!(CoxRossRubinsteinDigitalEngine::default(), &digital, 0.0);
    check!(CoxRossRubinsteinBarrierEngine::default(), &barrier, 1.0);
    check!(MonteCarloDigitalEngine::new(sampling)?, &digital, 0.0);
    check!(MonteCarloAsianEngine::new(sampling)?, &geometric, 1.0);
    check!(MonteCarloAsianEngine::new(sampling)?, &arithmetic, 1.0);
    Ok(())
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
                        &dated(&option, &context),
                        &context,
                        GreeksRequest::all(),
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
                        &dated(&option, &context),
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
        AnalyticEuropeanEngine.price_with_greeks(&dated(&option, &context), &context, request)?;
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
        &dated(&option, &away),
        &away,
        request,
        NumericalShiftSettings {
            spot_shift: Some(30.0),
            ..Default::default()
        },
    )?;
    assert_eq!(
        crossed.status(Greek::Delta),
        GreekStatus::StencilUnavailable
    );
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
        Some((end).time_of_day()),
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.observation_clock_time = None;
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
            &dated(&note, &context(spot)?),
            &context(spot)?,
            GreeksRequest::all(),
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
