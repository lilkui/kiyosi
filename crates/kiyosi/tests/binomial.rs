use kiyosi::engines::{BinomialSettings, BinomialVanilla, BinomialVanillaSettings};
use kiyosi::instruments::OptionContract;
use kiyosi::*;

#[test]
fn vanilla_batch_rejects_an_excessive_contract_count() -> Result<()> {
    let (start, end) = dates();
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = CoxRossRubinsteinVanillaEngine::default();
    let context = market(100.0, 0.05, 0.02, 0.2);
    let instruments = vec![BinomialVanilla::European(&call); 1_048_577];

    binding_support::validate_contract_batch_size(instruments.len() - 1)?;
    assert!(matches!(
        engine.price_many(&instruments, &context),
        Err(Error::InvalidInput(message)) if message.contains("contract batch exceeds")
    ));
    Ok(())
}

#[test]
fn american_binomial_batch_caps_combined_rollback_work() -> Result<()> {
    let (start, end) = dates();
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 16_000,
        ..Default::default()
    })?;
    let snapshot = market(100.0, 0.05, 0.02, 0.2);
    let instruments = [BinomialVanilla::American(&put); 8];
    assert!(matches!(
        engine.price_many(&instruments, &snapshot),
        Err(Error::InvalidInput(message)) if message.contains("one billion grid updates")
    ));
    let expired = snapshot.replace(PricingContextChanges {
        valuation_time: Some(end),
        ..Default::default()
    })?;
    assert_eq!(engine.price_many(&instruments, &expired)?, vec![0.0; 8]);
    Ok(())
}

#[test]
fn american_batch_matches_scalar_for_full_and_banded_trees() -> Result<()> {
    let (start, end) = dates();
    let snapshot = market(100.0, 0.05, 0.02, 0.2);
    let puts = [115.0, 90.0, 100.0, 115.0]
        .map(|strike| AmericanOption::new(OptionType::Put, strike, start, end).unwrap());
    let early = AmericanOption::new(OptionType::Call, 85.0, start, end)?;
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let short = puts
        .iter()
        .map(|put| {
            put.replace(VanillaOptionChanges {
                expiry_time: Some(start.add_days(180)?),
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut options = puts
        .iter()
        .zip(&short)
        .flat_map(|(long, short)| {
            [
                BinomialVanilla::American(long),
                BinomialVanilla::American(short),
            ]
        })
        .collect::<Vec<_>>();
    options.extend([
        BinomialVanilla::American(&early),
        BinomialVanilla::European(&call),
        BinomialVanilla::American(&puts[0]),
    ]);
    let full = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 1024,
        ..Default::default()
    })?;
    for engine in [
        full,
        CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
            rollback_tolerance: Some(1e-6),
            ..full.settings()
        })?,
    ] {
        let expected = options
            .iter()
            .map(|option| match option {
                BinomialVanilla::American(option) => engine.price(*option, &snapshot),
                BinomialVanilla::European(option) => engine.price(*option, &snapshot),
            })
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(engine.price_many(&options, &snapshot)?, expected);
    }
    Ok(())
}

#[test]
fn banded_american_call_matches_full_tree_and_original_spot_greeks() -> Result<()> {
    let (start, end) = dates();
    let call = AmericanOption::new(OptionType::Call, 92.0, start, end)?;
    let snapshot = market(100.0, 0.05, 0.08, 0.2);
    let full = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 4_800,
        ..Default::default()
    })?;
    let bounded = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        rollback_tolerance: Some(1e-6),
        ..full.settings()
    })?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Vega]);
    let expected = full.price_with_greeks(&call, &snapshot, request)?;
    let actual = bounded.price_with_greeks(&call, &snapshot, request)?;
    assert!((actual.price() - expected.price()).abs() < 1e-6);
    for greek in [Greek::Delta, Greek::Gamma, Greek::Vega] {
        assert!(
            (actual.require(greek)? - expected.require(greek)?).abs() < 1e-6,
            "{greek:?}"
        );
    }
    let fine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 20_000,
        ..Default::default()
    })?;
    assert!(matches!(
        fine.price(&call, &snapshot),
        Err(Error::InvalidInput(_))
    ));
    let settings = BinomialVanillaSettings {
        rollback_tolerance: Some(1e-6),
        ..fine.settings()
    };
    let fine = CoxRossRubinsteinVanillaEngine::new(settings)?;
    assert_eq!(fine.settings(), settings);
    let price = fine.price(&call, &snapshot)?;
    let dual = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    assert!((price - fine.price(&dual, &market(92.0, 0.08, 0.05, 0.2))?).abs() < 1e-7);
    Ok(())
}

#[test]
fn banded_american_put_preserves_price_within_the_truncation_budget() -> Result<()> {
    let (start, end) = dates();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    for (spot, rate, dividend) in [(100.0, 0.05, 0.02), (120.0, -0.03, 0.01), (75.0, 0.08, 0.0)] {
        let snapshot = market(spot, rate, dividend, 0.2);
        let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
            step_count: 4800,
            ..Default::default()
        })?;
        let exact = engine.price(&option, &snapshot)?;
        for tolerance in [0.1, 1e-6] {
            let bounded = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
                rollback_tolerance: Some(tolerance),
                ..engine.settings()
            })?
            .price(&option, &snapshot)?;
            assert!(bounded <= exact + 1e-9);
            assert!(exact - bounded <= tolerance + 1e-9);
        }
    }
    let snapshot = market(100.0, 0.05, 0.02, 0.2);
    let fine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 20_000,
        ..Default::default()
    })?;
    assert!(matches!(
        fine.price(&option, &snapshot),
        Err(Error::InvalidInput(_))
    ));
    let configured = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        rollback_tolerance: Some(1e-6),
        ..fine.settings()
    })?;
    let actual = configured.price(&option, &snapshot)?;
    let volatility = implied_volatility(
        &configured,
        &option,
        &snapshot,
        actual,
        ImpliedVolatilitySettings::new(0.15, 0.25),
    )?;
    assert!((volatility - 0.2).abs() < 1e-8);
    let reference = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 10_000,
        ..Default::default()
    })?
    .price(&option, &snapshot)?;
    assert!((actual - reference).abs() < 1e-4);
    assert!(matches!(
        CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
            rollback_tolerance: Some(0.0),
            ..fine.settings()
        }),
        Err(Error::InvalidInput(_))
    ));
    Ok(())
}

#[test]
fn tree_discounts_preserve_representable_prices_after_factor_rounding() -> Result<()> {
    let (start, end) = dates();
    let settings = BinomialSettings { step_count: 1 };
    let sigma = 0.2_f64;
    let probability = sigma.exp_m1() / (2.0 * sigma).exp_m1();
    for (rate, amount) in [(744.0, 1e300_f64), (800.0, 1e300), (-800.0, 1e-300)] {
        let context = market(amount, rate, rate, sigma);
        let digital = DigitalOption::cash_or_nothing(OptionType::Call, amount, amount, start, end)?;
        let vanilla = EuropeanOption::new(OptionType::Call, amount, start, end)?;
        let barrier = BarrierOption::new(BarrierOptionTerms::new(
            OptionType::Call,
            amount,
            start,
            end,
            2.0 * amount,
            BarrierType::UpAndOut,
        ))?;
        let expected_cash = (amount.ln() + probability.ln() - rate).exp();
        let expected_call = expected_cash * sigma.exp_m1();
        for (actual, expected) in [
            (
                CoxRossRubinsteinDigitalEngine::new(settings)?.price(&digital, &context)?,
                expected_cash,
            ),
            (
                CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
                    step_count: settings.step_count,
                    ..Default::default()
                })?
                .price(&vanilla, &context)?,
                expected_call,
            ),
            (
                CoxRossRubinsteinBarrierEngine::new(settings)?.price(&barrier, &context)?,
                expected_call,
            ),
        ] {
            assert!(
                (actual / expected - 1.0).abs() < 1e-12,
                "{rate}: {actual} != {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn implied_volatility_recovers_tree_quotes_with_zero_or_tiny_carry() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    for (rate, dividend) in [(0.0, 0.0), (0.05, 0.05), (1e-20, 0.0)] {
        let context = market(100.0, rate, dividend, 0.2);
        for steps in [1, 256] {
            let settings = BinomialSettings { step_count: steps };
            let vanilla = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
                step_count: settings.step_count,
                ..Default::default()
            })
            .unwrap();
            let cash = CoxRossRubinsteinDigitalEngine::new(settings).unwrap();
            for (quote, solved) in [
                (
                    vanilla.price(&option, &context).unwrap(),
                    implied_volatility(
                        &vanilla,
                        &option,
                        &context,
                        vanilla.price(&option, &context).unwrap(),
                        kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0),
                    ),
                ),
                (
                    cash.price(&digital, &context).unwrap(),
                    implied_volatility(
                        &cash,
                        &digital,
                        &context,
                        cash.price(&digital, &context).unwrap(),
                        kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0),
                    ),
                ),
            ] {
                let sigma = solved.unwrap();
                assert!((sigma - 0.2).abs() < 1e-6, "{quote}: {sigma}");
            }
        }
    }
}

#[test]
fn implied_volatility_keeps_the_only_admissible_endpoint() -> Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 1,
        ..Default::default()
    })?;
    for (rate, dividend) in [(0.125, 0.0), (0.0, 0.125)] {
        let context = market(100.0, rate, dividend, 0.125);
        let quote = engine.price(&option, &context)?;
        let bracket = ImpliedVolatilitySettings::new(0.0625, 0.125);
        assert!(
            (implied_volatility(&engine, &option, &context, quote, bracket)? - 0.125).abs() < 1e-16
        );
        assert!(matches!(
            implied_volatility(&engine, &option, &context, quote + 1.0, bracket),
            Err(Error::RootNotBracketed(_))
        ));
    }
    Ok(())
}

#[test]
fn implied_volatility_rejects_a_price_jump_without_a_matching_quote() {
    let (start, end) = dates();
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 110.0, 10.0, start, end).unwrap();
    let context = market(100.0, 0.0, 0.0, 0.2);
    let engine = CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: 1 }).unwrap();
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            1.0,
            ImpliedVolatilitySettings::new(0.01, 0.2)
        ),
        Err(Error::NonConvergence(_))
    ));
}

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
fn cash_digital_keeps_finite_price_when_discounted_payout_overflows() -> Result<()> {
    let payout = 1.75e308;
    let (start, end) = dates();
    let unit = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 1.0, start, end)?;
    let large = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, payout, start, end)?;
    let context = market(100.0, -0.05, -0.05, 0.2);
    for step_count in [15, 16_001] {
        let engine = CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count })?;
        let expected = engine.price(&unit, &context)? * payout;
        let price = engine.price(&large, &context)?;
        assert!(
            (price / expected - 1.0).abs() < 1e-12,
            "{step_count}: {price}"
        );
    }
    Ok(())
}

#[test]
fn one_step_gamma_falls_back_after_log_space_pricing() -> Result<()> {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100_000.0, start, end)?;
    let context = market(100_000.0, -700.0, -700.0, 0.2);
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 1,
        ..Default::default()
    })?;
    let shift = 10_000.0;
    let result = engine.price_with_greeks_with_settings(
        &option,
        &context,
        GreeksRequest::new([Greek::Gamma]),
        NumericalShiftSettings {
            spot_shift: Some(shift),
            ..Default::default()
        },
    )?;
    assert!((result.price() / 1.010_864_749_797_835_8e308 - 1.0).abs() < 1e-12);
    assert!((result.require(Greek::Gamma)? / result.price() * shift * shift).abs() < 1e-12);
    Ok(())
}

#[test]
fn american_rollback_budget_preserves_direct_tree_pricing() -> Result<()> {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let call = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 16_001,
        ..Default::default()
    })?;
    assert!(matches!(
        engine.price(&put, &context),
        Err(Error::InvalidInput(_))
    ));
    let boundary = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 16_000,
        ..Default::default()
    })?;
    assert!((boundary.price(&put, &context)? - 6.6606).abs() < 0.001);
    let expected = AnalyticEuropeanEngine.price(&call, &context)?;
    assert!((engine.price(&call, &context)? - expected).abs() < 0.001);
    Ok(())
}

#[test]
fn european_tree_refines_to_analytic_price_and_node_greeks() {
    let (start, end) = dates();
    let context = market(100.0, 0.05, 0.02, 0.2);
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let expected = AnalyticEuropeanEngine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        for steps in [255, 256, 4095, 4096] {
            let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
                step_count: steps,
                ..Default::default()
            })
            .unwrap();
            let actual = engine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                )
                .unwrap();
            assert!((actual.price() - expected.price()).abs() < 3.0 / f64::from(steps));
            assert!(
                (actual.require(Greek::Delta).unwrap() - expected.require(Greek::Delta).unwrap())
                    .abs()
                    < 0.3 / f64::from(steps)
            );
            assert!(
                (actual.require(Greek::Gamma).unwrap() - expected.require(Greek::Gamma).unwrap())
                    .abs()
                    < 0.04 / f64::from(steps)
            );
            assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
            assert!(actual.get(Greek::Vega).is_none());
        }
    }
}

#[test]
fn digital_tree_matches_one_step_cash_and_asset_payments() {
    let (start, end) = dates();
    let engine = CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: 1 }).unwrap();
    let context = market(100.0, 0.05, 0.02, 0.2);
    // One up branch pays 10 cash or 100*exp(0.2) assets, discounted at 5%.
    for (option, price, delta) in [
        (
            DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap(),
            5.001_536_925_673_387_5,
            0.248_341_078_440_725_8,
        ),
        (
            DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap(),
            61.088_909_960_574_12,
            3.033_244_781_719_736,
        ),
    ] {
        let result = engine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        assert!((result.price() - price).abs() < 1e-10);
        assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-10);
        for greek in Greek::ALL {
            assert_eq!(result.get(greek).is_some(), greek == Greek::Delta);
        }
    }
}

#[test]
fn american_early_exercise_and_no_dividend_limit() {
    let (start, end) = dates();
    for steps in [1, 2, 3, 17, 255, 256, 257] {
        let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
            step_count: steps,
            ..Default::default()
        })
        .unwrap();
        for (direction, strike, spot, rate, dividend, delta) in [
            (OptionType::Put, 100.0, 88.0, 0.15, 0.0, -1.0),
            (OptionType::Call, 88.0, 100.0, 0.0, 0.15, 1.0),
        ] {
            let option = AmericanOption::new(direction, strike, start, end).unwrap();
            let result = engine
                .price_with_greeks(
                    &option,
                    &market(spot, rate, dividend, 0.2),
                    GreeksRequest::all(),
                )
                .unwrap();
            assert_eq!(result.price(), 12.0);
            assert_eq!(result.get(Greek::Delta), Some(delta));
            assert_eq!(result.get(Greek::Gamma), Some(0.0));
        }
        let european = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let american = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let context = market(100.0, 0.05, 0.0, 0.2);
        assert!(
            (engine.price(&european, &context).unwrap()
                - engine.price(&american, &context).unwrap())
            .abs()
                < 1e-10
        );
        let result = engine
            .price_with_greeks(&american, &context, GreeksRequest::all())
            .unwrap();
        assert!(Greek::ALL.into_iter().all(|g| result.get(g).is_some()));
    }
}

#[test]
fn digital_odd_even_and_off_strike_grids_refine_with_negative_carry() {
    let (start, end) = dates();
    for (rate, dividend, sigma) in [
        (0.05, 0.02, 0.2),
        (-0.03, 0.01, 0.45),
        (0.01, 0.07, 0.3),
        (-0.02, -0.05, 0.15),
    ] {
        for spot in [95.0, 100.0, 105.0] {
            let context = market(spot, rate, dividend, sigma);
            for direction in [OptionType::Call, OptionType::Put] {
                for option in [
                    DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                    DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
                ] {
                    let expected = AnalyticDigitalEngine
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                        )
                        .unwrap();
                    for steps in [255, 256, 4095, 4096] {
                        let engine = CoxRossRubinsteinDigitalEngine::new(BinomialSettings {
                            step_count: steps,
                        })
                        .unwrap();
                        let actual = engine
                            .price_with_greeks(&option, &context, GreeksRequest::all())
                            .unwrap();
                        let budget = option.payout().unwrap_or(spot) / f64::from(steps).sqrt();
                        assert!((actual.price() - expected.price()).abs() < budget);
                        assert!(
                            (actual.require(Greek::Delta).unwrap()
                                - expected.require(Greek::Delta).unwrap())
                            .abs()
                                < budget / spot / sigma
                        );
                        assert!(
                            (actual.require(Greek::Gamma).unwrap()
                                - expected.require(Greek::Gamma).unwrap())
                            .abs()
                                < budget / spot / spot / sigma / sigma
                        );
                        assert_eq!(actual.price(), engine.price(&option, &context).unwrap());
                        assert!(actual.get(Greek::Vega).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn expiry_boundaries_settle_strictly_and_life_errors_precede_trees() {
    let (start, end) = dates();
    let vanilla = CoxRossRubinsteinVanillaEngine::default();
    let digital = CoxRossRubinsteinDigitalEngine::default();
    for direction in [OptionType::Call, OptionType::Put] {
        let european = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let american = AmericanOption::new(direction, 100.0, start, end).unwrap();
        for spot in [90.0, 100.0, 110.0] {
            let context = market(spot, 100.0, -100.0, 1e-200)
                .replace(PricingContextChanges {
                    valuation_time: Some(end),
                    ..Default::default()
                })
                .unwrap();
            let intrinsic = (if direction == OptionType::Call {
                spot - 100.0
            } else {
                100.0 - spot
            })
            .max(0.0);
            for result in [
                vanilla
                    .price_with_greeks(&european, &context, GreeksRequest::all())
                    .unwrap(),
                vanilla
                    .price_with_greeks(&american, &context, GreeksRequest::all())
                    .unwrap(),
            ] {
                assert_eq!(result.price(), intrinsic);
                assert_eq!(result.get(Greek::Theta), None);
                assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
                if (spot - 100.0).abs() <= 0.01 {
                    assert_eq!(result.get(Greek::Delta), None);
                } else {
                    let delta = if intrinsic == 0.0 {
                        0.0
                    } else if direction == OptionType::Call {
                        1.0
                    } else {
                        -1.0
                    };
                    assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-10);
                }
            }
            let cash_payout = 10.0;
            for (option, payout) in [
                (
                    DigitalOption::cash_or_nothing(direction, 100.0, cash_payout, start, end)
                        .unwrap(),
                    Some(cash_payout),
                ),
                (
                    DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
                    None,
                ),
            ] {
                let result = digital
                    .price_with_greeks(&option, &context, GreeksRequest::all())
                    .unwrap();
                assert_eq!(
                    result.price(),
                    if intrinsic > 0.0 {
                        payout.unwrap_or(spot)
                    } else {
                        0.0
                    }
                );
                assert_eq!(result.get(Greek::Theta), None);
                if (spot - 100.0).abs() <= 0.01 {
                    assert_eq!(result.get(Greek::Delta), None);
                } else {
                    let delta = f64::from(intrinsic > 0.0 && payout.is_none());
                    assert!((result.require(Greek::Delta).unwrap() - delta).abs() < 1e-10);
                }
                assert!(matches!(
                    implied_volatility(
                        &digital,
                        &option,
                        &context,
                        result.price(),
                        kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
                    )
                    .unwrap_err(),
                    Error::UnsupportedOperation(_)
                ));
            }
        }
        let spot = if direction == OptionType::Call {
            110.0
        } else {
            90.0
        };
        let before_start = market(spot, 0.0, 0.0, 0.0)
            .replace(PricingContextChanges {
                valuation_time: Some(start.add_days(-1).unwrap()),
                ..Default::default()
            })
            .unwrap();
        let after_expiry = before_start
            .replace(PricingContextChanges {
                valuation_time: Some(end.add_micros(1).unwrap()),
                ..Default::default()
            })
            .unwrap();
        let cash = DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap();
        let asset = DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap();
        for (product, actual, expected) in [
            (
                "vanilla",
                vanilla.price(&european, &before_start).unwrap(),
                10.0,
            ),
            (
                "cash digital",
                digital.price(&cash, &before_start).unwrap(),
                10.0,
            ),
            (
                "asset digital",
                digital.price(&asset, &before_start).unwrap(),
                spot,
            ),
        ] {
            assert!(
                (actual - expected).abs() < 1e-12,
                "{direction:?} {product} before effective time: {actual}, expected {expected}"
            );
        }
        for result in [
            vanilla.price(&american, &before_start),
            vanilla.price(&american, &after_expiry),
            vanilla.price(&european, &after_expiry),
            digital.price(&cash, &after_expiry),
            digital.price(&asset, &after_expiry),
        ] {
            assert!(matches!(result, Err(Error::InvalidInput(_))));
        }
    }
}

#[test]
fn settings_flat_curve_validation_and_unstable_trees_preserve_inputs() {
    assert_eq!(BinomialVanillaSettings::default().step_count, 256);
    assert_eq!(BinomialSettings::default().step_count, 256);
    for steps in [-1, 0, 1_000_001] {
        assert!(matches!(
            CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
                step_count: steps,
                ..Default::default()
            })
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: steps })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for steps in [1, 1_000_000] {
        assert_eq!(
            CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
                step_count: steps,
                ..Default::default()
            })
            .unwrap()
            .settings()
            .step_count,
            steps
        );
        assert_eq!(
            CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: steps })
                .unwrap()
                .settings()
                .step_count,
            steps
        );
    }
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let vanilla = CoxRossRubinsteinVanillaEngine::default();
    let digital = CoxRossRubinsteinDigitalEngine::default();
    let nodes = RateTermStructure::new(
        start,
        vec![start.add_days(180).unwrap(), end],
        vec![0.99, 0.95],
    )
    .unwrap();
    let nonflat = PricingContext::new(
        BlackScholesMertonParameters::new(nodes, 0.02, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    assert!(matches!(
        vanilla
            .price_with_greeks(&option, &nonflat, GreeksRequest::new([]))
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        digital
            .price_with_greeks(&cash, &nonflat, GreeksRequest::new([]))
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let flat = RateTermStructure::flat(0.05, Some(start)).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(flat, 0.02, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    assert_eq!(
        vanilla.price(&option, &context).unwrap(),
        vanilla
            .price(&option, &market(100.0, 0.05, 0.02, 0.2))
            .unwrap()
    );
    for settings in [
        NumericalShiftSettings {
            spot_shift: Some(0.0),
            ..Default::default()
        },
        NumericalShiftSettings {
            time_shift: Some(std::time::Duration::ZERO),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            kiyosi::numerical_greeks(&vanilla, &option, &context, GreeksRequest::all(), settings)
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for (sigma, rate, dividend) in [
        (1e-200, 0.05, 0.02),
        (50.0, 0.05, 0.02),
        (0.2, -1000.0, -1000.0),
    ] {
        let bad = market(100.0, rate, dividend, sigma);
        assert!(matches!(
            vanilla.price(&option, &bad).unwrap_err(),
            Error::ComputationFailure(_)
        ));
        assert!(matches!(
            digital.price(&cash, &bad).unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    let before = context.clone();
    let quote = vanilla.price(&option, &context).unwrap();
    assert!(matches!(
        implied_volatility(
            &vanilla,
            &option,
            &context,
            -1.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    assert_eq!(context, before);
    assert_eq!(vanilla.settings(), BinomialVanillaSettings::default());
    assert_eq!(vanilla.price(&option, &context).unwrap(), quote);
}

#[test]
fn implied_brackets_intersect_tree_domain_for_all_supported_contracts() {
    fn recover<I: OptionContract, E: PricingEngine<I>>(
        engine: &E,
        option: &I,
        context: &PricingContext,
    ) {
        let quote = engine.price(option, context).unwrap();
        let settings = ImpliedVolatilitySettings::new(0.0001, 0.4);
        assert!(
            (implied_volatility(engine, option, context, quote, settings).unwrap() - 0.2).abs()
                < 1e-6
        );
        assert!(matches!(
            implied_volatility(
                engine,
                option,
                context,
                quote,
                ImpliedVolatilitySettings::new(0.0, 0.01)
            )
            .unwrap_err(),
            Error::RootNotBracketed(_)
        ));
    }

    let (start, end) = dates();
    let vanilla = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 17,
        ..Default::default()
    })
    .unwrap();
    let digital = CoxRossRubinsteinDigitalEngine::new(BinomialSettings { step_count: 17 }).unwrap();
    let context = market(100.0, 0.15, 0.0, 0.2);

    recover(
        &vanilla,
        &EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap(),
        &context,
    );
    recover(
        &vanilla,
        &AmericanOption::new(OptionType::Put, 100.0, start, end).unwrap(),
        &context,
    );
    for direction in [OptionType::Call, OptionType::Put] {
        recover(
            &digital,
            &DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            &context,
        );
        recover(
            &digital,
            &DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
            &context,
        );
    }
}

#[test]
fn deterministic_endpoints_and_underflowed_tail_weights_remain_finite() {
    let (start, end) = dates();
    let digital = CoxRossRubinsteinDigitalEngine::default();
    for rate in [-3.2_f64, 3.2] {
        let context = market(100.0, rate, 0.0, 0.2);
        for direction in [OptionType::Call, OptionType::Put] {
            let pays = (direction == OptionType::Call) == (rate > 0.0);
            for option in [
                DigitalOption::cash_or_nothing(direction, 100.0, 10.0, start, end).unwrap(),
                DigitalOption::asset_or_nothing(direction, 100.0, start, end).unwrap(),
            ] {
                let expected = if pays {
                    option
                        .payout()
                        .map_or(100.0, |payout| payout * (-rate).exp())
                } else {
                    0.0
                };
                assert!((digital.price(&option, &context).unwrap() - expected).abs() < 1e-9);
            }
        }
    }
    let option = EuropeanOption::new(OptionType::Call, 1e280 * 8.0_f64.exp(), start, end).unwrap();
    let context = market(1e280, 0.04, 0.01, 0.2);
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 2048,
        ..Default::default()
    })
    .unwrap();
    // Tail probabilities underflow individually, while their payoff-weighted sum is positive.
    let price = engine.price(&option, &context).unwrap();
    // Independently rolled back in the public Python contract test.
    assert!((price / 2.249_054_371_067_649e-140 - 1.0).abs() < 1e-9);
    assert_eq!(
        price,
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([Greek::Delta, Greek::Gamma])
            )
            .unwrap()
            .price()
    );
}

#[test]
fn vanilla_numerical_completion_tracks_analytic_and_propagates_bump_failures() {
    let (start, end) = dates();
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 1000,
        ..Default::default()
    })
    .unwrap();
    let context = market(100.0, 0.04, 0.01, 0.2)
        .replace(PricingContextChanges {
            valuation_time: Some(Timestamp::new(start.date(), 12, 0, 0, 0).unwrap()),
            ..Default::default()
        })
        .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let actual = engine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        let expected = AnalyticEuropeanEngine
            .price_with_greeks(&option, &context, GreeksRequest::all())
            .unwrap();
        for greek in [
            Greek::Speed,
            Greek::Vanna,
            Greek::Zomma,
            Greek::Charm,
            Greek::Color,
        ] {
            let reference = expected.require(greek).unwrap();
            assert!(
                (actual.require(greek).unwrap() - reference).abs() < reference.abs() * 0.02 + 1e-7,
                "{greek:?}"
            );
        }
        let generic = PricingEngine::price_with_greeks_with_settings(
            &engine,
            &option,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(generic, actual);
    }
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 1,
        ..Default::default()
    })
    .unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let context = market(100.0, 0.15, 0.0, 0.2);
    let before = context.clone();
    let price = engine.price(&option, &context).unwrap();
    assert!(matches!(
        kiyosi::numerical_greeks(
            &engine,
            &option,
            &context,
            GreeksRequest::new([Greek::Vega]),
            NumericalShiftSettings {
                volatility_shift: Some(0.1),
                ..Default::default()
            }
        )
        .unwrap_err(),
        Error::ComputationFailure(_)
    ));
    assert_eq!(context, before);
    assert_eq!(engine.price(&option, &context).unwrap(), price);
}

#[test]
fn one_step_speed_respects_automatic_and_explicit_spot_bumps() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 0.015, start, end).unwrap();
    let context = market(0.015, 0.05, 0.02, 0.2);
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 1,
        ..Default::default()
    })
    .unwrap();
    let result = engine
        .price_with_greeks(&option, &context, GreeksRequest::new([Greek::Speed]))
        .unwrap();
    assert_eq!(result.price(), engine.price(&option, &context).unwrap());
    assert!(result.require(Greek::Speed).unwrap().abs() * context.spot_price().powi(2) < 1e-8);
    let explicit = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([Greek::Speed]),
        NumericalShiftSettings {
            spot_shift: Some(0.01),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        explicit.status(Greek::Speed),
        GreekStatus::StencilUnavailable
    );
}

#[test]
fn banded_put_greeks_match_the_full_tree() -> Result<()> {
    let (start, end) = dates();
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let snapshot = market(100.0, 0.05, 0.02, 0.2);
    let engine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 4800,
        ..Default::default()
    })?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Rho, Greek::Vega]);
    let exact = engine.price_with_greeks(&put, &snapshot, request)?;
    let bounded = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        rollback_tolerance: Some(1e-6),
        ..engine.settings()
    })?
    .price_with_greeks(&put, &snapshot, request)?;
    assert!((bounded.price() - exact.price()).abs() <= 1e-6);
    for greek in [Greek::Delta, Greek::Gamma, Greek::Rho, Greek::Vega] {
        assert!(
            (bounded.require(greek)? - exact.require(greek)?).abs() < 1e-6,
            "{greek:?}"
        );
    }
    let fine = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        step_count: 20_000,
        ..Default::default()
    })?;
    let result = CoxRossRubinsteinVanillaEngine::new(BinomialVanillaSettings {
        rollback_tolerance: Some(1e-6),
        ..fine.settings()
    })?
    .price_with_greeks(&put, &snapshot, request)?;
    assert!(result.require(Greek::Gamma)? > 0.0);
    Ok(())
}
