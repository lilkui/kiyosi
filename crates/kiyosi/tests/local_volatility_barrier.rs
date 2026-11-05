use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}

fn local(spot: f64, end: Timestamp, skew: bool) -> PricingContext {
    let (start, _) = dates();
    let values = if skew {
        vec![vec![0.3, 0.15], vec![0.35, 0.2]]
    } else {
        vec![vec![0.2; 2]; 2]
    };
    PricingContext::new(
        LocalVolatilityParameters::new(
            0.05,
            0.02,
            LocalVolatilitySurface::new(vec![start, end], vec![80.0, 120.0], values).unwrap(),
        )
        .unwrap(),
        spot,
        start,
    )
    .unwrap()
}

fn settings() -> FiniteDifferenceSettings {
    FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 800,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    }
}

fn request() -> GreeksRequest {
    GreeksRequest::new([Greek::Delta, Greek::Gamma])
}

fn assert_close(actual: &PricingResult, expected: &PricingResult) {
    // Full-domain LV continuation and BSM finite-edge formulas use different meshes.
    for (a, b, tolerance) in [
        (actual.price(), expected.price(), 0.002),
        (
            actual.require(Greek::Delta).unwrap(),
            expected.require(Greek::Delta).unwrap(),
            0.0002,
        ),
        (
            actual.require(Greek::Gamma).unwrap(),
            expected.require(Greek::Gamma).unwrap(),
            0.00003,
        ),
    ] {
        assert!(
            (a - b).abs() < tolerance,
            "{a} != {b}; tolerance {tolerance}"
        );
    }
}

#[test]
fn constant_surface_matches_single_and_double_barrier_states() {
    let (start, end) = dates();
    let local = local(100.0, end, false);
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let single = FiniteDifferenceBarrierEngine::new(settings()).unwrap();
    let double = FiniteDifferenceDoubleBarrierEngine::new(settings()).unwrap();
    for mode in [ObservationMode::Continuous, ObservationMode::Scheduled] {
        for direction in [OptionType::Call, OptionType::Put] {
            for kind in [
                BarrierType::UpAndIn,
                BarrierType::UpAndOut,
                BarrierType::DownAndIn,
                BarrierType::DownAndOut,
            ] {
                for timing in [RebateTiming::AtExpiry, RebateTiming::AtHit] {
                    let is_in = matches!(kind, BarrierType::UpAndIn | BarrierType::DownAndIn);
                    if is_in && timing == RebateTiming::AtHit {
                        continue;
                    }
                    let level = if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
                        120.0
                    } else {
                        80.0
                    };
                    let mut terms =
                        BarrierOptionTerms::new(direction, 100.0, start, end, level, kind);
                    terms.rebate = 2.0;
                    terms.rebate_timing = timing;
                    terms.observation_mode = mode;
                    if mode == ObservationMode::Scheduled {
                        terms.observation_times =
                            vec![Date::new(2025, 6, 30).unwrap().start_of_day(), end];
                    }
                    let option = BarrierOption::new(terms).unwrap();
                    let expected = single.price_with_greeks(&option, &bsm, request()).unwrap();
                    let actual = single
                        .price_with_greeks(&option, &local, request())
                        .unwrap();
                    assert_close(&actual, &expected);
                }
            }
            for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
                for timing in [RebateTiming::AtExpiry, RebateTiming::AtHit] {
                    if kind == DoubleBarrierType::KnockIn && timing == RebateTiming::AtHit {
                        continue;
                    }
                    let mut terms = DoubleBarrierOptionTerms::new(
                        direction, 100.0, start, end, 80.0, 120.0, kind,
                    );
                    terms.rebate = 2.0;
                    terms.rebate_timing = timing;
                    terms.observation_mode = mode;
                    if mode == ObservationMode::Scheduled {
                        terms.observation_times =
                            vec![Date::new(2025, 6, 30).unwrap().start_of_day(), end];
                    }
                    let option = DoubleBarrierOption::new(terms).unwrap();
                    let expected = double.price_with_greeks(&option, &bsm, request()).unwrap();
                    let actual = double
                        .price_with_greeks(&option, &local, request())
                        .unwrap();
                    assert_close(&actual, &expected);
                }
            }
        }
    }
}

#[test]
fn skewed_continuous_knock_in_out_parity_includes_native_greeks() {
    let (start, end) = dates();
    let context = local(100.0, end, true);
    let vanilla = FiniteDifferenceVanillaEngine::new(settings()).unwrap();
    let single = FiniteDifferenceBarrierEngine::new(settings()).unwrap();
    let double = FiniteDifferenceDoubleBarrierEngine::new(settings()).unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = EuropeanOption::new(direction, 100.0, start, end).unwrap();
        let expected = vanilla
            .price_with_greeks(&option, &context, request())
            .unwrap();
        let check = |inside: PricingResult, outside: PricingResult| {
            assert!(
                (inside.price() + outside.price() - expected.price() - 2.0 * (-0.05_f64).exp())
                    .abs()
                    < 0.002
            );
            for (greek, tolerance) in [(Greek::Delta, 0.0002), (Greek::Gamma, 0.00003)] {
                assert!(
                    (inside.require(greek).unwrap() + outside.require(greek).unwrap()
                        - expected.require(greek).unwrap())
                    .abs()
                        < tolerance
                );
            }
        };
        for (ki, ko, level) in [
            (BarrierType::UpAndIn, BarrierType::UpAndOut, 120.0),
            (BarrierType::DownAndIn, BarrierType::DownAndOut, 80.0),
        ] {
            let evaluate = |kind| {
                let mut terms = BarrierOptionTerms::new(direction, 100.0, start, end, level, kind);
                terms.rebate = 2.0;
                single
                    .price_with_greeks(&BarrierOption::new(terms).unwrap(), &context, request())
                    .unwrap()
            };
            check(evaluate(ki), evaluate(ko));
        }
        let evaluate = |kind| {
            let mut terms =
                DoubleBarrierOptionTerms::new(direction, 100.0, start, end, 80.0, 120.0, kind);
            terms.rebate = 2.0;
            double
                .price_with_greeks(
                    &DoubleBarrierOption::new(terms).unwrap(),
                    &context,
                    request(),
                )
                .unwrap()
        };
        check(
            evaluate(DoubleBarrierType::KnockIn),
            evaluate(DoubleBarrierType::KnockOut),
        );
    }
}

#[test]
fn remote_low_volatility_knock_in_does_not_subtract_nearly_equal_prices() {
    let (start, end) = dates();
    let model = LocalVolatilityParameters::new(
        0.0,
        0.0,
        LocalVolatilitySurface::new(vec![start, end], vec![80.0, 120.0], vec![vec![0.05; 2]; 2])
            .unwrap(),
    )
    .unwrap();
    let context = PricingContext::new(model, 100.0, start).unwrap();
    let option = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        150.0,
        BarrierType::UpAndIn,
    ))
    .unwrap();
    let price = FiniteDifferenceBarrierEngine::new(settings())
        .unwrap()
        .price(&option, &context)
        .unwrap();
    // The barrier is more than eight diffusion standard deviations away.
    assert!((0.0..1e-8).contains(&price), "rare knock-in value: {price}");
}

#[test]
fn skewed_single_barriers_match_independent_quantlib_reference() {
    let (start, end) = dates();
    let context = local(100.0, end, true);
    let engine = FiniteDifferenceBarrierEngine::new(FiniteDifferenceSettings {
        time_step_count: 1600,
        ..settings()
    })
    .unwrap();
    // QuantLib 1.43 explicit FixedLocalVolSurface, log-space FD; first-order
    // temporal Richardson estimates at 3200/6400/12800 agree within 3e-6/3e-7/1e-8.
    for (kind, direction, level, expected) in [
        (
            BarrierType::UpAndOut,
            OptionType::Call,
            120.0,
            [
                1.028_427_271_827_599_2,
                -0.022_116_708_290_671_667,
                -0.003_666_123_583_625_804_7,
            ],
        ),
        (
            BarrierType::DownAndOut,
            OptionType::Put,
            80.0,
            [
                0.732_591_698_460_598_4,
                0.011_564_760_173_786_65,
                -0.002_903_110_093_466_621,
            ],
        ),
    ] {
        let option = BarrierOption::new(BarrierOptionTerms::new(
            direction, 100.0, start, end, level, kind,
        ))
        .unwrap();
        let result = engine
            .price_with_greeks(&option, &context, request())
            .unwrap();
        for ((actual, expected), tolerance) in [
            result.price(),
            result.require(Greek::Delta).unwrap(),
            result.require(Greek::Gamma).unwrap(),
        ]
        .into_iter()
        .zip(expected)
        .zip([0.0005, 0.00003, 0.000_003])
        {
            assert!(
                (actual - expected).abs() < tolerance,
                "{kind:?}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn resolved_cash_uses_only_required_coverage_and_requests_fail_before_shortcuts() {
    let (start, end) = dates();
    let short_end = Date::new(2025, 1, 2).unwrap().start_of_day();
    let short = local(120.0, short_end, true);
    let engine = FiniteDifferenceBarrierEngine::new(settings()).unwrap();
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    terms.rebate = 3.0;
    let option = BarrierOption::new(terms.clone()).unwrap();
    let result = engine
        .price_with_greeks(&option, &short, request())
        .unwrap();
    assert!((result.price() - 3.0 * (-0.05_f64).exp()).abs() < 1e-14);
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    assert_eq!(
        engine
            .price_with_greeks(&option, &short, GreeksRequest::all())
            .unwrap()
            .get(Greek::VegaPerPercentagePoint),
        None
    );
    terms.rebate_timing = RebateTiming::AtHit;
    let paid = BarrierOption::new(terms)
        .unwrap()
        .replace_history(Some(BarrierTouchState::Touched));
    let later = short
        .replace(PricingContextChanges {
            valuation_time: Some(short_end),
            ..Default::default()
        })
        .unwrap();
    let result = engine.price_with_greeks(&paid, &later, request()).unwrap();
    assert_eq!(result.price(), 0.0);
    assert_eq!(result.require(Greek::Delta).unwrap(), 0.0);
    assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
    let active = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndIn,
    ))
    .unwrap();
    assert!(matches!(
        engine.price(&active, &short),
        Err(Error::InvalidInput(_))
    ));
    let full = local(120.0, end, true);
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    assert_eq!(
        engine.price(&active, &full).unwrap(),
        FiniteDifferenceVanillaEngine::new(settings())
            .unwrap()
            .price(&vanilla, &full)
            .unwrap()
    );
}
