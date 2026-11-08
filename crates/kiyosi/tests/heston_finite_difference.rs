use kiyosi::Timestamp;
use kiyosi::{
    AmericanOption, AnalyticHestonEuropeanEngine, Date, Error, EuropeanOption,
    FiniteDifferenceHestonVanillaEngine, GreeksRequest, HestonFiniteDifferenceSettings,
    HestonParameters, OptionType, PricingContext, RateTermStructure,
};

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}

fn context(xi: f64, correlation: f64) -> PricingContext {
    PricingContext::new(
        HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.5,
            vol_of_vol: xi,
            correlation,
        })
        .unwrap(),
        100.0,
        dates().0,
    )
    .unwrap()
}

#[test]
fn european_defaults_match_independent_references_and_parity() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    // QuantLib 1.43 Gatheral/Lobatto, stable between 1e-10 and 1e-12 tolerances.
    for (xi, reference) in [(0.3, 9.011_278_407_541_74), (0.9, 7.783_760_588_260_141)] {
        let ctx = context(xi, -0.7);
        let call = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let put = EuropeanOption::new(OptionType::Put, 100.0, start, end).unwrap();
        let actual = engine.price(&call, &ctx).unwrap();
        assert!((actual - reference).abs() <= 0.001 + 1e-4 * reference);
        let parity = 100.0 * ((-0.02_f64).exp() - (-0.05_f64).exp());
        assert!((actual - engine.price(&put, &ctx).unwrap() - parity).abs() < 2e-4);
    }
}

#[test]
fn american_prices_match_reference_and_exercise_bounds() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    for direction in [OptionType::Call, OptionType::Put] {
        let european = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let american = AmericanOption::new(direction, 100.0, start, end).unwrap();
        let euro = engine.price(&european, &context(0.3, -0.7)).unwrap();
        let price = engine.price(&american, &context(0.3, -0.7)).unwrap();
        assert!(price >= euro - 1e-8);
        assert!(price >= 0.0);
        if direction == OptionType::Put {
            // Shared QuantLib 1.43 HV reference and provenance in heston_finite_difference.json.
            let reference = 6.450_555_006_874_128;
            assert!((price - reference).abs() < 0.001 + 1e-4 * reference);
        }
    }
    let call = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let model = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.03.into(),
        dividend_yield: 0.1.into(),
        initial_variance: 0.04,
        long_run_variance: 0.04,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.3,
        correlation: -0.7,
    })
    .unwrap();
    let ctx = PricingContext::new(model, 200.0, start).unwrap();
    assert!((engine.price(&call, &ctx).unwrap() - 100.0).abs() < 1e-10);
}

#[test]
fn deterministic_variance_path_and_absorbing_zero_are_priced() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let model = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.05.into(),
        dividend_yield: 0.02.into(),
        initial_variance: 0.01,
        long_run_variance: 0.09,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.0,
        correlation: 1.0,
    })
    .unwrap();
    let ctx = PricingContext::new(model, 100.0, start).unwrap();
    let reference = AnalyticHestonEuropeanEngine::default()
        .price(&call, &ctx)
        .unwrap();
    assert!((engine.price(&call, &ctx).unwrap() - reference).abs() < 0.002);
    let zero = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.05.into(),
        dividend_yield: 0.2.into(),
        initial_variance: 0.0,
        long_run_variance: 0.0,
        mean_reversion_speed: 0.0,
        vol_of_vol: 0.9,
        correlation: -1.0,
    })
    .unwrap();
    let ctx = PricingContext::new(zero, 30.0, start).unwrap();
    let american = AmericanOption::new(
        OptionType::Put,
        100.0,
        start,
        Date::new(2028, 1, 1).unwrap().start_of_day(),
    )
    .unwrap();
    let optimum = (5.0_f64 / 6.0).ln() / -0.15;
    let expected = 100.0 * (-0.05 * optimum).exp() - 30.0 * (-0.2 * optimum).exp();
    assert!((engine.price(&american, &ctx).unwrap() - expected).abs() < 1e-12);
    assert!(expected > 70.0);
    for (side, risk, dividend) in [
        (OptionType::Call, 1e-18, 0.0),
        (OptionType::Put, 0.0, 1e-18),
    ] {
        let model = HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: risk.into(),
            dividend_yield: dividend.into(),
            initial_variance: 0.0,
            long_run_variance: 0.0,
            mean_reversion_speed: 1.0,
            vol_of_vol: 0.3,
            correlation: 0.0,
        })
        .unwrap();
        let ctx = PricingContext::new(model, 1e18, start).unwrap();
        let option = EuropeanOption::new(side, 1e18, start, end).unwrap();
        assert!((engine.price(&option, &ctx).unwrap() - 1.0).abs() < 1e-12);
    }
    for xi in [1e-8, 1e-200] {
        let price = engine.price(&call, &context(xi, -0.7)).unwrap();
        assert!((price - 9.227_005_508_154_036).abs() < 0.002);
    }
}

#[test]
fn zero_variance_can_restart_and_correlation_endpoints_are_supported() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    for (v0, theta, kappa, rho) in [
        (0.04, 0.04, 1.5, -1.0),
        (0.04, 0.04, 1.5, 1.0),
        (0.0, 0.04, 1.5, -0.7),
        (0.04, 0.04, 0.0, -0.7),
        (0.04, 0.0, 1.5, -0.7),
    ] {
        let model = HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: v0,
            long_run_variance: theta,
            mean_reversion_speed: kappa,
            vol_of_vol: 0.3,
            correlation: rho,
        })
        .unwrap();
        let ctx = PricingContext::new(model, 100.0, start).unwrap();
        let reference = AnalyticHestonEuropeanEngine::default()
            .price(&option, &ctx)
            .unwrap();
        let actual = FiniteDifferenceHestonVanillaEngine::default()
            .price(&option, &ctx)
            .unwrap();
        assert!(
            (actual - reference).abs() <= 0.001 + 1e-4 * reference,
            "v0={v0}, theta={theta}, kappa={kappa}, rho={rho}: {actual} != {reference}"
        );
    }
}

#[test]
fn node_curves_keep_optimal_exercise_under_negative_rates() {
    let (start, end) = dates();
    let middle = Date::new(2025, 7, 1).unwrap().start_of_day();
    let risk = RateTermStructure::new(start, vec![middle, end], vec![0.1_f64.exp(), 1.0]).unwrap();
    for xi in [0.0, 0.3] {
        let model = HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: risk.clone().into(),
            dividend_yield: 0.0.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.5,
            vol_of_vol: xi,
            correlation: -0.7,
        })
        .unwrap();
        let ctx = PricingContext::new(model, 0.01, start).unwrap();
        let put = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
        let expected = 100.0 * risk.factor(start, middle).unwrap() - 0.01;
        let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
            time_step_count: 31,
            ..Default::default()
        })
        .unwrap();
        assert!((engine.price(&put, &ctx).unwrap() - expected).abs() < 0.001);
    }
}

#[test]
fn expiry_and_validation_order_preserve_input_state() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let saved = engine.settings();
    let ctx = context(0.3, -0.7)
        .replace(kiyosi::PricingContextChanges {
            spot_price: Some(110.0),
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    assert!((engine.price(&option, &ctx).unwrap() - 10.0).abs() < 1e-12);
    let bsm = PricingContext::new(
        kiyosi::BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        110.0,
        end,
    )
    .unwrap();
    assert!(matches!(
        engine.price(&option, &bsm),
        Err(Error::UnsupportedOperation(_))
    ));
    let after = ctx
        .replace(kiyosi::PricingContextChanges {
            valuation_time: Some(Date::new(2026, 1, 2).unwrap().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &after),
        Err(Error::InvalidInput(_))
    ));
    let bad = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        variance_upper_boundary: Some(0.04),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        bad.price(&option, &context(0.3, -0.7)),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(engine.settings(), saved);
    assert!((ctx.spot_price() - 110.0).abs() < 1e-12);
}

#[test]
fn numerical_failure_is_distinct_from_invalid_model_inputs() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 3,
        variance_step_count: 3,
        time_step_count: 1,
        asset_upper_boundary: Some(1e308),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        engine.price(&option, &context(0.3, -0.7)),
        Err(Error::ComputationFailure(_))
    ));
    let model = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.0.into(),
        dividend_yield: 0.0.into(),
        initial_variance: 2000.0,
        long_run_variance: 2000.0,
        mean_reversion_speed: 0.0,
        vol_of_vol: 0.0,
        correlation: 0.0,
    })
    .unwrap();
    let ctx = PricingContext::new(model, 1e200, start).unwrap();
    let put = EuropeanOption::new(OptionType::Put, 1e-200, start, end).unwrap();
    let reference = AnalyticHestonEuropeanEngine::default()
        .price(&put, &ctx)
        .unwrap();
    assert!(reference > 0.9e-200);
    assert!(matches!(
        FiniteDifferenceHestonVanillaEngine::default().price(&put, &ctx),
        Err(Error::ComputationFailure(_))
    ));
}

#[test]
fn refined_tail_price_retains_relative_accuracy() {
    let start = dates().0;
    let option = EuropeanOption::new(
        OptionType::Put,
        70.0,
        start,
        Date::new(2025, 1, 31).unwrap().start_of_day(),
    )
    .unwrap();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 1200,
        variance_step_count: 200,
        time_step_count: 100,
        ..Default::default()
    })
    .unwrap();
    // Independent QuantLib 1.43 Gatheral/Lobatto, as in the analytic Heston tail scenario.
    let reference = 3.105_513_031_411_244e-6;
    let price = engine.price(&option, &context(0.3, -0.7)).unwrap();
    assert!(
        (price / reference - 1.0).abs() < 0.01,
        "{price} != {reference}"
    );
}

#[test]
fn maturity_and_moneyness_scenarios_match_analytic_prices() {
    let (start, end) = dates();
    let ctx = context(0.3, -0.7);
    for (expiry, strike) in [
        (Date::new(2025, 1, 8).unwrap().start_of_day(), 100.0),
        (Date::new(2030, 1, 1).unwrap().start_of_day(), 120.0),
        (end, 50.0),
        (end, 150.0),
    ] {
        let option = EuropeanOption::new(OptionType::Call, strike, start, expiry).unwrap();
        let reference = AnalyticHestonEuropeanEngine::default()
            .price(&option, &ctx)
            .unwrap();
        let actual = FiniteDifferenceHestonVanillaEngine::default()
            .price(&option, &ctx)
            .unwrap();
        assert!(
            (actual - reference).abs() <= 1e-5 * strike + 1e-4 * reference,
            "expiry={expiry:?}, strike={strike}: {actual} != {reference}"
        );
    }
}

#[test]
fn settings_and_unsupported_sensitivities_have_structured_errors() {
    for settings in [
        HestonFiniteDifferenceSettings {
            asset_step_count: 2,
            ..Default::default()
        },
        HestonFiniteDifferenceSettings {
            variance_step_count: 2,
            ..Default::default()
        },
        HestonFiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        },
        HestonFiniteDifferenceSettings {
            variance_upper_boundary: Some(f64::NAN),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceHestonVanillaEngine::new(settings),
            Err(Error::InvalidInput(_))
        ));
    }
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Put, 100.0, start, end).unwrap();
    assert!(matches!(
        FiniteDifferenceHestonVanillaEngine::default().price_with_greeks(
            &option,
            &context(0.3, -0.7),
            GreeksRequest::new([])
        ),
        Err(Error::InvalidInput(_))
    ));
}
