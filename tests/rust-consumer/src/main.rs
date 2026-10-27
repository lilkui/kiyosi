use kiyosi_core::{
    AnalyticVanillaEngine, BlackScholesMertonParameterChanges, BlackScholesMertonParameters, Date,
    EuropeanOption, ObservationSchedule, OptionType, PricingContext, PricingContextChanges,
    RateTermStructure, Result, TradingCalendar,
};

fn main() -> Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let end = Date::new(2026, 1, 1)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    let context = PricingContext::new(model, 100.0, start.into())?;
    let price = AnalyticVanillaEngine.price(&option, &context)?;
    assert!((price - 9.227005508154036).abs() < 1e-12);
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
    Ok(())
}
