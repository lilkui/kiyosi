use kiyosi_core::{
    AmericanOption, AmericanOptionChanges, AnalyticDigitalEngine, AnalyticVanillaEngine,
    BinomialSettings, BjerksundStenslandVanillaEngine, BlackScholesMertonParameterChanges,
    BlackScholesMertonParameters, CoxRossRubinsteinDigitalEngine, CoxRossRubinsteinVanillaEngine,
    Date, DigitalOption, EuropeanOption, Greek, GreeksRequest, ImpliedVolatilitySettings,
    NumericalShiftSettings, ObservationSchedule, OptionType, PricingContext, PricingContextChanges,
    RateTermStructure, Result, TradingCalendar, implied_volatility, numerical_greeks,
};

fn main() -> Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let end = Date::new(2026, 1, 1)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    let context = PricingContext::new(model, 100.0, start.into())?;
    let price = AnalyticVanillaEngine.price(&option, &context)?;
    assert!((price - 9.227005508154036).abs() < 1e-12);
    let greeks = AnalyticVanillaEngine.price_with_greeks(
        &option,
        &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]),
        NumericalShiftSettings::default(),
    )?;
    assert!((greeks.require(Greek::Delta)? - 0.586851146134764).abs() < 1e-12);
    assert!(!greeks.has(Greek::Gamma));
    assert!(
        (implied_volatility(
            &AnalyticVanillaEngine,
            &option,
            &context,
            price,
            ImpliedVolatilitySettings::default()
        )? - 0.2)
            .abs()
            < 1e-8
    );
    let numeric = numerical_greeks(
        &AnalyticVanillaEngine,
        &option,
        &context,
        GreeksRequest::new([Greek::Vega]),
        NumericalShiftSettings::default(),
    )?;
    assert!((numeric.require(Greek::Vega)? - greeks.require(Greek::Vega)?).abs() < 1e-7);
    let digital = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?;
    let digital_result = AnalyticDigitalEngine.price_with_greeks(
        &digital,
        &context,
        GreeksRequest::new([Greek::Delta]),
        Default::default(),
    )?;
    assert!((digital_result.price() - 4.945810910532236).abs() < 1e-12);
    assert!(digital_result.require(Greek::Delta)? > 0.0);
    assert!(
        (implied_volatility(
            &AnalyticDigitalEngine,
            &digital,
            &context,
            digital_result.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.22,
                ..Default::default()
            }
        )? - 0.2)
            .abs()
            < 1e-7
    );
    let american = AmericanOption::new(OptionType::Call, 100.0, start, end)?;
    let american_engine = BjerksundStenslandVanillaEngine;
    let american_result = american_engine.price_with_greeks(
        &american,
        &context,
        GreeksRequest::new([Greek::Delta]),
        Default::default(),
    )?;
    assert!(american_result.price() >= price);
    assert!(american_result.require(Greek::Delta)? > 0.0);
    assert!(
        (implied_volatility(
            &american_engine,
            &american,
            &context,
            american_result.price(),
            Default::default()
        )? - 0.2)
            .abs()
            < 1e-6
    );
    assert_eq!(
        american
            .replace(AmericanOptionChanges {
                strike: Some(105.0),
                ..Default::default()
            })?
            .strike(),
        105.0
    );
    let curve = RateTermStructure::new(start.into(), vec![end.into()], vec![(-0.05_f64).exp()])?;
    let curved = BlackScholesMertonParameters::new(curve, 0.02, 0.2)?;
    let curved = context.replace(PricingContextChanges {
        model_parameters: Some(curved),
        ..Default::default()
    })?;
    assert!((AnalyticVanillaEngine.price(&option, &curved)? - price).abs() < 1e-12);
    assert!(
        curved
            .model_parameters()
            .replace(BlackScholesMertonParameterChanges {
                volatility: Some(0.0),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(curved.model_parameters().volatility(), 0.2);
    let calendar = TradingCalendar::sse();
    assert!(!calendar.is_trading_day(Date::new(2026, 2, 23)?));
    assert_eq!(calendar.trading_days_per_year(), 243);
    let schedule = ObservationSchedule::fixed_interval(
        Date::new(2025, 1, 3)?,
        Date::new(2025, 1, 7)?,
        1,
        &TradingCalendar::default(),
    )?;
    assert_eq!(
        schedule.dates(),
        &[Date::new(2025, 1, 6)?, Date::new(2025, 1, 7)?]
    );
    let tree = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 1024 })?;
    let result = tree.price_with_greeks(
        &option,
        &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]),
        Default::default(),
    )?;
    assert!((result.price() - price).abs() < 0.003);
    assert!(
        (implied_volatility(&tree, &option, &context, result.price(), Default::default())? - 0.2)
            .abs()
            < 1e-7
    );
    let digital_tree = CoxRossRubinsteinDigitalEngine::default();
    let digital_result = digital_tree.price_with_greeks(
        &digital,
        &context,
        GreeksRequest::all(),
        Default::default(),
    )?;
    assert!(digital_result.has(Greek::Delta));
    assert!(!digital_result.has(Greek::Vega));
    Ok(())
}
