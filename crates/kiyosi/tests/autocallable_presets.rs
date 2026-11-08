mod support;
use support::dated;

use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}

fn market(spot: f64, valuation: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation,
    )
    .unwrap()
}

fn phoenix_terms() -> PhoenixPresetTerms {
    PhoenixPresetTerms::new(
        0.01,
        100.0,
        80.0,
        110.0,
        date(1, 1),
        date(4, 1),
        vec![date(2, 3), date(4, 1)],
    )
}

#[test]
fn fcn_dcn_expand_to_owned_phoenix_terms_and_pay_fixed_coupons_at_boundaries() {
    let engine = QuadratureAutocallableEngine::default();
    for (preset, barrier) in [
        (PhoenixPreset::FixedCouponNote, 0.0),
        (
            PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level: 90.0,
            },
            90.0,
        ),
    ] {
        for coupon in [0.01, -0.01] {
            let mut input = phoenix_terms();
            input.coupon_per_period = coupon;
            input.principal_redemption_ratio = 0.25;
            input.barrier_state = Some(AutocallableBarrierState::NoEvent);
            input.history_time = Some(input.effective_time);
            let note = input.expand(preset).unwrap();
            let mut expected = PhoenixTerms::new(
                PhoenixCoupon::PerPeriod(coupon),
                100.0,
                80.0,
                80.0,
                0.0,
                date(1, 1),
                date(4, 1),
                autocallable_observations(
                    &[date(2, 3), date(4, 1)],
                    &[110.0; 2],
                    None,
                    Some(&[barrier; 2]),
                )
                .unwrap(),
                Some((date(4, 1)).time_of_day()),
            );
            expected.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
            expected.observation_clock_time = None;
            expected.barrier_state = Some(AutocallableBarrierState::NoEvent);
            expected.history_time = Some(expected.effective_time);
            expected.principal_redemption_ratio = 0.25;
            assert_eq!(note, PhoenixOption::new(expected).unwrap());
            for (spot, base) in [
                (70.0, 0.15),
                (80.0, 0.25),
                (89.999, 0.25),
                (90.0, 0.25),
                (110.0, 0.25),
            ] {
                let expected = base + if spot >= barrier { coupon } else { 0.0 };
                assert!(
                    (engine
                        .price(
                            &dated(&note, &market(spot, date(4, 1))),
                            &market(spot, date(4, 1))
                        )
                        .unwrap()
                        - expected)
                        .abs()
                        < 1e-14
                );
            }
            // The shorter first period pays the same coupon, including on knock-out.
            assert_eq!(
                engine
                    .price(
                        &dated(&note, &market(110.0, date(2, 3))),
                        &market(110.0, date(2, 3))
                    )
                    .unwrap(),
                0.25 + coupon
            );
            let changed = note
                .replace(PhoenixOptionChanges {
                    coupon: Some(PhoenixCoupon::PerPeriod(0.02)),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(changed.terms().coupon, PhoenixCoupon::PerPeriod(0.02));
            assert_eq!(note.terms().coupon, PhoenixCoupon::PerPeriod(coupon));
            assert!(matches!(
                note.replace(PhoenixOptionChanges {
                    coupon: Some(PhoenixCoupon::PerPeriod(f64::NAN)),
                    ..Default::default()
                }),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(
                note.replace_history(None, None).unwrap().terms().coupon,
                note.terms().coupon
            );
        }
    }
    let mut input = phoenix_terms();
    input.knock_out_level = 85.0;
    input.barrier_state = Some(AutocallableBarrierState::NoEvent);
    input.history_time = Some(input.effective_time);
    let note = input
        .expand(PhoenixPreset::DigitalCouponNote {
            coupon_barrier_level: 90.0,
        })
        .unwrap();
    assert_eq!(
        engine
            .price(
                &dated(&note, &market(85.0, date(2, 3))),
                &market(85.0, date(2, 3))
            )
            .unwrap(),
        1.0
    );
}

#[test]
fn fixed_period_cashflows_exclude_settled_coupons_and_preserve_annual_accrual() {
    let quad = QuadratureAutocallableEngine::default();
    let fd = FiniteDifferenceAutocallableEngine::default();
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 4096,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for (preset, spot, total, remaining) in [
        (PhoenixPreset::FixedCouponNote, 100.0, 1.02, 1.01),
        (PhoenixPreset::FixedCouponNote, 70.0, 0.92, 0.91),
        (
            PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level: 90.0,
            },
            100.0,
            1.02,
            1.01,
        ),
        (
            PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level: 90.0,
            },
            85.0,
            1.0,
            1.0,
        ),
    ] {
        let note = phoenix_terms()
            .expand(preset)
            .unwrap()
            .replace_history(
                Some(AutocallableBarrierState::NoEvent),
                Some((phoenix_terms()).effective_time),
            )
            .unwrap();
        for (day, expected) in [(date(1, 1), total), (date(2, 4), remaining)] {
            // Positive near-zero volatility keeps these far-from-barrier payoffs effectively deterministic.
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(0.0, 0.0, 1e-8).unwrap(),
                spot,
                day,
            )
            .unwrap();
            for value in [
                quad.price(&dated(&note, &context), &context).unwrap(),
                fd.price(&dated(&note, &context), &context).unwrap(),
                mc.price(&dated(&note, &context), &context).unwrap(),
            ] {
                assert!((value - expected).abs() < 1e-9);
            }
        }
    }
    let fixed = phoenix_terms()
        .expand(PhoenixPreset::FixedCouponNote)
        .unwrap()
        .replace_history(
            Some(AutocallableBarrierState::NoEvent),
            Some((phoenix_terms()).effective_time),
        )
        .unwrap();
    let mut terms = fixed.terms().clone();
    terms.coupon = PhoenixCoupon::Annual(0.365);
    let annual = PhoenixOption::new(terms).unwrap();
    let context = market(110.0, date(2, 3));
    assert_eq!(
        quad.price(&dated(&fixed, &context), &context).unwrap(),
        1.01
    );
    assert_eq!(
        quad.price(&dated(&annual, &context), &context).unwrap(),
        1.0 + 0.365 * 33.0 / 365.0
    );
}

#[test]
fn fixed_period_coupons_match_independent_put_and_digital_values_in_all_engines() {
    let context = market(100.0, date(1, 1));
    let end = date(4, 1);
    let discount = (-0.04_f64 * 90.0 / 365.0).exp();
    let put = EuropeanOption::new(OptionType::Put, 80.0, date(1, 1), end).unwrap();
    let put_price = AnalyticEuropeanEngine
        .price(&dated(&put, &context), &context)
        .unwrap();
    let quad = QuadratureAutocallableEngine::default();
    let fd = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 1000,
        ..Default::default()
    })
    .unwrap();
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for preset in [
        PhoenixPreset::FixedCouponNote,
        PhoenixPreset::DigitalCouponNote {
            coupon_barrier_level: 90.0,
        },
    ] {
        let mut input = phoenix_terms();
        input.observation_times = vec![end];
        let note = input.expand(preset).unwrap();
        let coupon_value = match preset {
            PhoenixPreset::FixedCouponNote => 0.01 * discount,
            PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level,
            } => {
                let digital = DigitalOption::cash_or_nothing(
                    OptionType::Call,
                    coupon_barrier_level,
                    0.01,
                    date(1, 1),
                    end,
                )
                .unwrap();
                AnalyticDigitalEngine
                    .price(&dated(&digital, &context), &context)
                    .unwrap()
            }
        };
        let expected = discount - put_price / 100.0 + coupon_value;
        assert!((quad.price(&dated(&note, &context), &context).unwrap() - expected).abs() < 1e-12);
        assert!((fd.price(&dated(&note, &context), &context).unwrap() - expected).abs() < 3e-5);
        let estimate = mc
            .price_with_statistics(&dated(&note, &context), &context)
            .unwrap();
        assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap());
        for actual in [
            implied_phoenix_coupon(
                &quad,
                &dated(&note, &context),
                &context,
                quad.price(
                    &dated(
                        &note
                            .replace(PhoenixOptionChanges {
                                coupon: Some(PhoenixCoupon::PerPeriod(-0.02)),
                                ..Default::default()
                            })
                            .unwrap(),
                        &context,
                    ),
                    &context,
                )
                .unwrap(),
                ImpliedCouponSettings {
                    lower_bound: -0.05,
                    upper_bound: 0.05,
                    ..Default::default()
                },
            )
            .unwrap(),
            implied_phoenix_coupon(
                &fd,
                &dated(&note, &context),
                &context,
                fd.price(
                    &dated(
                        &note
                            .replace(PhoenixOptionChanges {
                                coupon: Some(PhoenixCoupon::PerPeriod(-0.02)),
                                ..Default::default()
                            })
                            .unwrap(),
                        &context,
                    ),
                    &context,
                )
                .unwrap(),
                ImpliedCouponSettings {
                    lower_bound: -0.05,
                    upper_bound: 0.05,
                    ..Default::default()
                },
            )
            .unwrap(),
            implied_phoenix_coupon(
                &mc,
                &dated(&note, &context),
                &context,
                mc.price(
                    &dated(
                        &note
                            .replace(PhoenixOptionChanges {
                                coupon: Some(PhoenixCoupon::PerPeriod(-0.02)),
                                ..Default::default()
                            })
                            .unwrap(),
                        &context,
                    ),
                    &context,
                )
                .unwrap(),
                ImpliedCouponSettings {
                    lower_bound: -0.05,
                    upper_bound: 0.05,
                    ..Default::default()
                },
            )
            .unwrap(),
        ] {
            let PhoenixCoupon::PerPeriod(value) = actual else {
                panic!("expected a per-period coupon")
            };
            assert!((value + 0.02).abs() < 1e-7);
        }
    }
}

#[test]
fn stage_cutoffs_are_inclusive_and_coupon_uses_the_whole_holding_period() {
    let dates = vec![date(2, 3), date(2, 4), date(3, 3), date(3, 4), date(4, 1)];
    let base = SnowballPresetTerms::new(
        0.2,
        100.0,
        80.0,
        110.0,
        date(1, 1),
        date(4, 1),
        dates.clone(),
        Some((date(4, 1)).time_of_day()),
    );
    for (preset, rates, maturity) in [
        (
            SnowballPreset::EarlyProfit {
                first_stage_end_time: date(2, 3),
                second_coupon_rate: 0.05,
            },
            [0.2, 0.05, 0.05, 0.05, 0.05],
            0.05,
        ),
        (
            SnowballPreset::Butterfly {
                first_stage_end_time: date(2, 3),
                second_coupon_rate: 0.05,
                second_stage_end_time: date(3, 3),
                third_coupon_rate: -0.02,
            },
            [0.2, 0.05, 0.05, -0.02, -0.02],
            -0.02,
        ),
    ] {
        let note = base
            .clone()
            .expand(preset)
            .unwrap()
            .replace_history(Some(AutocallableBarrierState::NoEvent), Some(date(1, 1)))
            .unwrap();
        assert_eq!(
            note.observations()
                .iter()
                .map(|r| r.coupon_rate().unwrap())
                .collect::<Vec<_>>(),
            rates
        );
        let mut expected = SnowballTerms::new(
            maturity,
            100.0,
            80.0,
            100.0,
            0.0,
            date(1, 1),
            date(4, 1),
            autocallable_observations(&dates, &[110.0; 5], Some(&rates), None).unwrap(),
            Some((date(4, 1)).time_of_day()),
        );
        expected.barrier_state = Some(AutocallableBarrierState::NoEvent);
        expected.history_time = Some(expected.effective_time);
        assert_eq!(note, SnowballOption::new(expected).unwrap());
        for ((&observation, &rate), days) in
            dates.iter().zip(&rates).zip([33.0, 34.0, 61.0, 62.0, 90.0])
        {
            let years = days / 365.0;
            let expected = 1.0 + rate * years;
            assert_eq!(
                QuadratureAutocallableEngine::default()
                    .price(
                        &dated(&note, &market(110.0, observation)),
                        &market(110.0, observation)
                    )
                    .unwrap(),
                expected
            );
            assert_eq!(
                FiniteDifferenceAutocallableEngine::default()
                    .price(
                        &dated(&note, &market(110.0, observation)),
                        &market(110.0, observation)
                    )
                    .unwrap(),
                expected
            );
            assert_eq!(
                MonteCarloAutocallableEngine::default()
                    .price(
                        &dated(&note, &market(110.0, observation)),
                        &market(110.0, observation)
                    )
                    .unwrap(),
                expected
            );
        }
        assert_eq!(
            QuadratureAutocallableEngine::default()
                .price(
                    &dated(&note, &market(100.0, date(4, 1))),
                    &market(100.0, date(4, 1))
                )
                .unwrap(),
            1.0 + maturity * 90.0 / 365.0
        );
    }
}

#[test]
fn stage_validation_checks_unused_rates_and_allows_empty_stages_and_non_observation_cutoffs() {
    let base = SnowballPresetTerms::new(
        0.2,
        100.0,
        80.0,
        110.0,
        date(1, 1),
        date(4, 1),
        vec![date(4, 1)],
        Some((date(4, 1)).time_of_day()),
    );
    for cutoff in [date(1, 1), date(2, 1)] {
        let note = base
            .clone()
            .expand(SnowballPreset::EarlyProfit {
                first_stage_end_time: cutoff,
                second_coupon_rate: -0.02,
            })
            .unwrap();
        assert_eq!(note.observations()[0].coupon_rate(), Some(-0.02));
    }
    let mut increasing = base.clone();
    increasing.coupon_rate = -0.02;
    let note = increasing
        .expand(SnowballPreset::Butterfly {
            first_stage_end_time: date(2, 1),
            second_coupon_rate: 0.0,
            second_stage_end_time: date(3, 1),
            third_coupon_rate: 0.2,
        })
        .unwrap();
    assert_eq!(note.terms().maturity_coupon_rate, 0.2);
    assert_eq!(note.observations()[0].coupon_rate(), Some(0.2));
    for cutoff in [
        Date::new(2024, 12, 31).unwrap().start_of_day(),
        date(4, 1),
        date(4, 2),
    ] {
        assert!(matches!(
            base.clone().expand(SnowballPreset::EarlyProfit {
                first_stage_end_time: cutoff,
                second_coupon_rate: 0.05
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    for second in [date(1, 31), date(2, 1), date(4, 1)] {
        assert!(matches!(
            base.clone().expand(SnowballPreset::Butterfly {
                first_stage_end_time: date(2, 1),
                second_coupon_rate: 0.05,
                second_stage_end_time: second,
                third_coupon_rate: 0.02
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut input = base.clone();
        input.coupon_rate = rate;
        assert!(matches!(
            input.expand(SnowballPreset::EarlyProfit {
                first_stage_end_time: date(2, 1),
                second_coupon_rate: 0.05
            }),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            base.clone().expand(SnowballPreset::Butterfly {
                first_stage_end_time: date(2, 1),
                second_coupon_rate: rate,
                second_stage_end_time: date(3, 1),
                third_coupon_rate: 0.02
            }),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            base.clone().expand(SnowballPreset::EarlyProfit {
                first_stage_end_time: date(2, 1),
                second_coupon_rate: rate
            }),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn phoenix_presets_reuse_core_validation_and_history_defaults() {
    for coupon in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut input = phoenix_terms();
        input.coupon_per_period = coupon;
        assert!(matches!(
            input.expand(PhoenixPreset::FixedCouponNote),
            Err(Error::InvalidInput(_))
        ));
    }
    for barrier in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            phoenix_terms().expand(PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level: barrier
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    for dates in [
        vec![],
        vec![date(4, 1), date(2, 3)],
        vec![date(2, 3); 2],
        vec![date(4, 2)],
    ] {
        let mut input = phoenix_terms();
        input.observation_times = dates;
        assert!(matches!(
            input.expand(PhoenixPreset::FixedCouponNote),
            Err(Error::InvalidInput(_))
        ));
    }
    let note = phoenix_terms()
        .expand(PhoenixPreset::FixedCouponNote)
        .unwrap();
    assert_eq!(note.barrier_state(), None);
    assert_eq!(note.terms().principal_redemption_ratio, 1.0);
    assert!(matches!(
        QuadratureAutocallableEngine::default().price(
            &dated(&note, &market(100.0, date(3, 3))),
            &market(100.0, date(3, 3))
        ),
        Err(Error::MissingHistory(_))
    ));
    let settled = note
        .replace_history(
            Some(AutocallableBarrierState::KnockedOut),
            Some(note.history_time().unwrap_or(note.effective_time())),
        )
        .unwrap();
    assert_eq!(
        QuadratureAutocallableEngine::default()
            .price(
                &dated(&settled, &market(100.0, date(3, 3))),
                &market(100.0, date(3, 3))
            )
            .unwrap(),
        0.0
    );
}
