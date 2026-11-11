mod support;
use support::dated;

use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}

fn surface(end: Timestamp, skew: bool) -> LocalVolatilitySurface {
    let row = if skew {
        vec![0.32, 0.24, 0.16]
    } else {
        vec![0.2; 3]
    };
    LocalVolatilitySurface::new(
        vec![date(1, 1), end],
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
        LocalVolatilityParameters::new(risk, dividend, surface(date(4, 1), skew)).unwrap(),
        spot,
        date(1, 1),
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

fn parity<I: support::DatedHistory, E: PricingEngine<I>>(
    engine: &E,
    option: &I,
    local: &PricingContext,
) {
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
        .price_with_greeks_with_settings(
            &dated(option, local),
            local,
            request(),
            engine.greek_settings(&dated(option, local)),
        )
        .unwrap();
    let reference = engine
        .price_with_greeks_with_settings(
            &dated(option, &bsm),
            &bsm,
            request(),
            engine.greek_settings(&dated(option, &bsm)),
        )
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
    let curve =
        RateTermStructure::new(date(1, 1), vec![date(2, 3), end], vec![1.002, 0.995]).unwrap();
    for risk in [Rate::Flat(0.04), Rate::Flat(-0.03), Rate::Curve(curve)] {
        let local = context(risk, -0.01, 101.0, false);
        for direction in [OptionType::Call, OptionType::Put] {
            for schedule in [None, Some(vec![date(2, 3), end]), Some(vec![])] {
                let option = Accumulator::new(AccumulatorTerms {
                    history_time: Some(date(1, 1)),
                    knock_out_settled: Some(false),
                    option_type: direction,
                    accumulated_quantity: Some(3.0),
                    knock_out_observation_times: schedule,
                    ..AccumulatorTerms::new(
                        100.0,
                        if direction == OptionType::Call {
                            110.0
                        } else {
                            90.0
                        },
                        1.0,
                        2.0,
                        date(1, 1).add_days(-1).unwrap(),
                        end,
                        (end).time_of_day(),
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
    let curve = RateTermStructure::new(start, vec![date(2, 3), end], vec![1.002, 0.995]).unwrap();
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
                Some((end).time_of_day()),
            );
            snow.knock_in_observation_mode = mode;
            snow.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
                .then_some(snow.expiry_time.time_of_day());
            snow.barrier_state = Some(AutocallableBarrierState::NoEvent);
            snow.history_time = Some(snow.effective_time);
            let mut ternary = TernarySnowballTerms::new(
                0.06,
                0.01,
                95.0,
                start,
                end,
                observations.clone(),
                Some((end).time_of_day()),
            );
            ternary.knock_in_observation_mode = mode;
            ternary.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
                .then_some(ternary.expiry_time.time_of_day());
            ternary.barrier_state = Some(AutocallableBarrierState::NoEvent);
            ternary.history_time = Some(ternary.effective_time);
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
                Some((end).time_of_day()),
            );
            phoenix.knock_in_observation_mode = mode;
            phoenix.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
                .then_some(phoenix.expiry_time.time_of_day());
            phoenix.barrier_state = Some(AutocallableBarrierState::NoEvent);
            phoenix.history_time = Some(phoenix.effective_time);
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
    let short = LocalVolatilityParameters::new(0.04, 0.01, surface(date(1, 2), true)).unwrap();
    let live = PricingContext::new(short.clone(), 101.0, start).unwrap();
    let settled = PricingContext::new(short, 120.0, start).unwrap();
    let acc = Accumulator::new(AccumulatorTerms {
        history_time: Some(start),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(
            100.0,
            110.0,
            1.0,
            2.0,
            start.add_days(-1).unwrap(),
            end,
            (end).time_of_day(),
        )
    })
    .unwrap();
    let acc_engine = FiniteDifferenceAccumulatorEngine::default();
    assert_eq!(
        acc_engine.price(&dated(&acc, &settled), &settled).unwrap(),
        60.0
    );
    assert!(matches!(
        acc_engine.price(&dated(&acc, &live), &live),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        acc_engine
            .price_with_greeks(&dated(&acc, &settled), &settled, GreeksRequest::all())
            .unwrap()
            .get(Greek::Vega),
        None
    );

    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.02,
        start,
        end,
        autocallable_observations(&[date(2, 3), end], &[110.0; 2], Some(&[0.08; 2]), None).unwrap(),
    ))
    .unwrap();
    let engine = FiniteDifferenceAutocallableEngine::default();
    let knocked_out = note
        .replace_history(
            Some(AutocallableBarrierState::KnockedOut),
            Some(note.expiry_time()),
        )
        .unwrap();
    let later = PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface(date(2, 4), true)).unwrap(),
        101.0,
        date(2, 4),
    )
    .unwrap();
    assert_eq!(
        engine.price(&dated(&knocked_out, &later), &later).unwrap(),
        0.0
    );
    assert!(matches!(
        engine.price(&dated(&note, &live), &live),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        engine
            .price_with_greeks(&dated(&knocked_out, &later), &later, GreeksRequest::all())
            .unwrap()
            .get(Greek::Vega),
        None
    );

    let fixed = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[110.0], Some(&[0.0]), None).unwrap(),
    ))
    .unwrap();
    let risk = RateTermStructure::new(start, vec![end], vec![0.99]).unwrap();
    let local = PricingContext::new(
        LocalVolatilityParameters::new(risk, 0.01, surface(date(1, 2), true)).unwrap(),
        101.0,
        start,
    )
    .unwrap();
    let result = engine
        .price_with_greeks(&dated(&fixed, &local), &local, request())
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
        Some((end).time_of_day()),
    );
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.observation_clock_time = None;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    terms.history_time = Some(terms.effective_time);
    let note = PhoenixOption::new(terms).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(settings()).unwrap();
    let local = context(0.04, 0.01, 90.0, true)
        .replace(PricingContextChanges {
            valuation_time: Some(date(2, 3)),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&dated(&note, &local), &local, request())
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    let expired = local
        .replace(PricingContextChanges {
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    let result = engine
        .price_with_greeks(&dated(&note, &expired), &expired, request())
        .unwrap();
    assert_eq!(result.get(Greek::Delta), None);
    assert_eq!(result.get(Greek::Gamma), None);
    assert_eq!(
        engine
            .price_with_greeks(&dated(&note, &expired), &expired, GreeksRequest::all())
            .unwrap()
            .get(Greek::Vega),
        None
    );
    let acc = Accumulator::new(AccumulatorTerms {
        history_time: Some(start),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(
            100.0,
            110.0,
            1.0,
            2.0,
            start.add_days(-1).unwrap(),
            end,
            (end).time_of_day(),
        )
    })
    .unwrap();
    let acc_engine = FiniteDifferenceAccumulatorEngine::default();
    let result = acc_engine
        .price_with_greeks(&dated(&acc, &expired), &expired, request())
        .unwrap();
    assert_eq!(result.price(), -50.0);
    assert!((result.require(Greek::Delta).unwrap() - 5.0).abs() < 1e-10);
    assert_eq!(result.require(Greek::Gamma).unwrap(), 0.0);
    assert_eq!(
        acc_engine
            .price_with_greeks(&dated(&acc, &expired), &expired, GreeksRequest::all())
            .unwrap()
            .get(Greek::Vega),
        None
    );
}

#[test]
fn early_surface_knots_survive_autocallable_time_refinement() {
    let start = date(1, 1);
    let end = date(4, 1);
    let first = Timestamp::new(start.date(), 7, 13, 0, 0).unwrap();
    let times = vec![start, first, date(1, 2), end];
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
        start,
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
        start,
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
    let expected = (-0.04_f64 * 90.0 / 365.0).exp()
        + AnalyticDigitalEngine
            .price(&dated(&digital, &bsm), &bsm)
            .unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 1600,
        time_step_count: 2000,
        ..settings()
    })
    .unwrap();
    let actual = engine.price(&dated(&note, &local), &local).unwrap();
    assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
}

fn fixed_surface_derivatives<I: support::DatedHistory, E: PricingEngine<I>>(
    engine: &E,
    option: &I,
    local: &PricingContext,
    tolerance: [f64; 2],
) {
    let value = engine
        .price_with_greeks_with_settings(
            &dated(option, local),
            local,
            request(),
            engine.greek_settings(&dated(option, local)),
        )
        .unwrap();
    let shift = 0.05;
    let price = |offset| {
        engine
            .price(
                &dated(
                    option,
                    &local
                        .replace(PricingContextChanges {
                            spot_price: Some(local.spot_price() + offset),
                            ..Default::default()
                        })
                        .unwrap(),
                ),
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
        history_time: Some(start),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(
            100.0,
            110.0,
            1.0,
            2.0,
            start.add_days(-1).unwrap(),
            end,
            (end).time_of_day(),
        )
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
        Some((end).time_of_day()),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    terms.history_time = Some(terms.effective_time);
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
        history_time: Some(start),
        knock_out_settled: Some(false),
        accumulated_quantity: Some(3.0),
        ..AccumulatorTerms::new(
            100.0,
            110.0,
            1.0,
            2.0,
            start.add_days(-1).unwrap(),
            end,
            (end).time_of_day(),
        )
    })
    .unwrap();
    let price = FiniteDifferenceAccumulatorEngine::new(settings)
        .unwrap()
        .price(&dated(&acc, &local), &local)
        .unwrap();
    assert!((price - (-209.700_112_294_595_6)).abs() < 3.1);
    let binary = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.06,
        start,
        end,
        observations.clone(),
    ))
    .unwrap();
    assert!(
        (engine.price(&dated(&binary, &local), &local).unwrap() - 1.005_744_900_754_879_5).abs()
            < 0.000_114
    );
    for (mode, snow_ref, ternary_ref, phoenix_ref) in [
        (
            KnockInObservationMode::EveryTradingDay,
            0.960_932_575_380_086_7,
            1.000_047_411_574_597_9,
            1.101_753_945_540_377_3,
        ),
        (
            KnockInObservationMode::AtExpiry,
            0.966_432_501_864_323_6,
            1.002_239_522_744_068_1,
            1.104_623_338_622_052_5,
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
            Some((end).time_of_day()),
        );
        snow.knock_in_observation_mode = mode;
        snow.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(snow.expiry_time.time_of_day());
        snow.barrier_state = Some(AutocallableBarrierState::NoEvent);
        snow.history_time = Some(snow.effective_time);
        let mut ternary = TernarySnowballTerms::new(
            0.06,
            0.01,
            95.0,
            start,
            end,
            observations.clone(),
            Some((end).time_of_day()),
        );
        ternary.knock_in_observation_mode = mode;
        ternary.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(ternary.expiry_time.time_of_day());
        ternary.barrier_state = Some(AutocallableBarrierState::NoEvent);
        ternary.history_time = Some(ternary.effective_time);
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
            Some((end).time_of_day()),
        );
        phoenix.knock_in_observation_mode = mode;
        phoenix.observation_clock_time = (mode == KnockInObservationMode::EveryTradingDay)
            .then_some(phoenix.expiry_time.time_of_day());
        phoenix.barrier_state = Some(AutocallableBarrierState::NoEvent);
        phoenix.history_time = Some(phoenix.effective_time);
        for (price, expected, tolerance) in [
            (
                engine
                    .price(&dated(&SnowballOption::new(snow).unwrap(), &local), &local)
                    .unwrap(),
                snow_ref,
                0.00023,
            ),
            (
                engine
                    .price(
                        &dated(&TernarySnowballOption::new(ternary).unwrap(), &local),
                        &local,
                    )
                    .unwrap(),
                ternary_ref,
                0.00013,
            ),
            (
                engine
                    .price(
                        &dated(&PhoenixOption::new(phoenix).unwrap(), &local),
                        &local,
                    )
                    .unwrap(),
                phoenix_ref,
                0.00083,
            ),
        ] {
            assert!(
                (price - expected).abs() < tolerance,
                "{price} != {expected}"
            );
        }
    }
}
