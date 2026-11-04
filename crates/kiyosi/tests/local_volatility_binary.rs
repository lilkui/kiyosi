use kiyosi::*;

fn start() -> Timestamp {
    Date::new(2025, 1, 1).unwrap().start_of_day()
}
fn end() -> Timestamp {
    Date::new(2026, 1, 1).unwrap().start_of_day()
}
fn middle() -> Timestamp {
    Date::new(2025, 6, 30).unwrap().start_of_day()
}

fn settings() -> FiniteDifferenceSettings {
    FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 800,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    }
}

fn surface(last: Timestamp, skew: bool) -> LocalVolatilitySurface {
    LocalVolatilitySurface::new(
        vec![start(), last],
        vec![60.0, 100.0, 140.0],
        if skew {
            vec![vec![0.32, 0.24, 0.16], vec![0.36, 0.26, 0.18]]
        } else {
            vec![vec![0.2; 3]; 2]
        },
    )
    .unwrap()
}

fn local(skew: bool) -> PricingContext {
    PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface(end(), skew)).unwrap(),
        100.0,
        start(),
    )
    .unwrap()
}

fn bsm() -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        100.0,
        start(),
    )
    .unwrap()
}

#[test]
fn constant_binary_surfaces_match_bsm_for_cash_asset_directions_and_monitoring() {
    let engine = FiniteDifferenceBinaryBarrierEngine::new(settings()).unwrap();
    for (kind, level) in [
        (BarrierType::UpAndIn, 120.13),
        (BarrierType::UpAndOut, 120.13),
        (BarrierType::DownAndIn, 79.87),
        (BarrierType::DownAndOut, 79.87),
    ] {
        for direction in [OptionType::Call, OptionType::Put] {
            for asset in [false, true] {
                for scheduled in [false, true] {
                    let mut terms =
                        BinaryBarrierTerms::new(direction, 100.0, start(), end(), level, kind);
                    if scheduled {
                        terms.observation_mode = ObservationMode::Scheduled;
                        terms.observation_times = vec![middle(), end()];
                    }
                    let option = if asset {
                        BinaryBarrierOption::asset(terms)
                    } else {
                        BinaryBarrierOption::cash(terms, 10.0)
                    }
                    .unwrap();
                    let expected = engine.price(&option, &bsm()).unwrap();
                    let actual = engine.price(&option, &local(false)).unwrap();
                    // Separate full-domain digital grids contribute their established FD error.
                    let tolerance = if asset { 0.008 } else { 0.0008 };
                    assert!(
                        (actual - expected).abs() < tolerance,
                        "{kind:?} {direction:?} asset={asset} scheduled={scheduled}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn constant_touch_surfaces_match_bsm_for_settlement_and_double_boundaries() {
    let single = FiniteDifferenceBinaryBarrierEngine::new(settings()).unwrap();
    let double = FiniteDifferenceDoubleTouchEngine::new(settings()).unwrap();
    for scheduled in [false, true] {
        for (kind, level) in [
            (BarrierType::UpAndIn, 120.13),
            (BarrierType::UpAndOut, 120.13),
            (BarrierType::DownAndIn, 79.87),
            (BarrierType::DownAndOut, 79.87),
        ] {
            for timing in [SettlementTiming::AtHit, SettlementTiming::AtExpiry] {
                if timing == SettlementTiming::AtHit
                    && !matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn)
                {
                    continue;
                }
                for asset in [false, true] {
                    let mut terms = TouchOptionTerms::new(start(), end(), level, kind);
                    terms.settlement_timing = timing;
                    if scheduled {
                        terms.observation_mode = ObservationMode::Scheduled;
                        terms.observation_times = vec![middle(), end()];
                    }
                    let option = if asset {
                        TouchOption::asset(terms)
                    } else {
                        TouchOption::cash(terms, 10.0)
                    }
                    .unwrap();
                    let expected = single.price(&option, &bsm()).unwrap();
                    let actual = single.price(&option, &local(false)).unwrap();
                    assert!(
                        (actual - expected).abs() < 1e-10,
                        "{kind:?} {timing:?}: {actual} != {expected}"
                    );
                }
            }
        }
        for (kind, timing) in [
            (DoubleBarrierType::KnockIn, SettlementTiming::AtHit),
            (DoubleBarrierType::KnockIn, SettlementTiming::AtExpiry),
            (DoubleBarrierType::KnockOut, SettlementTiming::AtExpiry),
        ] {
            let mut terms = DoubleTouchOptionTerms::new(start(), end(), 79.87, 120.13, kind);
            terms.settlement_timing = timing;
            if scheduled {
                terms.observation_mode = ObservationMode::Scheduled;
                terms.observation_times = vec![middle(), end()];
            }
            let option = DoubleTouchOption::cash(terms, 10.0).unwrap();
            let expected = double.price(&option, &bsm()).unwrap();
            let actual = double.price(&option, &local(false)).unwrap();
            assert!(
                (actual - expected).abs() < 1e-10,
                "{kind:?} {timing:?}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn skew_binary_in_out_parity_uses_unrestricted_digital_continuation() {
    let engine = FiniteDifferenceBinaryBarrierEngine::new(settings()).unwrap();
    let digital_engine = FiniteDifferenceDigitalEngine::new(settings()).unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for scheduled in [false, true] {
        for asset in [false, true] {
            for (inside, outside, level) in [
                (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.13),
                (BarrierType::DownAndIn, BarrierType::DownAndOut, 79.87),
            ] {
                let make = |kind| {
                    let mut terms = BinaryBarrierTerms::new(
                        OptionType::Put,
                        100.0,
                        start(),
                        end(),
                        level,
                        kind,
                    );
                    if scheduled {
                        terms.observation_mode = ObservationMode::Scheduled;
                        terms.observation_times = vec![middle(), end()];
                    }
                    if asset {
                        BinaryBarrierOption::asset(terms)
                    } else {
                        BinaryBarrierOption::cash(terms, 10.0)
                    }
                    .unwrap()
                };
                let digital = if asset {
                    DigitalOption::asset_or_nothing(OptionType::Put, 100.0, start(), end())
                } else {
                    DigitalOption::cash_or_nothing(OptionType::Put, 100.0, 10.0, start(), end())
                }
                .unwrap();
                let inn = engine
                    .price_with_greeks(&make(inside), &local(true), request, Default::default())
                    .unwrap();
                let out = engine
                    .price_with_greeks(&make(outside), &local(true), request, Default::default())
                    .unwrap();
                let total = digital_engine
                    .price_with_greeks(&digital, &local(true), request, Default::default())
                    .unwrap();
                // Independently meshed continuation and barrier solves obey parity up to FD error.
                let error = (inn.price() + out.price() - total.price()).abs();
                assert!(
                    error < if asset { 0.008 } else { 0.0008 },
                    "price {inside:?} asset={asset} scheduled={scheduled}: {error}"
                );
                for greek in [Greek::Delta, Greek::Gamma] {
                    let error = (inn.require(greek).unwrap() + out.require(greek).unwrap()
                        - total.require(greek).unwrap())
                    .abs();
                    let tolerance = if greek == Greek::Delta {
                        0.0002
                    } else {
                        0.00003
                    };
                    assert!(
                        error < tolerance,
                        "{greek:?} {inside:?} asset={asset} scheduled={scheduled}: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn local_touch_coverage_follows_observations_and_known_cashflows() {
    let engine = FiniteDifferenceBinaryBarrierEngine::new(settings()).unwrap();
    let curve = RateTermStructure::new(start(), vec![middle()], vec![0.97]).unwrap();
    let context = PricingContext::new(
        LocalVolatilityParameters::new(curve.clone(), curve, surface(middle(), true)).unwrap(),
        100.0,
        start(),
    )
    .unwrap();
    let mut terms = TouchOptionTerms::new(start(), end(), 120.0, BarrierType::UpAndIn);
    terms.observation_mode = ObservationMode::Scheduled;
    terms.observation_times = vec![middle()];
    terms.settlement_timing = SettlementTiming::AtHit;
    let hit = TouchOption::cash(terms.clone(), 10.0).unwrap();
    let price = engine.price(&hit, &context).unwrap();
    assert!(price > 0.0 && price < 9.7);
    terms.settlement_timing = SettlementTiming::AtExpiry;
    let expiry = TouchOption::cash(terms, 10.0).unwrap();
    assert!(matches!(
        engine.price(&expiry, &context),
        Err(Error::InvalidInput(_))
    ));
    let past = Timestamp::new(middle().date(), 0, 0, 0, 1).unwrap();
    let settled_context = PricingContext::new(
        LocalVolatilityParameters::new(
            0.04,
            0.01,
            surface(Date::new(2025, 7, 1).unwrap().start_of_day(), true),
        )
        .unwrap(),
        100.0,
        past,
    )
    .unwrap();
    assert_eq!(
        engine
            .price(
                &hit.replace_history(Some(BarrierTouchState::Touched)),
                &settled_context
            )
            .unwrap(),
        0.0
    );
    let cash = engine
        .price(
            &expiry.replace_history(Some(BarrierTouchState::Touched)),
            &settled_context,
        )
        .unwrap();
    let remaining = (185.0 - 1.0 / 86_400_000_000.0) / 365.0;
    assert!((cash - 10.0 * (-0.04_f64 * remaining).exp()).abs() < 1e-12);
    assert_eq!(
        engine
            .price_with_greeks(
                &hit.replace_history(Some(BarrierTouchState::Touched)),
                &settled_context,
                GreeksRequest::all(),
                Default::default(),
            )
            .unwrap()
            .get(Greek::Vega),
        None
    );
}

#[test]
fn local_binary_triggered_continuation_and_event_greeks_follow_history() {
    let engine = FiniteDifferenceBinaryBarrierEngine::new(settings()).unwrap();
    let mut terms = BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        start(),
        end(),
        120.0,
        BarrierType::UpAndIn,
    );
    terms.touch_state = Some(BarrierTouchState::Touched);
    let option = BinaryBarrierOption::cash(terms, 10.0).unwrap();
    let context = local(true)
        .replace(PricingContextChanges {
            valuation_time: Some(Date::new(2025, 1, 2).unwrap().start_of_day()),
            ..Default::default()
        })
        .unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start(), end()).unwrap();
    let expected = FiniteDifferenceDigitalEngine::new(settings())
        .unwrap()
        .price(&digital, &context)
        .unwrap();
    assert_eq!(engine.price(&option, &context).unwrap(), expected);
    let short = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                LocalVolatilityParameters::new(0.04, 0.01, surface(middle(), true))
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine.price(&option, &short),
        Err(Error::InvalidInput(_))
    ));
    let boundary = local(true)
        .replace(PricingContextChanges {
            spot_price: Some(120.0),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(
            &option.replace_history(None),
            &boundary,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            Default::default(),
        )
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    let expiry = boundary
        .replace(PricingContextChanges {
            valuation_time: Some(end()),
            ..Default::default()
        })
        .unwrap();
    let option = option.replace_history(Some(BarrierTouchState::Untouched));
    let result = engine
        .price_with_greeks(
            &option,
            &expiry,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            Default::default(),
        )
        .unwrap();
    assert_eq!(result.price(), 10.0);
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(
        engine
            .price_with_greeks(&option, &expiry, GreeksRequest::all(), Default::default())
            .unwrap()
            .get(Greek::Vega),
        None
    );
}

#[test]
fn skew_touch_greeks_hold_absolute_surface_fixed() {
    let engine = FiniteDifferenceDoubleTouchEngine::new(settings()).unwrap();
    let option = DoubleTouchOption::cash(
        DoubleTouchOptionTerms::new(start(), end(), 80.0, 120.0, DoubleBarrierType::KnockOut),
        10.0,
    )
    .unwrap();
    let context = local(true);
    let result = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta, Greek::Gamma]),
            Default::default(),
        )
        .unwrap();
    let price = |spot| {
        engine
            .price(
                &option,
                &context
                    .replace(PricingContextChanges {
                        spot_price: Some(spot),
                        ..Default::default()
                    })
                    .unwrap(),
            )
            .unwrap()
    };
    let h = 0.05;
    let up = price(100.0 + h);
    let down = price(100.0 - h);
    assert!((result.require(Greek::Delta).unwrap() - (up - down) / (2.0 * h)).abs() < 1e-6);
    assert!(
        (result.require(Greek::Gamma).unwrap() - (up + down - 2.0 * result.price()) / h.powi(2))
            .abs()
            < 1e-6
    );
    assert!((result.price() - engine.price(&option, &local(false)).unwrap()).abs() > 0.2);
}

#[test]
fn remote_binary_knock_in_remains_nonnegative_at_low_volatility() {
    let surface = LocalVolatilitySurface::new(
        vec![start(), end()],
        vec![50.0, 160.0],
        vec![vec![0.05; 2]; 2],
    )
    .unwrap();
    let context = PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface).unwrap(),
        100.0,
        start(),
    )
    .unwrap();
    let engine = FiniteDifferenceBinaryBarrierEngine::new(settings()).unwrap();
    for (kind, level) in [
        (BarrierType::UpAndIn, 150.0),
        (BarrierType::DownAndIn, 60.0),
    ] {
        for direction in [OptionType::Call, OptionType::Put] {
            let option = BinaryBarrierOption::cash(
                BinaryBarrierTerms::new(direction, 100.0, start(), end(), level, kind),
                10.0,
            )
            .unwrap();
            let price = engine.price(&option, &context).unwrap();
            assert!(
                (0.0..1e-8).contains(&price),
                "{kind:?} {direction:?}: {price}"
            );
        }
    }
}

#[test]
fn skew_binary_barriers_match_independent_quantlib_fd() {
    let model = LocalVolatilityParameters::new(
        0.05,
        0.02,
        LocalVolatilitySurface::new(
            vec![start(), end()],
            vec![80.0, 120.0],
            vec![vec![0.3, 0.15], vec![0.35, 0.2]],
        )
        .unwrap(),
    )
    .unwrap();
    let context = PricingContext::new(model, 100.0, start()).unwrap();
    let engine = FiniteDifferenceBinaryBarrierEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 1600,
        ..settings()
    })
    .unwrap();
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    // QuantLib 1.43, 3200 spatial nodes; extrapolated 6400/12800 time grids.
    // Python independently verifies the previous refinement before these budgets.
    for (option, expected) in [
        (
            BinaryBarrierOption::cash(
                BinaryBarrierTerms::new(
                    OptionType::Call,
                    100.0,
                    start(),
                    end(),
                    120.0,
                    BarrierType::UpAndOut,
                ),
                10.0,
            )
            .unwrap(),
            [
                1.374_253_078_380_399,
                -0.031_534_593_383_106_59,
                -0.004_735_134_551_215_346,
            ],
        ),
        (
            BinaryBarrierOption::asset(BinaryBarrierTerms::new(
                OptionType::Put,
                100.0,
                start(),
                end(),
                80.0,
                BarrierType::DownAndOut,
            ))
            .unwrap(),
            [
                9.998_057_673_962_732,
                0.172_085_024_991_698_23,
                -0.038_324_533_172_155_74,
            ],
        ),
    ] {
        let actual = engine
            .price_with_greeks(&option, &context, request, Default::default())
            .unwrap();
        for ((a, b), tolerance) in [
            actual.price(),
            actual.require(Greek::Delta).unwrap(),
            actual.require(Greek::Gamma).unwrap(),
        ]
        .into_iter()
        .zip(expected)
        .zip([0.003, 0.0003, 0.00003])
        {
            assert!((a - b).abs() < tolerance, "{a} != {b}");
        }
    }
}

#[test]
fn skew_double_boundaries_match_independent_path_estimates() {
    let expiry = Date::new(2025, 4, 1).unwrap().start_of_day();
    let model = LocalVolatilityParameters::new(
        0.04,
        0.01,
        LocalVolatilitySurface::new(
            vec![start(), expiry],
            vec![60.0, 100.0, 160.0],
            vec![vec![0.32, 0.24, 0.16]; 2],
        )
        .unwrap(),
    )
    .unwrap();
    let context = PricingContext::new(model, 101.0, start()).unwrap();
    let settings = FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 1600,
        asset_upper_boundary: Some(440.0),
        ..Default::default()
    };
    // 65,536 QL antithetic pairs; budgets include measured sampling/monitoring
    // uncertainty. Python retains the raw nested-grid evidence in the JSON fixture.
    for (kind, expected, budget) in [
        (DoubleBarrierType::KnockOut, 1.548_521_476_101_878_4, 0.0593),
        (DoubleBarrierType::KnockIn, 4.130_639_334_741_136, 0.115),
    ] {
        let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start(),
            expiry,
            85.0,
            115.0,
            kind,
        ))
        .unwrap();
        let value = FiniteDifferenceDoubleBarrierEngine::new(settings)
            .unwrap()
            .price(&option, &context)
            .unwrap();
        assert!((value - expected).abs() < budget);
    }
    for (kind, expected) in [
        (DoubleBarrierType::KnockOut, 5.640_453_799_066_48),
        (DoubleBarrierType::KnockIn, 4.261_400_863_972_829),
    ] {
        let terms = DoubleTouchOptionTerms::new(start(), expiry, 85.0, 115.0, kind);
        let option = DoubleTouchOption::cash(terms, 10.0).unwrap();
        let value = FiniteDifferenceDoubleTouchEngine::new(settings)
            .unwrap()
            .price(&option, &context)
            .unwrap();
        assert!((value - expected).abs() < 0.1244);
    }
}
