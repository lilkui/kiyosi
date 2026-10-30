use kiyosi::*;

#[test]
fn barrier_crr_prices_an_expiry_only_observation() {
    let start = Date::new(2025, 1, 1).unwrap();
    let end = Date::new(2025, 1, 3).unwrap();
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        101.0,
        BarrierType::UpAndOut,
    );
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_dates = vec![end];
    terms.rebate = 2.0;
    let option = BarrierOption::new(terms).unwrap();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.2).unwrap(),
        100.0,
        start.into(),
    )
    .unwrap();
    let engine = CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 1 }).unwrap();
    // One up move pays the rebate; one down move expires worthless.
    let up = (0.2_f64 * (2.0_f64 / 365.0).sqrt()).exp();
    let expected = 2.0 / (1.0 + up);
    assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-12);
}

fn terms(kind: BarrierType, days: u32) -> BarrierOptionTerms {
    BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        Date::new(2025, 1, 1).unwrap(),
        Date::new(2025, 1, 1 + days).unwrap(),
        if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
            102.0
        } else {
            98.0
        },
        kind,
    )
}
fn market(time: Timestamp, spot: f64, rate: f64, sigma: f64) -> PricingContext {
    PricingContext::with_calendar(
        BlackScholesMertonParameters::new(rate, 0.01, sigma).unwrap(),
        spot,
        time,
        TradingCalendar::all_days(),
    )
    .unwrap()
}
const KINDS: [BarrierType; 4] = [
    BarrierType::UpAndIn,
    BarrierType::UpAndOut,
    BarrierType::DownAndIn,
    BarrierType::DownAndOut,
];
fn is_in(kind: BarrierType) -> bool {
    matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn)
}
fn is_up(kind: BarrierType) -> bool {
    matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut)
}

// Independent path enumeration also prices subtrees for node-Greek references.
fn enumerated(
    option: &BarrierOption,
    context: &PricingContext,
    steps: usize,
    observed: &[bool],
    initial: (usize, usize),
) -> f64 {
    let (initial_level, initial_node) = initial;
    let time = context.valuation_time();
    let days = f64::from(option.expiry_date().day() - time.date().day())
        - time.micros_since_midnight() as f64 / 86_400_000_000.0;
    let dt = days / 365.0 / steps as f64;
    let model = context.model_parameters();
    let jump = model.volatility() * dt.sqrt();
    let rate = model.risk_free_rate().unwrap();
    let p = (((rate - model.dividend_yield().unwrap()) * dt).exp() - (-jump).exp())
        / (jump.exp() - (-jump).exp());
    let mut result = 0.0;
    for path in 0..(1 << (steps - initial_level)) {
        let mut node = initial_node;
        let mut hit = None;
        let mut weight = 1.0;
        for (level, &monitored) in observed
            .iter()
            .enumerate()
            .take(steps + 1)
            .skip(initial_level)
        {
            let asset = context.spot_price() * ((2.0 * node as f64 - level as f64) * jump).exp();
            if hit.is_none()
                && monitored
                && if is_up(option.barrier_type()) {
                    asset >= option.barrier_level()
                } else {
                    asset <= option.barrier_level()
                }
            {
                hit = Some(level);
            }
            if level < steps {
                let up = path & (1 << (level - initial_level)) != 0;
                node += usize::from(up);
                weight *= if up { p } else { 1.0 - p };
            }
        }
        let asset = context.spot_price() * ((2.0 * node as f64 - steps as f64) * jump).exp();
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let pays_rebate = hit.is_some() != is_in(option.barrier_type());
        let amount = if pays_rebate {
            option.rebate()
        } else {
            (sign * (asset - option.strike())).max(0.0)
        };
        let payment_level = if pays_rebate && option.rebate_timing() == RebateTiming::AtHit {
            hit.unwrap()
        } else {
            steps
        };
        result += weight * amount * (-rate * (payment_level - initial_level) as f64 * dt).exp();
    }
    result
}

#[test]
fn prices_and_node_greeks_match_independent_paths_across_supported_terms() {
    for steps in [1, 2, 5] {
        let engine = CoxRossRubinsteinBarrierEngine::new(BinomialSettings {
            step_count: steps as i32,
        })
        .unwrap();
        for kind in KINDS {
            for direction in [OptionType::Call, OptionType::Put] {
                for scheduled in [false, true] {
                    for rebate in [0.0, 2.0] {
                        for timing in [RebateTiming::AtHit, RebateTiming::AtExpiry] {
                            if is_in(kind) && timing == RebateTiming::AtHit {
                                continue;
                            }
                            let mut t = terms(kind, 10);
                            t.option_type = direction;
                            t.rebate = rebate;
                            t.rebate_timing = timing;
                            let context = market(t.effective_date.into(), 100.0, 0.04, 0.25);
                            let jump = 0.25 * (10.0 / 365.0 / steps as f64).sqrt();
                            t.barrier_level =
                                100.0 * (if is_up(kind) { 1.5 * jump } else { -1.5 * jump }).exp();
                            if scheduled {
                                t.observation_mode = ObservationMode::Scheduled;
                                t.observation_dates = vec![t.expiry_date];
                            }
                            let option = BarrierOption::new(t).unwrap();
                            let mut observed = vec![!scheduled; steps + 1];
                            observed[steps] = true;
                            let result = engine
                                .price_with_greeks(
                                    &option,
                                    &context,
                                    GreeksRequest::all(),
                                    Default::default(),
                                )
                                .unwrap();
                            assert!(
                                (result.price()
                                    - enumerated(&option, &context, steps, &observed, (0, 0)))
                                .abs()
                                    < 2e-12
                            );
                            assert_eq!(result.price(), engine.price(&option, &context).unwrap());
                            let low = enumerated(&option, &context, steps, &observed, (1, 0));
                            let high = enumerated(&option, &context, steps, &observed, (1, 1));
                            assert!(
                                (result.require(Greek::Delta).unwrap()
                                    - (high - low) / (100.0 * (jump.exp() - (-jump).exp())))
                                .abs()
                                    < 1e-10
                            );
                            assert_eq!(result.has(Greek::Gamma), steps >= 2);
                            if steps >= 2 {
                                let v: [f64; 3] = std::array::from_fn(|node| {
                                    enumerated(&option, &context, steps, &observed, (2, node))
                                });
                                let gamma = ((v[2] - v[1]) / (100.0 * (2.0 * jump).exp_m1())
                                    - (v[1] - v[0]) / (-100.0 * (-2.0 * jump).exp_m1()))
                                    / (50.0 * ((2.0 * jump).exp() - (-2.0 * jump).exp()));
                                assert!(
                                    (result.require(Greek::Gamma).unwrap() - gamma).abs() < 1e-10
                                );
                            }
                            for greek in Greek::ALL {
                                if !matches!(greek, Greek::Delta | Greek::Gamma) {
                                    assert!(!result.has(greek));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn exact_intraday_alignment_early_ending_monitoring_and_node_equality() {
    for kind in KINDS {
        let mut t = terms(kind, 4);
        t.barrier_level = 100.0;
        t.rebate = 2.0;
        t.observation_mode = ObservationMode::Scheduled;
        t.observation_dates = vec![Date::new(2025, 1, 3).unwrap()];
        let context = market(
            Timestamp::new(t.effective_date, 12, 0, 0, 0).unwrap(),
            100.0,
            -0.02,
            0.3,
        );
        let option = BarrierOption::new(t).unwrap();
        let engine =
            CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 7 }).unwrap();
        let expected = enumerated(
            &option,
            &context,
            7,
            &[false, false, false, true, false, false, false, false],
            (0, 0),
        );
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-11);
        let shifted = context
            .replace(PricingContextChanges {
                valuation_time: Some(Timestamp::new(option.effective_date(), 12, 0, 0, 1).unwrap()),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            engine.price(&option, &shifted).unwrap_err(),
            Error::UnsupportedOperation(_)
        ));
    }
    for sigma in [0.1, 0.2, 0.3, 0.4] {
        for up in [false, true] {
            let mut t = terms(
                if up {
                    BarrierType::UpAndOut
                } else {
                    BarrierType::DownAndOut
                },
                2,
            );
            t.option_type = if up {
                OptionType::Call
            } else {
                OptionType::Put
            };
            let jump = sigma * (1.0_f64 / 365.0).sqrt();
            t.barrier_level = 100.0 * (if up { jump } else { -jump }).exp();
            t.observation_mode = ObservationMode::Scheduled;
            t.observation_dates = vec![Date::new(2025, 1, 2).unwrap()];
            let context = market(t.effective_date.into(), 100.0, 0.04, sigma);
            let option = BarrierOption::new(t).unwrap();
            let engine =
                CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 2 }).unwrap();
            assert!(
                (engine.price(&option, &context).unwrap()
                    - enumerated(&option, &context, 2, &[false, true, false], (0, 0)))
                .abs()
                    < 1e-12
            );
        }
    }
}

#[test]
fn continuous_refinement_matches_analytic_prices_and_node_greeks() {
    for kind in KINDS {
        for direction in [OptionType::Call, OptionType::Put] {
            let mut t = terms(kind, 10);
            t.expiry_date = Date::new(2026, 1, 1).unwrap();
            t.barrier_level = if is_up(kind) { 120.0 } else { 80.0 };
            t.rebate = 2.0;
            t.option_type = direction;
            let context = market(t.effective_date.into(), 100.0, 0.04, 0.2);
            let option = BarrierOption::new(t).unwrap();
            let expected = AnalyticBarrierEngine
                .price_with_greeks(
                    &option,
                    &context,
                    GreeksRequest::new([Greek::Delta, Greek::Gamma]),
                    Default::default(),
                )
                .unwrap();
            for steps in [1023, 1024, 2047, 2048] {
                let result =
                    CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: steps })
                        .unwrap()
                        .price_with_greeks(
                            &option,
                            &context,
                            GreeksRequest::all(),
                            Default::default(),
                        )
                        .unwrap();
                assert!((result.price() - expected.price()).abs() < 0.25);
                assert!(
                    (result.require(Greek::Delta).unwrap()
                        - expected.require(Greek::Delta).unwrap())
                    .abs()
                        < 0.07
                );
                assert!(
                    (result.require(Greek::Gamma).unwrap()
                        - expected.require(Greek::Gamma).unwrap())
                    .abs()
                        < 0.01
                );
            }
        }
    }
}

#[test]
fn settings_validation_order_history_and_failure_state_match_core_contracts() {
    assert_eq!(
        CoxRossRubinsteinBarrierEngine::default().settings(),
        BinomialSettings::default()
    );
    for steps in [0, -1, 1_000_001] {
        assert!(matches!(
            CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: steps })
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
    for steps in [1, 1_000_000] {
        assert!(
            CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: steps }).is_ok()
        );
    }
    let engine = CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 3 }).unwrap();
    let mut t = terms(BarrierType::UpAndOut, 10);
    t.barrier_level = 120.0;
    t.rebate = 3.0;
    t.rebate_timing = RebateTiming::AtHit;
    t.observation_mode = ObservationMode::Scheduled;
    t.observation_dates = vec![
        t.effective_date,
        Date::new(2025, 1, 2).unwrap(),
        t.expiry_date,
    ];
    let context = market(
        Timestamp::new(t.effective_date, 12, 0, 0, 0).unwrap(),
        100.0,
        0.04,
        0.2,
    );
    let unknown = BarrierOption::new(t.clone()).unwrap();
    assert!(matches!(
        engine.price(&unknown, &context).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let untouched = unknown.replace(Some(BarrierTouchState::Untouched));
    assert!(matches!(
        engine.price(&untouched, &context).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let touched = unknown.replace(Some(BarrierTouchState::Touched));
    assert_eq!(engine.price(&touched, &context).unwrap(), 0.0);
    assert!(matches!(
        engine
            .price(&touched, &market(t.effective_date.into(), 100.0, 0.04, 0.2))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let hit = market(t.effective_date.into(), 120.0, 0.04, 0.2);
    let result = engine
        .price_with_greeks(&unknown, &hit, GreeksRequest::all(), Default::default())
        .unwrap();
    assert_eq!(result.price(), 3.0);
    assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
    assert!(matches!(
        engine
            .price_with_greeks(&unknown, &hit, GreeksRequest::default(), Default::default())
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &unknown,
                &hit,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    spot_shift: 0.0,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(unknown.touch_state(), None);
    assert_eq!(engine.settings().step_count, 3);

    let curve = RateTermStructure::new(
        t.effective_date.into(),
        vec![t.expiry_date.into()],
        vec![0.99],
    );
    let curve = curve.unwrap();
    let nonflat = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.01, 0.2).unwrap(),
        120.0,
        t.expiry_date.into(),
    )
    .unwrap();
    assert!(matches!(
        engine.price(&unknown, &nonflat).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert!(matches!(
        engine
            .price_with_greeks(
                &unknown,
                &nonflat,
                GreeksRequest::default(),
                Default::default()
            )
            .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let live = BarrierOption::new(terms(BarrierType::UpAndOut, 10)).unwrap();
    for (rate, sigma) in [(0.04, 1e-200), (0.04, 1e6), (-1e6, 0.2)] {
        assert!(matches!(
            engine
                .price(&live, &market(t.effective_date.into(), 100.0, rate, sigma))
                .unwrap_err(),
            Error::ComputationFailure(_)
        ));
    }
    for time in [
        Date::new(2024, 12, 31).unwrap().into(),
        Timestamp::new(t.expiry_date, 0, 0, 0, 1).unwrap(),
    ] {
        assert!(matches!(
            engine
                .price(&live, &market(time, 100.0, 0.04, 0.2))
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
    }
}

#[test]
fn resolved_states_expiry_and_original_fixing_times_match_analytic_settlement() {
    for kind in KINDS {
        let mut t = terms(kind, 10);
        t.barrier_level = 100.0;
        t.rebate = 3.0;
        t.observation_mode = ObservationMode::Scheduled;
        t.observation_dates = vec![Date::new(2025, 1, 3).unwrap()];
        let option = BarrierOption::new(t.clone()).unwrap();
        let engine = CoxRossRubinsteinBarrierEngine::default();
        for time in [
            t.observation_dates[0].into(),
            Timestamp::new(t.observation_dates[0], 12, 0, 0, 0).unwrap(),
            t.expiry_date.into(),
        ] {
            let option = if time > t.observation_dates[0].start_of_day() {
                option.replace(Some(BarrierTouchState::Untouched))
            } else {
                option.clone()
            };
            let context = market(time, 100.0, 0.04, 0.2);
            let result = engine
                .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                .unwrap();
            assert_eq!(
                result.price(),
                AnalyticBarrierEngine.price(&option, &context).unwrap()
            );
            if time == t.expiry_date.start_of_day() || time == t.observation_dates[0].start_of_day()
            {
                assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
            }
        }
        t.observation_mode = ObservationMode::Continuous;
        t.observation_dates.clear();
        for history in [BarrierTouchState::Untouched, BarrierTouchState::Touched] {
            t.touch_state = Some(history);
            for spot in [90.0, 100.0, 110.0] {
                let option = BarrierOption::new(t.clone()).unwrap();
                let context = market(t.expiry_date.into(), spot, 0.04, 0.2);
                let result = engine
                    .price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())
                    .unwrap();
                assert_eq!(
                    result.price(),
                    AnalyticBarrierEngine.price(&option, &context).unwrap()
                );
                assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
            }
        }
    }
}

#[test]
fn solver_tree_bounds_scalar_sensitivities_and_fixed_cashflows() {
    let mut t = terms(BarrierType::DownAndOut, 10);
    t.barrier_level = 60.0;
    t.expiry_date = Date::new(2026, 1, 1).unwrap();
    t.touch_state = Some(BarrierTouchState::Untouched);
    let context = market(t.effective_date.into(), 100.0, 0.04, 0.2);
    let option = BarrierOption::new(t.clone()).unwrap();
    let engine = CoxRossRubinsteinBarrierEngine::default();
    let quote = engine.price(&option, &context).unwrap();
    let sigma = implied_volatility(
        &engine,
        &option,
        &context,
        quote,
        ImpliedVolatilitySettings {
            lower_bound: 1e-8,
            upper_bound: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    let shifted = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                context
                    .model_parameters()
                    .replace(BlackScholesMertonParameterChanges {
                        volatility: Some(sigma),
                        ..Default::default()
                    })
                    .unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!((engine.price(&option, &shifted).unwrap() - quote).abs() < 2e-7);
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            quote,
            ImpliedVolatilitySettings {
                lower_bound: 0.0001,
                upper_bound: 0.0002,
                ..Default::default()
            }
        )
        .unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    let result = numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([Greek::Vega, Greek::Rho]),
        Default::default(),
    )
    .unwrap();
    let vanilla =
        EuropeanOption::new(t.option_type, t.strike, t.effective_date, t.expiry_date).unwrap();
    let expected = AnalyticEuropeanEngine
        .price_with_greeks(&vanilla, &context, GreeksRequest::all())
        .unwrap();
    assert!(
        (result.require(Greek::Vega).unwrap() - expected.require(Greek::Vega).unwrap()).abs()
            < 0.005
    );
    assert!(
        (result.require(Greek::Rho).unwrap() - expected.require(Greek::Rho).unwrap()).abs() < 0.005
    );
    t.touch_state = Some(BarrierTouchState::Touched);
    t.rebate = 7.0;
    let fixed = BarrierOption::new(t.clone()).unwrap();
    let later = market(Date::new(2025, 1, 2).unwrap().into(), 100.0, 0.04, 0.2);
    assert!(matches!(
        implied_volatility(
            &engine,
            &fixed,
            &later,
            engine.price(&fixed, &later).unwrap(),
            Default::default()
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    assert_eq!(engine.price(&option, &context).unwrap(), quote);
    assert_eq!(context.model_parameters().volatility(), 0.2);
}
