use kiyosi::*;

#[test]
fn deterministic_american_exercise_preserves_quoted_payoff() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let fixing = start.add_days(365)?;
    let spot = 90.549_063;
    let strike = 99.0_f64;
    let factor = 0.914_637_f64;
    let expected = (-strike).mul_add(factor, spot);
    assert!(expected > 0.0);

    // The rate is positive until the quoted exercise date, then negative.
    for (expiry, nodes, factors) in [
        (fixing, vec![fixing], vec![factor]),
        (
            fixing.add_days(365)?,
            vec![fixing, fixing.add_days(365)?],
            vec![factor, 1.0],
        ),
    ] {
        let curve = RateTermStructure::new(start, nodes, factors)?;
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(curve, 0.0, 0.0)?,
            spot,
            start,
        )?;
        let option = AmericanOption::new(OptionType::Call, strike, start, expiry)?;
        let engine = FiniteDifferenceVanillaEngine::default();
        let saved = engine.solve(&option, &context, 90.0..=91.0)?;
        for price in [engine.price(&option, &context)?, saved.price(spot)?] {
            assert!((price / expected - 1.0).abs() < 1e-12, "{price}");
        }
    }
    Ok(())
}

#[test]
fn deterministic_barrier_can_settle_before_a_bounded_curve_ends() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let coverage_end = start.add_days(180)?;
    let expiry = start.add_days(365)?;
    let risk = RateTermStructure::bounded_flat(0.1, start, coverage_end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(risk, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        102.0,
        BarrierType::UpAndOut,
    );
    terms.rebate = 7.0;
    terms.rebate_timing = RebateTiming::AtHit;
    let option = BarrierOption::new(terms)?;
    let price = AnalyticBarrierEngine.price(&option, &context)?;
    assert!((price - 7.0 / 1.02).abs() < 1e-14, "{price}");
    Ok(())
}

#[test]
fn scheduled_barrier_checks_the_deterministic_window_once() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(2)?;
    let checks = Arc::new(AtomicUsize::new(0));
    let calendar = TradingCalendar::new(
        {
            let checks = checks.clone();
            move |_| {
                checks.fetch_add(1, Ordering::Relaxed);
                false
            }
        },
        260,
    )?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.0,
        0.0,
        0.2,
        VarianceTimeConvention::Business(calendar),
    )?;
    let context = PricingContext::new(model, 100.0, start)?;
    let mut terms = TouchOptionTerms::new(start, end, 120.0, BarrierType::UpAndIn);
    terms.observation_times = Some(
        (1..=100)
            .map(|i| end.add_micros(-i))
            .rev()
            .collect::<Result<_>>()?,
    );
    let option = TouchOption::cash(terms, 7.0)?;
    assert!(!option.at_spot_discontinuity(&context, 0.01));
    assert_eq!(checks.load(Ordering::Relaxed), 2);
    Ok(())
}

#[test]
fn structured_knock_out_uses_exact_quoted_forward() -> Result<()> {
    let start = Timestamp::new(Date::new(2025, 1, 1)?, 12, 0, 0, 0)?;
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let factor = 99.0 / 128.0;
    let curve = RateTermStructure::new(start, vec![end], vec![factor])?;
    let model = BlackScholesMertonParameters::new(curve, 0.0, 0.0)?;
    let calendar = TradingCalendar::all_days();
    let note = SnowballOption::new(SnowballTerms::new(
        0.0,
        99.0,
        80.0,
        99.0,
        60.0,
        start,
        end,
        vec![AutocallableObservation::snowball(end, 128.0, 0.2)?],
        KnockInObservation::AtExpiry,
        calendar.clone(),
    ))?;
    let accumulator = Accumulator::new(AccumulatorTerms {
        calendar,
        knock_out_observation_times: Some(vec![end]),
        ..AccumulatorTerms::new(100.0, 128.0, 1.0, 1.0, start, end, end.time_of_day())
    })?;
    // Daily observations run from January 2 through the following January 1.
    assert_eq!(accumulator.accumulation_schedule()?.len(), 365);
    let units_before_expiry = 364.0;
    let coupon_year_fraction = 364.5 / 365.0;
    for (spot, hit) in [
        (libm::nextafter(99.0, 0.0), false),
        (99.0, true),
        (libm::nextafter(99.0, f64::INFINITY), true),
    ] {
        let context = PricingContext::new(model.clone(), spot, start)?;
        let expected = factor * (1.0 + f64::from(hit) * 0.2 * coupon_year_fraction);
        let price = QuadratureAutocallableEngine::default().price(&note, &context)?;
        assert!((price - expected).abs() < 1e-12);
        let expected = (units_before_expiry + f64::from(!hit)) * (spot - 100.0 * factor);
        let price = QuadratureAccumulatorEngine::default().price(&accumulator, &context)?;
        assert!((price - expected).abs() < 1e-10);
    }
    Ok(())
}

#[test]
fn structured_knock_out_preserves_a_forward_just_below_the_barrier() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let fixing = start.add_days(365)?;
    let end = fixing.add_days(365)?;
    let calendar = TradingCalendar::all_days();
    let spot = 97.044_553_354_850_8;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.03, 0.0, 0.0)?,
        spot,
        start,
    )?;
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.12,
        start,
        end,
        vec![AutocallableObservation::snowball(fixing, 100.0, 0.2)?],
        calendar.clone(),
    ))?;
    let accumulator = Accumulator::new(AccumulatorTerms {
        calendar,
        knock_out_observation_times: Some(vec![fixing]),
        ..AccumulatorTerms::new(90.0, 100.0, 1.0, 1.0, start, end, start.time_of_day())
    })?;
    let discount = (-0.06_f64).exp();
    let note_price = QuadratureAutocallableEngine::default().price(&note, &context)?;
    let accumulator_price = QuadratureAccumulatorEngine::default().price(&accumulator, &context)?;
    assert!((note_price - 1.24 * discount).abs() < 1e-12, "{note_price}");
    assert!((accumulator_price - 731.0 * (spot - 90.0 * discount)).abs() < 1e-10);
    Ok(())
}

#[test]
fn scheduled_touch_preserves_the_exact_final_curve_quote() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let dates = vec![start.add_days(30)?, start.add_days(61)?, end];
    let curve = RateTermStructure::new(start, dates.clone(), vec![1.01, 1.05, 1.0])?;
    let model = BlackScholesMertonParameters::new(curve, 0.0, 0.0)?;
    let mut terms = TouchOptionTerms::new(start, end, 100.0, BarrierType::UpAndIn);
    terms.observation_times = Some(dates);
    terms.settlement_timing = SettlementTiming::AtHit;
    let option = TouchOption::cash(terms, 7.0)?;
    let engine = FiniteDifferenceBinaryBarrierEngine::default();
    for (spot, expected) in [(libm::nextafter(100.0, 0.0), 0.0), (100.0, 7.0)] {
        let context = PricingContext::new(model.clone(), spot, start)?;
        assert!((engine.price(&option, &context)? - expected).abs() < 1e-12);
    }
    Ok(())
}

#[test]
fn accumulator_uses_base_quantity_at_an_exact_future_strike() -> Result<()> {
    let start = Timestamp::new(Date::new(2025, 1, 1)?, 12, 0, 0, 0)?;
    let fixing = Date::new(2025, 1, 2)?.start_of_day();
    let expiry = fixing.add_days(1)?;
    let curve = |base, fixing_factor, expiry_factor| {
        if let Some(base) = base {
            RateTermStructure::new(
                start.add_days(-1)?,
                vec![start, fixing, expiry],
                vec![base, fixing_factor, expiry_factor],
            )
        } else {
            RateTermStructure::new(
                start,
                vec![fixing, expiry],
                vec![fixing_factor, expiry_factor],
            )
        }
    };
    let engine = FiniteDifferenceAccumulatorEngine::default();
    let risk_factor = 1.0 - 33.0 / 1_048_576.0;
    for (risk_base, dividend_base, risk, dividend) in [
        (None, None, risk_factor, None),
        (None, None, risk_factor, Some(1.0 - 1.0 / 1_048_576.0)),
        (
            Some(1.0 - 9.0 / 32768.0),
            Some(1.0 - 4.0 / 32768.0),
            1.0 - 11.0 / 32768.0,
            Some(1.0 - 10.0 / 32768.0),
        ),
    ] {
        let expiry_factor = risk * (1.0 - 1.0 / 4096.0);
        let dividend_curve = match dividend {
            Some(factor) => Rate::from(curve(dividend_base, factor, factor)?),
            None => Rate::from(0.0),
        };
        let model = BlackScholesMertonParameters::new(
            curve(risk_base, risk, expiry_factor)?,
            dividend_curve,
            0.0,
        )?;
        let [risk_base, dividend_base, dividend] =
            [risk_base, dividend_base, dividend].map(|f| f.unwrap_or(1.0));
        let boundary = 100.0 * risk * dividend_base;
        let strike = 100.0 * dividend * risk_base;
        let option = Accumulator::new(AccumulatorTerms {
            knock_out_observation_times: Some(vec![]),
            ..AccumulatorTerms::new(strike, 120.0, 1.0, 2.0, start, expiry, fixing.time_of_day())
        })?;
        for (spot, quantity) in [
            (libm::nextafter(boundary, 0.0), 3.0),
            (boundary, 2.0),
            (libm::nextafter(boundary, f64::INFINITY), 2.0),
        ] {
            let context = PricingContext::new(model.clone(), spot, start)?;
            let expected =
                quantity * (spot * dividend / dividend_base - strike * expiry_factor / risk_base);
            assert!((engine.price(&option, &context)? - expected).abs() < 1e-12);
        }
    }
    Ok(())
}

fn verify<I: OptionContract>(
    option: &I,
    context: &PricingContext,
    expected: f64,
    engines: &[&dyn PricingEngine<I>],
) -> Result<()> {
    for engine in engines {
        assert!((engine.price(option, context)? - expected).abs() < 1e-10);
        let result = engine.price_with_greeks_with_settings(
            option,
            context,
            GreeksRequest::all(),
            Default::default(),
        )?;
        assert!((result.price() - expected).abs() < 1e-10);
        if result.supported().has(Greek::Delta) && !option.at_spot_discontinuity(context, 0.01) {
            assert!(result.get(Greek::Delta).is_some());
        }
        for value in result.values().iter().flatten() {
            assert!(value.is_finite());
        }
    }
    Ok(())
}

#[test]
fn equal_carry_preserves_a_spot_just_above_the_strike() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(25 * 365)?;
    let spot = libm::nextafter(100.0, f64::INFINITY);
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.1, 0.1, 0.0)?,
        spot,
        start,
    )?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let discount = (-2.5_f64).exp();
    assert!((AnalyticDigitalEngine.price(&digital, &context)? - 10.0 * discount).abs() < 1e-12);
    let expected = (spot - 100.0) * discount;
    assert!((AnalyticEuropeanEngine.price(&vanilla, &context)? / expected - 1.0).abs() < 1e-13);
    Ok(())
}

#[test]
fn zero_volatility_prices_agree_across_supported_engines() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(4)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let european = EuropeanOption::new(OptionType::Call, 90.0, start, end)?;
    verify(
        &european,
        &context,
        10.0,
        &[
            &AnalyticEuropeanEngine,
            &CoxRossRubinsteinVanillaEngine::default(),
            &QuadratureVanillaEngine::default(),
            &FiniteDifferenceVanillaEngine::default(),
            &MonteCarloVanillaEngine::default(),
        ],
    )?;
    let american = AmericanOption::new(OptionType::Call, 90.0, start, end)?;
    verify(
        &american,
        &context,
        10.0,
        &[
            &BjerksundStenslandVanillaEngine,
            &CoxRossRubinsteinVanillaEngine::default(),
            &QuadratureVanillaEngine::default(),
            &FiniteDifferenceVanillaEngine::default(),
            &MonteCarloVanillaEngine::default(),
        ],
    )?;
    for (digital, expected) in [
        (
            DigitalOption::cash_or_nothing(OptionType::Call, 90.0, 10.0, start, end)?,
            10.0,
        ),
        (
            DigitalOption::asset_or_nothing(OptionType::Call, 90.0, start, end)?,
            100.0,
        ),
    ] {
        verify(
            &digital,
            &context,
            expected,
            &[
                &AnalyticDigitalEngine,
                &CoxRossRubinsteinDigitalEngine::default(),
                &QuadratureDigitalEngine::default(),
                &FiniteDifferenceDigitalEngine::default(),
                &MonteCarloDigitalEngine::default(),
            ],
        )?;
    }
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 90.0, start, start, end, None)?;
    verify(
        &geometric,
        &context,
        10.0,
        &[
            &AnalyticGeometricAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 90.0, start, start, end, None)?;
    verify(
        &arithmetic,
        &context,
        10.0,
        &[
            &TurnbullWakemanArithmeticAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        90.0,
        start,
        end,
        80.0,
        BarrierType::DownAndOut,
    ))?;
    verify(
        &barrier,
        &context,
        10.0,
        &[
            &AnalyticBarrierEngine,
            &CoxRossRubinsteinBarrierEngine::default(),
            &QuadratureBarrierEngine::default(),
            &FiniteDifferenceBarrierEngine::default(),
            &MonteCarloBarrierEngine::default(),
        ],
    )?;
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        90.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))?;
    verify(
        &double,
        &context,
        10.0,
        &[
            &AnalyticDoubleBarrierEngine,
            &FiniteDifferenceDoubleBarrierEngine::default(),
            &MonteCarloBarrierEngine::default(),
        ],
    )?;
    for cash in [false, true] {
        let terms = BinaryBarrierTerms::new(
            OptionType::Call,
            90.0,
            start,
            end,
            80.0,
            BarrierType::DownAndOut,
        );
        let binary = if cash {
            BinaryBarrierOption::cash(terms, 10.0)?
        } else {
            BinaryBarrierOption::asset(terms)?
        };
        let expected = if cash { 10.0 } else { 100.0 };
        verify(
            &binary,
            &context,
            expected,
            &[
                &AnalyticBinaryBarrierEngine,
                &FiniteDifferenceBinaryBarrierEngine::default(),
            ],
        )?;
        let terms = TouchOptionTerms::new(start, end, 80.0, BarrierType::DownAndOut);
        let touch = if cash {
            TouchOption::cash(terms, 10.0)?
        } else {
            TouchOption::asset(terms)?
        };
        verify(
            &touch,
            &context,
            expected,
            &[
                &AnalyticBinaryBarrierEngine,
                &FiniteDifferenceBinaryBarrierEngine::default(),
            ],
        )?;
    }
    let touch = DoubleTouchOption::cash(
        DoubleTouchOptionTerms::new(start, end, 80.0, 120.0, DoubleBarrierType::KnockOut),
        10.0,
    )?;
    verify(
        &touch,
        &context,
        10.0,
        &[
            &AnalyticDoubleTouchEngine,
            &FiniteDifferenceDoubleTouchEngine::default(),
        ],
    )?;
    let observations = autocallable_observations(&[end], &[120.0], &[0.0])?;
    macro_rules! note {
        ($option:expr, $expected:expr) => {
            verify(
                &$option,
                &context,
                $expected,
                &[
                    &QuadratureAutocallableEngine::default(),
                    &FiniteDifferenceAutocallableEngine::default(),
                    &MonteCarloAutocallableEngine::default(),
                ],
            )?
        };
    }
    note!(
        SnowballOption::new(SnowballTerms::new(
            0.1,
            100.0,
            80.0,
            100.0,
            0.0,
            start,
            end,
            observations.clone(),
            kiyosi::KnockInObservation::EveryTradingDay {
                observation_clock_time: (end).time_of_day()
            },
            TradingCalendar::weekdays()
        ))?,
        1.0 + 0.1 * 4.0 / 365.0
    );
    note!(
        PhoenixOption::new(PhoenixTerms::new(
            0.1,
            100.0,
            80.0,
            100.0,
            0.0,
            start,
            end,
            [end]
                .iter()
                .zip([120.0].iter())
                .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
                .collect::<kiyosi::Result<Vec<_>>>()?,
            [end]
                .iter()
                .zip([0.0].iter())
                .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
                .collect::<kiyosi::Result<Vec<_>>>()?,
            kiyosi::KnockInObservation::EveryTradingDay {
                observation_clock_time: (end).time_of_day()
            },
            TradingCalendar::weekdays(),
        ))?,
        1.1
    );
    note!(
        BinarySnowballOption::new(BinarySnowballTerms::new(
            0.1,
            start,
            end,
            observations.clone(),
            TradingCalendar::weekdays()
        ))?,
        1.0 + 0.1 * 4.0 / 365.0
    );
    note!(
        TernarySnowballOption::new(TernarySnowballTerms::new(
            0.1,
            0.0,
            80.0,
            start,
            end,
            observations,
            kiyosi::KnockInObservation::EveryTradingDay {
                observation_clock_time: (end).time_of_day()
            },
            TradingCalendar::weekdays()
        ))?,
        1.0 + 0.1 * 4.0 / 365.0
    );
    let accumulator = Accumulator::new(AccumulatorTerms::new(
        90.0,
        120.0,
        1.0,
        2.0,
        start,
        end,
        (end).time_of_day(),
    ))?;
    verify(
        &accumulator,
        &context,
        50.0,
        &[
            &QuadratureAccumulatorEngine::default(),
            &FiniteDifferenceAccumulatorEngine::default(),
            &MonteCarloAccumulatorEngine::default(),
        ],
    )?;
    let range = RangeAccrualOption::new(RangeAccrualTerms::new(
        1000.0,
        0.1,
        100.0,
        110.0,
        start,
        end,
        (end).time_of_day(),
    ))?;
    verify(
        &range,
        &context,
        100.0 * 4.0 / 365.0,
        &[&AnalyticRangeAccrualEngine],
    )?;
    Ok(())
}

#[test]
fn deterministic_payoff_boundaries_leave_greeks_undefined_and_solvers_accept_zero() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    for engine in [
        &AnalyticEuropeanEngine as &dyn PricingEngine,
        &CoxRossRubinsteinVanillaEngine::default(),
        &FiniteDifferenceVanillaEngine::default(),
    ] {
        let result = engine.price_with_greeks_with_settings(
            &option,
            &context,
            GreeksRequest::all(),
            Default::default(),
        )?;
        assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
        assert_eq!(result.status(Greek::Vega), GreekStatus::Undefined);
        assert_eq!(
            implied_volatility(
                engine,
                &option,
                &context,
                0.0,
                ImpliedVolatilitySettings::new(0.0, 4.0)
            )?,
            0.0
        );
    }
    for invalid in [-0.1, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            BlackScholesMertonParameters::new(0.0, 0.0, invalid),
            Err(Error::InvalidInput(_))
        ));
    }
    Ok(())
}

#[test]
fn deterministic_american_greeks_follow_the_exercise_boundary() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(365)?;
    let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let saved = engine.solve(&option, &context, 90.0..=110.0)?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    for result in [
        engine.price_with_greeks(&option, &context, request)?,
        saved.price_with_greeks(100.0, request, None)?,
    ] {
        assert_eq!(result.price(), 0.0);
        assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
        assert_eq!(result.status(Greek::Gamma), GreekStatus::Undefined);
    }
    let spot = 100.0 * (-0.05_f64).exp();
    let result = saved.price_with_greeks(spot, request, None)?;
    assert!((result.price() - (100.0 - spot)).abs() < 1e-12);
    assert!((result.require(Greek::Delta)? + 1.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn deterministic_barrier_greeks_follow_fixings_and_in_life_curve_knots() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let knot = start.add_days(2)?;
    let end = start.add_days(4)?;
    let flat = RateTermStructure::flat(0.0, None)?;
    for (risk, dividend) in [
        (
            RateTermStructure::new(start, vec![knot, end], vec![0.5, 1.0])?,
            flat.clone(),
        ),
        (
            flat,
            RateTermStructure::new(start, vec![knot, end], vec![2.0, 1.0])?,
        ),
    ] {
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(risk, dividend, 0.0)?,
            100.0,
            start,
        )?;
        let continuous = TouchOption::cash(
            TouchOptionTerms::new(start, end, 200.0, BarrierType::UpAndIn),
            10.0,
        )?;
        let scheduled = continuous.replace(TouchOptionChanges {
            observation_times: Some(Some(vec![end])),
            ..Default::default()
        })?;
        let fixing_at_knot = scheduled.replace(TouchOptionChanges {
            observation_times: Some(Some(vec![knot, end])),
            ..Default::default()
        })?;
        let early_expiry = continuous.replace(TouchOptionChanges {
            expiry_time: Some(start.add_days(1)?),
            ..Default::default()
        })?;
        assert!(continuous.at_spot_discontinuity(&context, 0.01));
        assert!(fixing_at_knot.at_spot_discontinuity(&context, 0.01));
        assert!(!scheduled.at_spot_discontinuity(&context, 0.01));
        assert!(!early_expiry.at_spot_discontinuity(&context, 0.01));
        let double = DoubleTouchOption::cash(
            DoubleTouchOptionTerms::new(start, end, 50.0, 200.0, DoubleBarrierType::KnockIn),
            10.0,
        )?;
        assert!(double.at_spot_discontinuity(&context, 0.01));
        let engine = FiniteDifferenceBinaryBarrierEngine::default();
        let request = GreeksRequest::new([Greek::Delta]);
        assert!(
            engine
                .price_with_greeks(&continuous, &context, request)?
                .get(Greek::Delta)
                .is_none()
        );
        assert_eq!(
            engine
                .price_with_greeks(&scheduled, &context, request)?
                .require(Greek::Delta)?,
            0.0
        );
    }
    Ok(())
}

#[test]
fn deterministic_touch_discounts_across_distinct_curve_knots() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let end = start.add_days(365)?;
    let engine = FiniteDifferenceBinaryBarrierEngine::default();
    for sign in [-1.0, 1.0] {
        let risk = |day: i32| {
            -sign
                * (0.1 * f64::from(day.min(90))
                    + 0.03 * f64::from((day - 90).clamp(0, 180))
                    + 0.15 * f64::from((day - 270).max(0)))
                / 365.0
        };
        let dividend = |day: i32| {
            -sign * (0.02 * f64::from(day.min(180)) + 0.05 * f64::from((day - 180).max(0))) / 365.0
        };
        for dense in [false, true] {
            let curve = |days: Vec<i32>, log_factor: &dyn Fn(i32) -> f64| {
                RateTermStructure::new(
                    start,
                    days.iter()
                        .map(|&day| start.add_days(i64::from(day)))
                        .collect::<Result<_>>()?,
                    days.iter().map(|&day| log_factor(day).exp()).collect(),
                )
            };
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(
                    curve(
                        if dense {
                            (1..=365).collect()
                        } else {
                            vec![90, 270, 365]
                        },
                        &risk,
                    )?,
                    curve(
                        if dense {
                            (1..=365).collect()
                        } else {
                            vec![180, 365]
                        },
                        &dividend,
                    )?,
                    0.0,
                )?,
                100.0,
                start,
            )?;
            let mut terms = TouchOptionTerms::new(
                start,
                end,
                100.0 * (sign * 0.025).exp(),
                if sign > 0.0 {
                    BarrierType::UpAndIn
                } else {
                    BarrierType::DownAndIn
                },
            );
            terms.settlement_timing = SettlementTiming::AtHit;
            terms.calendar = TradingCalendar::all_days();
            let option = TouchOption::cash(terms, 7.0)?;
            let hit_time =
                270.0 / 365.0 + (0.025 - (0.08 * 90.0 + 0.01 * 90.0 - 0.02 * 90.0) / 365.0) / 0.1;
            let expected = 7.0 * (risk(270) - sign * 0.15 * (hit_time - 270.0 / 365.0)).exp();
            assert!((engine.price(&option, &context)? - expected).abs() < 1e-10);
            let scheduled = option.replace(TouchOptionChanges {
                observation_times: Some(Some(vec![start.add_days(300)?])),
                ..Default::default()
            })?;
            assert!((engine.price(&scheduled, &context)? - 7.0 * risk(300).exp()).abs() < 1e-10);
        }
    }
    Ok(())
}

#[test]
fn deterministic_paths_respect_hit_times_exercise_and_averaging_history() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let middle = start.add_days(180)?;
    let end = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.1, 0.02, 0.0)?,
        100.0,
        start,
    )?;
    let hit = 1.05_f64.ln() / 0.08;
    let expected = 7.0 * (-0.1 * hit).exp();
    let mut terms = BarrierOptionTerms::new(
        OptionType::Call,
        90.0,
        start,
        end,
        105.0,
        BarrierType::UpAndOut,
    );
    terms.rebate = 7.0;
    terms.rebate_timing = RebateTiming::AtHit;
    let barrier = BarrierOption::new(terms)?;
    verify(
        &barrier,
        &context,
        expected,
        &[
            &AnalyticBarrierEngine,
            &CoxRossRubinsteinBarrierEngine::default(),
            &QuadratureBarrierEngine::default(),
            &FiniteDifferenceBarrierEngine::default(),
            &MonteCarloBarrierEngine::default(),
        ],
    )?;
    let mut terms = TouchOptionTerms::new(start, end, 105.0, BarrierType::UpAndIn);
    terms.settlement_timing = SettlementTiming::AtHit;
    let continuous = TouchOption::cash(terms, 7.0)?;
    verify(
        &continuous,
        &context,
        expected,
        &[
            &AnalyticBinaryBarrierEngine,
            &FiniteDifferenceBinaryBarrierEngine::default(),
        ],
    )?;
    let scheduled = continuous.replace(TouchOptionChanges {
        observation_times: Some(Some(vec![end])),
        ..Default::default()
    })?;
    verify(
        &scheduled,
        &context,
        7.0 * (-0.1_f64).exp(),
        &[
            &AnalyticBinaryBarrierEngine,
            &FiniteDifferenceBinaryBarrierEngine::default(),
        ],
    )?;
    let risk = RateTermStructure::new(
        start,
        vec![middle, end],
        vec![
            (-0.2_f64 * 180.0 / 365.0).exp(),
            (0.2_f64 * 5.0 / 365.0).exp(),
        ],
    )?;
    let reversing = PricingContext::new(
        BlackScholesMertonParameters::new(risk, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let engine = FiniteDifferenceBinaryBarrierEngine::default();
    assert!((engine.price(&continuous, &reversing)? - 7.0 / 1.05).abs() < 1e-10);
    assert_eq!(engine.price(&scheduled, &reversing)?, 0.0);
    let american = AmericanOption::new(OptionType::Call, 90.5, start, end)?;
    let exercise_context = PricingContext::new(
        BlackScholesMertonParameters::new(0.1, 0.09, 0.0)?,
        100.0,
        start,
    )?;
    let exercise = (0.1_f64 * 90.5 / (0.09 * 100.0)).ln() / 0.01;
    let expected = 100.0 * (-0.09 * exercise).exp() - 90.5 * (-0.1 * exercise).exp();
    verify(
        &american,
        &exercise_context,
        expected,
        &[
            &BjerksundStenslandVanillaEngine,
            &CoxRossRubinsteinVanillaEngine::default(),
            &QuadratureVanillaEngine::default(),
            &FiniteDifferenceVanillaEngine::default(),
            &MonteCarloVanillaEngine::default(),
        ],
    )?;
    let seasoned = PricingContext::new(
        BlackScholesMertonParameters::new(0.08, 0.02, 0.0)?,
        100.0,
        middle,
    )?;
    let remaining = 185.0 / 365.0;
    let arithmetic_average = 80.0 * 180.0 / 365.0 + 100.0 * (0.06_f64 * remaining).exp_m1() / 0.06;
    let geometric_average = (80.0_f64.ln() * 180.0 / 365.0
        + remaining * (100.0_f64.ln() + 0.06 * remaining / 2.0))
        .exp();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 85.0, start, start, end, Some(80.0))?;
    verify(
        &arithmetic,
        &seasoned,
        (arithmetic_average - 85.0) * (-0.08 * remaining).exp(),
        &[
            &TurnbullWakemanArithmeticAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 85.0, start, start, end, Some(80.0))?;
    verify(
        &geometric,
        &seasoned,
        (geometric_average - 85.0) * (-0.08 * remaining).exp(),
        &[
            &AnalyticGeometricAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    Ok(())
}

#[test]
fn deterministic_structured_events_keep_monitoring_modes_and_settlement_units() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let middle = start.add_days(180)?;
    let end = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.1, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let terms = BinarySnowballTerms::new(
        0.1,
        start,
        end,
        autocallable_observations(&[middle, end], &[105.0, 105.0], &[0.1, 0.1])?,
        TradingCalendar::all_days(),
    );
    let note = BinarySnowballOption::new(terms)?;
    verify(
        &note,
        &context,
        (1.0 + 0.1 * 180.0 / 365.0) * (-0.1_f64 * 180.0 / 365.0).exp(),
        &[
            &QuadratureAutocallableEngine::default(),
            &FiniteDifferenceAutocallableEngine::default(),
            &MonteCarloAutocallableEngine::default(),
        ],
    )?;
    let mut terms = AccumulatorTerms::new(90.0, 105.0, 1.0, 2.0, start, end, (end).time_of_day());
    terms.calendar = TradingCalendar::all_days();
    terms.knock_out_observation_times = Some(vec![middle]);
    let accumulator = Accumulator::new(terms)?;
    let expected =
        180.0 * (100.0 * (0.1_f64 * 180.0 / 365.0).exp() - 90.0) * (-0.1_f64 * 180.0 / 365.0).exp();
    verify(
        &accumulator,
        &context,
        expected,
        &[
            &QuadratureAccumulatorEngine::default(),
            &FiniteDifferenceAccumulatorEngine::default(),
            &MonteCarloAccumulatorEngine::default(),
        ],
    )?;
    let intraday_knots = vec![
        Timestamp::new(start.add_days(30)?.date(), 12, 0, 0, 0)?,
        Timestamp::new(middle.date(), 12, 0, 0, 0)?,
        end,
    ];
    let factors = intraday_knots
        .iter()
        .map(|&time| (-0.1 * start.years_until(time)).exp())
        .collect();
    let curve = RateTermStructure::new(start, intraday_knots, factors)?;
    let curved = PricingContext::new(
        BlackScholesMertonParameters::new(curve, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    assert!(
        (FiniteDifferenceAccumulatorEngine::default().price(&accumulator, &curved)? - expected)
            .abs()
            < 1e-10
    );
    assert!(
        (FiniteDifferenceAutocallableEngine::default().price(&note, &curved)?
            - (1.0 + 0.1 * 180.0 / 365.0) * (-0.1_f64 * 180.0 / 365.0).exp())
        .abs()
            < 1e-12
    );

    let recovery = PricingContext::new(
        BlackScholesMertonParameters::new(0.6, 0.0, 0.0)?,
        70.0,
        start,
    )?;
    let mut terms = SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        autocallable_observations(&[start.add_days(30)?, end], &[300.0, 300.0], &[0.0; 2])?,
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (end).time_of_day(),
        },
        TradingCalendar::all_days(),
    );
    terms.knock_in_observation = KnockInObservation::AtExpiry;
    let expiry_only = SnowballOption::new(terms)?;
    verify(
        &expiry_only,
        &recovery,
        1.1 * (-0.6_f64).exp(),
        &[
            &QuadratureAutocallableEngine::default(),
            &FiniteDifferenceAutocallableEngine::default(),
            &MonteCarloAutocallableEngine::default(),
        ],
    )?;
    Ok(())
}

#[test]
fn deterministic_puts_forward_start_averages_and_sobol_statistics_are_consistent() -> Result<()> {
    let start = Date::new(2025, 1, 6)?.start_of_day();
    let middle = start.add_days(180)?;
    let end = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?,
        100.0,
        start,
    )?;
    let put = EuropeanOption::new(OptionType::Put, 110.0, start, end)?;
    verify(
        &put,
        &context,
        10.0,
        &[
            &AnalyticEuropeanEngine,
            &CoxRossRubinsteinVanillaEngine::default(),
            &QuadratureVanillaEngine::default(),
            &FiniteDifferenceVanillaEngine::default(),
            &MonteCarloVanillaEngine::default(),
        ],
    )?;
    let result = AnalyticEuropeanEngine.price_with_greeks(
        &put,
        &context,
        GreeksRequest::new([Greek::Delta]),
    )?;
    assert!((result.require(Greek::Delta)? + 1.0).abs() < 1e-10);
    let asset_put = DigitalOption::asset_or_nothing(OptionType::Put, 110.0, start, end)?;
    verify(
        &asset_put,
        &context,
        100.0,
        &[
            &AnalyticDigitalEngine,
            &CoxRossRubinsteinDigitalEngine::default(),
            &FiniteDifferenceDigitalEngine::default(),
            &MonteCarloDigitalEngine::default(),
        ],
    )?;
    let result = AnalyticDigitalEngine.price_with_greeks(
        &asset_put,
        &context,
        GreeksRequest::new([Greek::Delta]),
    )?;
    assert!((result.require(Greek::Delta)? - 1.0).abs() < 1e-10);
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.08, 0.02, 0.0)?,
        100.0,
        start,
    )?;
    let future = 185.0 / 365.0;
    let lead = 180.0 / 365.0;
    let arithmetic_average =
        100.0 * (0.06_f64 * lead).exp() * (0.06_f64 * future).exp_m1() / (0.06 * future);
    let geometric_average = 100.0 * (0.06_f64 * (lead + future / 2.0)).exp();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 90.0, start, middle, end, None)?;
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 90.0, start, middle, end, None)?;
    let expected_arithmetic = (arithmetic_average - 90.0) * (-0.08_f64).exp();
    let expected_geometric = (geometric_average - 90.0) * (-0.08_f64).exp();
    verify(
        &arithmetic,
        &context,
        expected_arithmetic,
        &[
            &TurnbullWakemanArithmeticAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    verify(
        &geometric,
        &context,
        expected_geometric,
        &[
            &AnalyticGeometricAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    let sobol = MonteCarloAsianEngine::new(
        MonteCarloSettings {
            path_count: 1,
            sampling: MonteCarloSampling::Sobol,
            antithetic: true,
            ..Default::default()
        }
        .into(),
    )?;
    let estimate = sobol.price_with_statistics(&arithmetic, &context)?;
    assert!((estimate.price() - expected_arithmetic).abs() < 1e-10);
    assert_eq!(estimate.standard_error(), None);
    assert_eq!(estimate.confidence_interval(), None);
    Ok(())
}

#[test]
fn quoted_fixings_use_exact_payoff_boundaries() -> Result<()> {
    let start = Date::new(2025, 1, 2)?.start_of_day();
    let expiry = start.add_days(365)?;
    let curve = |factor| RateTermStructure::new(start, vec![expiry], vec![factor]);
    let model =
        BlackScholesMertonParameters::new(curve(0.927_734_375)?, curve(0.930_664_062_5)?, 0.0)?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 119.125, 10.0, start, expiry)?;
    for (spot, expected) in [
        (118.75, 0.0),
        (libm::nextafter(118.75, f64::INFINITY), 9.277_343_75),
    ] {
        let context = PricingContext::new(model.clone(), spot, start)?;
        assert!((AnalyticDigitalEngine.price(&digital, &context)? - expected).abs() < 1e-12);
    }

    let model = BlackScholesMertonParameters::new(curve(0.625)?, 0.0, 0.0)?;
    let mut terms = TouchOptionTerms::new(start, expiry, 128.0, BarrierType::UpAndIn);
    terms.observation_times = Some(vec![expiry]);
    let touch = TouchOption::cash(terms, 10.0)?;
    for (spot, expected) in [(libm::nextafter(80.0, 0.0), 0.0), (80.0, 6.25)] {
        let context = PricingContext::new(model.clone(), spot, start)?;
        assert!((AnalyticBinaryBarrierEngine.price(&touch, &context)? - expected).abs() < 1e-12);
    }

    let expiry = start.add_days(1)?;
    let model = BlackScholesMertonParameters::new(
        RateTermStructure::new(start, vec![expiry], vec![0.9375])?,
        0.0,
        0.0,
    )?;
    let option = RangeAccrualOption::new(RangeAccrualTerms::new(
        365.0,
        1.0,
        127.0,
        128.0,
        start,
        expiry,
        expiry.time_of_day(),
    ))?;
    for (spot, expected) in [
        (120.0, 0.9375),
        (libm::nextafter(120.0, f64::INFINITY), 0.0),
    ] {
        let context = PricingContext::new(model.clone(), spot, start)?;
        assert!((AnalyticRangeAccrualEngine.price(&option, &context)? - expected).abs() < 1e-12);
    }
    Ok(())
}

#[test]
fn accumulator_settlement_preserves_the_quoted_strike_difference() -> Result<()> {
    let start = Timestamp::new(Date::new(2025, 1, 1)?, 12, 0, 0, 0)?;
    let end = start.add_days(365)?;
    let factor = 0.999_389_648_437_5;
    let strike = 100.0;
    let curve = RateTermStructure::new(start, vec![end], vec![factor])?;
    let model = BlackScholesMertonParameters::new(curve, 0.0, 0.0)?;
    let option = Accumulator::new(AccumulatorTerms {
        calendar: TradingCalendar::all_days(),
        knock_out_observation_times: Some(vec![]),
        ..AccumulatorTerms::new(
            strike,
            200.0,
            1e14,
            1.0,
            start,
            end,
            TimeOfDay::new(0, 0, 0, 0)?,
        )
    })?;
    let at_strike = strike * factor;
    for spot in [
        libm::nextafter(at_strike, 0.0),
        at_strike,
        libm::nextafter(at_strike, f64::INFINITY),
    ] {
        let context = PricingContext::new(model.clone(), spot, start)?;
        let expected = 365.0 * 1e14 * (spot - at_strike);
        let price = QuadratureAccumulatorEngine::default().price(&option, &context)?;
        assert!((price - expected).abs() < 1e-10, "{price} != {expected}");
    }
    Ok(())
}

#[test]
fn accumulator_discounts_large_settlement_before_rounding() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = start.add_days(365)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(750.0, 750.0, 0.0)?,
        110.0,
        start,
    )?;
    for (knock_out_level, purchases) in [(110.0, 365.0_f64), (200.0, 366.0_f64)] {
        let option = Accumulator::new(AccumulatorTerms {
            calendar: TradingCalendar::all_days(),
            knock_out_observation_times: Some(vec![end]),
            ..AccumulatorTerms::new(
                100.0,
                knock_out_level,
                1e300,
                2.0,
                start,
                end,
                end.time_of_day(),
            )
        })?;
        let expected = (purchases.ln() + 1e301_f64.ln() - 750.0).exp();
        for price in [
            FiniteDifferenceAccumulatorEngine::default().price(&option, &context)?,
            QuadratureAccumulatorEngine::default().price(&option, &context)?,
            MonteCarloAccumulatorEngine::default().price(&option, &context)?,
        ] {
            assert!((price / expected - 1.0).abs() < 1e-12);
        }
    }
    Ok(())
}
