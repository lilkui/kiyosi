use kiyosi::*;

fn instant(year: i32, month: u32, day: u32, hour: u32) -> Timestamp {
    Timestamp::new(Date::new(year, month, day).unwrap(), hour, 0, 0, 0).unwrap()
}

#[test]
fn business_time_counts_fractional_trading_dates_with_fixed_basis() {
    let friday = instant(2026, 11, 13, 12);
    let monday = instant(2026, 11, 16, 12);
    let clock = ModelTimeConvention::Business(TradingCalendar::weekdays());
    assert!((clock.year_fraction(friday, monday).unwrap() - 1.0 / 261.0).abs() < 1e-15);
    assert!((clock.year_fraction(monday, friday).unwrap() + 1.0 / 261.0).abs() < 1e-15);
    assert_eq!(clock.year_fraction(friday, friday).unwrap(), 0.0);
    let micro = friday.add_micros(1).unwrap();
    let expected = 1.0 / (86_400_000_000.0 * 261.0);
    assert!((clock.year_fraction(friday, micro).unwrap() / expected - 1.0).abs() < 1e-14);
    let weekend = instant(2026, 11, 14, 0);
    assert_eq!(
        clock
            .year_fraction(weekend, weekend.add_days(2).unwrap())
            .unwrap(),
        0.0
    );
    let leap_start = instant(2024, 2, 28, 12);
    let leap_end = instant(2024, 3, 1, 12);
    assert!(
        (ModelTimeConvention::default()
            .year_fraction(leap_start, leap_end)
            .unwrap()
            - 2.0 / 365.0)
            .abs()
            < 1e-15
    );
    let all_days = ModelTimeConvention::Business(TradingCalendar::all_days());
    assert!((all_days.year_fraction(leap_start, leap_end).unwrap() - 2.0 / 365.0).abs() < 1e-15);
    let year_end = instant(2025, 12, 31, 12);
    let next_year = year_end.add_days(1).unwrap();
    let amended = TradingCalendar::custom(
        TradingCalendar::weekdays(),
        vec![year_end.date()],
        vec![],
        Some(243),
    )
    .unwrap();
    assert!(
        (ModelTimeConvention::Business(amended)
            .year_fraction(year_end, next_year)
            .unwrap()
            - 0.5 / 243.0)
            .abs()
            < 1e-15
    );
}

#[test]
fn model_defaults_replacement_and_calendar_validation_preserve_units() {
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap();
    assert_eq!(
        model.time_convention(),
        &ModelTimeConvention::Actual365Fixed
    );
    let clock = ModelTimeConvention::Business(TradingCalendar::weekdays());
    let changed = model
        .replace(BlackScholesMertonParameterChanges {
            time_convention: Some(clock.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.time_convention(), &clock);
    assert_eq!(changed.volatility(), 0.2);
    assert_eq!(
        model.time_convention(),
        &ModelTimeConvention::Actual365Fixed
    );
    assert!(matches!(
        ModelTimeConvention::from_inputs(true, None),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        ModelTimeConvention::from_inputs(false, Some(TradingCalendar::weekdays())),
        Err(Error::InvalidInput(_))
    ));
    let inputs = HestonParameterInputs {
        risk_free_rate: 0.05.into(),
        dividend_yield: 0.02.into(),
        initial_variance: 0.04,
        long_run_variance: 0.04,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.3,
        correlation: -0.7,
    };
    assert!(matches!(
        HestonParameters::new_with_time_convention(inputs.clone(), &clock),
        Err(Error::UnsupportedOperation(_))
    ));
    let heston = HestonParameters::new(inputs).unwrap();
    assert!(matches!(
        heston.replace(HestonParameterChanges {
            time_convention: Some(clock),
            ..Default::default()
        }),
        Err(Error::UnsupportedOperation(_))
    ));
    assert_eq!(
        heston.time_convention(),
        &ModelTimeConvention::Actual365Fixed
    );
}

#[test]
fn european_pde_uses_business_variance_and_natural_day_rates() {
    let start = instant(2026, 11, 13, 12);
    let end = start.add_days(3).unwrap();
    let clock = ModelTimeConvention::Business(TradingCalendar::weekdays());
    let variance_time = clock.year_fraction(start, end).unwrap();
    let natural_time = start.years_until(end);
    let model =
        BlackScholesMertonParameters::new_with_time_convention(0.07, 0.02, 0.3, clock).unwrap();
    let context = PricingContext::new(model, 100.0, start).unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let reference = PricingContext::new(
        BlackScholesMertonParameters::new(0.07, 0.02, 0.3 * (variance_time / natural_time).sqrt())
            .unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let expected = AnalyticEuropeanEngine.price(&option, &reference).unwrap();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 600,
        time_step_count: 300,
        ..Default::default()
    })
    .unwrap();
    assert!((engine.price(&option, &context).unwrap() - expected).abs() < 0.002);
    let surface =
        LocalVolatilitySurface::new(vec![start, end], vec![50.0, 150.0], vec![vec![0.3; 2]; 2])
            .unwrap();
    let local = LocalVolatilityParameters::new_with_time_convention(
        0.07,
        0.02,
        surface,
        context.model_parameters().time_convention().clone(),
    )
    .unwrap();
    let local = context
        .replace(PricingContextChanges {
            model_parameters: Some(local.into()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&option, &local).unwrap() - engine.price(&option, &context).unwrap()).abs()
            < 1e-10
    );
    assert!(matches!(
        AnalyticEuropeanEngine.price(&option, &context),
        Err(Error::UnsupportedOperation(_))
    ));
}

#[test]
fn implied_volatility_rejects_closed_horizon() -> Result<()> {
    let start = instant(2026, 11, 14, 12);
    let end = start.add_days(1)?;
    let model = BlackScholesMertonParameters::new_with_time_convention(
        0.0,
        0.0,
        0.2,
        ModelTimeConvention::Business(TradingCalendar::weekdays()),
    )?;
    let context = PricingContext::new(model, 100.0, start)?;
    let option = EuropeanOption::new(OptionType::Call, 90.0, start, end)?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let price = engine.price(&option, &context)?;
    for lower in [0.0, 0.1] {
        assert!(matches!(
            implied_volatility(
                &engine,
                &option,
                &context,
                price,
                ImpliedVolatilitySettings::new(lower, 1.0)
            ),
            Err(Error::UnsupportedOperation(_))
        ));
    }
    Ok(())
}

#[test]
fn changing_local_volatility_preserves_natural_dates_and_business_variance() {
    let start = instant(2026, 11, 13, 12);
    let end = start.add_days(3).unwrap();
    let integral = |days: f64| {
        0.2_f64.powi(2) * days + 0.2 * 0.1 * days.powi(2) + 0.1_f64.powi(2) * days.powi(3) / 3.0
    };
    let variance = (integral(0.5) + integral(3.0) - integral(2.5)) / 261.0;
    let surface = LocalVolatilitySurface::new(
        vec![start, end],
        vec![50.0, 150.0],
        vec![vec![0.2; 2], vec![0.5; 2]],
    )
    .unwrap();
    let model = LocalVolatilityParameters::new_with_time_convention(
        0.07,
        0.02,
        surface,
        ModelTimeConvention::Business(TradingCalendar::weekdays()),
    )
    .unwrap();
    let context = PricingContext::new(model, 100.0, start).unwrap();
    let reference = PricingContext::new(
        BlackScholesMertonParameters::new(0.07, 0.02, (variance / start.years_until(end)).sqrt())
            .unwrap(),
        100.0,
        start,
    )
    .unwrap();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        asset_step_count: 600,
        time_step_count: 300,
        ..Default::default()
    })
    .unwrap();
    let expected = AnalyticEuropeanEngine.price(&option, &reference).unwrap();
    assert!((engine.price(&option, &context).unwrap() - expected).abs() < 0.002);
}
