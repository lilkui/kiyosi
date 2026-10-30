use kiyosi::*;

fn day(y: i32, m: u32, d: u32) -> Date {
    Date::new(y, m, d).unwrap()
}
fn terms(kind: BarrierType) -> BinaryBarrierTerms {
    BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        day(2025, 1, 1),
        day(2026, 1, 1),
        120.0,
        kind,
    )
}
fn market(spot: f64, time: Date) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        time.into(),
    )
    .unwrap()
}

#[test]
fn binary_branches_preserve_in_out_parity() {
    let engine = AnalyticBinaryBarrierEngine;
    for direction in [OptionType::Call, OptionType::Put] {
        for (inside, outside, level) in [
            (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.0),
            (BarrierType::DownAndIn, BarrierType::DownAndOut, 80.0),
        ] {
            for strike in [70.0, 80.0, 100.0, 120.0, 130.0] {
                for asset in [false, true] {
                    let mut t = terms(inside);
                    t.option_type = direction;
                    t.strike = strike;
                    t.barrier_level = level;
                    let vanilla = if asset {
                        DigitalOption::asset_or_nothing(
                            direction,
                            strike,
                            t.effective_date,
                            t.expiry_date,
                        )
                    } else {
                        DigitalOption::cash_or_nothing(
                            direction,
                            strike,
                            10.0,
                            t.effective_date,
                            t.expiry_date,
                        )
                    }
                    .unwrap();
                    let make = |t| {
                        if asset {
                            BinaryBarrierOption::asset(t)
                        } else {
                            BinaryBarrierOption::cash(t, 10.0)
                        }
                        .unwrap()
                    };
                    let context = market(100.0, t.effective_date);
                    let a = engine.price(&make(t.clone()), &context).unwrap();
                    t.barrier_type = outside;
                    let b = engine.price(&make(t), &context).unwrap();
                    let expected = AnalyticDigitalEngine.price(&vanilla, &context).unwrap();
                    assert!(
                        (a + b - expected).abs() < 1e-10,
                        "{direction:?} {inside:?} {strike} {asset}: {a} + {b} != {expected}"
                    );
                }
            }
        }
    }
}

fn touch(kind: BarrierType) -> TouchOptionTerms {
    TouchOptionTerms::new(day(2025, 1, 1), day(2026, 1, 1), 120.0, kind)
}
#[test]
fn strict_prior_history_and_current_settlement_are_distinct() {
    let engine = AnalyticBinaryBarrierEngine;
    for asset in [false, true] {
        for timing in [SettlementTiming::AtHit, SettlementTiming::AtExpiry] {
            let mut t = touch(BarrierType::UpAndIn);
            t.settlement_timing = timing;
            let option = if asset {
                TouchOption::asset(t)
            } else {
                TouchOption::cash(t, 10.0)
            }
            .unwrap();
            assert_eq!(option.touch_state(), None);
            let now = engine
                .price(&option, &market(130.0, day(2025, 1, 1)))
                .unwrap();
            let amount = if asset { 130.0 } else { 10.0 };
            let expected = if timing == SettlementTiming::AtHit {
                amount
            } else {
                amount * (-if asset { 0.01_f64 } else { 0.04_f64 }).exp()
            };
            assert!((now - expected).abs() < 1e-12);
            assert!(matches!(
                engine
                    .price(&option, &market(130.0, day(2025, 1, 2)))
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            let known = option.replace(Some(BarrierTouchState::Touched));
            assert!(matches!(
                engine
                    .price(&known, &market(130.0, day(2025, 1, 1)))
                    .unwrap_err(),
                Error::InvalidInput(_)
            ));
            let later = market(100.0, day(2025, 7, 1));
            let expected = if timing == SettlementTiming::AtHit {
                0.0
            } else {
                (if asset { 100.0 } else { 10.0 })
                    * (-if asset { 0.01_f64 } else { 0.04_f64 } * 184.0 / 365.0).exp()
            };
            assert!((engine.price(&known, &later).unwrap() - expected).abs() < 1e-12);
            assert_eq!(known.replace(None), option);
            let quote = engine.price(&known, &later).unwrap();
            assert!(matches!(
                implied_volatility(&engine, &known, &later, quote, Default::default()).unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
            assert!(matches!(
                implied_volatility(
                    &engine,
                    &option,
                    &market(130.0, day(2025, 1, 1)),
                    now,
                    Default::default(),
                )
                .unwrap_err(),
                Error::UnsupportedOperation(_)
            ));
        }
    }
    for kind in [BarrierType::UpAndIn, BarrierType::UpAndOut] {
        let option = BinaryBarrierOption::cash(terms(kind), 10.0).unwrap();
        assert!(matches!(
            engine
                .price(&option, &market(130.0, day(2025, 7, 1)))
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        let paid = option.replace(Some(BarrierTouchState::Touched));
        let quote = engine
            .price(&paid, &market(100.0, day(2025, 7, 1)))
            .unwrap();
        assert_eq!(quote == 0.0, kind == BarrierType::UpAndOut);
        assert_eq!(option.touch_state(), None);
    }
}

#[test]
fn schedules_stop_monitoring_and_only_observe_at_midnight() {
    let engine = AnalyticBinaryBarrierEngine;
    let fixing = day(2025, 1, 2);
    for kind in [BarrierType::UpAndIn, BarrierType::UpAndOut] {
        let mut t = touch(kind);
        t.observation_mode = ObservationMode::Scheduled;
        t.observation_dates = vec![fixing];
        let unknown = TouchOption::cash(t.clone(), 10.0).unwrap();
        assert!(engine.price(&unknown, &market(120.0, fixing)).is_ok());
        assert!(matches!(
            engine
                .price(&unknown, &market(100.0, day(2025, 1, 3)))
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        t.touch_state = Some(BarrierTouchState::Untouched);
        let option = TouchOption::cash(t.clone(), 10.0).unwrap();
        let expected = if kind == BarrierType::UpAndIn {
            0.0
        } else {
            10.0 * (-0.04_f64 * 363.0 / 365.0).exp()
        };
        assert!(
            (engine
                .price(&option, &market(130.0, day(2025, 1, 3)))
                .unwrap()
                - expected)
                .abs()
                < 1e-12
        );
        let intraday = market(130.0, fixing)
            .replace(PricingContextChanges {
                valuation_time: Some(Timestamp::new(fixing, 0, 0, 0, 1).unwrap()),
                ..Default::default()
            })
            .unwrap();
        let price = engine.price(&option, &intraday).unwrap();
        assert_eq!(price == 0.0, kind == BarrierType::UpAndIn);
        t.touch_state = Some(BarrierTouchState::Touched);
        let touched = TouchOption::cash(t, 10.0).unwrap();
        assert!(matches!(
            engine.price(&touched, &market(120.0, fixing)).unwrap_err(),
            Error::InvalidInput(_)
        ));
        let mut b = terms(kind);
        b.observation_mode = ObservationMode::Scheduled;
        b.observation_dates = vec![fixing];
        b.touch_state = Some(BarrierTouchState::Untouched);
        let binary = BinaryBarrierOption::cash(b, 10.0).unwrap();
        let value = engine
            .price(&binary, &market(130.0, day(2025, 1, 3)))
            .unwrap();
        assert_eq!(value == 0.0, kind == BarrierType::UpAndIn);
        assert!(matches!(
            implied_volatility(&engine, &option, &intraday, price, Default::default()).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    let mut t = touch(BarrierType::UpAndIn);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![fixing];
    assert!(matches!(
        engine
            .price(
                &TouchOption::cash(t.clone(), 10.0).unwrap(),
                &market(100.0, t.effective_date),
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    t.observation_dates = vec![day(2025, 1, 4), t.expiry_date];
    assert!(matches!(
        engine
            .price(
                &TouchOption::cash(t, 10.0).unwrap(),
                &market(120.0, day(2025, 1, 1)),
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn validation_expiry_and_numerical_analysis_preserve_contracts() {
    let engine = AnalyticBinaryBarrierEngine;
    let t = terms(BarrierType::UpAndOut);
    for payout in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(matches!(
            BinaryBarrierOption::cash(t.clone(), payout).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            TouchOption::cash(touch(BarrierType::UpAndIn), payout).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut invalid = t.clone();
    invalid.strike = 0.0;
    invalid.barrier_level = 0.0;
    assert!(matches!(
        BinaryBarrierOption::asset(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut invalid = touch(BarrierType::UpAndOut);
    invalid.settlement_timing = SettlementTiming::AtHit;
    assert!(matches!(
        TouchOption::asset(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    for level in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut invalid = touch(BarrierType::UpAndIn);
        invalid.barrier_level = level;
        assert!(matches!(
            TouchOption::asset(invalid).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut invalid = touch(BarrierType::UpAndIn);
    invalid.observation_mode = ObservationMode::Scheduled;
    assert!(matches!(
        TouchOption::asset(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut invalid = t.clone();
    invalid.expiry_date = day(2024, 1, 1);
    assert!(matches!(
        BinaryBarrierOption::asset(invalid).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let option = BinaryBarrierOption::cash(t, 10.0)
        .unwrap()
        .replace(Some(BarrierTouchState::Untouched));
    let context = market(100.0, day(2025, 1, 1));
    let result = engine
        .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| result.has(g)));
    let controls = ImpliedVolatilitySettings {
        lower_bound: 0.15,
        upper_bound: 0.25,
        ..Default::default()
    };
    assert!(
        (implied_volatility(&engine, &option, &context, result.price(), controls).unwrap() - 0.2)
            .abs()
            < 1e-6
    );
    assert!(matches!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::new([]),
                Default::default(),
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    for spot in [100.0, 120.0] {
        let expiry = market(spot, option.expiry_date());
        assert_eq!(engine.price(&option, &expiry).unwrap(), 0.0);
        let result = engine
            .price_with_greeks(&option, &expiry, GreeksRequest::all(), Default::default())
            .unwrap();
        assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
    }
    let boundary = engine
        .price_with_greeks(
            &option,
            &market(120.0, day(2025, 1, 1)),
            GreeksRequest::all(),
            Default::default(),
        )
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| !boundary.has(g)));
    let curve = RateTermStructure::new(
        context.valuation_time(),
        vec![option.expiry_date().into()],
        vec![0.95],
    )
    .unwrap();
    let model = context
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let curves = context
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &curves).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(context.model_parameters().volatility(), 0.2);
    assert_eq!(option.payout(), Some(10.0));
}

#[test]
fn first_hit_and_scheduled_prices_retain_scaled_tails() {
    let engine = AnalyticBinaryBarrierEngine;
    for up in [false, true] {
        let kind = if up {
            BarrierType::UpAndIn
        } else {
            BarrierType::DownAndIn
        };
        let ratio = (if up { 8.0_f64 } else { -8.0_f64 }).exp();
        for amount in [1e200, 1e300] {
            let expected = if up {
                1.3325295301744355e-51
            } else {
                3.9722145459424451e-48
            } * (amount / 1e300);
            for timing in [SettlementTiming::AtHit, SettlementTiming::AtExpiry] {
                let mut t = touch(kind);
                t.barrier_level = 100.0 * ratio;
                t.settlement_timing = timing;
                let option = TouchOption::cash(t.clone(), amount).unwrap();
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
                    100.0,
                    t.effective_date.into(),
                )
                .unwrap();
                let actual = engine.price(&option, &context).unwrap();
                assert!(
                    (actual / expected - 1.0).abs() < 1e-11,
                    "{actual} != {expected}"
                );
                t.barrier_level = amount;
                let asset = TouchOption::asset(t).unwrap();
                let asset_context = context
                    .replace(PricingContextChanges {
                        spot_price: Some(amount / ratio),
                        ..Default::default()
                    })
                    .unwrap();
                let actual = engine.price(&asset, &asset_context).unwrap();
                assert!((actual / expected - 1.0).abs() < 1e-11);
            }
        }
    }
    for scale in [1.0, 1e306] {
        for (kind, expected) in [
            (BarrierType::UpAndIn, 0.25739250301126404),
            (BarrierType::UpAndOut, 0.742607496988736),
        ] {
            let mut t = touch(kind);
            t.barrier_level = 170.0 * scale;
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![
                day(2025, 4, 1),
                day(2025, 7, 1),
                day(2025, 10, 1),
                t.expiry_date,
            ];
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.0, 0.0, 1.0).unwrap(),
                100.0 * scale,
                t.effective_date.into(),
            )
            .unwrap();
            let value = engine
                .price(&TouchOption::cash(t, 1.0).unwrap(), &context)
                .unwrap();
            assert!((value - expected).abs() < 1e-12);
        }
    }
}

#[test]
fn binary_prices_survive_variance_underflow_and_small_volatility_carry() {
    let engine = AnalyticBinaryBarrierEngine;
    for sigma in [1e-160, 1e-200] {
        for carry in [-0.3, -0.02, 0.0, 0.02, 0.3] {
            for (inside, outside, level, up) in [
                (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.0, true),
                (BarrierType::DownAndIn, BarrierType::DownAndOut, 80.0, false),
            ] {
                for kind in [inside, outside] {
                    for direction in [OptionType::Call, OptionType::Put] {
                        for asset in [false, true] {
                            let mut t = terms(kind);
                            t.option_type = direction;
                            t.barrier_level = level;
                            let context = PricingContext::new(
                                BlackScholesMertonParameters::new(0.03, 0.03 - carry, sigma)
                                    .unwrap(),
                                100.0,
                                t.effective_date.into(),
                            )
                            .unwrap();
                            let terminal = 100.0 * carry.exp();
                            let touched = if up {
                                terminal >= level
                            } else {
                                terminal <= level
                            };
                            let weight = if carry == 0.0 {
                                0.5
                            } else if (direction == OptionType::Call && terminal > 100.0)
                                || (direction == OptionType::Put && terminal < 100.0)
                            {
                                1.0
                            } else {
                                0.0
                            };
                            let expected = if (kind == inside) == touched {
                                (if asset { terminal } else { 10.0 }) * (-0.03_f64).exp() * weight
                            } else {
                                0.0
                            };
                            let option = if asset {
                                BinaryBarrierOption::asset(t)
                            } else {
                                BinaryBarrierOption::cash(t, 10.0)
                            }
                            .unwrap();
                            let actual = engine.price(&option, &context).unwrap();
                            assert!(
                                (actual - expected).abs() < 1e-11,
                                "{sigma} {carry} {kind:?} {direction:?}: {actual} != {expected}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn touch_variance_underflow_retains_first_passage_and_half_probability_boundary() {
    let engine = AnalyticBinaryBarrierEngine;
    for up in [false, true] {
        let barrier: f64 = if up { 101.0 } else { 99.0 };
        let kind = if up {
            BarrierType::UpAndIn
        } else {
            BarrierType::DownAndIn
        };
        for sigma in [1e-160, 1e-200, f64::from_bits(1)] {
            for crossing in [false, true] {
                let dividend = if crossing {
                    if up { 0.0 } else { 0.1 }
                } else {
                    0.05
                };
                let context = PricingContext::new(
                    BlackScholesMertonParameters::new(0.05, dividend, sigma).unwrap(),
                    100.0,
                    day(2025, 1, 1).into(),
                )
                .unwrap();
                let hit_time = if crossing {
                    ((barrier - 100.0) / 100.0).ln_1p() / (0.05 - dividend)
                } else {
                    0.0
                };
                for timing in [SettlementTiming::AtHit, SettlementTiming::AtExpiry] {
                    let mut t = touch(kind);
                    t.barrier_level = barrier;
                    t.settlement_timing = timing;
                    let discount = (-0.05
                        * if timing == SettlementTiming::AtHit {
                            hit_time
                        } else {
                            1.0
                        })
                    .exp();
                    let cash = TouchOption::cash(t.clone(), 1.0).unwrap();
                    let asset = TouchOption::asset(t).unwrap();
                    let expected = if crossing { discount } else { 0.0 };
                    assert!((engine.price(&cash, &context).unwrap() - expected).abs() < 1e-12);
                    let expected = if !crossing {
                        0.0
                    } else if timing == SettlementTiming::AtHit {
                        barrier * discount
                    } else {
                        100.0 * (-dividend).exp()
                    };
                    assert!((engine.price(&asset, &context).unwrap() - expected).abs() < 1e-10);
                }
                let mut t = touch(if up {
                    BarrierType::UpAndOut
                } else {
                    BarrierType::DownAndOut
                });
                t.barrier_level = barrier;
                let expected = if crossing { 0.0 } else { (-0.05_f64).exp() };
                assert!(
                    (engine
                        .price(&TouchOption::cash(t.clone(), 1.0).unwrap(), &context)
                        .unwrap()
                        - expected)
                        .abs()
                        < 1e-12
                );
                let expected = if crossing {
                    0.0
                } else {
                    100.0 * (-dividend).exp()
                };
                assert!(
                    (engine
                        .price(&TouchOption::asset(t).unwrap(), &context)
                        .unwrap()
                        - expected)
                        .abs()
                        < 1e-10
                );
            }
        }
        let drift = ((barrier - 100.0) / 100.0).ln_1p();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(0.0, -drift, 1e-200).unwrap(),
            100.0,
            day(2025, 1, 1).into(),
        )
        .unwrap();
        let mut t = touch(kind);
        t.barrier_level = barrier;
        assert_eq!(
            engine
                .price(&TouchOption::cash(t, 1.0).unwrap(), &context)
                .unwrap(),
            0.5
        );
    }
}

#[test]
fn overflowing_reflection_and_extreme_first_hit_discounts_remain_finite() {
    let engine = AnalyticBinaryBarrierEngine;
    let (spot, barrier) = (1e20_f64, 1e-300_f64);
    let drift = barrier.ln() - spot.ln();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, -drift, 1e-200).unwrap(),
        spot,
        day(2025, 1, 1).into(),
    )
    .unwrap();
    let mut t = touch(BarrierType::DownAndIn);
    t.barrier_level = barrier;
    assert_eq!(
        engine
            .price(&TouchOption::cash(t, 1.0).unwrap(), &context)
            .unwrap(),
        0.5
    );
    for sigma in [1e-160, 1e-200] {
        let carry = std::f64::consts::LN_2;
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(carry, 0.0, sigma).unwrap(),
            0.5,
            day(2025, 1, 1).into(),
        )
        .unwrap();
        let mut t = terms(BarrierType::UpAndIn);
        t.option_type = OptionType::Put;
        t.strike = 1.0;
        t.barrier_level = 1.0;
        let value = engine
            .price(&BinaryBarrierOption::cash(t, 1e300).unwrap(), &context)
            .unwrap();
        let expected = (1e300 * sigma) / (4.0 * carry * (2.0 * std::f64::consts::PI).sqrt());
        assert!((value / expected - 1.0).abs() < 1e-12);
    }
    for up in [false, true] {
        let (spot, barrier, rate) = if up {
            (1e-200_f64, 1e200_f64, -1000.0)
        } else {
            (1e200_f64, 1e-200_f64, 1000.0)
        };
        let expected = (spot.ln() - rate * ((barrier.ln() - spot.ln()).abs() / 1000.0)).exp();
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, 2.0 * rate, 1e-200).unwrap(),
            spot,
            day(2025, 1, 1).into(),
        )
        .unwrap();
        let mut t = touch(if up {
            BarrierType::UpAndIn
        } else {
            BarrierType::DownAndIn
        });
        t.barrier_level = barrier;
        t.settlement_timing = SettlementTiming::AtHit;
        let value = engine
            .price(&TouchOption::cash(t, spot).unwrap(), &context)
            .unwrap();
        assert!((value / expected - 1.0).abs() < 1e-11);
    }
}

#[test]
fn settled_binaries_delegate_extreme_digital_prices_and_strict_strike_boundaries() {
    let engine = AnalyticBinaryBarrierEngine;
    for direction in [OptionType::Call, OptionType::Put] {
        for asset in [false, true] {
            for history in [BarrierTouchState::Touched, BarrierTouchState::Untouched] {
                let mut t = terms(if history == BarrierTouchState::Touched {
                    BarrierType::DownAndIn
                } else {
                    BarrierType::DownAndOut
                });
                t.option_type = direction;
                t.barrier_level = 50.0;
                t.observation_mode = ObservationMode::Scheduled;
                t.observation_dates = vec![t.effective_date];
                t.touch_state = Some(history);
                for sigma in [1e155, 0.5] {
                    let mut t = t.clone();
                    if sigma == 0.5 {
                        t.strike = 1e9;
                    }
                    let context = PricingContext::new(
                        BlackScholesMertonParameters::new(0.0, 0.0, sigma).unwrap(),
                        if sigma == 0.5 { 1.0 } else { 100.0 },
                        day(2025, 1, 2).into(),
                    )
                    .unwrap();
                    let digital = if asset {
                        DigitalOption::asset_or_nothing(
                            direction,
                            t.strike,
                            t.effective_date,
                            t.expiry_date,
                        )
                    } else {
                        DigitalOption::cash_or_nothing(
                            direction,
                            t.strike,
                            1e305,
                            t.effective_date,
                            t.expiry_date,
                        )
                    }
                    .unwrap();
                    let option = if asset {
                        BinaryBarrierOption::asset(t)
                    } else {
                        BinaryBarrierOption::cash(t, 1e305)
                    }
                    .unwrap();
                    assert_eq!(
                        engine.price(&option, &context).unwrap(),
                        AnalyticDigitalEngine.price(&digital, &context).unwrap()
                    );
                }
                let option = if asset {
                    BinaryBarrierOption::asset(t)
                } else {
                    BinaryBarrierOption::cash(t, 7.0)
                }
                .unwrap();
                for spot in [99.0, 100.0, 101.0] {
                    let exercised = if direction == OptionType::Call {
                        spot > 100.0
                    } else {
                        spot < 100.0
                    };
                    let expected = if exercised {
                        if asset { spot } else { 7.0 }
                    } else {
                        0.0
                    };
                    assert_eq!(
                        engine
                            .price(&option, &market(spot, option.expiry_date()))
                            .unwrap(),
                        expected
                    );
                }
            }
        }
    }
}
