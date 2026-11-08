use kiyosi::*;

mod support;

fn verify<I: OptionContract + Clone>(
    option: &I,
    context: &PricingContext,
    expected: f64,
    engines: &[&dyn PricingEngine<I>],
) -> Result<()> {
    let dated = support::dated(option, context);
    let option = &dated;
    for engine in engines {
        assert!((engine.price(option, context)? - expected).abs() < 1e-10);
        let result = engine.price_with_greeks(option, context, GreeksRequest::all())?;
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
        GeometricAveragePriceOption::new(OptionType::Call, 90.0, start, start, end, None, None)?;
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
        ArithmeticAveragePriceOption::new(OptionType::Call, 90.0, start, start, end, None, None)?;
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
            &MonteCarloDoubleBarrierEngine::default(),
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
    let observations = autocallable_observations(&[end], &[120.0], Some(&[0.0]), None)?;
    macro_rules! note {
        ($option:expr) => {
            verify(
                &$option,
                &context,
                1.0 + 0.1 * 4.0 / 365.0,
                &[
                    &QuadratureAutocallableEngine::default(),
                    &FiniteDifferenceAutocallableEngine::default(),
                    &MonteCarloAutocallableEngine::default(),
                ],
            )?
        };
    }
    note!(SnowballOption::new(SnowballTerms::new(
        0.1,
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        observations.clone(),
        Some((end).time_of_day())
    ))?);
    note!(PhoenixOption::new(PhoenixTerms::new(
        kiyosi::PhoenixCoupon::Annual(0.1),
        100.0,
        80.0,
        100.0,
        0.0,
        start,
        end,
        autocallable_observations(&[end], &[120.0], None, Some(&[0.0]))?,
        Some((end).time_of_day())
    ))?);
    note!(BinarySnowballOption::new(BinarySnowballTerms::new(
        0.1,
        start,
        end,
        observations.clone()
    ))?);
    note!(TernarySnowballOption::new(TernarySnowballTerms::new(
        0.1,
        0.0,
        80.0,
        start,
        end,
        observations,
        Some((end).time_of_day())
    ))?);
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
        let result = engine.price_with_greeks(&option, &context, GreeksRequest::all())?;
        assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
        assert_eq!(result.status(Greek::Vega), GreekStatus::Undefined);
        assert_eq!(
            implied_volatility(
                engine,
                &option,
                &context,
                0.0,
                ImpliedVolatilitySettings::default()
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
        observation_mode: Some(ObservationMode::Scheduled),
        observation_times: Some(vec![end]),
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
    let arithmetic = ArithmeticAveragePriceOption::new(
        OptionType::Call,
        85.0,
        start,
        start,
        end,
        Some(80.0),
        Some(start),
    )?;
    verify(
        &arithmetic,
        &seasoned,
        (arithmetic_average - 85.0) * (-0.08 * remaining).exp(),
        &[
            &TurnbullWakemanArithmeticAveragePriceEngine,
            &MonteCarloAsianEngine::default(),
        ],
    )?;
    let geometric = GeometricAveragePriceOption::new(
        OptionType::Call,
        85.0,
        start,
        start,
        end,
        Some(80.0),
        Some(start),
    )?;
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
    let mut terms = BinarySnowballTerms::new(
        0.1,
        start,
        end,
        autocallable_observations(&[middle, end], &[105.0, 105.0], Some(&[0.1, 0.1]), None)?,
    );
    terms.calendar = TradingCalendar::all_days();
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
        autocallable_observations(
            &[start.add_days(30)?, end],
            &[300.0, 300.0],
            Some(&[0.0; 2]),
            None,
        )?,
        Some((end).time_of_day()),
    );
    terms.calendar = TradingCalendar::all_days();
    terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
    terms.observation_clock_time = None;
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
        ArithmeticAveragePriceOption::new(OptionType::Call, 90.0, start, middle, end, None, None)?;
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 90.0, start, middle, end, None, None)?;
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
    let sobol = MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 1,
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        ..Default::default()
    })?;
    let estimate = sobol.price_with_statistics(&arithmetic, &context)?;
    assert!((estimate.price() - expected_arithmetic).abs() < 1e-10);
    assert_eq!(estimate.standard_error(), None);
    assert_eq!(estimate.confidence_interval(), None);
    Ok(())
}
