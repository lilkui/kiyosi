use kiyosi::*;

fn date(month: u32, day: u32) -> Date {
    Date::new(2025, month, day).unwrap()
}

fn surface(end: Timestamp, skew: bool) -> LocalVolatilitySurface {
    let row = if skew {
        vec![0.32, 0.24, 0.16]
    } else {
        vec![0.2; 3]
    };
    LocalVolatilitySurface::new(
        vec![date(1, 1).into(), end],
        vec![60.0, 100.0, 160.0],
        vec![row.clone(), row],
    )
    .unwrap()
}

fn context(
    risk: impl Into<Rate>,
    dividend: impl Into<Rate>,
    spot: f64,
    skew: bool,
) -> PricingContext {
    PricingContext::new(
        LocalVolatilityParameters::new(risk, dividend, surface(date(4, 1).into(), skew)).unwrap(),
        spot,
        date(1, 1).into(),
    )
    .unwrap()
}

fn settings() -> FiniteDifferenceSettings {
    FiniteDifferenceSettings {
        asset_step_count: 600,
        time_step_count: 800,
        asset_upper_boundary: Some(440.0),
        ..Default::default()
    }
}

fn request() -> GreeksRequest {
    GreeksRequest::new([Greek::Delta, Greek::Gamma])
}

fn parity<I: OptionContract, E: PricingEngine<I>>(engine: &E, option: &I, local: &PricingContext) {
    let model = local.model_parameters();
    let bsm = local
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(
                    model.risk_free_curve().clone(),
                    model.dividend_curve().clone(),
                    0.2,
                )
                .unwrap()
                .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    engine.validate_applicability(local).unwrap();
    let actual = engine
        .price_with_greeks(option, local, request(), Default::default())
        .unwrap();
    let reference = engine
        .price_with_greeks(option, &bsm, request(), Default::default())
        .unwrap();
    assert!(
        (actual.price() - reference.price()).abs() < 1e-9,
        "{} != {}",
        actual.price(),
        reference.price()
    );
    for greek in [Greek::Delta, Greek::Gamma] {
        match (actual.get(greek), reference.get(greek)) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-9, "{greek:?}: {a} != {b}"),
            (a, b) => assert_eq!(a, b, "{greek:?}"),
        }
    }
}

#[test]
fn constant_surface_matches_accumulator_directions_schedules_and_rate_curves() {
    let engine = FiniteDifferenceAccumulatorEngine::new(settings()).unwrap();
    let end = date(4, 1);
    let curve = RateTermStructure::new(
        date(1, 1).into(),
        vec![date(2, 3).into(), end.into()],
        vec![1.002, 0.995],
    )
    .unwrap();
    for risk in [Rate::Flat(0.04), Rate::Flat(-0.03), Rate::Curve(curve)] {
        let local = context(risk, -0.01, 101.0, false);
        for direction in [OptionType::Call, OptionType::Put] {
            for schedule in [None, Some(vec![date(2, 3), end]), Some(vec![])] {
                let option = Accumulator::new(AccumulatorTerms {
                    option_type: direction,
                    accumulated_quantity: 3.0,
                    knock_out_observation_dates: schedule,
                    ..AccumulatorTerms::new(
                        100.0,
                        if direction == OptionType::Call {
                            110.0
                        } else {
                            90.0
                        },
                        1.0,
                        2.0,
                        date(1, 1),
                        end,
                    )
                })
                .unwrap();
                parity(&engine, &option, &local);
            }
        }
    }
}

#[test]
fn constant_surface_matches_every_autocallable_variant_and_knock_in_mode() {
    let engine = FiniteDifferenceAutocallableEngine::new(settings()).unwrap();
    let start = date(1, 1);
    let end = date(4, 1);
    let observations = autocallable_observations(
        &[date(2, 3), date(3, 3), end],
        &[110.0, 108.0, 105.0],
        Some(&[0.08; 3]),
        None,
    )
    .unwrap();
    let curve = RateTermStructure::new(
        start.into(),
        vec![date(2, 3).into(), end.into()],
        vec![1.002, 0.995],
    )
    .unwrap();
    for risk in [Rate::Flat(0.04), Rate::Flat(-0.03), Rate::Curve(curve)] {
        let local = context(risk, -0.01, 101.0, false);
        for mode in [
            KnockInObservationMode::AtExpiry,
            KnockInObservationMode::EveryTradingDay,
        ] {
            let mut snow = SnowballTerms::new(
                0.06,
                100.0,
                95.0,
                100.0,
                60.0,
                start,
                end,
                observations.clone(),
            );
            snow.knock_in_observation_mode = mode;
            snow.barrier_state = Some(AutocallableBarrierState::None);
            let mut ternary =
                TernarySnowballTerms::new(0.06, 0.01, 95.0, start, end, observations.clone());
            ternary.knock_in_observation_mode = mode;
            ternary.barrier_state = Some(AutocallableBarrierState::None);
            let mut phoenix = PhoenixTerms::new(
                0.08,
                100.0,
                95.0,
                100.0,
                60.0,
                start,
                end,
                autocallable_observations(
                    &[date(2, 3), date(3, 3), end],
                    &[110.0, 108.0, 105.0],
                    None,
                    Some(&[95.0; 3]),
                )
                .unwrap(),
            );
            phoenix.knock_in_observation_mode = mode;
            phoenix.barrier_state = Some(AutocallableBarrierState::None);
            parity(&engine, &SnowballOption::new(snow).unwrap(), &local);
            parity(
                &engine,
                &TernarySnowballOption::new(ternary).unwrap(),
                &local,
            );
            parity(&engine, &PhoenixOption::new(phoenix).unwrap(), &local);
            parity(
                &engine,
                &BinarySnowballOption::new(BinarySnowballTerms::new(
                    0.06,
                    start,
                    end,
                    observations.clone(),
                ))
                .unwrap(),
                &local,
            );
        }
    }
}

#[test]
fn coverage_follows_remaining_exposure_and_unsupported_requests_precede_settlement() {
    let start = date(1, 1);
    let end = date(4, 1);
    let short =
        LocalVolatilityParameters::new(0.04, 0.01, surface(date(1, 2).into(), true)).unwrap();
    let live = PricingContext::new(short.clone(), 101.0, start.into()).unwrap();
    let settled = PricingContext::new(short, 120.0, start.into()).unwrap();
    let acc = Accumulator::new(AccumulatorTerms {
        accumulated_quantity: 3.0,
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, start, end)
    })
    .unwrap();
    let acc_engine = FiniteDifferenceAccumulatorEngine::default();
    assert_eq!(acc_engine.price(&acc, &settled).unwrap(), 60.0);
    assert!(matches!(
        acc_engine.price(&acc, &live),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        acc_engine.price_with_greeks(&acc, &settled, GreeksRequest::all(), Default::default()),
        Err(Error::UnsupportedOperation(_))
    ));

    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.02,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[110.0; 2], Some(&[0.08; 2]), None).unwrap(),
    ))
    .unwrap();
    let engine = FiniteDifferenceAutocallableEngine::default();
    let knocked_out = note
        .replace(Some(AutocallableBarrierState::KnockedOut))
        .unwrap();
    let later = PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface(date(2, 4).into(), true)).unwrap(),
        101.0,
        date(2, 4).into(),
    )
    .unwrap();
    assert_eq!(engine.price(&knocked_out, &later).unwrap(), 0.0);
    assert!(matches!(
        engine.price(&note, &live),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        engine.price_with_greeks(
            &knocked_out,
            &later,
            GreeksRequest::all(),
            Default::default()
        ),
        Err(Error::UnsupportedOperation(_))
    ));

    let fixed = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[110.0], None, None).unwrap(),
    ))
    .unwrap();
    let risk = RateTermStructure::new(start.into(), vec![end.into()], vec![0.99]).unwrap();
    let local = PricingContext::new(
        LocalVolatilityParameters::new(risk, 0.01, surface(date(1, 2).into(), true)).unwrap(),
        101.0,
        start.into(),
    )
    .unwrap();
    let result = engine
        .price_with_greeks(&fixed, &local, request(), Default::default())
        .unwrap();
    assert!((result.price() - 0.99).abs() < 1e-14);
    assert_eq!(result.require(Greek::Delta).unwrap(), 0.0);
    assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
}

#[test]
fn event_thresholds_and_expiry_preserve_absent_spot_greeks() {
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
        autocallable_observations(&[date(2, 3), end], &[110.0; 2], None, Some(&[90.0; 2])).unwrap(),
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let note = PhoenixOption::new(terms).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(settings()).unwrap();
    let local = context(0.04, 0.01, 90.0, true)
        .replace(PricingContextChanges {
            valuation_time: Some(date(2, 3).into()),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&note, &local, request(), Default::default())
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    let expired = local
        .replace(PricingContextChanges {
            valuation_time: Some(end.into()),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&note, &expired, request(), Default::default())
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    assert!(matches!(
        engine.price_with_greeks(&note, &expired, GreeksRequest::all(), Default::default()),
        Err(Error::UnsupportedOperation(_))
    ));
    let acc = Accumulator::new(AccumulatorTerms {
        accumulated_quantity: 3.0,
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, start, end)
    })
    .unwrap();
    let acc_engine = FiniteDifferenceAccumulatorEngine::default();
    let result = acc_engine
        .price_with_greeks(&acc, &expired, request(), Default::default())
        .unwrap();
    assert_eq!(result.price(), -50.0);
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    assert!(matches!(
        acc_engine.price_with_greeks(&acc, &expired, GreeksRequest::all(), Default::default()),
        Err(Error::UnsupportedOperation(_))
    ));
}

#[test]
fn early_surface_knots_survive_autocallable_time_refinement() {
    let start = date(1, 1);
    let end = date(4, 1);
    let first = Timestamp::new(start, 7, 13, 0, 0).unwrap();
    let times = vec![start.into(), first, date(1, 2).into(), end.into()];
    let sigmas: [f64; 4] = [1.5, 0.1, 0.2, 0.2];
    let surface = LocalVolatilitySurface::new(
        times,
        vec![60.0, 160.0],
        sigmas.iter().map(|&s| vec![s; 2]).collect(),
    )
    .unwrap();
    let local = PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface).unwrap(),
        103.0,
        start.into(),
    )
    .unwrap();
    let first_days = (7.0 * 60.0 + 13.0) / (24.0 * 60.0);
    let variance: f64 = [first_days, 1.0 - first_days, 89.0]
        .into_iter()
        .zip(sigmas.windows(2))
        .map(|(days, pair)| {
            days * (pair[0].powi(2) + pair[0] * pair[1] + pair[1].powi(2)) / (3.0 * 365.0)
        })
        .sum();
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, (variance / (90.0 / 365.0)).sqrt()).unwrap(),
        103.0,
        start.into(),
    )
    .unwrap();
    let coupon = 0.5;
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[100.0], Some(&[coupon]), None).unwrap(),
    ))
    .unwrap();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, coupon * 90.0 / 365.0, start, end)
            .unwrap();
    let expected =
        (-0.04_f64 * 90.0 / 365.0).exp() + AnalyticDigitalEngine.price(&digital, &bsm).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 2000,
        ..settings()
    })
    .unwrap();
    let actual = engine.price(&note, &local).unwrap();
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}

fn fixed_surface_derivatives<I: OptionContract, E: PricingEngine<I>>(
    engine: &E,
    option: &I,
    local: &PricingContext,
    tolerance: [f64; 2],
) {
    let value = engine
        .price_with_greeks(option, local, request(), Default::default())
        .unwrap();
    let shift = 0.05;
    let price = |offset| {
        engine
            .price(
                option,
                &local
                    .replace(PricingContextChanges {
                        spot_price: Some(local.spot_price() + offset),
                        ..Default::default()
                    })
                    .unwrap(),
            )
            .unwrap()
    };
    let up = price(shift);
    let down = price(-shift);
    let delta = (up - down) / (2.0 * shift);
    let gamma = (up - 2.0 * value.price() + down) / shift.powi(2);
    assert!((value.require(Greek::Delta).unwrap() - delta).abs() < tolerance[0]);
    assert!((value.require(Greek::Gamma).unwrap() - gamma).abs() < tolerance[1]);
}

#[test]
fn skewed_multievent_prices_supply_fixed_surface_delta_and_gamma() {
    let local = context(0.04, 0.01, 101.0, true);
    let start = date(1, 1);
    let end = date(4, 1);
    let acc = Accumulator::new(AccumulatorTerms {
        accumulated_quantity: 3.0,
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, start, end)
    })
    .unwrap();
    let acc_engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        ..settings()
    })
    .unwrap();
    fixed_surface_derivatives(&acc_engine, &acc, &local, [0.001, 0.003]);
    let mut terms = SnowballTerms::new(
        0.06,
        100.0,
        95.0,
        100.0,
        60.0,
        start,
        end,
        autocallable_observations(
            &[date(2, 3), date(3, 3), end],
            &[110.0, 108.0, 105.0],
            Some(&[0.08; 3]),
            None,
        )
        .unwrap(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::None);
    let note = SnowballOption::new(terms).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        ..settings()
    })
    .unwrap();
    fixed_surface_derivatives(&engine, &note, &local, [1e-6, 1e-5]);
}

#[test]
fn skewed_multievent_prices_match_independent_quantlib_paths() {
    let local = context(0.04, 0.01, 101.0, true);
    let start = date(1, 1);
    let end = date(4, 1);
    let settings = FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 1600,
        ..settings()
    };
    let engine = FiniteDifferenceAutocallableEngine::new(settings).unwrap();
    let observations = autocallable_observations(
        &[date(2, 3), date(3, 3), end],
        &[110.0, 108.0, 105.0],
        Some(&[0.08; 3]),
        None,
    )
    .unwrap();
    // Frozen QL path means and five-SE + measured time-bias + FD budgets;
    // Python verifies the full evidence in local_volatility_structured.json.
    let acc = Accumulator::new(AccumulatorTerms {
        accumulated_quantity: 3.0,
        ..AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0, start, end)
    })
    .unwrap();
    let price = FiniteDifferenceAccumulatorEngine::new(settings)
        .unwrap()
        .price(&acc, &local)
        .unwrap();
    assert!((price - (-209.7001122945956)).abs() < 3.1);
    let binary = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.06,
        start,
        end,
        observations.clone(),
    ))
    .unwrap();
    assert!((engine.price(&binary, &local).unwrap() - 1.0057449007548795).abs() < 0.000114);
    for (mode, snow_ref, ternary_ref, phoenix_ref) in [
        (
            KnockInObservationMode::EveryTradingDay,
            0.9609325753800867,
            1.0000474115745979,
            0.9657946996141008,
        ),
        (
            KnockInObservationMode::AtExpiry,
            0.9664325018643236,
            1.0022395227440681,
            0.968664092694738,
        ),
    ] {
        let mut snow = SnowballTerms::new(
            0.06,
            100.0,
            95.0,
            100.0,
            60.0,
            start,
            end,
            observations.clone(),
        );
        snow.knock_in_observation_mode = mode;
        snow.barrier_state = Some(AutocallableBarrierState::None);
        let mut ternary =
            TernarySnowballTerms::new(0.06, 0.01, 95.0, start, end, observations.clone());
        ternary.knock_in_observation_mode = mode;
        ternary.barrier_state = Some(AutocallableBarrierState::None);
        let mut phoenix = PhoenixTerms::new(
            0.08,
            100.0,
            95.0,
            100.0,
            60.0,
            start,
            end,
            autocallable_observations(
                &[date(2, 3), date(3, 3), end],
                &[110.0, 108.0, 105.0],
                None,
                Some(&[95.0; 3]),
            )
            .unwrap(),
        );
        phoenix.knock_in_observation_mode = mode;
        phoenix.barrier_state = Some(AutocallableBarrierState::None);
        for (price, expected, tolerance) in [
            (
                engine
                    .price(&SnowballOption::new(snow).unwrap(), &local)
                    .unwrap(),
                snow_ref,
                0.00023,
            ),
            (
                engine
                    .price(&TernarySnowballOption::new(ternary).unwrap(), &local)
                    .unwrap(),
                ternary_ref,
                0.00013,
            ),
            (
                engine
                    .price(&PhoenixOption::new(phoenix).unwrap(), &local)
                    .unwrap(),
                phoenix_ref,
                0.00022,
            ),
        ] {
            assert!(
                (price - expected).abs() < tolerance,
                "{price} != {expected}"
            );
        }
    }
}
