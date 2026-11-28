#[path = "support/observation_schedule.rs"]
mod schedule_fixture;
use schedule_fixture::schedule as fixture_schedule;

use kiyosi::engines::{FiniteDifferenceEngine, FiniteDifferenceScheme, FiniteDifferenceSettings};
use kiyosi::*;

fn terms(kind: DoubleBarrierType) -> DoubleTouchOptionTerms {
    DoubleTouchOptionTerms::new(
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
        80.0,
        120.0,
        kind,
    )
}

fn market(spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.03, 0.01, 0.2).unwrap(),
        spot,
        Date::new(2025, 1, 1).unwrap().start_of_day(),
    )
    .unwrap()
}

#[test]
fn short_double_barriers_resolve_a_near_boundary() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_micros(15 * 60 * 1_000_000)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.02)?,
        100.0,
        start,
    )?;
    let touch = DoubleTouchOption::cash(
        DoubleTouchOptionTerms::new(start, end, 99.98, 125.0, DoubleBarrierType::KnockIn),
        100.0,
    )?;
    let barrier = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        95.0,
        start,
        end,
        99.98,
        125.0,
        DoubleBarrierType::KnockIn,
    ))?;
    let touch_engine = FiniteDifferenceDoubleTouchEngine::default();
    let barrier_engine = FiniteDifferenceDoubleBarrierEngine::default();
    for (price, reference, tolerance) in [
        (
            touch_engine.price(&touch, &context)?,
            AnalyticDoubleTouchEngine.price(&touch, &context)?,
            0.01,
        ),
        (
            barrier_engine.price(&barrier, &context)?,
            AnalyticDoubleBarrierEngine.price(&barrier, &context)?,
            0.001,
        ),
    ] {
        assert!(
            (price - reference).abs() < tolerance,
            "double barrier {price} vs {reference}"
        );
    }
    Ok(())
}

#[test]
fn double_barrier_refinement_preserves_the_opposite_boundary() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let end = start.add_days(5 * 365)?;
    let model = BlackScholesMertonParameters::new(0.02, 0.083, 0.017)?;
    let context = PricingContext::new(model, 100.0, start)?;
    let touch = DoubleTouchOption::cash(
        DoubleTouchOptionTerms::new(start, end, 57.5, 100.016, DoubleBarrierType::KnockIn),
        100.0,
    )?;
    let reference = AnalyticDoubleTouchEngine.price(&touch, &context)?;
    let price = FiniteDifferenceDoubleTouchEngine::default().price(&touch, &context)?;
    assert!(
        (price - reference).abs() < 0.25,
        "double touch {price} vs {reference}"
    );
    Ok(())
}

#[test]
fn narrow_continuous_double_no_touch_preserves_nonnegative_prices() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(7)?;
    let engine = FiniteDifferenceDoubleTouchEngine::default();
    for (spot, lower, upper, rate, dividend, volatility) in [
        (100.0, 99.9, 100.1, 0.05, 0.02, 0.2),
        (120.0, 119.88, 126.0, 0.15, -0.1, 1.0),
    ] {
        let model = BlackScholesMertonParameters::new(rate, dividend, volatility)?;
        let context = PricingContext::new(model, spot, start)?;
        let touch = DoubleTouchOption::cash(
            DoubleTouchOptionTerms::new(start, end, lower, upper, DoubleBarrierType::KnockOut),
            1.0,
        )?;
        let saved = engine.solve(&touch, &context, (spot - 0.05)..=(spot + 0.05))?;
        for actual in [engine.price(&touch, &context)?, saved.price(spot)?] {
            assert!(
                (0.0..=1e-12).contains(&actual),
                "unexpected survival value {actual} at {spot}"
            );
        }
    }
    Ok(())
}

#[test]
fn analytic_touch_payments_require_only_the_settlement_curve() -> Result<()> {
    let terms = terms(DoubleBarrierType::KnockIn);
    let start = terms.effective_time;
    let end = terms.expiry_time;
    let single = TouchOptionTerms::new(start, end, 120.0, BarrierType::UpAndIn);
    let cash = TouchOption::cash(single.clone(), 10.0)?;
    let asset = TouchOption::asset(single)?;
    let double = DoubleTouchOption::cash(terms, 10.0)?;
    let short = RateTermStructure::bounded_flat(0.02, start, start.add_days(180)?)?;
    let cash_context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, short.clone(), 0.2)?,
        120.0,
        start,
    )?;
    let asset_context = PricingContext::new(
        BlackScholesMertonParameters::new(short, 0.01, 0.2)?,
        120.0,
        start,
    )?;
    for value in [
        AnalyticBinaryBarrierEngine.price(&cash, &cash_context)?,
        AnalyticDoubleTouchEngine.price(&double, &cash_context)?,
    ] {
        assert!((value - 10.0 * (-0.05_f64).exp()).abs() < 1e-13);
    }
    assert!(
        (AnalyticBinaryBarrierEngine.price(&asset, &asset_context)? - 120.0 * (-0.01_f64).exp())
            .abs()
            < 1e-13
    );
    for result in [
        AnalyticBinaryBarrierEngine.price(&cash, &asset_context),
        AnalyticDoubleTouchEngine.price(&double, &asset_context),
        AnalyticBinaryBarrierEngine.price(&asset, &cash_context),
    ] {
        assert!(matches!(result, Err(Error::InvalidInput(_))));
    }
    Ok(())
}

#[test]
fn double_touch_at_hit_returns_zero_when_both_payments_underflow() -> Result<()> {
    for (lower, upper, dividend) in [(99.99, 150.0, 0.0), (50.0, 100.01, 0.10)] {
        let mut terms = terms(DoubleBarrierType::KnockIn);
        terms.lower_barrier = lower;
        terms.upper_barrier = upper;
        terms.settlement_timing = SettlementTiming::AtHit;
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.05, dividend, 0.0001)?,
            100.0,
            terms.effective_time,
        )?;
        let option = DoubleTouchOption::cash(terms, 1.0)?;
        assert_eq!(
            AnalyticDoubleTouchEngine
                .price(&option, &context)?
                .to_bits(),
            0
        );
    }
    Ok(())
}

#[test]
fn analytic_at_hit_cash_converges_near_the_upper_barrier() -> Result<()> {
    let context = market(100.0).replace(PricingContextChanges {
        model_parameters: Some(BlackScholesMertonParameters::new(0.03, 0.0, 0.2)?.into()),
        ..Default::default()
    })?;
    for (lower, upper, expected, tolerance) in [
        (80.0, 100.000_001, 1.0, 1e-8),
        // The lower barrier is over 23 standard deviations away. Use the single-touch value.
        (1.0, 100.000_000_01, 0.999_999_999_613_655_6, 1e-12),
        (1.0, libm::nextafter(100.0, f64::INFINITY), 1.0, 1e-12),
    ] {
        let mut t = terms(DoubleBarrierType::KnockIn);
        t.lower_barrier = lower;
        t.upper_barrier = upper;
        t.settlement_timing = SettlementTiming::AtHit;
        let price = AnalyticDoubleTouchEngine.price(&DoubleTouchOption::cash(t, 1.0)?, &context)?;
        assert!(
            price <= 1.0 && (price - expected).abs() < tolerance,
            "barriers {lower}, {upper}: {price}"
        );
    }
    Ok(())
}

#[test]
fn analytic_cashflows_match_series_pde_and_payout_scaling() {
    let engine = AnalyticDoubleTouchEngine;
    let pde = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 1600,
        ..Default::default()
    })
    .unwrap();
    for (kind, timing, expected) in [
        (
            DoubleBarrierType::KnockIn,
            SettlementTiming::AtExpiry,
            0.603_183_970_029_436_4,
        ),
        (
            DoubleBarrierType::KnockIn,
            SettlementTiming::AtHit,
            0.611_941_520_996_109_3,
        ),
        (
            DoubleBarrierType::KnockOut,
            SettlementTiming::AtExpiry,
            0.367_261_563_519_071_74,
        ),
    ] {
        for payout in [1.0, 7.5] {
            let mut t = terms(kind);
            t.settlement_timing = timing;
            let option = DoubleTouchOption::cash(t, payout).unwrap();
            let price = engine.price(&option, &market(100.0)).unwrap();
            assert!((price - payout * expected).abs() < payout * 1e-11);
            assert!((price - pde.price(&option, &market(100.0)).unwrap()).abs() < payout * 1e-5);
        }
    }
    for rate in [-0.01, 0.0, 0.04] {
        let context = market(100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, 0.02, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let one = DoubleTouchOption::cash(terms(DoubleBarrierType::KnockIn), 1.0).unwrap();
        let no = DoubleTouchOption::cash(terms(DoubleBarrierType::KnockOut), 1.0).unwrap();
        assert!(
            (engine.price(&one, &context).unwrap() + engine.price(&no, &context).unwrap()
                - (-rate).exp())
            .abs()
                < 1e-13
        );
        let mut hit = terms(DoubleBarrierType::KnockIn);
        hit.settlement_timing = SettlementTiming::AtHit;
        let hit = DoubleTouchOption::cash(hit, 1.0).unwrap();
        assert!(
            (engine.price(&hit, &context).unwrap() - pde.price(&hit, &context).unwrap()).abs()
                < 1e-5
        );
    }
}

#[test]
fn analytic_discount_and_payout_scaling_preserve_representable_prices() {
    let engine = AnalyticDoubleTouchEngine;
    // Independent driftless Fourier survival and first-exit series, scaled before exponentiation.
    for (rate, payout, one, no, hit) in [
        (
            -800.0,
            1e-300,
            1.694_588_084_898_427_2e47,
            1.031_786_487_214_204e47,
            Some(1.550_613_852_825_589e44),
        ),
        (
            800.0,
            1e300,
            2.279_780_860_203_958_5e-48,
            1.388_093_723_973_642e-48,
            None,
        ),
    ] {
        let context = market(100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, rate - 0.02, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        for (kind, timing, expected) in [
            (
                DoubleBarrierType::KnockIn,
                SettlementTiming::AtExpiry,
                Some(one),
            ),
            (
                DoubleBarrierType::KnockOut,
                SettlementTiming::AtExpiry,
                Some(no),
            ),
            (DoubleBarrierType::KnockIn, SettlementTiming::AtHit, hit),
        ] {
            let Some(expected) = expected else { continue };
            let mut t = terms(kind);
            t.settlement_timing = timing;
            let option = DoubleTouchOption::cash(t.clone(), payout).unwrap();
            let actual = engine.price(&option, &context).unwrap();
            let tolerance = if timing == SettlementTiming::AtHit {
                1e-8
            } else {
                1e-11
            };
            assert!(
                (actual / expected - 1.0).abs() < tolerance,
                "{kind:?} {timing:?}: {actual} != {expected}"
            );
            if rate < 0.0 {
                let unrepresentable = DoubleTouchOption::cash(t, 1.0).unwrap();
                assert!(matches!(
                    engine.price(&unrepresentable, &context),
                    Err(Error::ComputationFailure(_))
                ));
            }
        }
    }
}

#[test]
fn analytic_early_at_hit_payments_avoid_negative_rate_cancellation() {
    let engine = AnalyticDoubleTouchEngine;
    for up in [true, false] {
        for sigma in [0.001, 1e-6, 1e-12] {
            let rate = -40.0;
            let dividend = if up {
                rate - 0.03
            } else {
                rate + 0.03 - sigma * sigma
            };
            let drift = rate - dividend - 0.5 * sigma * sigma;
            let towards = if up { drift } else { -drift };
            let mut t = terms(DoubleBarrierType::KnockIn);
            t.settlement_timing = SettlementTiming::AtHit;
            t.lower_barrier = if up { 80.0 } else { 100.0 / 1.003 };
            t.upper_barrier = if up { 100.3 } else { 125.0 };
            let option = DoubleTouchOption::cash(t, 1.0).unwrap();
            let context = market(100.0)
                .replace(PricingContextChanges {
                    model_parameters: Some(
                        BlackScholesMertonParameters::new(rate, dividend, sigma)
                            .unwrap()
                            .into(),
                    ),
                    ..Default::default()
                })
                .unwrap();
            // The far barrier and first hits after one year contribute below 1e-140 here.
            let expected = (-2.0 * rate * 1.003_f64.ln()
                / (towards + (towards * towards + 2.0 * rate * sigma * sigma).sqrt()))
            .exp();
            let actual = engine
                .price(&option, &context)
                .unwrap_or_else(|error| panic!("{up} {sigma}: {error}"));
            assert!(
                (actual / expected - 1.0).abs() < 1e-9,
                "{up} {sigma}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn analytic_exit_density_series_and_discounted_peaks_match_independent_values() {
    let engine = AnalyticDoubleTouchEngine;
    let mut t = terms(DoubleBarrierType::KnockIn);
    t.settlement_timing = SettlementTiming::AtHit;
    let option = DoubleTouchOption::cash(t.clone(), 1.0).unwrap();
    for (sigma, rate, expected) in [
        (0.25, 0.03, 0.796_252_277_079_910_5),
        (0.25, -0.03, 0.820_653_042_958_894_9),
        (0.6, 0.03, 0.996_583_908_565_960_9),
        (0.6, -0.03, 1.003_385_959_174_340_4),
    ] {
        let context = market(100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, 0.01, sigma)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let actual = engine.price(&option, &context).unwrap();
        assert!(
            (actual - expected).abs() < 1e-11,
            "{sigma} {rate}: {actual} != {expected}"
        );
    }
    // Discounted payments after expiry are below exp(-800); the infinite-horizon transform applies.
    for payout in [1.0_f64, 1e300] {
        let context = market(100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(800.0, 799.98, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let band = (120.0_f64 / 80.0).ln();
        let x = (100.0_f64 / 80.0).ln();
        let expected = (payout.ln() + (200.0 * (x - band / 2.0)).cosh().ln()
            - (100.0 * band).cosh().ln())
        .exp();
        let option = DoubleTouchOption::cash(t.clone(), payout).unwrap();
        let actual = engine.price(&option, &context).unwrap();
        assert!((actual / expected - 1.0).abs() < 1e-10);
    }
    t.lower_barrier = 99.5;
    t.upper_barrier = 100.5;
    let option = DoubleTouchOption::cash(t, 1.0).unwrap();
    for rate in [-0.03_f64, 0.03] {
        let context = market(100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, rate - 0.02, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let k = (2.0 * rate.abs()).sqrt() / 0.2;
        let half_band = (100.5_f64 / 99.5).ln() / 2.0;
        let offset = (100.0_f64 / 99.5).ln() - half_band;
        let expected = if rate > 0.0 {
            (k * offset).cosh() / (k * half_band).cosh()
        } else {
            (k * offset).cos() / (k * half_band).cos()
        };
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-11);
    }
    let mut t = terms(DoubleBarrierType::KnockIn);
    t.expiry_time = Date::new(2025, 1, 2).unwrap().start_of_day();
    t.lower_barrier = 70.0;
    t.upper_barrier = 140.0;
    t.settlement_timing = SettlementTiming::AtHit;
    let context = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(1e-10, 0.03, 0.01)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    for payout in [1.0, 1e300] {
        let option = DoubleTouchOption::cash(t.clone(), payout).unwrap();
        assert_eq!(engine.price(&option, &context).unwrap(), 0.0);
    }
}

#[test]
fn analytic_history_current_boundaries_and_expiry_preserve_cashflows() {
    let engine = AnalyticDoubleTouchEngine;
    let later = Date::new(2025, 1, 2).unwrap().start_of_day();
    for (kind, timing) in [
        (DoubleBarrierType::KnockIn, SettlementTiming::AtExpiry),
        (DoubleBarrierType::KnockIn, SettlementTiming::AtHit),
        (DoubleBarrierType::KnockOut, SettlementTiming::AtExpiry),
    ] {
        let mut t = terms(kind);
        t.settlement_timing = timing;
        let option = DoubleTouchOption::cash(t, 3.0).unwrap();
        let known = option
            .replace(DoubleTouchOptionChanges {
                touch_state: Some(Some(BarrierTouchState::Untouched)),
                ..Default::default()
            })
            .unwrap();
        for spot in [79.0, 80.0, 120.0, 121.0] {
            let expected = if kind == DoubleBarrierType::KnockOut {
                0.0
            } else if timing == SettlementTiming::AtHit {
                3.0
            } else {
                3.0 * (-0.03_f64).exp()
            };
            assert!((engine.price(&option, &market(spot)).unwrap() - expected).abs() < 1e-12);
            assert!(matches!(
                engine.price(&option, &context(spot, later)),
                Err(Error::MissingHistory(_))
            ));
            assert_eq!(
                engine
                    .price(&known, &context(spot, option.expiry_time()))
                    .unwrap(),
                if kind == DoubleBarrierType::KnockIn {
                    3.0
                } else {
                    0.0
                }
            );
        }
        let paid = option
            .replace(DoubleTouchOptionChanges {
                touch_state: Some(Some(BarrierTouchState::Touched)),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&paid, &market(100.0)),
            Err(Error::InvalidInput(_))
        ));
        let expected = if timing == SettlementTiming::AtHit || kind == DoubleBarrierType::KnockOut {
            0.0
        } else {
            3.0 * (-0.03_f64 * 364.0 / 365.0).exp()
        };
        assert!((engine.price(&paid, &context(100.0, later)).unwrap() - expected).abs() < 1e-12);
        let terminal = engine
            .price_with_greeks(
                &known,
                &context(100.0, option.expiry_time()),
                GreeksRequest::all(),
            )
            .unwrap();
        assert_eq!(
            terminal.price(),
            if kind == DoubleBarrierType::KnockIn {
                0.0
            } else {
                3.0
            }
        );
        for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
            assert_eq!(terminal.get(greek), None);
        }
        for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
            assert_eq!(terminal.get(greek), Some(0.0), "{greek:?} at expiry");
        }
        assert!(matches!(
            implied_volatility(
                &engine,
                &paid,
                &context(100.0, later),
                expected,
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
            ),
            Err(Error::UnsupportedOperation(_))
        ));
        assert_eq!(option.touch_state(), None);
    }
}

#[test]
fn analytic_schedules_validate_then_resolve_or_reject() {
    let engine = AnalyticDoubleTouchEngine;
    let fixing = Date::new(2025, 6, 30).unwrap().start_of_day();
    let mut t = terms(DoubleBarrierType::KnockIn);
    t.observation_schedule = Some(fixture_schedule(vec![fixing], t.calendar.as_ref()));
    let option = DoubleTouchOption::cash(t.clone(), 2.0).unwrap();
    assert!(matches!(
        engine.price(&option, &market(125.0)),
        Err(Error::UnsupportedOperation(_))
    ));
    let after = context(100.0, Date::new(2025, 7, 1).unwrap().start_of_day());
    assert!(matches!(
        engine.price(&option, &after),
        Err(Error::MissingHistory(_))
    ));
    let no_history = option
        .replace(DoubleTouchOptionChanges {
            touch_state: Some(Some(BarrierTouchState::Untouched)),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&no_history, &context(120.0, fixing)).unwrap()
            - 2.0 * (-0.03_f64 * 185.0 / 365.0).exp())
        .abs()
            < 1e-12
    );
    assert_eq!(engine.price(&no_history, &after).unwrap(), 0.0);
    t.barrier_type = DoubleBarrierType::KnockOut;
    let no = DoubleTouchOption::cash(t.clone(), 2.0)
        .unwrap()
        .replace(DoubleTouchOptionChanges {
            touch_state: Some(Some(BarrierTouchState::Untouched)),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&no, &after).unwrap() - 2.0 * (-0.03_f64 * 184.0 / 365.0).exp()).abs()
            < 1e-12
    );
    t.observation_schedule = Some(fixture_schedule(
        vec![Date::new(2025, 1, 4).unwrap().start_of_day()],
        Some(&TradingCalendar::all_days()),
    ));
    t.calendar = Some(TradingCalendar::weekdays());
    assert!(matches!(
        DoubleTouchOption::cash(t, 1.0),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn analytic_node_curves_reject_even_resolved_payments_and_failures_preserve_inputs() {
    let engine = AnalyticDoubleTouchEngine;
    let option = DoubleTouchOption::cash(terms(DoubleBarrierType::KnockIn), 1.0).unwrap();
    let original = option.clone();
    let curve = RateTermStructure::new(
        option.effective_time(),
        vec![
            option.effective_time().add_days(180).unwrap(),
            option.expiry_time(),
        ],
        vec![0.99, 0.97],
    )
    .unwrap();
    for risk_free in [false, true] {
        let model = market(120.0)
            .model_parameters()
            .as_bsm()
            .unwrap()
            .replace(BlackScholesMertonParameterChanges {
                risk_free_rate: risk_free.then(|| curve.clone().into()),
                dividend_yield: (!risk_free).then(|| curve.clone().into()),
                ..Default::default()
            })
            .unwrap();
        let curved = market(120.0)
            .replace(PricingContextChanges {
                model_parameters: Some(model.into()),
                ..Default::default()
            })
            .unwrap();
        for spot in [100.0, 120.0] {
            let context = curved
                .replace(PricingContextChanges {
                    spot_price: Some(spot),
                    ..Default::default()
                })
                .unwrap();
            assert!(matches!(
                engine.price(&option, &context),
                Err(Error::UnsupportedOperation(_))
            ));
            assert!(matches!(
                engine.price_with_greeks(&option, &context, GreeksRequest::all()),
                Err(Error::UnsupportedOperation(_))
            ));
            assert!(matches!(
                implied_volatility(
                    &engine,
                    &option,
                    &context,
                    f64::NAN,
                    kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
                ),
                Err(Error::UnsupportedOperation(_))
            ));
        }
    }
    let enormous = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.03, 0.01, f64::MAX)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &enormous),
        Err(Error::ComputationFailure(_))
    ));
    for date in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        Date::new(2026, 1, 2).unwrap().start_of_day(),
    ] {
        assert!(matches!(
            engine.price(&option, &context(100.0, date)),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(option, original);
}

#[test]
fn analytic_greeks_implied_volatility_and_boundary_availability() {
    let engine = AnalyticDoubleTouchEngine;
    for (kind, timing) in [
        (DoubleBarrierType::KnockIn, SettlementTiming::AtExpiry),
        (DoubleBarrierType::KnockIn, SettlementTiming::AtHit),
        (DoubleBarrierType::KnockOut, SettlementTiming::AtExpiry),
    ] {
        let mut t = terms(kind);
        t.settlement_timing = timing;
        t.touch_state = Some(BarrierTouchState::Untouched);
        let option = DoubleTouchOption::cash(t, 1.0).unwrap();
        let result = engine
            .price_with_greeks(&option, &market(100.0), GreeksRequest::all())
            .unwrap();
        for greek in [
            Greek::Delta,
            Greek::Gamma,
            Greek::Vega,
            Greek::Rho,
            Greek::Theta,
        ] {
            assert!(result.get(greek).is_some());
        }
        let solved = implied_volatility(
            &engine,
            &option,
            &market(100.0),
            result.price(),
            ImpliedVolatilitySettings::new(0.15, 0.25),
        )
        .unwrap();
        assert!((solved - 0.2).abs() < 1e-7);
        for spot in [80.0, 120.0] {
            let boundary = engine
                .price_with_greeks(&option, &market(spot), GreeksRequest::all())
                .unwrap();
            assert_eq!(boundary.get(Greek::Delta), None);
            assert_eq!(boundary.get(Greek::Gamma), None);
        }
    }
}

#[test]
fn analytic_tiny_volatility_preserves_drift_hit_timing_and_narrow_band_survival() {
    let engine = AnalyticDoubleTouchEngine;
    for up in [false, true] {
        let mut t = terms(DoubleBarrierType::KnockIn);
        t.lower_barrier = if up { 80.0 } else { 98.0 };
        t.upper_barrier = if up { 102.0 } else { 120.0 };
        let context = market(100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(0.04, if up { 0.01 } else { 0.07 }, 1e-12)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        for timing in [SettlementTiming::AtExpiry, SettlementTiming::AtHit] {
            t.settlement_timing = timing;
            let hit = (if up { 1.02_f64 } else { 100.0 / 98.0 }).ln() / 0.03;
            let expected = 3.0
                * (-0.04
                    * if timing == SettlementTiming::AtHit {
                        hit
                    } else {
                        1.0
                    })
                .exp();
            let option = DoubleTouchOption::cash(t.clone(), 3.0).unwrap();
            assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-7);
        }
    }
    let mut t = terms(DoubleBarrierType::KnockOut);
    t.lower_barrier = 99.5;
    t.upper_barrier = 100.5;
    let option = DoubleTouchOption::cash(t, 1.0).unwrap();
    assert!(engine.price(&option, &market(100.0)).unwrap() < 1e-100);
    let still = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.03, 0.03, 1e-12)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!((engine.price(&option, &still).unwrap() - (-0.03_f64).exp()).abs() < 1e-12);
}

#[test]
fn continuous_touch_cashflows_match_independent_series_and_converge() {
    let coarse = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 80,
        time_step_count: 100,
        ..Default::default()
    })
    .unwrap();
    let fine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 1600,
        ..Default::default()
    })
    .unwrap();
    let one = DoubleTouchOption::cash(terms(DoubleBarrierType::KnockIn), 1.0).unwrap();
    let no = DoubleTouchOption::cash(terms(DoubleBarrierType::KnockOut), 1.0).unwrap();
    let mut at_hit = terms(DoubleBarrierType::KnockIn);
    at_hit.settlement_timing = SettlementTiming::AtHit;
    let at_hit = DoubleTouchOption::cash(at_hit, 1.0).unwrap();
    // Absorbing log-Brownian sine series, independent of the finite-difference solver.
    for (option, expected) in [
        (&one, 0.603_183_970_029_436_4),
        (&at_hit, 0.611_941_520_996_109_3),
        (&no, 0.367_261_563_519_071_74),
    ] {
        let price = fine.price(option, &market(100.0)).unwrap();
        assert!((price - expected).abs() < 1e-5, "{price} != {expected}");
        assert!(
            (price - expected).abs()
                < (coarse.price(option, &market(100.0)).unwrap() - expected).abs()
        );
    }
    assert!(
        (fine.price(&one, &market(100.0)).unwrap() + fine.price(&no, &market(100.0)).unwrap()
            - (-0.03_f64).exp())
        .abs()
            < 1e-8
    );
}

fn context(spot: f64, time: Timestamp) -> PricingContext {
    market(spot)
        .replace(PricingContextChanges {
            valuation_time: Some(time),
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn cash_terms_defaults_validation_and_owning_history() {
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let t = terms(kind);
        let option = DoubleTouchOption::cash(t.clone(), 1.0).unwrap();
        assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
        assert_eq!(option.touch_state(), None);
        assert!(option.observation_schedule().is_none());
        let known = option
            .replace(DoubleTouchOptionChanges {
                touch_state: Some(Some(BarrierTouchState::Untouched)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
        assert_eq!(option.touch_state(), None);
        assert_eq!(
            known
                .replace(DoubleTouchOptionChanges {
                    touch_state: Some(None),
                    ..Default::default()
                })
                .unwrap(),
            option
        );
        for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            assert!(matches!(
                DoubleTouchOption::cash(t.clone(), invalid).unwrap_err(),
                Error::InvalidInput(_)
            ));
            for lower in [false, true] {
                let mut bad = t.clone();
                if lower {
                    bad.lower_barrier = invalid;
                } else {
                    bad.upper_barrier = invalid;
                }
                assert!(matches!(
                    DoubleTouchOption::cash(bad, 1.0).unwrap_err(),
                    Error::InvalidInput(_)
                ));
            }
        }
        for lower in [120.0, 121.0] {
            let mut bad = t.clone();
            bad.lower_barrier = lower;
            assert!(matches!(
                DoubleTouchOption::cash(bad, 1.0).unwrap_err(),
                Error::InvalidInput(_)
            ));
        }
        let mut bad = t.clone();
        bad.expiry_time = Date::new(2024, 12, 31).unwrap().start_of_day();
        assert!(matches!(
            DoubleTouchOption::cash(bad, 1.0).unwrap_err(),
            Error::InvalidInput(_)
        ));
        let mut bad = t.clone();
        bad.observation_schedule = Some(fixture_schedule(vec![], bad.calendar.as_ref()));
        assert!(matches!(
            DoubleTouchOption::cash(bad, 1.0).unwrap_err(),
            Error::InvalidInput(_)
        ));
        let first = Date::new(2025, 6, 30).unwrap().start_of_day();
        for dates in [vec![t.expiry_time, first], vec![first, first]] {
            assert!(matches!(
                ObservationSchedule::new(
                    dates,
                    t.effective_time,
                    t.expiry_time,
                    &TradingCalendar::weekdays()
                ),
                Err(Error::InvalidInput(_))
            ));
        }
        let mut scheduled = t;
        scheduled.observation_schedule = Some(fixture_schedule(
            vec![first, scheduled.expiry_time],
            scheduled.calendar.as_ref(),
        ));
        let owned = DoubleTouchOption::cash(scheduled.clone(), 1.0).unwrap();
        scheduled.observation_schedule = None;
        assert_eq!(
            owned.observation_schedule().unwrap().times(),
            &[first, owned.expiry_time()]
        );
    }
    let mut invalid = terms(DoubleBarrierType::KnockOut);
    invalid.settlement_timing = SettlementTiming::AtHit;
    assert!(matches!(
        DoubleTouchOption::cash(invalid.clone(), 1.0).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        DoubleTouchOption::cash(invalid, 0.0).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "Exact comparisons select fixed test cases and payoff boundaries."
)]
fn current_prior_and_expiry_payments_resolve_before_grid_validation() {
    let engine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    })
    .unwrap();
    let later = Date::new(2025, 1, 2).unwrap().start_of_day();
    for timing in [SettlementTiming::AtExpiry, SettlementTiming::AtHit] {
        let mut t = terms(DoubleBarrierType::KnockIn);
        t.settlement_timing = timing;
        let option = DoubleTouchOption::cash(t, 3.0).unwrap();
        let paid = option
            .replace(DoubleTouchOptionChanges {
                touch_state: Some(Some(BarrierTouchState::Touched)),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&paid, &market(100.0)).unwrap_err(),
            Error::InvalidInput(_)
        ));
        for spot in [79.0, 80.0, 120.0, 121.0] {
            let expected = if timing == SettlementTiming::AtHit {
                3.0
            } else {
                3.0 * (-0.03_f64).exp()
            };
            assert!((engine.price(&option, &market(spot)).unwrap() - expected).abs() < 1e-12);
            assert!(matches!(
                engine.price(&option, &context(spot, later)).unwrap_err(),
                Error::MissingHistory(_)
            ));
        }
        let expected = if timing == SettlementTiming::AtHit {
            0.0
        } else {
            3.0 * (-0.03_f64 * 364.0 / 365.0).exp()
        };
        assert!((engine.price(&paid, &context(100.0, later)).unwrap() - expected).abs() < 1e-12);
        assert!(matches!(
            implied_volatility(
                &engine,
                &paid,
                &context(100.0, later),
                expected,
                kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0),
            )
            .unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let option = DoubleTouchOption::cash(terms(kind), 3.0)
            .unwrap()
            .replace(DoubleTouchOptionChanges {
                touch_state: Some(Some(BarrierTouchState::Untouched)),
                ..Default::default()
            })
            .unwrap();
        for spot in [80.0, 100.0, 120.0] {
            let at = context(spot, option.expiry_time());
            let result = engine
                .price_with_greeks(&option, &at, GreeksRequest::all())
                .unwrap();
            let touched = spot != 100.0;
            assert_eq!(
                result.price(),
                if touched == (option.barrier_type() == DoubleBarrierType::KnockIn) {
                    3.0
                } else {
                    0.0
                }
            );
            for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
                assert_eq!(result.get(greek), None);
            }
            for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
                assert_eq!(result.get(greek), Some(0.0), "{greek:?} at expiry");
            }
        }
    }
}

#[test]
fn scheduled_one_two_and_early_fixings_match_independent_integrals() {
    let first = Date::new(2025, 6, 30).unwrap().start_of_day();
    let engine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 1600,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    // Conditional lognormal integrals from the independent Python reference.
    for (dates, expected) in [
        (
            vec![terms(DoubleBarrierType::KnockIn).expiry_time],
            [
                0.304_897_549_279_567_64,
                0.304_897_549_279_567_64,
                0.665_547_984_268_940_6,
            ],
        ),
        (
            vec![first, Date::new(2026, 1, 1).unwrap().start_of_day()],
            [
                0.355_769_695_628_499,
                0.358_062_774_300_611_7,
                0.614_675_837_920_009_1,
            ],
        ),
        (
            vec![first],
            [
                0.149_662_441_098_767_42,
                0.151_955_519_770_880_07,
                0.820_783_092_449_740_8,
            ],
        ),
    ] {
        for (index, (kind, timing)) in [
            (DoubleBarrierType::KnockIn, SettlementTiming::AtExpiry),
            (DoubleBarrierType::KnockIn, SettlementTiming::AtHit),
            (DoubleBarrierType::KnockOut, SettlementTiming::AtExpiry),
        ]
        .into_iter()
        .enumerate()
        {
            let mut t = terms(kind);
            t.lower_barrier = 80.125;
            t.upper_barrier = 120.125;
            t.observation_schedule = Some(fixture_schedule(dates.clone(), t.calendar.as_ref()));
            t.settlement_timing = timing;
            let option = DoubleTouchOption::cash(t, 1.0).unwrap();
            let price = engine.price(&option, &market(100.25)).unwrap();
            assert!(
                (price - expected[index]).abs() < 3e-5,
                "{dates:?} {kind:?} {timing:?}: {price} != {}",
                expected[index]
            );
        }
    }
}

#[test]
fn early_window_history_utc_calendar_and_fixed_cash_greeks() {
    let engine = FiniteDifferenceDoubleTouchEngine::default();
    let first = Date::new(2025, 6, 30).unwrap().start_of_day();
    let mut t = terms(DoubleBarrierType::KnockOut);
    t.observation_schedule = Some(fixture_schedule(vec![first], t.calendar.as_ref()));
    let option = DoubleTouchOption::cash(t.clone(), 2.0).unwrap();
    assert_eq!(engine.price(&option, &context(120.0, first)).unwrap(), 0.0);
    let after = Timestamp::new(first.date(), 0, 0, 0, 1).unwrap();
    assert!(matches!(
        engine.price(&option, &context(130.0, after)).unwrap_err(),
        Error::MissingHistory(_)
    ));
    let known = option
        .replace(DoubleTouchOptionChanges {
            touch_state: Some(Some(BarrierTouchState::Untouched)),
            ..Default::default()
        })
        .unwrap();
    let ctx = context(130.0, after);
    let remaining = (185.0_f64 - 1e-6 / 86400.0) / 365.0;
    let expected = 2.0 * (-0.03 * remaining).exp();
    let result = engine
        .price_with_greeks(&known, &ctx, GreeksRequest::all())
        .unwrap();
    assert!((result.price() - expected).abs() < 1e-12);
    assert_eq!(result.require(Greek::Delta).unwrap(), 0.0);
    assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
    assert!((result.require(Greek::Rho).unwrap() + remaining * expected).abs() < 1e-8);
    assert!(matches!(
        implied_volatility(
            &engine,
            &known,
            &ctx,
            expected,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    t.observation_schedule = Some(fixture_schedule(
        vec![Date::new(2025, 1, 4).unwrap().start_of_day()],
        Some(&TradingCalendar::all_days()),
    ));
    t.calendar = Some(TradingCalendar::weekdays());
    assert!(matches!(
        DoubleTouchOption::cash(t, 1.0),
        Err(Error::InvalidInput(_))
    ));
    let at = engine
        .price_with_greeks(
            &option
                .replace(DoubleTouchOptionChanges {
                    touch_state: Some(Some(BarrierTouchState::Untouched)),
                    ..Default::default()
                })
                .unwrap(),
            &context(100.0, first),
            GreeksRequest::new([Greek::Theta, Greek::Charm, Greek::Color]),
        )
        .unwrap();
    assert!(
        [Greek::Theta, Greek::Charm, Greek::Color]
            .into_iter()
            .all(|g| at.get(g).is_none())
    );
}

#[test]
fn greeks_iv_failures_and_concurrency_preserve_inputs() {
    let engine = FiniteDifferenceDoubleTouchEngine::default();
    for (kind, timing) in [
        (DoubleBarrierType::KnockIn, SettlementTiming::AtExpiry),
        (DoubleBarrierType::KnockIn, SettlementTiming::AtHit),
        (DoubleBarrierType::KnockOut, SettlementTiming::AtExpiry),
    ] {
        let mut t = terms(kind);
        t.settlement_timing = timing;
        t.touch_state = Some(BarrierTouchState::Untouched);
        let option = DoubleTouchOption::cash(t, 1.0).unwrap();
        let ctx = market(100.0);
        let result = engine
            .price_with_greeks(&option, &ctx, GreeksRequest::all())
            .unwrap();
        assert!(Greek::ALL.into_iter().all(|g| result.get(g).is_some()));
        let scalar = numerical_greeks(
            &engine,
            &option,
            &ctx,
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
        for g in Greek::ALL {
            assert!(
                (result.require(g).unwrap() - scalar.require(g).unwrap()).abs() < 1e-5,
                "{kind:?} {timing:?} {g:?}"
            );
        }
        let controls = ImpliedVolatilitySettings::new(0.15, 0.25);
        let sigma = implied_volatility(&engine, &option, &ctx, result.price(), controls).unwrap();
        assert!((sigma - 0.2).abs() < 1e-6);
        assert!(matches!(
            implied_volatility(&engine, &option, &ctx, 2.0, controls).unwrap_err(),
            Error::RootNotBracketed(_)
        ));

        assert!(matches!(
            kiyosi::numerical_greeks(
                &engine,
                &option,
                &ctx,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    spot_shift: Some(0.0),
                    ..Default::default()
                }
            )
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        for spot in [80.0, 120.0] {
            let at = engine
                .price_with_greeks(&option, &market(spot), GreeksRequest::all())
                .unwrap();
            assert_eq!(at.get(Greek::Delta), None);
            assert_eq!(at.get(Greek::Gamma), None);
        }
        assert_eq!(engine.price(&option, &ctx).unwrap(), result.price());
        assert_eq!(ctx, market(100.0));
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| engine.price(&option, &ctx).unwrap()))
                .collect();
            for handle in handles {
                assert_eq!(handle.join().unwrap(), result.price());
            }
        });
    }
}

#[test]
fn node_curves_and_early_at_hit_horizons_match_digital_cash_portfolios() {
    let first = Date::new(2025, 6, 30).unwrap().start_of_day();
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let engine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 1600,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    for early in [false, true] {
        let horizon = if early { first } else { end };
        let curve = RateTermStructure::new(
            terms(DoubleBarrierType::KnockIn).effective_time,
            if early { vec![first] } else { vec![first, end] },
            if early { vec![0.97] } else { vec![0.97, 0.99] },
        )
        .unwrap();
        let ctx = market(125.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(curve.clone(), 0.01, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let mut t = terms(DoubleBarrierType::KnockIn);
        t.lower_barrier = 80.125;
        t.upper_barrier = 120.125;
        t.observation_schedule = Some(fixture_schedule(vec![horizon], t.calendar.as_ref()));
        t.settlement_timing = SettlementTiming::AtHit;
        let mut touched = 0.0;
        for (direction, level) in [
            (OptionType::Put, t.lower_barrier),
            (OptionType::Call, t.upper_barrier),
        ] {
            let digital =
                DigitalOption::cash_or_nothing(direction, level, 1.0, t.effective_time, horizon)
                    .unwrap();
            touched += AnalyticDigitalEngine.price(&digital, &ctx).unwrap();
        }
        let option = DoubleTouchOption::cash(t.clone(), 1.0).unwrap();
        assert!((engine.price(&option, &ctx).unwrap() - touched).abs() < 3e-5);
        t.settlement_timing = SettlementTiming::AtExpiry;
        if early {
            assert!(matches!(
                engine
                    .price(&DoubleTouchOption::cash(t, 1.0).unwrap(), &ctx)
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
        } else {
            t.barrier_type = DoubleBarrierType::KnockOut;
            let no = DoubleTouchOption::cash(t, 1.0).unwrap();
            assert!((engine.price(&no, &ctx).unwrap() - (0.99 - touched)).abs() < 3e-5);
        }
    }
}

#[test]
fn narrow_band_tiny_volatility_and_grid_validation() {
    let mut t = terms(DoubleBarrierType::KnockOut);
    t.lower_barrier = 99.5;
    t.upper_barrier = 100.5;
    let option = DoubleTouchOption::cash(t.clone(), 1.0).unwrap();
    let engine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 160,
        time_step_count: 800,
        ..Default::default()
    })
    .unwrap();
    assert!(engine.price(&option, &market(100.0)).unwrap() < 1e-8);
    let still = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.03, 0.03, 1e-12)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!((engine.price(&option, &still).unwrap() - (-0.03_f64).exp()).abs() < 1e-8);
    for settings in [
        FiniteDifferenceSettings {
            asset_step_count: 2,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        },
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(f64::INFINITY),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            FiniteDifferenceDoubleTouchEngine::new(settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let unstable = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        time_step_count: 1,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        unstable.price(&option, &market(100.0)),
        Err(Error::InvalidInput(_))
    ));
    let clipped = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(100.5),
        ..Default::default()
    })
    .unwrap();
    t.observation_schedule = Some(fixture_schedule(vec![t.expiry_time], t.calendar.as_ref()));
    let scheduled = DoubleTouchOption::cash(t, 1.0).unwrap();
    assert!(matches!(
        clipped.price(&scheduled, &market(100.0)),
        Err(Error::InvalidInput(_))
    ));
}
