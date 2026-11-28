use kiyosi::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}
fn market(spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        spot,
        dates().0,
    )
    .unwrap()
}
fn option() -> EuropeanOption {
    EuropeanOption::new(OptionType::Call, 100.0, dates().0, dates().1).unwrap()
}

#[test]
fn saved_accumulator_matches_scalar_deterministic_fixings() -> Result<()> {
    let (start, end) = dates();
    let engine = FiniteDifferenceAccumulatorEngine::default();
    for (direction, barrier) in [(OptionType::Call, 115.0), (OptionType::Put, 85.0)] {
        let mut terms =
            AccumulatorTerms::new(100.0, barrier, 0.1, 2.5, start, end, start.time_of_day());
        terms.calendar = TradingCalendar::all_days();
        terms.option_type = direction;
        terms.knock_out_observation_times =
            Some(vec![start.add_days(120)?, start.add_days(240)?, end]);
        let option = Accumulator::new(terms)?;
        for rate in [-0.12, 0.0, 0.12] {
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(rate, 0.0, 0.0)?,
                100.0,
                start,
            )?;
            let solution = engine.solve(&option, &context, 75.0..=125.0)?;
            let spots = [125.0, 75.0, 85.0, 100.0, 115.0, 100.0, 93.0, 108.0];
            let prices = solution.price_many(&spots)?;
            assert_eq!(
                prices.len(),
                spots.len(),
                "saved accumulator spot quote count"
            );
            for (&spot, actual) in spots.iter().zip(prices) {
                let scenario = context.replace(PricingContextChanges {
                    spot_price: Some(spot),
                    ..Default::default()
                })?;
                close(actual, engine.price(&option, &scenario)?, 1e-9);
            }
        }
    }
    Ok(())
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected}; tolerance={tolerance}"
    );
}

#[test]
fn log_grid_covers_low_saved_put_endpoint() -> Result<()> {
    let put = EuropeanOption::new(OptionType::Put, 100.0, dates().0, dates().1)?;
    let low = 0.0001;
    let engine = FiniteDifferenceVanillaEngine::default();
    let solution = engine.solve(&put, &market(150.0), low..=120.0)?;
    let expected = AnalyticEuropeanEngine.price(&put, &market(low))?;
    close(solution.price(low)?, expected, 0.001);
    close(engine.price(&put, &market(low))?, expected, 0.001);
    assert!(solution.price(120.0)? < solution.price(low)?);
    Ok(())
}

#[test]
fn log_grid_keeps_continuous_barrier_equality() -> Result<()> {
    let barrier = 80.1_f64;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        dates().0,
        dates().1,
        barrier,
        BarrierType::DownAndOut,
    );
    terms.rebate = 7.0;
    let option = BarrierOption::new(terms)?;
    let engine = FiniteDifferenceBarrierEngine::default();
    let below_barrier = libm::nextafter(barrier, 0.0);
    let solution = engine.solve(&option, &market(100.0), below_barrier..=100.0)?;
    let rebate = AnalyticBarrierEngine.price(&option, &market(barrier))?;
    close(solution.price(below_barrier)?, rebate, 1e-12);
    close(solution.price(barrier)?, rebate, 1e-12);
    let expected = AnalyticBarrierEngine.price(&option, &market(barrier + 0.01))?;
    close(solution.price(barrier + 0.01)?, expected, 0.001);
    Ok(())
}

#[test]
fn log_grid_respects_discounted_asset_digital_cap_with_carry() -> Result<()> {
    let option = DigitalOption::asset_or_nothing(OptionType::Call, 100.0, dates().0, dates().1)?;
    let market = PricingContext::new(
        BlackScholesMertonParameters::new(0.11, 0.04, 0.08)?,
        250.0,
        dates().0,
    )?;
    let value = FiniteDifferenceDigitalEngine::default().price(&option, &market)?;
    assert!(value <= 250.0 * (-0.04_f64).exp());
    close(
        value,
        AnalyticDigitalEngine.price(&option, &market)?,
        0.000_01,
    );
    Ok(())
}

#[test]
fn automatic_european_grid_resolves_low_volatility_with_strong_carry() -> Result<()> {
    let start = dates().0;
    let end = start.add_days(730)?;
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let digitals = [
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 100.0, start, end)?,
        DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end)?,
    ];
    for volatility in [0.01, 0.012_720, 0.012_735] {
        let market = PricingContext::new(
            BlackScholesMertonParameters::new(0.15, -0.03, volatility)?,
            70.0,
            start,
        )?;
        let engine = FiniteDifferenceVanillaEngine::default();
        let expected = AnalyticEuropeanEngine.price(&vanilla, &market)?;
        close(engine.price(&vanilla, &market)?, expected, 0.001);
        close(
            engine.solve(&vanilla, &market, 68.0..=72.0)?.price(70.0)?,
            expected,
            0.001,
        );
        let engine = FiniteDifferenceDigitalEngine::default();
        for digital in &digitals {
            let expected = AnalyticDigitalEngine.price(digital, &market)?;
            close(engine.price(digital, &market)?, expected, 0.001);
            close(
                engine.solve(digital, &market, 68.0..=72.0)?.price(70.0)?,
                expected,
                0.001,
            );
        }
    }
    Ok(())
}

#[test]
fn saved_vanilla_matches_analytic_and_scalar_batch_contracts() {
    let solution = FiniteDifferenceVanillaEngine::default()
        .solve(&option(), &market(200.0), 80.0..=120.0)
        .unwrap();
    assert_eq!(solution.spot_range(), 80.0..=120.0);
    assert_eq!(solution.valuation_time(), dates().0);
    assert_eq!(
        solution.supported(),
        GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed])
    );
    let spots = [80.0, 93.7, 100.0, 108.3, 120.0, 100.0];
    let prices = solution.price_many(&spots).unwrap();
    let results = solution
        .price_many_with_greeks(&spots, GreeksRequest::all(), None)
        .unwrap();
    assert_eq!(prices.len(), spots.len(), "scalar batch result count");
    assert_eq!(results.len(), spots.len(), "Greeks batch result count");
    for ((spot, price), result) in spots.into_iter().zip(prices).zip(results) {
        assert_eq!(price, solution.price(spot).unwrap());
        assert_eq!(
            result,
            solution
                .price_with_greeks(spot, GreeksRequest::all(), None)
                .unwrap()
        );
        let expected = AnalyticEuropeanEngine
            .price_with_greeks(&option(), &market(spot), GreeksRequest::all())
            .unwrap();
        close(price, expected.price(), 0.001);
        close(
            result.require(Greek::Delta).unwrap(),
            expected.require(Greek::Delta).unwrap(),
            0.0001,
        );
        close(
            result.require(Greek::Gamma).unwrap(),
            expected.require(Greek::Gamma).unwrap(),
            0.0003,
        );
        if spot > 80.0 && spot < 120.0 {
            close(
                result.require(Greek::Speed).unwrap(),
                expected.require(Greek::Speed).unwrap(),
                1e-5,
            );
        }
        assert_eq!(result.status(Greek::Vega), GreekStatus::Unsupported);
    }
    assert_eq!(solution.price_many(&[]).unwrap(), Vec::<f64>::new());
    assert_eq!(
        solution
            .price_many_with_greeks(&[], GreeksRequest::all(), None)
            .unwrap(),
        Vec::<PricingResult>::new()
    );
}

#[test]
fn domain_errors_and_stencil_limits_preserve_saved_values() {
    let engine = FiniteDifferenceVanillaEngine::default();
    let option = option();
    let context = market(100.0);
    for range in [
        0.0..=120.0,
        100.0..=100.0,
        120.0..=80.0,
        -1.0..=120.0,
        f64::NAN..=120.0,
        80.0..=f64::INFINITY,
    ] {
        assert!(matches!(
            engine.solve(&option, &context, range),
            Err(Error::InvalidInput(_))
        ));
    }
    let solution = engine.solve(&option, &context, 80.0..=120.0).unwrap();
    let expected = solution.price(100.0).unwrap();
    for spot in [79.999, 120.001, -1.0, f64::INFINITY, f64::NAN] {
        assert!(matches!(solution.price(spot), Err(Error::InvalidInput(_))));
        assert!(matches!(
            solution.price_many(&[100.0, spot]),
            Err(Error::InvalidInput(_))
        ));
    }
    for shift in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(matches!(
            solution.price_many_with_greeks(&[], GreeksRequest::new([]), Some(shift)),
            Err(Error::InvalidInput(_))
        ));
    }
    for spot in [80.0, 120.0] {
        let result = solution
            .price_with_greeks(spot, GreeksRequest::all(), None)
            .unwrap();
        assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
        assert_eq!(result.status(Greek::Speed), GreekStatus::StencilUnavailable);
    }
    let result = solution
        .price_with_greeks(100.0, GreeksRequest::all(), Some(50.0))
        .unwrap();
    assert_eq!(result.status(Greek::Delta), GreekStatus::Available);
    assert_eq!(result.status(Greek::Speed), GreekStatus::StencilUnavailable);
    assert_eq!(solution.price(100.0).unwrap(), expected);
    let clipped = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(110.0),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        clipped.solve(&option, &context, 80.0..=120.0),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn saved_barriers_apply_current_touch_rules_across_the_interval() {
    for kind in [
        BarrierType::UpAndIn,
        BarrierType::UpAndOut,
        BarrierType::DownAndIn,
        BarrierType::DownAndOut,
    ] {
        let mut terms = BarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            dates().0,
            dates().1,
            if matches!(kind, BarrierType::UpAndIn | BarrierType::UpAndOut) {
                120.0
            } else {
                80.0
            },
            kind,
        );
        terms.rebate = 2.0;
        let option = BarrierOption::new(terms).unwrap();
        let solution = FiniteDifferenceBarrierEngine::default()
            .solve(&option, &market(150.0), 70.0..=130.0)
            .unwrap();
        for spot in [70.0, 79.9, 80.0, 80.1, 99.75, 119.9, 120.0, 120.1, 130.0] {
            let expected = AnalyticBarrierEngine.price(&option, &market(spot)).unwrap();
            close(solution.price(spot).unwrap(), expected, 0.005);
        }
        let at_barrier = solution
            .price_with_greeks(option.barrier_level(), GreeksRequest::all(), None)
            .unwrap();
        assert_eq!(at_barrier.status(Greek::Delta), GreekStatus::Undefined);
    }
}

#[test]
fn accumulator_queries_recompute_current_purchase_and_knockout() {
    let start = dates().0;
    let end = Date::new(2025, 1, 8).unwrap().start_of_day();
    let mut terms = AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        2.0,
        Date::new(2024, 12, 30).unwrap().start_of_day(),
        end,
        start.time_of_day(),
    );
    terms.accumulated_quantity = Some(3.0);
    terms.knock_out_settled = Some(false);
    let option = Accumulator::new(terms).unwrap();
    let engine = FiniteDifferenceAccumulatorEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    let solution = engine.solve(&option, &market(115.0), 90.0..=115.0).unwrap();
    for spot in [90.0, 99.9, 100.0, 100.1, 109.9, 110.0, 115.0] {
        close(
            solution.price(spot).unwrap(),
            engine.price(&option, &market(spot)).unwrap(),
            1e-10,
        );
    }
    assert_eq!(solution.price(110.0).unwrap(), 30.0);
    assert_eq!(solution.price(115.0).unwrap(), 45.0);
}

#[test]
fn autocallable_queries_select_current_knockin_coupon_and_knockout() {
    let start = dates().0;
    let end = Date::new(2025, 2, 3).unwrap().start_of_day();
    let mut terms = PhoenixTerms::new(
        0.08,
        100.0,
        80.0,
        100.0,
        60.0,
        start,
        end,
        [start, end]
            .iter()
            .zip([110.0; 2].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        [start, end]
            .iter()
            .zip([90.0; 2].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        KnockInObservation::EveryTradingDay {
            observation_clock_time: start.time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);
    let option = PhoenixOption::new(terms).unwrap();
    let engine = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })
    .unwrap();
    let solution = engine.solve(&option, &market(115.0), 70.0..=115.0).unwrap();
    for spot in [
        70.0, 79.9, 80.0, 80.1, 89.9, 90.0, 90.1, 109.9, 110.0, 115.0,
    ] {
        close(
            solution.price(spot).unwrap(),
            engine.price(&option, &market(spot)).unwrap(),
            1e-4,
        );
        close(
            solution.price(spot).unwrap(),
            QuadratureAutocallableEngine::default()
                .price(&option, &market(spot))
                .unwrap(),
            1e-4,
        );
    }
    let result = solution
        .price_with_greeks(90.0, GreeksRequest::all(), None)
        .unwrap();
    assert_eq!(result.status(Greek::Gamma), GreekStatus::Undefined);
}

#[test]
fn saved_autocallable_batch_reuses_closed_variance_horizon() -> Result<()> {
    let (start, end) = dates();
    let visits = Arc::new(AtomicUsize::new(0));
    let calendar_visits = Arc::clone(&visits);
    let calendar = TradingCalendar::new(
        move |_| {
            calendar_visits.fetch_add(1, Ordering::Relaxed);
            false
        },
        252,
    )?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.03,
        0.01,
        0.2,
        VarianceTimeConvention::Business(calendar),
    )?;
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.02,
        start,
        end,
        SnowballObservation::from_columns(&[end], &[110.0], &[0.04])?,
        TradingCalendar::all_days(),
    ))?;
    let context = PricingContext::new(model, 100.0, start)?;
    let solution =
        FiniteDifferenceAutocallableEngine::default().solve(&note, &context, 90.0..=120.0)?;
    visits.store(0, Ordering::Relaxed);

    let prices = solution.price_many(&[100.0, 120.0, 100.0])?;
    let discount = (-0.03_f64).exp();
    assert_eq!(prices.len(), 3);
    close(prices[0], 1.02 * discount, 1e-12);
    close(prices[1], 1.04 * discount, 1e-12);
    close(prices[2], prices[0], 1e-12);
    assert_eq!(visits.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn local_volatility_and_heston_keep_model_capabilities() {
    let local = LocalVolatilityParameters::new(
        0.05,
        0.02,
        LocalVolatilitySurface::new(
            vec![dates().0, dates().1],
            vec![50.0, 150.0],
            vec![vec![0.2; 2]; 2],
        )
        .unwrap(),
    )
    .unwrap();
    let context = PricingContext::new(local, 100.0, dates().0).unwrap();
    let solution = FiniteDifferenceVanillaEngine::default()
        .solve(&option(), &context, 80.0..=120.0)
        .unwrap();
    close(
        solution.price(105.0).unwrap(),
        AnalyticEuropeanEngine
            .price(&option(), &market(105.0))
            .unwrap(),
        0.001,
    );
    assert_eq!(
        solution.supported(),
        GreeksRequest::new([Greek::Delta, Greek::Gamma])
    );
    let heston = HestonParameters::new(HestonParameterInputs {
        risk_free_rate: 0.05.into(),
        dividend_yield: 0.02.into(),
        initial_variance: 0.04,
        long_run_variance: 0.04,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.3,
        correlation: -0.7,
    })
    .unwrap();
    let context = PricingContext::new(heston, 100.0, dates().0).unwrap();
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let solution = engine.solve(&option(), &context, 80.0..=120.0).unwrap();
    assert_eq!(solution.supported(), GreeksRequest::new([]));
    for spot in [80.0, 100.0, 105.0, 120.0] {
        let shifted = context
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap();
        let expected = AnalyticHestonEuropeanEngine::default()
            .price(&option(), &shifted)
            .unwrap();
        close(solution.price(spot).unwrap(), expected, 0.003);
        assert_eq!(
            solution
                .price_with_greeks(spot, GreeksRequest::all(), None)
                .unwrap()
                .status(Greek::Delta),
            GreekStatus::Unsupported
        );
    }
    assert!(matches!(
        engine.solve(&option(), &context, 0.0..=120.0),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn saved_business_time_vanilla_grid_queries_are_bounded() -> Result<()> {
    let (start, end) = dates();
    let valuation = start.add_days(3)?;
    let clock = VarianceTimeConvention::Business(TradingCalendar::weekdays());
    let model =
        BlackScholesMertonParameters::new_with_variance_time_convention(0.05, 0.02, 0.2, clock)?;
    let context = PricingContext::new(model, 100.0, valuation)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let solution = engine.solve(&option, &context, 80.0..=120.0)?;
    let result = solution.price_with_greeks(100.0, GreeksRequest::new([Greek::Delta]), None)?;
    assert_eq!(
        kiyosi::binding_support::finite_difference_solution_query_work(&solution),
        0
    );
    close(result.price(), engine.price(&option, &context)?, 0.001);
    assert!((0.5..0.6).contains(&result.require(Greek::Delta)?));
    Ok(())
}

#[test]
fn saved_deterministic_american_uses_dense_exercise_intervals() -> Result<()> {
    let start = dates().0;
    let times = (1..=1200)
        .map(|day| start.add_days(day))
        .collect::<Result<Vec<_>>>()?;
    let end = start.add_days(1200)?;
    let curve = RateTermStructure::new(start, times, vec![1.0; 1200])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let option = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let solution = engine.solve(&option, &context, 80.0..=120.0)?;
    let delta = GreeksRequest::new([Greek::Delta]);
    assert_eq!(
        kiyosi::binding_support::finite_difference_solution_query_work(&solution),
        1200
    );
    close(solution.price(110.0)?, 10.0, 1e-12);
    close(
        solution
            .price_with_greeks(110.0, delta, None)?
            .require(Greek::Delta)?,
        1.0,
        1e-9,
    );
    assert_eq!(
        solution
            .price_with_greeks(100.0, delta, None)?
            .status(Greek::Delta),
        GreekStatus::Undefined,
    );
    Ok(())
}

#[test]
fn deterministic_american_exercise_tie_has_undefined_spot_greeks() -> Result<()> {
    let (start, end) = dates();
    let nodes = vec![start.add_days(180)?, end];
    let risk = RateTermStructure::new(start, nodes.clone(), vec![1.0, 61.0 / 64.0])?;
    let dividend = RateTermStructure::new(start, nodes, vec![62.0 / 64.0; 2])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(risk, dividend, 0.0)?,
        150.0,
        start,
    )?;
    let option = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let solution = engine.solve(&option, &context, 145.0..=155.0)?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for result in [
        engine.price_with_greeks(&option, &context, request)?,
        solution.price_with_greeks(150.0, request, None)?,
    ] {
        close(result.price(), 50.0, 1e-12);
        assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
        assert_eq!(result.status(Greek::Gamma), GreekStatus::Undefined);
    }
    for (spot, delta) in [(149.0, 62.0 / 64.0), (151.0, 1.0)] {
        let result = solution.price_with_greeks(spot, request, None)?;
        close(result.require(Greek::Delta)?, delta, 1e-9);
        close(result.require(Greek::Gamma)?, 0.0, 1e-8);
    }
    let spot = 149.5;
    let context = context.replace(PricingContextChanges {
        spot_price: Some(spot),
        ..Default::default()
    })?;
    for result in [
        engine.price_with_greeks_with_settings(
            &option,
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(1.0),
                ..Default::default()
            },
        )?,
        solution.price_with_greeks(spot, request, Some(1.0))?,
    ] {
        close(result.price(), 49.515_625, 1e-12);
        assert_eq!(result.status(Greek::Delta), GreekStatus::StencilUnavailable);
        assert_eq!(result.status(Greek::Gamma), GreekStatus::StencilUnavailable);
    }
    let smooth = PricingContext::new(
        BlackScholesMertonParameters::new(0.1, 0.05, 0.0)?,
        200.0 * (-0.025_f64).exp(),
        start,
    )?;
    let result = engine.price_with_greeks(&option, &smooth, request)?;
    close(result.require(Greek::Delta)?, (-0.025_f64).exp(), 1e-9);
    close(result.require(Greek::Gamma)?, 0.005, 1e-8);
    Ok(())
}

#[test]
fn saved_absorbing_heston_american_reports_its_exercise_intervals() -> Result<()> {
    let start = dates().0;
    let end = start.add_days(1200)?;
    let times = (1..=1200)
        .map(|day| start.add_days(day))
        .collect::<Result<Vec<_>>>()?;
    let curve = RateTermStructure::new(start, times, vec![1.0; 1200])?;
    let model = HestonParameters::new(HestonParameterInputs {
        risk_free_rate: curve.into(),
        dividend_yield: 0.0.into(),
        initial_variance: 0.0,
        long_run_variance: 0.0,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.3,
        correlation: -0.7,
    })?;
    let context = PricingContext::new(model, 100.0, start)?;
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let engine = FiniteDifferenceHestonVanillaEngine::default();
    let solution = engine.solve(&option, &context, 80.0..=120.0)?;
    assert_eq!(
        kiyosi::binding_support::finite_difference_solution_query_work(&solution),
        1200
    );
    let values = solution.price_many(&[90.0, 110.0, 90.0])?;
    let expected = [10.0, 0.0, 10.0];
    assert_eq!(values.len(), expected.len(), "Heston batch result count");
    for (value, expected) in values.into_iter().zip(expected) {
        close(value, expected, 1e-12);
    }
    assert_eq!(solution.supported(), GreeksRequest::new([]));
    Ok(())
}

#[test]
fn expiry_deterministic_and_cancelled_queries_keep_their_contracts() {
    let expiry = market(100.0)
        .replace(PricingContextChanges {
            valuation_time: Some(dates().1),
            ..Default::default()
        })
        .unwrap();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_upper_boundary: Some(1.0),
        ..Default::default()
    })
    .unwrap();
    let solution = engine.solve(&option(), &expiry, 80.0..=120.0).unwrap();
    assert_eq!(
        solution.price_many(&[80.0, 100.0, 120.0]).unwrap(),
        vec![0.0, 0.0, 20.0]
    );
    assert_eq!(
        solution
            .price_with_greeks(100.0, GreeksRequest::all(), None)
            .unwrap()
            .status(Greek::Delta),
        GreekStatus::Undefined
    );
    assert_eq!(
        solution
            .price_with_greeks(110.0, GreeksRequest::all(), None)
            .unwrap()
            .require(Greek::Delta)
            .unwrap(),
        1.0
    );
    let deterministic = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.0).unwrap(),
        100.0,
        dates().0,
    )
    .unwrap();
    let saved = engine
        .solve(&option(), &deterministic, 80.0..=120.0)
        .unwrap();
    for spot in [80.0, 105.0, 120.0] {
        let shifted = deterministic
            .replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })
            .unwrap();
        close(
            saved.price(spot).unwrap(),
            AnalyticEuropeanEngine.price(&option(), &shifted).unwrap(),
            1e-12,
        );
    }
    let before_cancellation = saved.price(100.0).unwrap();
    let token = CancellationToken::default();
    token.cancel();
    assert!(
        matches!(token.run(|| saved.price_many(&[100.0; 100])), Err(Error::Io(ref error)) if error.kind() == std::io::ErrorKind::Interrupted)
    );
    assert!(matches!(
        token.run(|| engine.solve(&option(), &deterministic, 80.0..=120.0)),
        Err(Error::Io(_))
    ));
    assert_eq!(saved.price(100.0).unwrap(), before_cancellation);
}

#[test]
fn owned_solutions_compare_and_support_concurrent_reads() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<FiniteDifferenceSolution>();
    let solution = FiniteDifferenceVanillaEngine::default()
        .solve(&option(), &market(100.0), 80.0..=120.0)
        .unwrap();
    assert_eq!(solution, solution.clone());
    assert_eq!(
        solution,
        FiniteDifferenceVanillaEngine::default()
            .solve(&option(), &market(100.0), 80.0..=120.0)
            .unwrap()
    );
    let spots = [80.0, 95.0, 100.0, 105.0, 120.0];
    let expected = solution.price_many(&spots).unwrap();
    let actual = std::thread::scope(|scope| {
        let handles: Vec<_> = spots
            .iter()
            .map(|&spot| {
                let solution = &solution;
                scope.spawn(move || solution.price(spot).unwrap())
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(actual, expected);
}

#[test]
fn digital_binary_double_barrier_and_touch_solutions_match_analytics() {
    fn check<I, E: FiniteDifferenceEngine<I>, A: PricingEngine<I>>(
        engine: &E,
        analytic: &A,
        instrument: &I,
        tolerance: f64,
    ) {
        let solution = engine
            .solve(instrument, &market(150.0), 70.0..=130.0)
            .unwrap();
        for spot in [70.0, 79.9, 80.0, 90.0, 100.0, 110.0, 120.0, 120.1, 130.0] {
            close(
                solution.price(spot).unwrap(),
                analytic.price(instrument, &market(spot)).unwrap(),
                tolerance,
            );
        }
    }
    for asset in [false, true] {
        let digital = if asset {
            DigitalOption::asset_or_nothing(OptionType::Call, 100.0, dates().0, dates().1)
        } else {
            DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, dates().0, dates().1)
        }
        .unwrap();
        check(
            &FiniteDifferenceDigitalEngine::default(),
            &AnalyticDigitalEngine,
            &digital,
            if asset { 0.01 } else { 0.001 },
        );
    }
    for kind in [BarrierType::UpAndIn, BarrierType::UpAndOut] {
        let terms =
            BinaryBarrierTerms::new(OptionType::Call, 100.0, dates().0, dates().1, 120.0, kind);
        let binary = BinaryBarrierOption::cash(terms, 10.0).unwrap();
        check(
            &FiniteDifferenceBinaryBarrierEngine::default(),
            &AnalyticBinaryBarrierEngine,
            &binary,
            0.003,
        );
        let touch = TouchOption::cash(
            TouchOptionTerms::new(dates().0, dates().1, 120.0, kind),
            10.0,
        )
        .unwrap();
        check(
            &FiniteDifferenceBinaryBarrierEngine::default(),
            &AnalyticBinaryBarrierEngine,
            &touch,
            0.003,
        );
    }
    for kind in [DoubleBarrierType::KnockIn, DoubleBarrierType::KnockOut] {
        let barrier = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            dates().0,
            dates().1,
            80.0,
            120.0,
            kind,
        ))
        .unwrap();
        check(
            &FiniteDifferenceDoubleBarrierEngine::default(),
            &AnalyticDoubleBarrierEngine,
            &barrier,
            0.003,
        );
        let touch = DoubleTouchOption::cash(
            DoubleTouchOptionTerms::new(dates().0, dates().1, 80.0, 120.0, kind),
            10.0,
        )
        .unwrap();
        check(
            &FiniteDifferenceDoubleTouchEngine::default(),
            &AnalyticDoubleTouchEngine,
            &touch,
            0.003,
        );
    }
}

#[test]
fn american_exercise_floor_greeks_follow_the_quoted_price() -> Result<()> {
    let (start, end) = dates();
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let spot = 73.47;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.0, 0.2)?,
        spot,
        start,
    )?;
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 40,
        asset_upper_boundary: Some(400.0),
        ..Default::default()
    })?;
    let solution = engine.solve(&option, &context, 70.0..=100.0)?;
    let h = 0.001;
    let prices = solution.price_many(&[spot - h, spot, spot + h])?;
    close(prices[1], 26.53, 1e-12);
    close((prices[2] - prices[0]) / (2.0 * h), -1.0, 1e-9);
    close((prices[2] - 2.0 * prices[1] + prices[0]) / h / h, 0.0, 1e-8);
    let greeks = GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed]);
    for result in [
        engine.price_with_greeks_with_settings(
            &option,
            &context,
            greeks,
            NumericalShiftSettings {
                spot_shift: Some(h),
                ..Default::default()
            },
        )?,
        solution.price_with_greeks(spot, greeks, Some(h))?,
    ] {
        close(result.price(), prices[1], 1e-12);
        close(result.require(Greek::Delta)?, -1.0, 1e-12);
        close(result.require(Greek::Gamma)?, 0.0, 1e-12);
        close(result.require(Greek::Speed)?, 0.0, 1e-12);
    }
    Ok(())
}
