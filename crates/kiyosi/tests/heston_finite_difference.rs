use kiyosi::PricingEngine;
use kiyosi::engines::VanillaBatchEngine;

use kiyosi::Timestamp;
use kiyosi::{
    AmericanOption, AnalyticHestonEuropeanEngine, Date, Error, EuropeanOption,
    FiniteDifferenceHestonVanillaEngine, Greek, GreekStatus, GreeksRequest, HestonParameters,
    OptionType, PricingContext, RateTermStructure, TradingCalendar,
    engines::HestonFiniteDifferenceSettings,
};

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}

#[test]
fn american_heston_surface_caps_combined_expiry_rollbacks() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 96,
        variance_step_count: 64,
        time_step_count: 1600,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    let expiries = (0..120)
        .map(|days| end.add_days(-days))
        .collect::<kiyosi::Result<Vec<_>>>()?;
    assert!(matches!(
        engine.price_surface(&option, &context(0.3, -0.7), &expiries, &[100.0]),
        Err(Error::InvalidInput(message)) if message.contains("one billion grid updates")
    ));
    Ok(())
}

#[test]
fn european_surface_matches_analytic_prices_and_preserves_both_input_orders() -> kiyosi::Result<()>
{
    let (start, end) = dates();
    let middle = start.add_days(90)?;
    let market = context(0.3, -0.7);
    let template = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let strikes = [110.0, 90.0, 100.0, 90.0];
    let expiries = [end, start, middle, end];
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let rows = engine.price_surface(&template, &market, &expiries, &strikes)?;
    assert_eq!(rows.len(), expiries.len());
    assert_eq!(rows[0], rows[3]);
    assert_eq!(rows[1], [0.0, 10.0, 0.0, 10.0]);
    let analytic = AnalyticHestonEuropeanEngine::default();
    for (&expiry, row) in expiries.iter().zip(rows) {
        assert_eq!(row.len(), strikes.len());
        assert_eq!(row[1].to_bits(), row[3].to_bits());
        for (value, strike) in row.into_iter().zip(strikes) {
            let contract = EuropeanOption::new(OptionType::Call, strike, start, expiry)?;
            let reference = analytic.price(&contract, &market)?;
            assert!((value - reference).abs() < 0.002, "{value} vs {reference}");
        }
    }
    assert_eq!(
        engine.price_surface(&template, &market, &[end], &[])?,
        vec![Vec::<f64>::new()]
    );
    assert!(matches!(
        engine.price_surface(&template, &market, &[start.add_days(-1)?], &strikes),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn american_surface_matches_scalar_expiries_and_preserves_order() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let market = context(0.3, -0.7);
    let template = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let expiries = [end, start, start.add_days(30)?, start.add_days(90)?, end];
    let strikes = [110.0, 90.0, 100.0, 90.0];
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let rows = engine.price_surface(&template, &market, &expiries, &strikes)?;
    assert_eq!(rows.len(), expiries.len());
    assert_eq!(rows[0], rows[4]);
    assert_eq!(rows[1], [10.0, 0.0, 0.0, 0.0]);
    for (&expiry, row) in expiries.iter().zip(rows) {
        assert_eq!(row.len(), strikes.len());
        assert_eq!(row[1].to_bits(), row[3].to_bits());
        let option = template.replace(kiyosi::VanillaOptionChanges {
            expiry_time: Some(expiry),
            ..Default::default()
        })?;
        let strip = engine.price_strikes(&option, &market, &strikes)?;
        assert_eq!(strip.len(), strikes.len(), "independent strike quote count");
        for (value, reference) in row.into_iter().zip(strip) {
            assert!(
                (value - reference).abs() < 0.003,
                "{expiry:?}: {value} vs {reference}"
            );
        }
    }
    Ok(())
}

#[test]
fn automatic_american_surface_uses_an_independent_mesh_for_short_tenors() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let short = start.add_micros(1_000_000)?;
    let mid = start.add_days(90)?;
    let market = context(0.3, -0.7);
    let model = market
        .model_parameters()
        .as_heston()?
        .replace(kiyosi::HestonParameterChanges {
            dividend_yield: Some(0.0.into()),
            ..Default::default()
        })?;
    let market = PricingContext::new(model, 100.0, start)?;
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let call = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
    let european = EuropeanOption::new(OptionType::Call, 100.0, start, short)?;
    let reference = AnalyticHestonEuropeanEngine::default().price(&european, &market)?;
    for expiries in [&[short, end][..], &[short, mid, end][..]] {
        let rows = engine.price_surface(&call, &market, expiries, &[100.0])?;
        assert!(
            (rows[0][0] - reference).abs() < 1e-6,
            "{} vs {reference}",
            rows[0][0]
        );
    }
    Ok(())
}

#[test]
fn heston_finite_difference_with_recurring_calendar_rates() -> kiyosi::Result<()> {
    let (start, _) = dates();
    let end = start.add_days(364)?;
    let curve = RateTermStructure::bounded_flat_with_calendar(
        0.05,
        start,
        end,
        TradingCalendar::weekdays(),
    )?;
    let model = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: curve.into(),
        dividend_yield: 0.02.into(),
        initial_variance: 0.04,
        long_run_variance: 0.04,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.3,
        correlation: -0.7,
    })?;
    let market = PricingContext::new(model, 100.0, start)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 256,
        variance_step_count: 96,
        time_step_count: 52,
        ..Default::default()
    })?;
    let value = engine.price(&option, &market)?;
    let expected = AnalyticHestonEuropeanEngine::default().price(&option, &market)?;
    assert!((value - expected).abs() < 0.001, "{value} vs {expected}");
    Ok(())
}

#[test]
fn heston_strike_batches_match_independent_prices_in_input_order() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let market = context(0.3, -0.7);
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let strikes = [110.0, 90.0, 100.0, 90.0];
    let calls = engine.price_strikes(&call, &market, &strikes)?;
    let puts = engine.price_strikes(&put, &market, &strikes)?;
    assert_eq!(calls.len(), strikes.len(), "call quote count");
    assert_eq!(puts.len(), strikes.len(), "put quote count");
    for ((strike, actual_call), actual_put) in strikes.into_iter().zip(calls).zip(puts) {
        let call = EuropeanOption::new(OptionType::Call, strike, start, end)?;
        let put = AmericanOption::new(OptionType::Put, strike, start, end)?;
        let reference = AnalyticHestonEuropeanEngine::default().price(&call, &market)?;
        assert!(
            (actual_call - reference).abs() < 2e-3,
            "{strike}: {actual_call} != {reference}"
        );
        let reference = engine.price(&put, &market)?;
        assert!(
            (actual_put - reference).abs() < 2e-4,
            "{strike}: {actual_put} != {reference}"
        );
    }
    assert_eq!(
        engine.price_strikes(&call, &market, &[])?,
        Vec::<f64>::new()
    );
    assert!(matches!(
        engine.price_strikes(&call, &market, &[90.0, 0.0]),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn absorbing_heston_preserves_quoted_forward_payoffs() -> kiyosi::Result<()> {
    use kiyosi::{HestonParameterInputs, PricingContextChanges, engines::FiniteDifferenceEngine};

    let (start, end) = dates();
    let strike = 1e17;
    let spot = 1.2e17;
    let model = HestonParameters::new(HestonParameterInputs {
        risk_free_rate: RateTermStructure::new(start, vec![end], vec![0.75])?.into(),
        dividend_yield: RateTermStructure::new(start, vec![end], vec![0.625])?.into(),
        initial_variance: 0.0,
        long_run_variance: 0.0,
        mean_reversion_speed: 1.0,
        vol_of_vol: 0.3,
        correlation: 0.0,
    })?;
    let context = PricingContext::new(model, spot, start)?;
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let spots = [spot - 16.0, spot, spot + 16.0];
    for (direction, expected) in [
        (OptionType::Call, [0.0, 0.0, 10.0]),
        (OptionType::Put, [10.0, 0.0, 0.0]),
    ] {
        let option = EuropeanOption::new(direction, strike, start, end)?;
        let saved = engine.solve(&option, &context, spots[0]..=spots[2])?;
        for (spot, expected) in spots.into_iter().zip(expected) {
            let shifted = context.replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })?;
            for actual in [engine.price(&option, &shifted)?, saved.price(spot)?] {
                assert!((actual - expected).abs() < 1e-12, "{spot}: {actual}");
            }
        }
    }
    Ok(())
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
fn heston_grid_resolves_low_integrated_variance() -> kiyosi::Result<()> {
    use kiyosi::engines::FiniteDifferenceEngine;

    let start = Date::new(2025, 1, 6)?.start_of_day();
    let model = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.04.into(),
        dividend_yield: 0.01.into(),
        initial_variance: 0.005_f64.powi(2),
        long_run_variance: 0.005_f64.powi(2),
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.0,
        correlation: -0.7,
    })?;
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    for (days, spot, tolerance) in [(1, 100.0, 1.5e-5), (365, 96.0, 3.5e-4)] {
        let contract = EuropeanOption::new(OptionType::Call, 100.0, start, start.add_days(days)?)?;
        let market = PricingContext::new(model.clone(), spot, start)?;
        let expected = AnalyticHestonEuropeanEngine::default().price(&contract, &market)?;
        let saved = engine.solve(&contract, &market, spot - 0.01..=spot + 0.01)?;
        for actual in [engine.price(&contract, &market)?, saved.price(spot)?] {
            assert!(
                (actual - expected).abs() < tolerance,
                "{days}d: {actual} != {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn rejects_oversized_heston_workspace_before_pricing() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 4_000,
        variance_step_count: 4_000,
        time_step_count: 1,
        ..Default::default()
    })
    .unwrap();
    let contract = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let market = context(0.3, -0.7);
    assert!(matches!(
        engine.price(&contract, &market),
        Err(Error::InvalidInput(_))
    ));
    let expired = market
        .replace(kiyosi::PricingContextChanges {
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    assert!(engine.price(&contract, &expired).unwrap().abs() < 1e-14);
}

#[test]
fn single_row_heston_workspace_includes_asset_axis_buffers() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 7_000_000,
        time_step_count: 1,
        ..Default::default()
    })?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let model = context(0.0, 0.0).model_parameters().as_heston()?.replace(
        kiyosi::HestonParameterChanges {
            initial_variance: Some(1e6),
            ..Default::default()
        },
    )?;
    // The automatic domain overflows before allocation if the workspace check regresses.
    let market = PricingContext::new(model, 100.0, start)?;
    assert!(matches!(
        engine.price(&option, &market),
        Err(Error::InvalidInput(message)) if message.contains("workspace")
    ));
    Ok(())
}

#[test]
fn heston_workspace_includes_off_grid_curve_knots() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(1)?, end], vec![0.999, 0.95])?;
    let model = context(0.3, -0.7).model_parameters().as_heston()?.replace(
        kiyosi::HestonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        },
    )?;
    let market = PricingContext::new(model, 100.0, start)?;
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 3,
        variance_step_count: 2_003_246,
        time_step_count: 44,
        // Stop before the large allocation if the workspace check regresses.
        variance_upper_boundary: Some(0.01),
        ..Default::default()
    })?;
    let result = engine.price(&option, &market);
    assert!(
        matches!(&result, Err(Error::InvalidInput(message)) if message.contains("workspace")),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn heston_grid_rejects_oversized_rollback_before_pricing() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 1000,
        variance_step_count: 200,
        time_step_count: 100_000,
        ..Default::default()
    })
    .unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    assert!(matches!(
        engine.price(&option, &context(0.3, -0.7)),
        Err(Error::InvalidInput(message)) if message.contains("rollback")
    ));
}

#[test]
fn vanilla_defaults_match_independent_references_and_exercise_bounds() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    // QuantLib Gatheral/Lobatto European references and an HV American put reference.
    for (xi, european_reference, american_put_reference) in [
        (0.3, 9.011_278_407_541_74, Some(6.450_555_006_874_128)),
        (0.9, 7.783_760_588_260_141, None),
    ] {
        let ctx = context(xi, -0.7);
        let call = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let put = EuropeanOption::new(OptionType::Put, 100.0, start, end).unwrap();
        let call_price = engine.price(&call, &ctx).unwrap();
        let put_price = engine.price(&put, &ctx).unwrap();
        assert!((call_price - european_reference).abs() <= 0.001 + 1e-4 * european_reference);
        let parity = 100.0 * ((-0.02_f64).exp() - (-0.05_f64).exp());
        assert!((call_price - put_price - parity).abs() < 2e-4);
        if let Some(reference) = american_put_reference {
            let call = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
            let put = AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap();
            let american_call = engine.price(&call, &ctx).unwrap();
            let american_put = engine.price(&put, &ctx).unwrap();
            assert!(american_call >= call_price - 1e-8);
            assert!(american_put >= put_price - 1e-8);
            assert!((american_put - reference).abs() < 0.001 + 1e-4 * reference);
        }
    }
}

#[test]
fn american_call_is_worth_intrinsic_value_in_the_exercise_region() {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
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
fn coarse_wide_asset_grid_keeps_heston_price_within_payoff_bounds() {
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
    let actual = engine.price(&option, &context(0.3, -0.7)).unwrap();
    let discounted_spot = 100.0 * (-0.02_f64).exp();
    assert!(actual > 0.0 && actual <= discounted_spot, "price={actual}");
}

#[test]
fn numerical_failure_is_distinct_from_invalid_model_inputs() {
    let (start, end) = dates();
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
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 100,
        variance_step_count: 25,
        time_step_count: 50,
        ..Default::default()
    })
    .unwrap();
    let result = engine
        .price_with_greeks(&option, &context(0.3, -0.7), GreeksRequest::new([]))
        .unwrap();
    assert_eq!(result.status(Greek::Delta), GreekStatus::NotRequested);
    assert!(result.values().iter().all(Option::is_none));
}

#[test]
fn american_heston_dense_curves_preserve_price_and_bound_boundary_work() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 40,
        variance_step_count: 3,
        time_step_count: 1,
        ..Default::default()
    })?;
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let market = |count: i32, perturbation: f64, dividend: f64| -> kiyosi::Result<PricingContext> {
        let times = (1..=count)
            .map(|j| start.add_micros(31_536_000_000_000 * i64::from(j) / i64::from(count)))
            .collect::<kiyosi::Result<_>>()?;
        let factors = (1..=count)
            .map(|j| {
                (-0.05 * f64::from(j) / f64::from(count) - perturbation * f64::from(j % 2)).exp()
            })
            .collect();
        let curve = RateTermStructure::new(start, times, factors)?;
        PricingContext::new(
            HestonParameters::new(kiyosi::HestonParameterInputs {
                risk_free_rate: curve.into(),
                dividend_yield: dividend.into(),
                initial_variance: 0.04,
                long_run_variance: 0.04,
                mean_reversion_speed: 1.5,
                vol_of_vol: 0.3,
                correlation: -0.7,
            })?,
            100.0,
            start,
        )
    };
    let flat = engine.price(&option, &market(2000, 0.0, 0.02)?)?;
    let perturbed = engine.price(&option, &market(2000, 0.000_005, 0.02)?)?;
    assert!(
        flat > 0.0 && (perturbed - flat).abs() < 0.002,
        "{perturbed} != {flat}"
    );
    let dense = engine.price(&option, &market(6000, 0.000_005, 0.02)?)?;
    assert!((dense - flat).abs() < 0.002, "{dense} != {flat}");
    let indexed = engine.price(&option, &market(6000, 0.000_02, 0.02)?)?;
    assert!((indexed - flat).abs() < 0.01, "{indexed} != {flat}");
    assert!(matches!(
        engine.price(&option, &market(6000, 0.000_02, 0.2)?),
        Err(Error::InvalidInput(message)) if message.contains("exercise boundary")
    ));
    Ok(())
}

#[test]
fn fixed_heston_strike_batches_preserve_boundary_and_payoff_order() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let strikes = [110.0, 90.0, 100.0, 90.0];
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    for market in [context(0.3, -0.7), context(0.0, 0.0)] {
        let calls = engine.price_strikes(&call, &market, &strikes)?;
        let puts = engine.price_strikes(&put, &market, &strikes)?;
        assert_eq!(calls.len(), strikes.len(), "call quote count");
        assert_eq!(puts.len(), strikes.len(), "put quote count");
        assert_eq!(calls[1], calls[3]);
        assert_eq!(puts[1], puts[3]);
        for ((strike, c), p) in strikes.into_iter().zip(calls).zip(puts) {
            let call = EuropeanOption::new(OptionType::Call, strike, start, end)?;
            let put = AmericanOption::new(OptionType::Put, strike, start, end)?;
            let scalar_call = engine.price(&call, &market)?;
            let scalar_put = engine.price(&put, &market)?;
            assert!(
                (c - scalar_call).abs() < 0.002,
                "{strike}: {c} vs {scalar_call}"
            );
            assert!(
                (p - scalar_put).abs() < 0.002,
                "{strike}: {p} vs {scalar_put}"
            );
        }
        assert!(matches!(
            engine.price_strikes(&call, &market, &[90.0, 400.0]),
            Err(Error::InvalidInput(_))
        ));
    }
    Ok(())
}

#[test]
fn dense_fixed_heston_batch_resolves_the_full_strike_range() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let market = context(0.3, -0.7);
    let strikes: Vec<_> = (0..64).map(|i| 85.0 + 30.0 * f64::from(i) / 63.0).collect();
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 256,
        variance_step_count: 64,
        time_step_count: 128,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end)?;
        let expected =
            AnalyticHestonEuropeanEngine::default().price_strikes(&option, &market, &strikes)?;
        let actual = engine.price_strikes(&option, &market, &strikes)?;
        assert_eq!(actual.len(), strikes.len());
        assert_eq!(expected.len(), strikes.len(), "analytic quote count");
        for ((strike, actual), expected) in strikes.iter().zip(actual).zip(expected) {
            assert!(
                (actual - expected).abs() < 0.002,
                "{direction:?} strike {strike}: {actual} vs {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn dense_european_surface_fits_the_indexed_payoff_work_budget() -> kiyosi::Result<()> {
    let (start, _) = dates();
    let end = start.add_days(1600)?;
    let expiries = (90..=1600)
        .map(|day| start.add_days(day))
        .collect::<kiyosi::Result<Vec<_>>>()?;
    let strikes = (0..700)
        .map(|i| 95.0 + f64::from(i) / 70.0)
        .collect::<Vec<_>>();
    let template = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let market = context(0.3, -0.7);
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: 64,
        variance_step_count: 16,
        time_step_count: 1600,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    let surface = engine.price_surface(&template, &market, &expiries, &strikes)?;
    assert_eq!(surface.len(), expiries.len());
    for (&expiry, row) in expiries.iter().zip(&surface) {
        assert_eq!(row.len(), strikes.len(), "strike quote count at {expiry:?}");
    }
    let selected = engine.price_surface(&template, &market, &[expiries[0], end], &strikes)?;
    assert_eq!(selected.len(), 2, "selected expiry row count");
    for (index, expected) in [0, expiries.len() - 1].into_iter().zip(selected) {
        assert_eq!(
            expected.len(),
            strikes.len(),
            "selected expiry strike count"
        );
        for (&price, expected) in surface[index].iter().zip(expected) {
            assert!((price - expected).abs() < 1e-9);
        }
    }
    Ok(())
}

#[test]
fn dated_american_expiry_tiles_match_independent_strips() -> kiyosi::Result<()> {
    let (start, end) = dates();
    let mut terms = context(0.3, -0.7).model_parameters().as_heston()?.clone();
    let curve = RateTermStructure::new(start, vec![start.add_days(150)?, end], vec![0.98, 0.945])?;
    terms = terms.replace(kiyosi::HestonParameterChanges {
        risk_free_rate: Some(curve.into()),
        ..Default::default()
    })?;
    let market = PricingContext::new(terms, 100.0, start)?;
    let template = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let strikes = [110.0, 90.0, 100.0, 90.0];
    let expiries = [
        end,
        start.add_days(280)?,
        start.add_days(290)?,
        start.add_days(300)?,
        start.add_days(310)?,
        start.add_days(320)?,
        start.add_days(330)?,
        start.add_days(340)?,
        end,
    ];
    let rows = engine.price_surface(&template, &market, &expiries, &strikes)?;
    assert_eq!(rows.len(), expiries.len(), "expiry row count");
    assert_eq!(rows[0], rows[8]);
    for (&expiry, row) in expiries.iter().zip(rows) {
        assert_eq!(row.len(), strikes.len(), "strike quote count");
        assert_eq!(row[1].to_bits(), row[3].to_bits());
        let option = AmericanOption::new(OptionType::Put, 100.0, start, expiry)?;
        let strip = engine.price_strikes(&option, &market, &strikes)?;
        assert_eq!(strip.len(), strikes.len(), "independent strike quote count");
        for (value, reference) in row.into_iter().zip(strip) {
            assert!(
                (value - reference).abs() < 0.003,
                "{expiry:?}: {value} vs {reference}"
            );
        }
    }
    Ok(())
}
