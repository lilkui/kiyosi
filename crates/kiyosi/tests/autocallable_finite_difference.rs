#[path = "support/autocallable.rs"]
mod autocallable_support;

use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}
fn market(spot: f64, valuation: impl Into<Timestamp>) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        valuation.into(),
    )
    .unwrap()
}

#[test]
fn saved_snowball_does_not_replay_the_closed_prefix_at_each_fixing_level() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let start = date(1, 4);
    let end = start.add_days(2)?.add_micros(12 * 3_600_000_000)?;
    let counts = Arc::new(AtomicUsize::new(0));
    let calendar = TradingCalendar::new(
        {
            let counts = Arc::clone(&counts);
            move |_| {
                counts.fetch_add(1, Ordering::Relaxed);
                true
            }
        },
        365,
    )?;
    let times = (1..=500)
        .map(|seconds| start.add_micros(seconds * 1_000_000))
        .collect::<Result<Vec<_>>>()?;
    let levels: Vec<_> = (0..500).map(|i| 110.0 - f64::from(i) * 0.04).collect();
    let note = SnowballOption::new(SnowballTerms::new(
        0.08,
        100.0,
        50.0,
        100.0,
        40.0,
        start,
        end,
        autocallable_observations(&times, &levels, &[0.05; 500])?,
        KnockInObservation::EveryTradingDay {
            observation_clock_time: TimeOfDay::new(0, 0, 0, 0)?,
        },
        calendar,
    ))?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.04,
        0.04,
        0.2,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
    )?;
    let context = PricingContext::new(model, 100.0, start)?;
    let engine = FiniteDifferenceAutocallableEngine::default();
    counts.store(0, Ordering::Relaxed);
    let saved = engine.solve(&note, &context, 89.0..=111.0)?;
    let calendar_queries = counts.load(Ordering::Relaxed);
    assert!(
        calendar_queries < 100,
        "{calendar_queries} calendar queries"
    );
    assert!((saved.price(100.0)? - engine.price(&note, &context)?).abs() < 1e-12);
    Ok(())
}

#[test]
fn scalar_fixed_snowball_consults_the_future_calendar_once() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let calendar_calls = Arc::clone(&calls);
    let calendar = TradingCalendar::new(
        move |_| {
            calendar_calls.fetch_add(1, Ordering::Relaxed);
            true
        },
        365,
    )?;
    let start = date(1, 2);
    let expiry = start.add_days(2)?;
    let mut terms = SnowballTerms::new(
        0.0,
        100.0,
        80.0,
        100.0,
        100.0,
        start,
        expiry,
        vec![AutocallableObservation::snowball(expiry, 110.0, 0.0)?],
        KnockInObservation::EveryTradingDay {
            observation_clock_time: TimeOfDay::new(0, 0, 0, 0)?,
        },
        calendar,
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let note = SnowballOption::new(terms)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
        100.0,
        start.add_micros(1)?,
    )?;
    calls.store(0, Ordering::Relaxed);
    let price = FiniteDifferenceAutocallableEngine::default().price(&note, &context)?;
    assert!((price - 1.0).abs() <= f64::EPSILON);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn terminal_binary_matches_independent_cash_and_digital_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        date(1, 1),
        end,
        autocallable_observations(&[end], &[100.0], &[0.1]).unwrap(),
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 0.1, date(1, 1), end).unwrap();
    let context = market(100.0, date(1, 1));
    let expected = (-0.04_f64).exp() + AnalyticDigitalEngine.price(&digital, &context).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::default();
    let actual = engine.price(&note, &context).unwrap();
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}

#[test]
fn past_knock_out_levels_do_not_size_the_grid() -> Result<()> {
    let start = date(1, 1);
    let valuation = start.add_days(30)?;
    let end = valuation.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.03, 0.0, 0.2)?,
        100.0,
        valuation,
    )?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 0.4, start, end)?;
    let term = start.years_until(end);
    let expected = (1.0 + 0.1 * term) * (-0.03_f64).exp()
        + term * AnalyticDigitalEngine.price(&digital, &context)?;
    for past_level in [100.0, 10_000_000.0] {
        let mut terms = BinarySnowballTerms::new(
            0.1,
            start,
            end,
            autocallable_observations(&[start, end], &[past_level, 100.0], &[0.5; 2])?,
            TradingCalendar::all_days(),
        );
        terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
        let note = BinarySnowballOption::new(terms)?;
        for asset_upper_boundary in [None, Some(400.0)] {
            let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
                asset_upper_boundary,
                ..Default::default()
            })?;
            let price = engine.price(&note, &context)?;
            assert!((price - expected).abs() < 1e-5, "{price} != {expected}");
        }
    }
    Ok(())
}

#[test]
fn monitoring_modes_and_all_variants_agree_with_quadrature() {
    fn check<T: AutocallableTerms>(note: &AutocallableNote<T>, context: &PricingContext) {
        let reference = QuadratureAutocallableEngine::new(QuadratureSettings {
            asset_step_count: 1024,
            ..Default::default()
        })
        .unwrap()
        .price(note, context)
        .unwrap();
        let fd = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
            asset_step_count: 800,
            time_step_count: 2000,
            asset_upper_boundary: Some(400.0),
            ..Default::default()
        })
        .unwrap();
        let actual = fd.price(note, context).unwrap();
        assert!((actual - reference).abs() < 1e-4, "{actual} != {reference}");
    }

    let start = date(1, 1);
    let end = date(4, 1);
    let observations = autocallable_observations(
        &[date(2, 3), date(3, 3), end],
        &[110.0, 108.0, 105.0],
        &[0.08; 3],
    )
    .unwrap();
    let context = market(100.0, start);
    for mode in [
        KnockInObservation::AtExpiry,
        KnockInObservation::EveryTradingDay {
            observation_clock_time: kiyosi::TimeOfDay::new(0, 0, 0, 0).unwrap(),
        },
    ] {
        let (snow, ternary, phoenix) = autocallable_support::multi_event_terms(mode);

        check(&SnowballOption::new(snow).unwrap(), &context);
        check(&TernarySnowballOption::new(ternary).unwrap(), &context);
        check(&PhoenixOption::new(phoenix).unwrap(), &context);
    }
    check(
        &BinarySnowballOption::new(BinarySnowballTerms::new(
            0.06,
            start,
            end,
            observations,
            TradingCalendar::weekdays(),
        ))
        .unwrap(),
        &context,
    );
}

#[test]
fn greeks_and_solvers_preserve_inputs_and_unidentifiable_errors() {
    let end = date(4, 1);
    let mut terms = PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        end,
        [date(2, 3), end]
            .iter()
            .zip([110.0, 105.0].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        [date(2, 3), end]
            .iter()
            .zip([90.0; 2].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.knock_in_observation = KnockInObservation::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);

    let note = PhoenixOption::new(terms).unwrap();
    let original = note.clone();
    let context = market(103.0, date(1, 1));
    let engine = FiniteDifferenceAutocallableEngine::default();
    let result = kiyosi::numerical_greeks(
        &engine,
        &note,
        &context,
        GreeksRequest::all(),
        NumericalShiftSettings {
            spot_shift: Some(0.25),
            ..Default::default()
        },
    )
    .unwrap();
    for greek in Greek::ALL {
        assert!(result.has(greek), "{greek:?}");
    }
    let sigma = implied_volatility(
        &engine,
        &note,
        &context,
        result.price(),
        ImpliedVolatilitySettings::new(0.1, 0.3),
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-6, "{sigma}");
    for coupon in [-0.05, 0.18] {
        let target = note
            .replace(PhoenixOptionChanges {
                coupon_per_period: Some(coupon),
                ..Default::default()
            })
            .unwrap();
        let quote = engine.price(&target, &context).unwrap();
        let solved = implied_phoenix_coupon(
            &engine,
            &note,
            &context,
            quote,
            ImpliedCouponSettings {
                lower_bound: -0.1,
                upper_bound: 0.3,
                price_tolerance: 1e-11,
                ..Default::default()
            },
        )
        .unwrap();
        let rate = solved;
        assert!((rate - coupon).abs() < 1e-8);
        assert!(
            (engine
                .price(
                    &note
                        .replace(PhoenixOptionChanges {
                            coupon_per_period: Some(solved),
                            ..Default::default()
                        })
                        .unwrap(),
                    &context
                )
                .unwrap()
                - quote)
                .abs()
                < 1e-11
        );
    }
    let settled = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    let later = market(100.0, date(3, 3));
    assert_eq!(engine.price(&settled, &later).unwrap(), 0.0);
    assert!(matches!(
        implied_volatility(
            &engine,
            &settled,
            &later,
            0.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        implied_phoenix_coupon(&engine, &settled, &later, 0.0, Default::default()).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(note, original);
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
}

#[test]
fn defaults_validation_history_and_node_curves_are_preserved() {
    let defaults = FiniteDifferenceAutocallableEngine::default().settings();
    assert_eq!(
        (defaults.asset_step_count, defaults.time_step_count),
        (800, 400)
    );
    assert_eq!(
        FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings::default())
            .unwrap()
            .settings(),
        defaults
    );
    assert!(matches!(
        FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
            time_step_count: 0,
            ..Default::default()
        })
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let start = date(1, 1);
    let end = date(4, 1);
    let note = SnowballOption::new(SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[110.0; 2], &[0.08; 2]).unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    let engine = FiniteDifferenceAutocallableEngine::default();
    assert!(matches!(
        engine.price(&note, &market(100.0, date(1, 2))).unwrap_err(),
        Error::MissingHistory(_)
    ));
    let known = note
        .replace(SnowballOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
            ..Default::default()
        })
        .unwrap();
    let impossible = note
        .replace(SnowballOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price(&impossible, &market(100.0, start))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let clipped = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(110.0),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        clipped.price(&known, &market(100.0, start)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let unstable = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        time_step_count: 1,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        unstable.price(&known, &market(100.0, start)).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let ctx = market(100.0, start);
    let curve = RateTermStructure::new(
        start,
        vec![date(2, 3), end],
        vec![
            (-0.04_f64 * 33.0 / 365.0).exp(),
            (-0.04_f64 * 90.0 / 365.0).exp(),
        ],
    )
    .unwrap();
    let curved = ctx
        .replace(PricingContextChanges {
            model_parameters: Some(
                ctx.model_parameters()
                    .as_bsm()
                    .unwrap()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(curve.into()),
                        ..Default::default()
                    })
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&known, &curved).unwrap() - engine.price(&known, &ctx).unwrap()).abs()
            < 1e-12
    );
    let short = RateTermStructure::new(start, vec![date(2, 3)], vec![0.99]).unwrap();
    let short_ctx = ctx
        .replace(PricingContextChanges {
            model_parameters: Some(
                ctx.model_parameters()
                    .as_bsm()
                    .unwrap()
                    .replace(BlackScholesMertonParameterChanges {
                        risk_free_rate: Some(short.into()),
                        ..Default::default()
                    })
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&known, &short_ctx).unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(note.barrier_state(), None);
}

#[test]
fn current_threshold_greeks_and_fixed_node_curve_identifiability() {
    let start = date(1, 1);
    let end = date(4, 1);
    let mut terms = PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        [date(2, 3), end]
            .iter()
            .zip([110.0; 2].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        [date(2, 3), end]
            .iter()
            .zip([90.0; 2].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.knock_in_observation = KnockInObservation::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);

    let engine = FiniteDifferenceAutocallableEngine::default();
    let note = PhoenixOption::new(terms).unwrap();
    let result = engine
        .price_with_greeks(&note, &market(90.0, date(2, 3)), GreeksRequest::all())
        .unwrap();
    for greek in [Greek::Delta, Greek::Gamma, Greek::Speed, Greek::Theta] {
        assert!(!result.has(greek), "{greek:?}");
    }
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let curve = RateTermStructure::new(start, vec![end], vec![0.99]).unwrap();
    let model = BlackScholesMertonParameters::new(curve, 0.01, 0.2).unwrap();
    let ctx = PricingContext::new(model, 100.0, start).unwrap();
    let fixed = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[110.0], &[0.0]).unwrap(),
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    let result = engine
        .price_with_greeks(&fixed, &ctx, GreeksRequest::all())
        .unwrap();
    assert!((result.price() - 0.99).abs() < 1e-14);
    assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
    assert!(matches!(
        implied_volatility(
            &engine,
            &fixed,
            &ctx,
            result.price(),
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn binary_domains_use_actual_levels_in_small_price_units() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let mut reference: Option<f64> = None;
    for level in [100.0, 1e-6, 1e-9] {
        let note = BinarySnowballOption::new(BinarySnowballTerms::new(
            0.0,
            start,
            end,
            autocallable_observations(&[end], &[level], &[0.1]).unwrap(),
            TradingCalendar::weekdays(),
        ))
        .unwrap();
        let ctx = market(level, start);
        let price = FiniteDifferenceAutocallableEngine::default()
            .price(&note, &ctx)
            .unwrap();
        let expected = QuadratureAutocallableEngine::default()
            .price(&note, &ctx)
            .unwrap();
        assert!(
            (price - expected).abs() < 1e-5,
            "level={level}: {price} != {expected}"
        );
        if let Some(value) = reference {
            assert!((price - value).abs() < 1e-10);
        }
        reference = Some(price);
        let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
            time_step_count: 2000,
            asset_upper_boundary: Some(4.0 * level),
            ..Default::default()
        })
        .unwrap();
        assert!((engine.price(&note, &ctx).unwrap() - expected).abs() < 1e-5);
    }
}

#[test]
fn native_spot_greeks_do_not_require_a_feasible_scalar_bump() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let ctx = market(100.0, start);
    let engine = FiniteDifferenceAutocallableEngine::default();
    for coupon in [0.0, 0.1] {
        let note = BinarySnowballOption::new(BinarySnowballTerms::new(
            0.0,
            start,
            end,
            autocallable_observations(&[end], &[100.0], &[coupon]).unwrap(),
            TradingCalendar::weekdays(),
        ))
        .unwrap();
        let request = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed]);
        let reference = engine.price_with_greeks(&note, &ctx, request).unwrap();
        for shift in [200.0, 1e-300] {
            let actual = kiyosi::numerical_greeks(
                &engine,
                &note,
                &ctx,
                request,
                NumericalShiftSettings {
                    spot_shift: Some(shift),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(reference.has(Greek::Delta));
            assert!(!actual.has(Greek::Delta));
            assert!(reference.has(Greek::Gamma));
            assert!(!actual.has(Greek::Gamma));
            assert!(!actual.has(Greek::Speed));
        }
    }
}

#[test]
fn explicit_uniform_log_grid_checks_stencil_stability() {
    let start = date(1, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[100.0; 2], &[0.1; 2]).unwrap(),
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    let ctx = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let settings = FiniteDifferenceSettings {
        asset_step_count: 40,
        time_step_count: 10,
        scheme: FiniteDifferenceScheme::ExplicitEuler,
        asset_upper_boundary: Some(400.0),
    };
    let engine = FiniteDifferenceAutocallableEngine::new(settings).unwrap();
    assert!(matches!(
        engine.price(&note, &ctx).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let stable = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 11,
        ..settings
    })
    .unwrap();
    assert!(stable.price(&note, &ctx).unwrap() > 0.0);
}

#[test]
fn flat_bsm_market_tangents_match_autocallable_price_bumps() -> Result<()> {
    let context = market(100.0, date(1, 1));
    let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 1200,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    let measures = [Greek::Rho, Greek::DividendRho, Greek::Vega];
    let request = GreeksRequest::new(measures);
    let modes = [
        KnockInObservation::AtExpiry,
        KnockInObservation::EveryTradingDay {
            observation_clock_time: date(1, 1).time_of_day(),
        },
    ];
    for mode in modes {
        let (terms, _, _) = autocallable_support::multi_event_terms(mode);
        let note = SnowballOption::new(terms)?;
        let actual = engine.price_with_greeks(&note, &context, request)?;
        let bumped = engine.price_with_greeks_with_settings(
            &note,
            &context,
            request,
            NumericalShiftSettings {
                rate_shift: 0.0002,
                volatility_shift: Some(0.0002),
                ..Default::default()
            },
        )?;
        assert_eq!(actual.price(), engine.price(&note, &context)?);
        for greek in measures {
            let actual = actual.require(greek)?;
            let expected = bumped.require(greek)?;
            assert!(
                (actual - expected).abs() < 0.0002,
                "{greek:?}: {actual} vs {expected}"
            );
        }
    }
    Ok(())
}
