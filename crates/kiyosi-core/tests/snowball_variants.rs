use kiyosi_core::*;

fn date(month: u32, day: u32) -> Date {
    Date::new(2025, month, day).unwrap()
}
fn market(spot: f64, valuation: Date) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation.into(),
    )
    .unwrap()
}
fn observations() -> Vec<AutocallableObservation> {
    autocallable_observations(&[date(4, 1)], &[110.0], Some(&[0.08]), None).unwrap()
}

#[test]
fn binary_has_no_knock_in_and_pays_signed_flat_maturity_coupon() {
    let mut terms = BinarySnowballTerms::new(-0.02, date(1, 1), date(4, 1), observations());
    terms.principal_ratio = 0.5;
    let note = BinarySnowballOption::new(terms).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    // No historical monitoring before the first KO, even after the effective date.
    assert!(
        engine
            .price(&note, &market(80.0, date(2, 3)))
            .unwrap()
            .is_finite()
    );
    for spot in [1.0, 80.0, 110.0] {
        let expected = 0.5 + if spot >= 110.0 { 0.08 } else { -0.02 } * 90.0 / 365.0;
        assert_eq!(
            engine.price(&note, &market(spot, date(4, 1))).unwrap(),
            expected
        );
    }
    assert!(matches!(
        note.replace(Some(AutocallableBarrierState::KnockedIn))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(note.barrier_state(), None);
}

#[test]
fn ternary_strict_knock_in_pays_floor_and_rejects_floor_above_coupon() {
    let mut terms =
        TernarySnowballTerms::new(0.06, -0.02, 80.0, date(1, 1), date(4, 1), observations());
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.principal_ratio = 0.5;
    let note = TernarySnowballOption::new(terms.clone()).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    for (spot, coupon) in [(79.999, -0.02), (80.0, 0.06), (109.0, 0.06), (110.0, 0.08)] {
        assert_eq!(
            engine.price(&note, &market(spot, date(4, 1))).unwrap(),
            0.5 + coupon * 90.0 / 365.0
        );
    }
    terms.minimum_coupon_rate = 0.07;
    assert!(matches!(
        TernarySnowballOption::new(terms).unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn every_preset_expands_to_explicit_validated_terms_and_prices() {
    let dates = vec![date(2, 3), date(3, 3), date(4, 1)];
    let base = SnowballPresetTerms::new(
        0.08,
        100.0,
        80.0,
        110.0,
        date(1, 1),
        date(4, 1),
        dates.clone(),
    );
    let cases = [
        (
            SnowballPreset::Standard,
            [110.0; 3],
            [0.08; 3],
            0.08,
            100.0,
            0.0,
            KnockInObservationMode::EveryTradingDay,
        ),
        (
            SnowballPreset::European,
            [110.0; 3],
            [0.08; 3],
            0.08,
            100.0,
            0.0,
            KnockInObservationMode::AtExpiry,
        ),
        (
            SnowballPreset::StepDown {
                knock_out_level_decrement: 2.0,
            },
            [110.0, 108.0, 106.0],
            [0.08; 3],
            0.08,
            100.0,
            0.0,
            KnockInObservationMode::EveryTradingDay,
        ),
        (
            SnowballPreset::BothDown {
                coupon_rate_decrement: 0.01,
                knock_out_level_decrement: 2.0,
            },
            [110.0, 108.0, 106.0],
            [0.08, 0.07, 0.06],
            0.06,
            100.0,
            0.0,
            KnockInObservationMode::EveryTradingDay,
        ),
        (
            SnowballPreset::DualCoupon {
                maturity_coupon_rate: -0.02,
            },
            [110.0; 3],
            [0.08; 3],
            -0.02,
            100.0,
            0.0,
            KnockInObservationMode::EveryTradingDay,
        ),
        (
            SnowballPreset::Parachute {
                final_knock_out_level: 95.0,
            },
            [110.0, 110.0, 95.0],
            [0.08; 3],
            0.08,
            100.0,
            0.0,
            KnockInObservationMode::EveryTradingDay,
        ),
        (
            SnowballPreset::Otm { upper_strike: 90.0 },
            [110.0; 3],
            [0.08; 3],
            0.08,
            90.0,
            0.0,
            KnockInObservationMode::EveryTradingDay,
        ),
        (
            SnowballPreset::LossCapped { lower_strike: 60.0 },
            [110.0; 3],
            [0.08; 3],
            0.08,
            100.0,
            60.0,
            KnockInObservationMode::EveryTradingDay,
        ),
    ];
    let engine = QuadratureAutocallableEngine::default();
    for (preset, levels, coupons, maturity, upper, lower, mode) in cases {
        let mut explicit = SnowballTerms::new(
            maturity,
            100.0,
            80.0,
            upper,
            lower,
            date(1, 1),
            date(4, 1),
            autocallable_observations(&dates, &levels, Some(&coupons), None).unwrap(),
        );
        explicit.knock_in_observation_mode = mode;
        let expected = SnowballOption::new(explicit).unwrap();
        let actual = base.clone().expand(preset).unwrap();
        for (a, b) in actual.observations().iter().zip(expected.observations()) {
            assert_eq!(a.date, b.date);
            assert!((a.knock_out_level - b.knock_out_level).abs() < 1e-14);
            assert!((a.coupon_rate - b.coupon_rate).abs() < 1e-14);
        }
        assert_eq!(actual.terms().knock_in_observation_mode, mode);
        assert!((actual.terms().maturity_coupon_rate - maturity).abs() < 1e-14);
        assert_eq!(actual.terms().upper_strike, upper);
        assert_eq!(actual.terms().lower_strike, lower);
        let context = market(100.0, date(1, 1));
        assert!(
            (engine.price(&actual, &context).unwrap() - engine.price(&expected, &context).unwrap())
                .abs()
                < 1e-12
        );
    }
}

fn probability_below(spot: f64, level: f64, time: f64) -> f64 {
    let z =
        ((level / spot).ln() - (0.04 - 0.01 - 0.3_f64.powi(2) / 2.0) * time) / (0.3 * time.sqrt());
    0.5 * libm::erfc(-z / std::f64::consts::SQRT_2)
}

#[test]
fn fixed_coupon_variants_match_independent_terminal_digital_portfolios() {
    let engine = QuadratureAutocallableEngine::default();
    let time = 90.0 / 365.0;
    for spot in [60.0, 80.0, 100.0, 120.0] {
        for coupon in [-0.2, 0.0, 0.08] {
            let records =
                autocallable_observations(&[date(4, 1)], &[110.0], Some(&[coupon]), None).unwrap();
            let mut binary =
                BinarySnowballTerms::new(0.06, date(1, 1), date(4, 1), records.clone());
            binary.principal_ratio = 0.25;
            let mut ternary =
                TernarySnowballTerms::new(0.06, -0.02, 80.0, date(1, 1), date(4, 1), records);
            ternary.principal_ratio = 0.25;
            ternary.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
            let context = market(spot, date(1, 1));
            let above = 1.0 - probability_below(spot, 110.0, time);
            let expected_binary =
                (-0.04 * time).exp() * (0.25 + time * (0.06 + (coupon - 0.06) * above));
            let expected_ternary = expected_binary
                - (-0.04 * time).exp() * time * 0.08 * probability_below(spot, 80.0, time);
            assert!(
                (engine
                    .price(&BinarySnowballOption::new(binary).unwrap(), &context)
                    .unwrap()
                    - expected_binary)
                    .abs()
                    < 1e-12
            );
            assert!(
                (engine
                    .price(&TernarySnowballOption::new(ternary).unwrap(), &context)
                    .unwrap()
                    - expected_ternary)
                    .abs()
                    < 1e-12
            );
        }
    }
}

#[test]
fn fixed_variants_keep_history_analysis_and_failure_guarantees() {
    fn check<T: AutocallableTerms + std::fmt::Debug>(note: AutocallableNote<T>) {
        let engine = QuadratureAutocallableEngine::default();
        let context = market(103.0, date(1, 1));
        let snapshot = note.clone();
        let result = engine
            .price_with_greeks(
                &note,
                &context,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    spot_shift: 0.25,
                    ..Default::default()
                },
            )
            .unwrap();
        for greek in Greek::ALL {
            assert!(result.has(greek), "{greek:?}");
        }
        let solved = implied_volatility(
            &engine,
            &note,
            &context,
            result.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.2,
                upper_bound: 0.4,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((solved - 0.3).abs() < 1e-6);
        assert!(matches!(
            implied_volatility(&engine, &note, &context, 10.0, Default::default()).unwrap_err(),
            Error::RootNotBracketed(_)
        ));
        let missing = note.replace(None).unwrap();
        assert!(matches!(
            engine
                .price(&missing, &market(100.0, date(4, 2)))
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        let settled = note
            .replace(Some(AutocallableBarrierState::KnockedOut))
            .unwrap();
        assert!(matches!(
            engine.price(&settled, &context).unwrap_err(),
            Error::InvalidInput(_)
        ));
        let at_expiry = engine
            .price_with_greeks(
                &note,
                &market(110.0, date(4, 1)),
                GreeksRequest::all(),
                Default::default(),
            )
            .unwrap();
        for greek in Greek::ALL {
            assert!(!at_expiry.has(greek));
        }
        assert_eq!(note, snapshot);
        assert_eq!(context.model_parameters().volatility(), 0.3);
    }
    check(
        BinarySnowballOption::new(BinarySnowballTerms::new(
            0.06,
            date(1, 1),
            date(4, 1),
            observations(),
        ))
        .unwrap(),
    );
    let mut terms =
        TernarySnowballTerms::new(0.06, -0.02, 80.0, date(1, 1), date(4, 1), observations());
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    check(TernarySnowballOption::new(terms).unwrap());
}

#[test]
fn ternary_daily_layers_converge_and_retain_recovered_knock_in_history() {
    let mut terms = TernarySnowballTerms::new(
        0.06,
        -0.02,
        95.0,
        date(1, 1),
        date(4, 1),
        autocallable_observations(
            &[date(2, 3), date(3, 3), date(4, 1)],
            &[110.0, 108.0, 105.0],
            Some(&[0.08; 3]),
            None,
        )
        .unwrap(),
    );
    let context = market(100.0, date(1, 1));
    let mut modes = Vec::new();
    for mode in [
        KnockInObservationMode::AtExpiry,
        KnockInObservationMode::EveryTradingDay,
    ] {
        terms.knock_in_observation_mode = mode;
        let note = TernarySnowballOption::new(terms.clone()).unwrap();
        let prices: Vec<_> = [(512, 8.0), (1024, 8.0), (1024, 10.0)]
            .into_iter()
            .map(|(n, e)| {
                QuadratureAutocallableEngine::new(QuadratureSettings {
                    asset_step_count: n,
                    integration_standard_deviations: e,
                })
                .unwrap()
                .price(&note, &context)
                .unwrap()
            })
            .collect();
        assert!((prices[0] - prices[1]).abs() < 1e-4, "{prices:?}");
        assert!((prices[1] - prices[2]).abs() < 1e-4, "{prices:?}");
        modes.push(prices[0]);
    }
    assert!(modes[1] < modes[0] - 0.001);
    let note = TernarySnowballOption::new(terms).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    assert!(matches!(
        engine.price(&note, &market(100.0, date(1, 2))).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let known = note
        .replace(Some(AutocallableBarrierState::KnockedIn))
        .unwrap();
    assert_eq!(
        engine.price(&known, &market(100.0, date(4, 1))).unwrap(),
        1.0 - 0.02 * 90.0 / 365.0
    );
}

#[test]
fn preset_validation_uses_expanded_terms_and_preserves_inputs() {
    let base = SnowballPresetTerms::new(
        0.08,
        100.0,
        80.0,
        110.0,
        date(1, 1),
        date(4, 1),
        vec![date(2, 3), date(4, 1)],
    );
    let snapshot = base.clone();
    for preset in [
        SnowballPreset::StepDown {
            knock_out_level_decrement: 110.0,
        },
        SnowballPreset::BothDown {
            coupon_rate_decrement: f64::INFINITY,
            knock_out_level_decrement: 1.0,
        },
        SnowballPreset::DualCoupon {
            maturity_coupon_rate: f64::NAN,
        },
        SnowballPreset::Parachute {
            final_knock_out_level: 0.0,
        },
        SnowballPreset::Otm { upper_strike: -1.0 },
        SnowballPreset::LossCapped {
            lower_strike: 101.0,
        },
    ] {
        assert!(matches!(
            base.clone().expand(preset).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert_eq!(base, snapshot);
    }
    let mut unordered = base.clone();
    unordered.observation_dates.reverse();
    assert!(matches!(
        unordered.expand(SnowballPreset::Standard).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut configured = base;
    configured.barrier_state = Some(AutocallableBarrierState::KnockedIn);
    configured.principal_ratio = 0.5;
    let note = configured.expand(SnowballPreset::Standard).unwrap();
    assert_eq!(note.terms().principal_ratio, 0.5);
    assert_eq!(
        note.barrier_state(),
        Some(AutocallableBarrierState::KnockedIn)
    );
}

#[test]
fn fixed_variants_validate_numeric_schedule_and_history_inputs_at_construction() {
    for value in [f64::NAN, f64::INFINITY] {
        let binary = BinarySnowballTerms::new(value, date(1, 1), date(4, 1), observations());
        assert!(matches!(
            BinarySnowballOption::new(binary).unwrap_err(),
            Error::InvalidInput(_)
        ));
        let ternary =
            TernarySnowballTerms::new(0.06, value, 80.0, date(1, 1), date(4, 1), observations());
        assert!(matches!(
            TernarySnowballOption::new(ternary).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for bad in [
        Vec::new(),
        vec![AutocallableObservation::new(date(4, 2), 110.0)],
        vec![AutocallableObservation::new(date(4, 1), 110.0); 2],
    ] {
        assert!(matches!(
            BinarySnowballOption::new(BinarySnowballTerms::new(
                0.06,
                date(1, 1),
                date(4, 1),
                bad.clone()
            ))
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            TernarySnowballOption::new(TernarySnowballTerms::new(
                0.06,
                0.02,
                80.0,
                date(1, 1),
                date(4, 1),
                bad
            ))
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    let mut binary = BinarySnowballTerms::new(0.06, date(1, 1), date(4, 1), observations());
    binary.principal_ratio = -1.0;
    assert!(matches!(
        BinarySnowballOption::new(binary).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut ternary =
        TernarySnowballTerms::new(0.06, 0.02, 0.0, date(1, 1), date(4, 1), Vec::new());
    // Schedule validation precedes knock-in validation, as in the legacy contract.
    assert!(matches!(
        TernarySnowballOption::new(ternary.clone()).unwrap_err(),
        Error::InvalidInput(_)
    ));
    ternary.observations = observations();
    assert!(matches!(
        TernarySnowballOption::new(ternary).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut binary = BinarySnowballTerms::new(0.06, date(1, 1), date(4, 1), observations());
    binary.observations[0].coupon_barrier_level = 1.0;
    assert!(matches!(
        BinarySnowballOption::new(binary).unwrap_err(),
        Error::InvalidInput(_)
    ));
}
