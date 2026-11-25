use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use kiyosi::*;

fn instant(year: i32, month: u32, day: u32, hour: u32) -> Timestamp {
    Timestamp::new(Date::new(year, month, day).unwrap(), hour, 0, 0, 0).unwrap()
}

#[test]
fn fd_rejects_pre_effective_valuations_before_calendar_search() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let calendar = TradingCalendar::new(
        {
            let calls = Arc::clone(&calls);
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                false
            }
        },
        252,
    )?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.05,
        0.02,
        0.2,
        VarianceTimeConvention::Business(calendar),
    )?;
    let start = instant(2026, 1, 1, 0);
    let expiry = start.add_days(7)?;
    let context = PricingContext::new(model, 100.0, start.add_days(-1)?)?;
    let american = AmericanOption::new(OptionType::Call, 100.0, start, expiry)?;
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        80.0,
        BarrierType::DownAndOut,
    ))?;
    let binary = BinaryBarrierOption::cash(
        BinaryBarrierTerms::new(
            OptionType::Call,
            100.0,
            start,
            expiry,
            80.0,
            BarrierType::DownAndOut,
        ),
        10.0,
    )?;
    let snowball = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.06,
        start,
        expiry,
        vec![AutocallableObservation::snowball(expiry, 110.0, 0.08)?],
        TradingCalendar::weekdays(),
    ))?;
    let assert_rejected = |product: &str, price| {
        assert!(
            matches!(price, Err(Error::InvalidInput(message)) if message == "valuation outside contract life"),
            "{product} must reject a pre-effective valuation"
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "{product} must reject before consulting the variance calendar"
        );
    };
    assert_rejected(
        "American",
        FiniteDifferenceVanillaEngine::default().price(&american, &context),
    );
    assert_rejected(
        "barrier",
        FiniteDifferenceBarrierEngine::default().price(&barrier, &context),
    );
    assert_rejected(
        "binary barrier",
        FiniteDifferenceBinaryBarrierEngine::default().price(&binary, &context),
    );
    assert_rejected(
        "Snowball",
        FiniteDifferenceAutocallableEngine::default().price(&snowball, &context),
    );
    Ok(())
}

#[test]
fn business_time_counts_fractional_trading_dates_with_fixed_basis() {
    let friday = instant(2026, 11, 13, 12);
    let monday = instant(2026, 11, 16, 12);
    let clock = VarianceTimeConvention::Business(TradingCalendar::weekdays());
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
        (VarianceTimeConvention::default()
            .year_fraction(leap_start, leap_end)
            .unwrap()
            - 2.0 / 365.0)
            .abs()
            < 1e-15
    );
    let all_days = VarianceTimeConvention::Business(TradingCalendar::all_days());
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
        (VarianceTimeConvention::Business(amended)
            .year_fraction(year_end, next_year)
            .unwrap()
            - 0.5 / 243.0)
            .abs()
            < 1e-15
    );
}

#[test]
fn all_days_365_matches_natural_clock_for_constant_surface() -> Result<()> {
    let start = instant(2026, 11, 13, 12);
    let end = start.add_days(3)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let surface =
        LocalVolatilitySurface::new(vec![start, end], vec![50.0, 150.0], vec![vec![0.3; 2]; 2])?;
    let natural = LocalVolatilityParameters::new(0.07, 0.02, surface)?;
    let business = natural.replace(LocalVolatilityParameterChanges {
        variance_time_convention: Some(VarianceTimeConvention::Business(
            TradingCalendar::all_days(),
        )),
        ..Default::default()
    })?;
    let engine = FiniteDifferenceVanillaEngine::default();
    let expected = engine.price(&option, &PricingContext::new(natural, 100.0, start)?)?;
    let actual = engine.price(&option, &PricingContext::new(business, 100.0, start)?)?;
    assert!((actual - expected).abs() <= 1e-10, "{actual} != {expected}");
    Ok(())
}

#[test]
fn all_days_365_matches_natural_clock_for_finite_difference_theta() -> Result<()> {
    let start = instant(2025, 1, 1, 0);
    let end = start.add_days(365)?;
    let natural = BlackScholesMertonParameters::new(0.05, 0.02, 0.04)?;
    let business = natural.replace(BlackScholesMertonParameterChanges {
        variance_time_convention: Some(VarianceTimeConvention::Business(
            TradingCalendar::all_days(),
        )),
        ..Default::default()
    })?;
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let settings = FiniteDifferenceSettings {
        asset_step_count: 80,
        time_step_count: 80,
        ..Default::default()
    };
    let vanilla_engine = FiniteDifferenceVanillaEngine::new(settings)?;
    let digital_engine = FiniteDifferenceDigitalEngine::new(settings)?;
    let request = GreeksRequest::new([Greek::Theta]);
    let theta = |model| -> Result<[f64; 2]> {
        let context = PricingContext::new(model, 100.0, start)?;
        Ok([
            vanilla_engine
                .price_with_greeks(&vanilla, &context, request)?
                .require(Greek::Theta)?,
            digital_engine
                .price_with_greeks(&digital, &context, request)?
                .require(Greek::Theta)?,
        ])
    };
    for ((expected, actual), product) in theta(natural)?
        .into_iter()
        .zip(theta(business)?)
        .zip(["vanilla", "digital"])
    {
        assert!(
            (actual - expected).abs() <= 1e-10,
            "{product}: {actual} != {expected}"
        );
    }
    Ok(())
}

#[test]
fn all_days_365_matches_natural_clock_for_barriers_and_continuations() -> Result<()> {
    let start = instant(2025, 1, 1, 0);
    let end = start.add_days(365)?;
    let natural = BlackScholesMertonParameters::new(0.05, 0.02, 1.0)?;
    let business = natural.replace(BlackScholesMertonParameterChanges {
        variance_time_convention: Some(VarianceTimeConvention::Business(
            TradingCalendar::all_days(),
        )),
        ..Default::default()
    })?;
    let natural = PricingContext::new(natural, 100.0, start)?;
    let business = PricingContext::new(business, 100.0, start)?;
    let option = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        BarrierType::DownAndOut,
    ))?;
    let barrier = FiniteDifferenceBarrierEngine::default();
    let expected = barrier.price(&option, &natural)?;
    assert!((barrier.price(&option, &business)? - expected).abs() <= 1e-10);
    let natural_solution = barrier.solve(&option, &natural, 99.0..=101.0)?;
    let business_solution = barrier.solve(&option, &business, 99.0..=101.0)?;
    assert_eq!(natural_solution.query_work(), 0);
    assert_eq!(business_solution.query_work(), 0);
    assert!((business_solution.price(100.0)? - natural_solution.price(100.0)?).abs() <= 1e-10);

    let binary = BinaryBarrierOption::cash(
        BinaryBarrierTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            110.0,
            BarrierType::DownAndIn,
        ),
        10.0,
    )?;
    let engine = FiniteDifferenceBinaryBarrierEngine::default();
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let expected = AnalyticDigitalEngine.price(&digital, &natural)?;
    assert!((engine.price(&binary, &business)? - expected).abs() <= 1e-10);
    let solution = engine.solve(&binary, &business, 99.0..=101.0)?;
    assert!((solution.price(100.0)? - expected).abs() <= 1e-10);
    Ok(())
}

#[test]
fn model_defaults_replacement_and_calendar_validation_preserve_units() {
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap();
    assert_eq!(
        model.variance_time_convention(),
        &VarianceTimeConvention::Actual365Fixed
    );
    let clock = VarianceTimeConvention::Business(TradingCalendar::weekdays());
    let changed = model
        .replace(BlackScholesMertonParameterChanges {
            variance_time_convention: Some(clock.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.variance_time_convention(), &clock);
    assert_eq!(changed.volatility(), 0.2);
    assert_eq!(
        model.variance_time_convention(),
        &VarianceTimeConvention::Actual365Fixed
    );
    assert!(matches!(
        VarianceTimeConvention::from_inputs(true, None),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        VarianceTimeConvention::from_inputs(false, Some(TradingCalendar::weekdays())),
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
        HestonParameters::new_with_variance_time_convention(inputs.clone(), &clock),
        Err(Error::UnsupportedOperation(_))
    ));
    let heston = HestonParameters::new(inputs).unwrap();
    assert!(matches!(
        heston.replace(HestonParameterChanges {
            variance_time_convention: Some(clock),
            ..Default::default()
        }),
        Err(Error::UnsupportedOperation(_))
    ));
    assert_eq!(
        heston.variance_time_convention(),
        &VarianceTimeConvention::Actual365Fixed
    );
}

#[test]
fn european_pde_uses_business_variance_and_natural_day_rates() {
    let start = instant(2026, 11, 13, 12);
    let end = start.add_days(3).unwrap();
    let clock = VarianceTimeConvention::Business(TradingCalendar::weekdays());
    let variance_time = clock.year_fraction(start, end).unwrap();
    let natural_time = start.years_until(end);
    let model =
        BlackScholesMertonParameters::new_with_variance_time_convention(0.07, 0.02, 0.3, clock)
            .unwrap();
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
    let local = LocalVolatilityParameters::new_with_variance_time_convention(
        0.07,
        0.02,
        surface,
        context
            .model_parameters()
            .variance_time_convention()
            .clone(),
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
fn continuous_barriers_resolve_before_variance_resumes() -> Result<()> {
    let start = instant(2026, 11, 14, 0);
    let expiry = start.add_days(3)?;
    let clock = VarianceTimeConvention::Business(TradingCalendar::weekdays());
    let model =
        BlackScholesMertonParameters::new_with_variance_time_convention(0.07, 0.02, 0.3, clock)?;
    let context = PricingContext::new(model, 100.0, start)?;
    let option = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        90.0,
        start,
        expiry,
        100.01,
        BarrierType::UpAndOut,
    ))?;
    let engine = FiniteDifferenceBarrierEngine::default();
    assert_eq!(engine.price(&option, &context)?, 0.0);
    let solution = engine.solve(&option, &context, 99.0..=101.0)?;
    assert_eq!(solution.price(100.0)?, 0.0);
    assert!(solution.price(99.0)? > 0.0);
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            0.0,
            ImpliedVolatilitySettings::new(0.0, 1.0),
        ),
        Err(Error::UnsupportedOperation(_))
    ));
    Ok(())
}

#[test]
fn implied_volatility_rejects_closed_horizon() -> Result<()> {
    let start = instant(2026, 11, 14, 12);
    let end = start.add_days(1)?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.0,
        0.0,
        0.2,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
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
fn closed_weekend_needs_no_surface_coverage_at_expiry() -> Result<()> {
    let start = instant(2026, 11, 14, 0);
    let expiry = start.add_days(1)?;
    let surface = LocalVolatilitySurface::new(
        vec![start, instant(2026, 11, 14, 12)],
        vec![80.0, 120.0],
        vec![vec![0.2; 2]; 2],
    )?;
    let model = LocalVolatilityParameters::new_with_variance_time_convention(
        0.0,
        0.0,
        surface,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
    )?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
    let context = PricingContext::new(model, 110.0, start)?;
    let engine = FiniteDifferenceVanillaEngine::default();
    assert!((engine.price(&option, &context)? - 10.0).abs() < 1e-12);
    let solution = engine.solve(&option, &context, 90.0..=120.0)?;
    assert!((solution.price(110.0)? - 10.0).abs() < 1e-12);
    assert_eq!(solution.price(90.0)?, 0.0);
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
    let model = LocalVolatilityParameters::new_with_variance_time_convention(
        0.07,
        0.02,
        surface,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
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
