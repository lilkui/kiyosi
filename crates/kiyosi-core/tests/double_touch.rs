use kiyosi_core::*;

fn terms(kind: DoubleBarrierType) -> DoubleTouchOptionTerms {
    DoubleTouchOptionTerms::new(
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2026, 1, 1).unwrap(),
        80.0,
        120.0,
        kind,
    )
}

fn market(spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.03, 0.01, 0.2).unwrap(),
        spot,
        Date::new(2025, 1, 1).unwrap().into(),
    )
    .unwrap()
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
        (&one, 0.6031839700294364),
        (&at_hit, 0.6119415209961093),
        (&no, 0.36726156351907174),
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

fn category<T: std::fmt::Debug>(result: Result<T>, expected: ErrorCategory) {
    assert_eq!(result.unwrap_err().category, expected);
}

fn context(spot: f64, time: Timestamp) -> PricingContext {
    market(spot)
        .replace(PricingContextChanges {
            valuation_time: Some(time),
            calendar: Some(TradingCalendar::all_days()),
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn cash_terms_defaults_validation_and_owning_history() {
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let t = terms(kind);
        let option = DoubleTouchOption::cash(t.clone(), 1.0).unwrap();
        assert_eq!(option.is_one_touch(), kind == DoubleBarrierType::KnockIn);
        assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
        assert_eq!(option.observation_mode(), ObservationMode::Continuous);
        assert_eq!(option.touch_state(), None);
        assert!(option.observation_dates().is_empty());
        let known = option.replace(Some(BarrierTouchState::Untouched)).unwrap();
        assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
        assert_eq!(option.touch_state(), None);
        assert_eq!(known.replace(None).unwrap(), option);
        for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            category(
                DoubleTouchOption::cash(t.clone(), invalid),
                ErrorCategory::InvalidParameter,
            );
            for lower in [false, true] {
                let mut bad = t.clone();
                if lower {
                    bad.lower_barrier = invalid;
                } else {
                    bad.upper_barrier = invalid;
                }
                category(
                    DoubleTouchOption::cash(bad, 1.0),
                    ErrorCategory::InvalidParameter,
                );
            }
        }
        for lower in [120.0, 121.0] {
            let mut bad = t.clone();
            bad.lower_barrier = lower;
            category(
                DoubleTouchOption::cash(bad, 1.0),
                ErrorCategory::InvalidParameter,
            );
        }
        let mut bad = t.clone();
        bad.expiry_date = Date::new(2024, 12, 31).unwrap();
        category(
            DoubleTouchOption::cash(bad, 1.0),
            ErrorCategory::InvalidTimeRange,
        );
        let mut bad = t.clone();
        bad.observation_mode = ObservationMode::Scheduled;
        category(
            DoubleTouchOption::cash(bad, 1.0),
            ErrorCategory::InvalidSchedule,
        );
        let first = Date::new(2025, 6, 30).unwrap();
        for (mode, dates) in [
            (ObservationMode::Continuous, vec![t.expiry_date]),
            (ObservationMode::Scheduled, vec![t.expiry_date, first]),
            (ObservationMode::Scheduled, vec![first, first]),
        ] {
            let mut bad = t.clone();
            bad.observation_mode = mode;
            bad.observation_dates = dates;
            category(
                DoubleTouchOption::cash(bad, 1.0),
                ErrorCategory::InvalidSchedule,
            );
        }
        let mut scheduled = t;
        scheduled.observation_mode = ObservationMode::Scheduled;
        scheduled.observation_dates = vec![first, scheduled.expiry_date];
        let owned = DoubleTouchOption::cash(scheduled.clone(), 1.0).unwrap();
        scheduled.observation_dates.clear();
        assert_eq!(owned.observation_dates(), &[first, owned.expiry_date()]);
    }
    let mut invalid = terms(DoubleBarrierType::KnockOut);
    invalid.settlement_timing = SettlementTiming::AtHit;
    category(
        DoubleTouchOption::cash(invalid.clone(), 1.0),
        ErrorCategory::InvalidOption,
    );
    category(
        DoubleTouchOption::cash(invalid, 0.0),
        ErrorCategory::InvalidParameter,
    );
}

#[test]
fn current_prior_and_expiry_payments_resolve_before_grid_validation() {
    let engine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    })
    .unwrap();
    let later = Date::new(2025, 1, 2).unwrap().into();
    for timing in [SettlementTiming::AtExpiry, SettlementTiming::AtHit] {
        let mut t = terms(DoubleBarrierType::KnockIn);
        t.settlement_timing = timing;
        let option = DoubleTouchOption::cash(t, 3.0).unwrap();
        let paid = option.replace(Some(BarrierTouchState::Touched)).unwrap();
        category(
            engine.price(&paid, &market(100.0)),
            ErrorCategory::InvalidOption,
        );
        for spot in [79.0, 80.0, 120.0, 121.0] {
            let expected = if timing == SettlementTiming::AtHit {
                3.0
            } else {
                3.0 * (-0.03_f64).exp()
            };
            assert!((engine.price(&option, &market(spot)).unwrap() - expected).abs() < 1e-12);
            category(
                engine.price(&option, &context(spot, later)),
                ErrorCategory::InvalidParameter,
            );
        }
        let expected = if timing == SettlementTiming::AtHit {
            0.0
        } else {
            3.0 * (-0.03_f64 * 364.0 / 365.0).exp()
        };
        assert!((engine.price(&paid, &context(100.0, later)).unwrap() - expected).abs() < 1e-12);
        category(
            implied_volatility(
                &engine,
                &paid,
                &context(100.0, later),
                expected,
                Default::default(),
            ),
            ErrorCategory::UnsupportedOperation,
        );
    }
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let option = DoubleTouchOption::cash(terms(kind), 3.0)
            .unwrap()
            .replace(Some(BarrierTouchState::Untouched))
            .unwrap();
        for spot in [80.0, 100.0, 120.0] {
            let at = context(spot, option.expiry_date().into());
            let result = engine
                .price_with_greeks(&option, &at, GreeksRequest::all(), Default::default())
                .unwrap();
            let touched = spot != 100.0;
            assert_eq!(
                result.price(),
                if touched == option.is_one_touch() {
                    3.0
                } else {
                    0.0
                }
            );
            assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
        }
    }
}

#[test]
fn scheduled_one_two_and_early_fixings_match_independent_integrals() {
    let first = Date::new(2025, 6, 30).unwrap();
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
            vec![terms(DoubleBarrierType::KnockIn).expiry_date],
            [0.30489754927956764, 0.30489754927956764, 0.6655479842689406],
        ),
        (
            vec![first, Date::new(2026, 1, 1).unwrap()],
            [0.355769695628499, 0.3580627743006117, 0.6146758379200091],
        ),
        (
            vec![first],
            [0.14966244109876742, 0.15195551977088007, 0.8207830924497408],
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
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = dates.clone();
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
    let first = Date::new(2025, 6, 30).unwrap();
    let mut t = terms(DoubleBarrierType::KnockOut);
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![first];
    let option = DoubleTouchOption::cash(t.clone(), 2.0).unwrap();
    assert_eq!(
        engine
            .price(&option, &context(120.0, first.into()))
            .unwrap(),
        0.0
    );
    let after = Timestamp::new(first, 0, 0, 0, 1).unwrap();
    category(
        engine.price(&option, &context(130.0, after)),
        ErrorCategory::InvalidParameter,
    );
    let known = option.replace(Some(BarrierTouchState::Untouched)).unwrap();
    let ctx = context(130.0, after);
    let remaining = (185.0_f64 - 1e-6 / 86400.0) / 365.0;
    let expected = 2.0 * (-0.03 * remaining).exp();
    let result = engine
        .price_with_greeks(&known, &ctx, GreeksRequest::all(), Default::default())
        .unwrap();
    assert!((result.price() - expected).abs() < 1e-12);
    assert_eq!(result.require(Greek::Delta).unwrap(), 0.0);
    assert_eq!(result.require(Greek::Vega).unwrap(), 0.0);
    assert!((result.require(Greek::Rho).unwrap() + remaining * expected / 100.0).abs() < 1e-8);
    category(
        implied_volatility(&engine, &known, &ctx, expected, Default::default()),
        ErrorCategory::UnsupportedOperation,
    );
    t.observation_dates = vec![Date::new(2025, 1, 4).unwrap()];
    let weekend = DoubleTouchOption::cash(t, 1.0).unwrap();
    category(
        engine.price(&weekend, &market(130.0)),
        ErrorCategory::InvalidDate,
    );
    let at = engine
        .price_with_greeks(
            &option.replace(Some(BarrierTouchState::Untouched)).unwrap(),
            &context(100.0, first.into()),
            GreeksRequest::new([Greek::Theta, Greek::Charm, Greek::Color]),
            Default::default(),
        )
        .unwrap();
    assert!(
        [Greek::Theta, Greek::Charm, Greek::Color]
            .into_iter()
            .all(|g| !at.has(g))
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
            .price_with_greeks(&option, &ctx, GreeksRequest::all(), Default::default())
            .unwrap();
        assert!(Greek::ALL.into_iter().all(|g| result.has(g)));
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
        let controls = ImpliedVolatilitySettings {
            lower_bound: 0.15,
            upper_bound: 0.25,
            ..Default::default()
        };
        let sigma = implied_volatility(&engine, &option, &ctx, result.price(), controls).unwrap();
        assert!((sigma - 0.2).abs() < 1e-6);
        category(
            implied_volatility(&engine, &option, &ctx, 2.0, controls),
            ErrorCategory::UnbracketedVolatility,
        );
        category(
            engine.price_with_greeks(&option, &ctx, GreeksRequest::default(), Default::default()),
            ErrorCategory::InvalidParameter,
        );
        category(
            engine.price_with_greeks(
                &option,
                &ctx,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    spot_shift: 0.0,
                    ..Default::default()
                },
            ),
            ErrorCategory::InvalidParameter,
        );
        for spot in [80.0, 120.0] {
            let at = engine
                .price_with_greeks(
                    &option,
                    &market(spot),
                    GreeksRequest::all(),
                    Default::default(),
                )
                .unwrap();
            assert!(Greek::ALL.into_iter().all(|g| !at.has(g)));
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
    let first = Date::new(2025, 6, 30).unwrap();
    let end = Date::new(2026, 1, 1).unwrap();
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
            terms(DoubleBarrierType::KnockIn).effective_date.into(),
            if early {
                vec![first.into()]
            } else {
                vec![first.into(), end.into()]
            },
            if early { vec![0.97] } else { vec![0.97, 0.99] },
        )
        .unwrap();
        let ctx = market(125.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(curve.clone(), 0.01, 0.2).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        let mut t = terms(DoubleBarrierType::KnockIn);
        t.lower_barrier = 80.125;
        t.upper_barrier = 120.125;
        t.observation_mode = ObservationMode::Scheduled;
        t.observation_dates = vec![horizon];
        t.settlement_timing = SettlementTiming::AtHit;
        let mut touched = 0.0;
        for (direction, level) in [
            (OptionType::Put, t.lower_barrier),
            (OptionType::Call, t.upper_barrier),
        ] {
            let digital =
                DigitalOption::cash_or_nothing(direction, level, 1.0, t.effective_date, horizon)
                    .unwrap();
            touched += AnalyticDigitalEngine.price(&digital, &ctx).unwrap();
        }
        let option = DoubleTouchOption::cash(t.clone(), 1.0).unwrap();
        assert!((engine.price(&option, &ctx).unwrap() - touched).abs() < 3e-5);
        t.settlement_timing = SettlementTiming::AtExpiry;
        if early {
            category(
                engine.price(&DoubleTouchOption::cash(t, 1.0).unwrap(), &ctx),
                ErrorCategory::InvalidTimeRange,
            );
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
    let option = DoubleTouchOption::cash(t, 1.0).unwrap();
    let engine = FiniteDifferenceDoubleTouchEngine::new(FiniteDifferenceSettings {
        asset_step_count: 160,
        time_step_count: 800,
        ..Default::default()
    })
    .unwrap();
    assert!(engine.price(&option, &market(100.0)).unwrap() < 1e-8);
    let still = market(100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.03, 0.03, 1e-12).unwrap()),
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
        category(
            FiniteDifferenceDoubleTouchEngine::new(settings),
            ErrorCategory::InvalidParameter,
        );
    }
    for settings in [
        FiniteDifferenceSettings {
            asset_upper_boundary: Some(100.5),
            ..Default::default()
        },
        FiniteDifferenceSettings {
            scheme: FiniteDifferenceScheme::ExplicitEuler,
            time_step_count: 1,
            ..Default::default()
        },
    ] {
        let engine = FiniteDifferenceDoubleTouchEngine::new(settings).unwrap();
        category(
            engine.price(&option, &market(100.0)),
            ErrorCategory::InvalidParameter,
        );
    }
}
