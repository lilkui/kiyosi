mod support;
use support::dated;

use kiyosi::*;

#[test]
fn monte_carlo_coupon_solvers_require_a_replayable_seed() -> Result<()> {
    let unseeded = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 1024,
        max_active_workers: 1,
        ..Default::default()
    })?;
    let engine = unseeded.with_resolved_seed()?;
    let context = market(100.0, date(1, 1));
    let convention = CouponQuoteConvention::ShiftMaturityCoupon;
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.06,
        date(1, 1),
        date(4, 1),
        autocallable_observations(&[date(4, 1)], &[110.0], Some(&[0.08]), None)?,
    ))?;
    let quote = engine.price(&note.with_quoted_coupon(0.12, convention)?, &context)?;
    assert!(matches!(
        implied_snowball_coupon(
            &unseeded,
            &note,
            &context,
            quote,
            convention,
            Default::default()
        ),
        Err(Error::InvalidInput(_))
    ));
    let coupon = implied_snowball_coupon(
        &engine,
        &note,
        &context,
        quote,
        convention,
        Default::default(),
    )?;
    assert!((coupon - 0.12).abs() < 1e-8);
    assert!(
        (engine.price(&note.with_quoted_coupon(coupon, convention)?, &context)? - quote).abs()
            <= 1e-8
    );
    for coupon in [PhoenixCoupon::Annual(0.08), PhoenixCoupon::PerPeriod(0.02)] {
        let note = PhoenixOption::new(PhoenixTerms::new(
            coupon,
            100.0,
            80.0,
            100.0,
            0.0,
            date(1, 1),
            date(4, 1),
            autocallable_observations(&[date(4, 1)], &[110.0], None, Some(&[90.0]))?,
            Some(date(1, 1).time_of_day()),
        ))?;
        let quote = engine.price(&note, &context)?;
        assert!(matches!(
            implied_phoenix_coupon(&unseeded, &note, &context, quote, Default::default()),
            Err(Error::InvalidInput(_))
        ));
        let solved = implied_phoenix_coupon(&engine, &note, &context, quote, Default::default())?;
        assert!((engine.price(&note.with_quoted_coupon(solved)?, &context)? - quote).abs() <= 1e-8);
    }
    assert!(engine.sampling_settings().seed.is_some());
    assert_eq!(unseeded.sampling_settings().seed, None);
    Ok(())
}

#[test]
fn ternary_coupon_solver_intersects_bounds_with_valid_maturity_coupons() -> Result<()> {
    let context = market(100.0, date(1, 1));
    for engine in [
        &QuadratureAutocallableEngine::new(QuadratureSettings {
            asset_step_count: 64,
            ..Default::default()
        })? as &dyn PricingEngine<TernarySnowballOption>,
        &FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
            asset_step_count: 200,
            time_step_count: 100,
            ..Default::default()
        })?,
        &MonteCarloAutocallableEngine::new(MonteCarloSettings {
            path_count: 512,
            seed: Some(42),
            max_active_workers: 1,
            ..Default::default()
        })?,
    ] {
        for (maturity, minimum, anchor) in [
            (0.1, 0.05, 0.1),
            (0.06, 0.02, 0.08),
            (0.07, 0.02, 0.13),
            (
                0.404_715_228_338_913_3,
                0.002_630_147_007_026_187,
                0.402_909_625_916_403_95,
            ),
        ] {
            let note = TernarySnowballOption::new(TernarySnowballTerms::new(
                maturity,
                minimum,
                80.0,
                date(1, 1),
                date(4, 1),
                autocallable_observations(&[date(4, 1)], &[110.0], Some(&[anchor]), None)?,
                Some((date(4, 1)).time_of_day()),
            ))?;
            let original = note.clone();
            let quote = engine.price(&dated(&note, &context), &context)?;
            for lower in [0.0, -0.1] {
                let solved = implied_snowball_coupon(
                    engine,
                    &dated(&note, &context),
                    &context,
                    quote,
                    CouponQuoteConvention::ShiftMaturityCoupon,
                    ImpliedCouponSettings {
                        lower_bound: lower,
                        ..Default::default()
                    },
                )?;
                assert!((solved - anchor).abs() < 1e-7);
            }
            assert!(matches!(
                implied_snowball_coupon(
                    engine,
                    &dated(&note, &context),
                    &context,
                    quote,
                    CouponQuoteConvention::ShiftMaturityCoupon,
                    ImpliedCouponSettings {
                        lower_bound: -0.1,
                        upper_bound: 0.01,
                        ..Default::default()
                    }
                ),
                Err(Error::RootNotBracketed(_))
            ));
            assert!(matches!(
                implied_snowball_coupon(
                    engine,
                    &dated(&note, &context),
                    &context,
                    quote,
                    CouponQuoteConvention::ShiftMaturityCoupon,
                    ImpliedCouponSettings {
                        lower_bound: f64::NAN,
                        ..Default::default()
                    }
                ),
                Err(Error::InvalidInput(_))
            ));
            let fixed =
                note.with_quoted_coupon(0.0, CouponQuoteConvention::PreserveMaturityCoupon)?;
            let solved = implied_snowball_coupon(
                engine,
                &dated(&note, &context),
                &context,
                engine.price(&dated(&fixed, &context), &context)?,
                CouponQuoteConvention::PreserveMaturityCoupon,
                Default::default(),
            )?;
            assert_eq!(solved, 0.0);
            assert_eq!(note, original);
        }
        let boundary = TernarySnowballOption::new(TernarySnowballTerms::new(
            0.05,
            0.05,
            80.0,
            date(1, 1),
            date(4, 1),
            autocallable_observations(&[date(4, 1)], &[110.0], Some(&[0.05]), None)?,
            Some((date(4, 1)).time_of_day()),
        ))?;
        let solved = implied_snowball_coupon(
            engine,
            &dated(&boundary, &context),
            &context,
            engine.price(&dated(&boundary, &context), &context)?,
            CouponQuoteConvention::ShiftMaturityCoupon,
            Default::default(),
        )?;
        assert!((solved - 0.05).abs() < 1e-15);
    }
    Ok(())
}

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
        terms.observations[0] = terms.observations[0]
            .replace(AutocallableObservationChanges {
                coupon_rate: Some(0.18),
                ..Default::default()
            })
            .unwrap();
        terms.observations[1] = terms.observations[1]
            .replace(AutocallableObservationChanges {
                coupon_rate: Some(0.22),
                ..Default::default()
            })
            .unwrap();
        if convention == CouponQuoteConvention::ShiftMaturityCoupon {
            terms.maturity_coupon_rate = 0.16;
        }
        let target = BinarySnowballOption::new(terms).unwrap();
        let quote = engine.price(&dated(&target, &context), &context).unwrap();
        let solved = implied_snowball_coupon(
            &engine,
            &dated(&note, &context),
            &context,
            quote,
            convention,
            Default::default(),
        )
        .unwrap();
        assert!((solved - 0.18).abs() < 1e-7, "{solved}");
        let replaced = note.with_quoted_coupon(solved, convention).unwrap();
        assert!(
            (engine.price(&dated(&replaced, &context), &context).unwrap() - quote).abs() < 1e-8
        );
    }
    assert_eq!(note, original);
}

#[test]
fn all_products_recover_prices_with_daily_knock_in_and_signed_coupons() {
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
                let target = note.with_quoted_coupon(coupon, convention).unwrap();
                let quote = engine.price(&dated(&target, context), context).unwrap();
                let settings = ImpliedCouponSettings {
                    lower_bound: -0.1,
                    upper_bound: 0.3,
                    price_tolerance: 1e-11,
                    ..Default::default()
                };
                let solved = implied_snowball_coupon(
                    engine,
                    &dated(note, context),
                    context,
                    quote,
                    convention,
                    settings,
                )
                .unwrap();
                assert!((solved - coupon).abs() < 1e-8, "{solved}");
                assert!(
                    (engine
                        .price(
                            &dated(
                                &note.with_quoted_coupon(solved, convention).unwrap(),
                                context
                            ),
                            context
                        )
                        .unwrap()
                        - quote)
                        .abs()
                        < 1e-11
                );
                assert!(
                    (target.observations()[1].coupon_rate().unwrap()
                        - target.observations()[0].coupon_rate().unwrap()
                        - 0.04)
                        .abs()
                        < 1e-14
                );
            }
        }
        assert_eq!(*note, original);
    }

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
        Some((date(4, 1)).time_of_day()),
    ))
    .unwrap();
    let ternary = TernarySnowballOption::new(TernarySnowballTerms::new(
        0.06,
        -0.2,
        80.0,
        date(1, 1),
        date(4, 1),
        records,
        Some((date(4, 1)).time_of_day()),
    ))
    .unwrap();

    recover(&engine, &snowball, &context);
    recover(&engine, &binary(), &context);
    recover(&engine, &ternary, &context);
    assert_eq!(
        ternary
            .with_quoted_coupon(0.18, CouponQuoteConvention::ShiftMaturityCoupon)
            .unwrap()
            .terms()
            .minimum_coupon_rate,
        -0.2
    );

    let note = PhoenixOption::new(PhoenixTerms::new(
        kiyosi::PhoenixCoupon::Annual(0.08),
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
        Some((date(4, 1)).time_of_day()),
    ))
    .unwrap();
    for coupon in [-0.05, 0.18] {
        let quote = engine
            .price(
                &dated(
                    &note
                        .replace(PhoenixOptionChanges {
                            coupon: Some(PhoenixCoupon::Annual(coupon)),
                            ..Default::default()
                        })
                        .unwrap(),
                    &context,
                ),
                &context,
            )
            .unwrap();
        let solved = implied_phoenix_coupon(
            &engine,
            &dated(&note, &context),
            &context,
            quote,
            ImpliedCouponSettings {
                lower_bound: -0.1,
                ..Default::default()
            },
        )
        .unwrap();
        let PhoenixCoupon::Annual(rate) = solved else {
            panic!("expected an annual coupon")
        };
        assert!((rate - coupon).abs() < 1e-7);
        assert!(
            (engine
                .price(
                    &dated(
                        &note
                            .replace(PhoenixOptionChanges {
                                coupon: Some(solved),
                                ..Default::default()
                            })
                            .unwrap(),
                        &context
                    ),
                    &context
                )
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
            &dated(&note, &context),
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
                &dated(
                    &note
                        .with_quoted_coupon(bound, PreserveMaturityCoupon)
                        .unwrap(),
                    &context,
                ),
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
    let known = note
        .replace_history(Some(AutocallableBarrierState::NoEvent), None)
        .unwrap();
    let terminal = market(100.0, date(4, 1));
    let quote = engine.price(&dated(&known, &terminal), &terminal).unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &dated(&known, &terminal),
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
        &dated(&known, &terminal),
        &terminal,
        quote,
        ShiftMaturityCoupon,
        defaults,
    )
    .unwrap();
    assert!((shifted - 0.08).abs() < 1e-8);
    let knocked_out = note
        .replace_history(Some(AutocallableBarrierState::KnockedOut), None)
        .unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &dated(&knocked_out, &market(100.0, date(3, 3))),
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
            &dated(&note, &terminal),
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
            &dated(&note, &terminal),
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
            Ok((self.0)(note.observations()[0].coupon_rate().unwrap()))
        }
    }
    let note = binary();
    let context = market(100.0, date(1, 1));
    let solve = |engine: &Curve, quote, settings| {
        implied_snowball_coupon(
            engine,
            &dated(&note, &context),
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
    assert!(matches!(
        solve(&nonlinear, 0.3, interval_only),
        Err(Error::NonConvergence(_))
    ));
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
            (!matches!(preset, SnowballPreset::European)).then_some(date(4, 1).time_of_day()),
        )
        .expand(preset)
        .unwrap();
        for convention in [
            CouponQuoteConvention::ShiftMaturityCoupon,
            CouponQuoteConvention::PreserveMaturityCoupon,
        ] {
            let target = note.with_quoted_coupon(0.18, convention).unwrap();
            let quote = engine.price(&dated(&target, &context), &context).unwrap();
            let solved = implied_snowball_coupon(
                &engine,
                &dated(&note, &context),
                &context,
                quote,
                convention,
                Default::default(),
            )
            .unwrap();
            assert!((solved - 0.18).abs() < 1e-7, "{preset:?}: {solved}");
            assert!(
                (engine
                    .price(
                        &dated(
                            &note.with_quoted_coupon(solved, convention).unwrap(),
                            &context
                        ),
                        &context
                    )
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
        + (0.18 - 0.06)
            * term
            * AnalyticDigitalEngine
                .price(&dated(&digital, &context), &context)
                .unwrap();
    let solved = implied_snowball_coupon(
        &engine,
        &dated(&note, &context),
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
            &dated(&extreme, &market(100.0, date(4, 2))),
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
        Some((date(4, 1)).time_of_day()),
    ))
    .unwrap();
    let quote = engine.price(&dated(&ternary, &context), &context).unwrap();
    let coupon = implied_snowball_coupon(
        &engine,
        &dated(&ternary, &context),
        &context,
        quote,
        ShiftMaturityCoupon,
        Default::default(),
    )
    .unwrap();
    assert!((coupon - ternary.observations()[0].coupon_rate().unwrap()).abs() < 1e-7);
    assert_eq!(ternary.terms().maturity_coupon_rate, 0.06);
    let curve = RateTermStructure::new(
        date(1, 1),
        vec![date(4, 1), (date(4, 1)).add_days(1).unwrap()],
        vec![0.99, (0.99) * 0.99],
    )
    .unwrap();
    let model = context
        .model_parameters()
        .as_bsm()
        .unwrap()
        .replace(BlackScholesMertonParameterChanges {
            risk_free_rate: Some(curve.into()),
            ..Default::default()
        })
        .unwrap();
    let curved = context
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
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
                &dated(&note, &curved),
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
        kiyosi::PhoenixCoupon::Annual(0.08),
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        records,
        Some((date(4, 1)).time_of_day()),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let phoenix = PhoenixOption::new(terms).unwrap();
    let quote = engine.price(&dated(&phoenix, &context), &context).unwrap();
    assert!(matches!(
        implied_phoenix_coupon(
            &engine,
            &dated(&phoenix, &context),
            &context,
            quote,
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let current = market(110.0, date(4, 1));
    let quote = engine
        .price(
            &dated(
                &phoenix
                    .replace(PhoenixOptionChanges {
                        coupon: Some(PhoenixCoupon::Annual(0.18)),
                        ..Default::default()
                    })
                    .unwrap(),
                &current,
            ),
            &current,
        )
        .unwrap();
    let solved = implied_phoenix_coupon(
        &engine,
        &dated(&phoenix, &current),
        &current,
        quote,
        Default::default(),
    )
    .unwrap();
    let PhoenixCoupon::Annual(rate) = solved else {
        panic!("expected an annual coupon")
    };
    assert!((rate - 0.18).abs() < 1e-8);
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
        Some((date(4, 1)).time_of_day()),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms).unwrap();
    let quote = engine.price(&dated(&note, &context), &context).unwrap();
    assert!(matches!(
        implied_snowball_coupon(
            &engine,
            &dated(&note, &context),
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
        Some((date(1, 13)).time_of_day()),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.03, 0.0, 0.3).unwrap(),
        100.25,
        Timestamp::new(date(1, 1).date(), 12, 0, 0, 0).unwrap(),
    )
    .unwrap();
    let quote = engine.price(&dated(&note, &context), &context).unwrap();
    for convention in [
        CouponQuoteConvention::ShiftMaturityCoupon,
        CouponQuoteConvention::PreserveMaturityCoupon,
    ] {
        let solved = implied_snowball_coupon(
            &engine,
            &dated(&note, &context),
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
                .price(
                    &dated(
                        &note.with_quoted_coupon(solved, convention).unwrap(),
                        &context
                    ),
                    &context
                )
                .unwrap()
                - quote)
                .abs()
                < 1e-13
        );
    }
}
