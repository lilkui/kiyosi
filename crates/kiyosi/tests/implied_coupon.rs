use kiyosi::*;

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

fn binary() -> BinarySnowballOption {
    BinarySnowballOption::new(BinarySnowballTerms::new(
        0.06,
        date(1, 1),
        date(4, 1),
        autocallable_observations(
            &[date(2, 3), date(4, 1)],
            &[110.0, 105.0],
            Some(&[0.08, 0.12]),
            None,
        )
        .unwrap(),
    ))
    .unwrap()
}

#[test]
fn binary_quote_recovers_schedule_shift_and_maturity_conventions() {
    let engine = QuadratureAutocallableEngine::default();
    let context = market(100.0, date(1, 1));
    let note = binary();
    let original = note.clone();
    for convention in [
        CouponQuoteConvention::ShiftMaturityCoupon,
        CouponQuoteConvention::PreserveMaturityCoupon,
    ] {
        let mut terms = note.terms().clone();
        terms.observations[0].coupon_rate = 0.18;
        terms.observations[1].coupon_rate = 0.22;
        if convention == CouponQuoteConvention::ShiftMaturityCoupon {
            terms.maturity_coupon_rate = 0.16;
        }
        let target = BinarySnowballOption::new(terms).unwrap();
        let quote = engine.price(&target, &context).unwrap();
        let solved = implied_snowball_coupon(
            &engine,
            &note,
            &context,
            quote,
            convention,
            Default::default(),
        )
        .unwrap();
        assert!((solved - 0.18).abs() < 1e-7, "{solved}");
        let replaced = note.with_coupon(solved, convention).unwrap();
        assert!((engine.price(&replaced, &context).unwrap() - quote).abs() < 1e-8);
    }
    assert_eq!(note, original);
}

#[test]
fn all_products_recover_prices_with_daily_knock_in_and_signed_coupons() {
    let engine = QuadratureAutocallableEngine::new(QuadratureSettings {
        asset_step_count: 64,
        ..Default::default()
    })
    .unwrap();
    let context = market(100.0, date(1, 1));
    let records = binary().observations().to_vec();
    let snowball = SnowballOption::new(SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        records.clone(),
    ))
    .unwrap();
    let ternary = TernarySnowballOption::new(TernarySnowballTerms::new(
        0.06,
        -0.2,
        80.0,
        date(1, 1),
        date(4, 1),
        records,
    ))
    .unwrap();
    fn recover<T: SnowballCouponTerms + std::fmt::Debug>(
        engine: &QuadratureAutocallableEngine,
        note: &AutocallableNote<T>,
        context: &PricingContext,
    ) {
        let original = note.clone();
        for convention in [
            CouponQuoteConvention::ShiftMaturityCoupon,
            CouponQuoteConvention::PreserveMaturityCoupon,
        ] {
            for coupon in [-0.05, 0.18] {
                let target = note.with_coupon(coupon, convention).unwrap();
                let quote = engine.price(&target, context).unwrap();
                let settings = ImpliedCouponSettings {
                    lower_bound: -0.1,
                    upper_bound: 0.3,
                    price_tolerance: 1e-11,
                    ..Default::default()
                };
                let solved =
                    implied_snowball_coupon(engine, note, context, quote, convention, settings)
                        .unwrap();
                assert!((solved - coupon).abs() < 1e-8, "{solved}");
                assert!(
                    (engine
                        .price(&note.with_coupon(solved, convention).unwrap(), context)
                        .unwrap()
                        - quote)
                        .abs()
                        < 1e-11
                );
                assert!(
                    (target.observations()[1].coupon_rate
                        - target.observations()[0].coupon_rate
                        - 0.04)
                        .abs()
                        < 1e-14
                );
            }
        }
        assert_eq!(*note, original);
    }
    recover(&engine, &snowball, &context);
    recover(&engine, &binary(), &context);
    recover(&engine, &ternary, &context);
    assert_eq!(
        ternary
            .with_coupon(0.18, CouponQuoteConvention::ShiftMaturityCoupon)
            .unwrap()
            .terms()
            .minimum_coupon_rate,
        -0.2
    );

    let note = PhoenixOption::new(PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        autocallable_observations(
            &[date(2, 3), date(4, 1)],
            &[110.0, 105.0],
            None,
            Some(&[95.0, 95.0]),
        )
        .unwrap(),
    ))
    .unwrap();
    for coupon in [-0.05, 0.18] {
        let quote = engine
            .price(&note.with_coupon(coupon).unwrap(), &context)
            .unwrap();
        let solved = implied_coupon(
            &engine,
            &note,
            &context,
            quote,
            ImpliedCouponSettings {
                lower_bound: -0.1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((solved - coupon).abs() < 1e-7);
        assert!(
            (engine
                .price(&note.with_coupon(solved).unwrap(), &context)
                .unwrap()
                - quote)
                .abs()
                < 1e-8
        );
    }
    assert_eq!(note.terms().coupon, PhoenixCoupon::Annual(0.08));
}

#[test]
fn bounds_defaults_validation_and_identifiability_preserve_inputs() {
    use CouponQuoteConvention::*;
    let engine = QuadratureAutocallableEngine::default();
    let context = market(100.0, date(1, 1));
    let note = binary();
    let solve = |quote, settings| {
        implied_snowball_coupon(
            &engine,
            &note,
            &context,
            quote,
            PreserveMaturityCoupon,
            settings,
        )
    };
    let defaults = ImpliedCouponSettings::default();
    assert_eq!(
        (
            defaults.lower_bound,
            defaults.upper_bound,
            defaults.max_iterations
        ),
        (0.0, 2.0, 100)
    );
    assert_eq!(
        (defaults.price_tolerance, defaults.parameter_tolerance),
        (1e-8, 1e-8)
    );
    for bound in [defaults.lower_bound, defaults.upper_bound] {
        let quote = engine
            .price(
                &note.with_coupon(bound, PreserveMaturityCoupon).unwrap(),
                &context,
            )
            .unwrap();
        assert_eq!(solve(quote, defaults).unwrap(), bound);
    }
    for settings in [
        ImpliedCouponSettings {
            lower_bound: f64::NAN,
            ..defaults
        },
        ImpliedCouponSettings {
            upper_bound: f64::INFINITY,
            ..defaults
        },
        ImpliedCouponSettings {
            lower_bound: 2.0,
            ..defaults
        },
        ImpliedCouponSettings {
            lower_bound: 3.0,
            ..defaults
        },
        ImpliedCouponSettings {
            price_tolerance: 0.0,
            ..defaults
        },
        ImpliedCouponSettings {
            price_tolerance: f64::NAN,
            ..defaults
        },
        ImpliedCouponSettings {
            parameter_tolerance: -1.0,
            ..defaults
        },
        ImpliedCouponSettings {
            parameter_tolerance: f64::INFINITY,
            ..defaults
        },
        ImpliedCouponSettings {
            max_iterations: 0,
            ..defaults
        },
    ] {
        assert!(matches!(
            solve(1.0, settings).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for quote in [f64::NAN, f64::INFINITY] {
        assert!(matches!(
            solve(quote, defaults).unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    assert!(matches!(
        solve(10.0, defaults).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    let known = note.replace(Some(AutocallableBarrierState::None)).unwrap();
    let terminal = market(100.0, date(4, 1));
    let quote = engine.price(&known, &terminal).unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &known,
            &terminal,
            quote,
            PreserveMaturityCoupon,
            defaults
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let shifted = implied_snowball_coupon(
        &engine,
        &known,
        &terminal,
        quote,
        ShiftMaturityCoupon,
        defaults,
    )
    .unwrap();
    assert!((shifted - 0.08).abs() < 1e-8);
    let knocked_out = note
        .replace(Some(AutocallableBarrierState::KnockedOut))
        .unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &knocked_out,
            &market(100.0, date(3, 3)),
            0.0,
            ShiftMaturityCoupon,
            defaults
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &note,
            &terminal,
            f64::NAN,
            PreserveMaturityCoupon,
            defaults
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &note,
            &terminal,
            quote,
            PreserveMaturityCoupon,
            defaults
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(note, binary());
    assert_eq!(context, market(100.0, date(1, 1)));
}

#[test]
fn custom_engine_failures_and_bisection_use_stable_categories() {
    struct Curve(fn(f64) -> f64);
    impl PricingEngine<BinarySnowballOption> for Curve {
        fn price(&self, note: &BinarySnowballOption, _: &PricingContext) -> Result<f64> {
            Ok((self.0)(note.observations()[0].coupon_rate))
        }
    }
    let note = binary();
    let context = market(100.0, date(1, 1));
    let solve = |engine: &Curve, quote, settings| {
        implied_snowball_coupon(
            engine,
            &note,
            &context,
            quote,
            CouponQuoteConvention::PreserveMaturityCoupon,
            settings,
        )
    };
    let s = ImpliedCouponSettings::default();
    for curve in [
        Curve(|_| f64::NAN),
        Curve(|_| f64::INFINITY),
        Curve(|c| if c == 0.0 { -f64::MAX } else { f64::MAX }),
    ] {
        assert!(matches!(
            solve(&curve, f64::MAX, s).unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    let nonlinear = Curve(|c| c * c);
    assert!(matches!(
        solve(
            &nonlinear,
            0.3,
            ImpliedCouponSettings {
                max_iterations: 1,
                ..s
            }
        )
        .unwrap_err(),
        Error::NonConvergence(_)
    ));
    assert!((solve(&nonlinear, 0.3, s).unwrap() - 0.3_f64.sqrt()).abs() < 1e-7);
    let interval_only = ImpliedCouponSettings {
        parameter_tolerance: 2.0,
        price_tolerance: 1e-30,
        max_iterations: 1,
        ..s
    };
    assert_eq!(solve(&nonlinear, 0.3, interval_only).unwrap(), 1.0);
    let interior_nan = Curve(|c| if c > 0.0 && c < 2.0 { f64::NAN } else { c });
    assert!(matches!(
        solve(&interior_nan, 0.3, s).unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn named_presets_reuse_the_same_coupon_contract() {
    let engine = QuadratureAutocallableEngine::new(QuadratureSettings {
        asset_step_count: 64,
        ..Default::default()
    })
    .unwrap();
    let context = market(100.0, date(1, 1));
    for preset in [
        SnowballPreset::Standard,
        SnowballPreset::European,
        SnowballPreset::StepDown {
            knock_out_level_decrement: 2.0,
        },
        SnowballPreset::BothDown {
            coupon_rate_decrement: 0.01,
            knock_out_level_decrement: 2.0,
        },
        SnowballPreset::DualCoupon {
            maturity_coupon_rate: 0.06,
        },
        SnowballPreset::Parachute {
            final_knock_out_level: 95.0,
        },
        SnowballPreset::Otm { upper_strike: 90.0 },
        SnowballPreset::LossCapped { lower_strike: 60.0 },
    ] {
        let note = SnowballPresetTerms::new(
            0.08,
            100.0,
            80.0,
            110.0,
            date(1, 1),
            date(4, 1),
            vec![date(2, 3), date(4, 1)],
        )
        .expand(preset)
        .unwrap();
        for convention in [
            CouponQuoteConvention::ShiftMaturityCoupon,
            CouponQuoteConvention::PreserveMaturityCoupon,
        ] {
            let target = note.with_coupon(0.18, convention).unwrap();
            let quote = engine.price(&target, &context).unwrap();
            let solved = implied_snowball_coupon(
                &engine,
                &note,
                &context,
                quote,
                convention,
                Default::default(),
            )
            .unwrap();
            assert!((solved - 0.18).abs() < 1e-7, "{preset:?}: {solved}");
            assert!(
                (engine
                    .price(&note.with_coupon(solved, convention).unwrap(), &context)
                    .unwrap()
                    - quote)
                    .abs()
                    < 1e-8
            );
        }
    }
}

#[test]
fn terminal_quotes_match_independent_analytics_and_validate_replacements_before_pricing() {
    use CouponQuoteConvention::*;
    let engine = QuadratureAutocallableEngine::default();
    let context = market(100.0, date(1, 1));
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.06,
        date(1, 1),
        date(4, 1),
        autocallable_observations(&[date(4, 1)], &[110.0], Some(&[0.08]), None).unwrap(),
    ))
    .unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 110.0, 1.0, date(1, 1), date(4, 1))
            .unwrap();
    let term = 90.0 / 365.0;
    let quote = (-0.04_f64 * term).exp() * (1.0 + 0.06 * term)
        + (0.18 - 0.06) * term * AnalyticDigitalEngine.price(&digital, &context).unwrap();
    let solved = implied_snowball_coupon(
        &engine,
        &note,
        &context,
        quote,
        PreserveMaturityCoupon,
        Default::default(),
    )
    .unwrap();
    assert!((solved - 0.18).abs() < 1e-8);
    let mut terms = note.terms().clone();
    terms.maturity_coupon_rate = f64::MAX;
    let extreme = BinarySnowballOption::new(terms).unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &extreme,
            &market(100.0, date(4, 2)),
            1.0,
            ShiftMaturityCoupon,
            ImpliedCouponSettings {
                upper_bound: f64::MAX,
                ..Default::default()
            }
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let ternary = TernarySnowballOption::new(TernarySnowballTerms::new(
        0.06,
        0.02,
        80.0,
        date(1, 1),
        date(4, 1),
        note.observations().to_vec(),
    ))
    .unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &ternary,
            &context,
            1.0,
            ShiftMaturityCoupon,
            Default::default()
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(ternary.terms().maturity_coupon_rate, 0.06);
    let curve =
        RateTermStructure::new(date(1, 1).into(), vec![date(4, 1).into()], vec![0.99]).unwrap();
    let model = context
        .model_parameters()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let curved = context
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    for (observed, expected) in [
        (
            f64::NAN,
            (|error| matches!(error, Error::InvalidInput(_))) as fn(Error) -> bool,
        ),
        (
            1.0,
            (|error| matches!(error, Error::UnsupportedOperation(_))) as fn(Error) -> bool,
        ),
    ] {
        assert!(expected(
            implied_snowball_coupon(
                &engine,
                &note,
                &curved,
                observed,
                PreserveMaturityCoupon,
                Default::default()
            )
            .unwrap_err()
        ));
    }
}

#[test]
fn terminal_unpaid_phoenix_and_knocked_in_notes_have_no_coupon_exposure() {
    let engine = QuadratureAutocallableEngine::default();
    let context = market(70.0, date(4, 1));
    let records = autocallable_observations(&[date(4, 1)], &[110.0], None, Some(&[90.0])).unwrap();
    let mut terms = PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        records,
    );
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let phoenix = PhoenixOption::new(terms).unwrap();
    let quote = engine.price(&phoenix, &context).unwrap();
    assert!(matches!(
        implied_coupon(&engine, &phoenix, &context, quote, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let current = market(110.0, date(4, 1));
    let quote = engine
        .price(&phoenix.with_coupon(0.18).unwrap(), &current)
        .unwrap();
    assert!(
        (implied_coupon(&engine, &phoenix, &current, quote, Default::default()).unwrap() - 0.18)
            .abs()
            < 1e-8
    );
    let records = autocallable_observations(&[date(4, 1)], &[110.0], Some(&[0.08]), None).unwrap();
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        records,
    );
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let note = SnowballOption::new(terms).unwrap();
    let quote = engine.price(&note, &context).unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &note,
            &context,
            quote,
            CouponQuoteConvention::ShiftMaturityCoupon,
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn coupon_endpoints_can_change_daily_knock_in_layers() {
    let engine = QuadratureAutocallableEngine::new(QuadratureSettings {
        asset_step_count: 17,
        ..Default::default()
    })
    .unwrap();
    let mut terms = SnowballTerms::new(
        0.073,
        100.0,
        99.0,
        100.0,
        100.0,
        date(1, 1),
        date(1, 13),
        autocallable_observations(
            &[date(1, 7), date(1, 13)],
            &[110.0, 110.0],
            Some(&[0.073, 0.083]),
            None,
        )
        .unwrap(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let note = SnowballOption::new(terms).unwrap();
    let context = PricingContext::with_calendar(
        BlackScholesMertonParameters::new(0.03, 0.0, 0.3).unwrap(),
        100.25,
        Timestamp::new(date(1, 1), 12, 0, 0, 0).unwrap(),
        TradingCalendar::all_days(),
    )
    .unwrap();
    let quote = engine.price(&note, &context).unwrap();
    for convention in [
        CouponQuoteConvention::ShiftMaturityCoupon,
        CouponQuoteConvention::PreserveMaturityCoupon,
    ] {
        let solved = implied_snowball_coupon(
            &engine,
            &note,
            &context,
            quote,
            convention,
            ImpliedCouponSettings {
                lower_bound: 0.0,
                upper_bound: 0.2,
                price_tolerance: 1e-13,
                parameter_tolerance: 1e-12,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((solved - 0.073).abs() < 1e-9, "{solved}");
        assert!(
            (engine
                .price(&note.with_coupon(solved, convention).unwrap(), &context)
                .unwrap()
                - quote)
                .abs()
                < 1e-13
        );
    }
}
