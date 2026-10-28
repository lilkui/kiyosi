use kiyosi_core::{
    AmericanOption, AmericanOptionChanges, AnalyticDigitalEngine, AnalyticVanillaEngine,
    BinomialSettings, BjerksundStenslandVanillaEngine, BlackScholesMertonParameterChanges,
    BlackScholesMertonParameters, CoxRossRubinsteinDigitalEngine, CoxRossRubinsteinVanillaEngine,
    Date, DigitalOption, EuropeanOption, FiniteDifferenceDigitalEngine, FiniteDifferenceScheme,
    FiniteDifferenceSettings, FiniteDifferenceVanillaEngine, Greek, GreeksRequest,
    ImpliedVolatilitySettings, MonteCarloDigitalEngine, MonteCarloSampling, MonteCarloSettings,
    MonteCarloVanillaEngine, NumericalShiftSettings, ObservationSchedule, OptionType,
    PricingContext, PricingContextChanges, QuadratureDigitalEngine, QuadratureVanillaEngine,
    RateTermStructure, Result, TradingCalendar, implied_volatility, numerical_greeks,
};

fn main() -> Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let end = Date::new(2026, 1, 1)?;
    let barrier = kiyosi_core::BarrierOption::new(kiyosi_core::BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        kiyosi_core::BarrierType::DownAndOut,
    ))?
    .replace(Some(kiyosi_core::BarrierTouchState::Untouched))?;
    let double =
        kiyosi_core::DoubleBarrierOption::new(kiyosi_core::DoubleBarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            80.0,
            120.0,
            kiyosi_core::DoubleBarrierType::KnockOut,
        ))?;
    let double_context = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
        100.0,
        start.into(),
    )?;
    assert!(
        (kiyosi_core::AnalyticDoubleBarrierEngine.price(&double, &double_context)?
            - 1.0838814592583166)
            .abs()
            < 1e-9
    );
    assert_eq!(
        double
            .replace(Some(kiyosi_core::BarrierTouchState::Untouched))?
            .touch_state(),
        Some(kiyosi_core::BarrierTouchState::Untouched)
    );
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    let context = PricingContext::new(model, 100.0, start.into())?;
    let binary = kiyosi_core::BinaryBarrierOption::cash(
        kiyosi_core::BinaryBarrierTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            120.0,
            kiyosi_core::BarrierType::UpAndOut,
        ),
        10.0,
    )?;
    assert!(kiyosi_core::AnalyticBinaryBarrierEngine.price(&binary, &context)? > 0.0);
    let mut touch_terms =
        kiyosi_core::TouchOptionTerms::new(start, end, 100.0, kiyosi_core::BarrierType::UpAndIn);
    touch_terms.settlement_timing = kiyosi_core::SettlementTiming::AtHit;
    let touch = kiyosi_core::TouchOption::asset(touch_terms)?;
    assert_eq!(
        kiyosi_core::AnalyticBinaryBarrierEngine.price(&touch, &context)?,
        100.0
    );
    let barrier_result = kiyosi_core::AnalyticBarrierEngine.price_with_greeks(
        &barrier,
        &context,
        GreeksRequest::all(),
        NumericalShiftSettings::default(),
    )?;
    assert!(barrier_result.require(Greek::Delta)? > 0.0);
    let barrier_sigma = implied_volatility(
        &kiyosi_core::AnalyticBarrierEngine,
        &barrier,
        &context,
        barrier_result.price(),
        ImpliedVolatilitySettings {
            lower_bound: 0.15,
            upper_bound: 0.25,
            ..Default::default()
        },
    )?;
    assert!((barrier_sigma - 0.2).abs() < 1e-7);
    let geometric = kiyosi_core::GeometricAveragePriceOption::new(
        OptionType::Call,
        100.0,
        start,
        start,
        end,
        kiyosi_core::DEFAULT_REALIZED_AVERAGE,
    )?;
    let arithmetic = kiyosi_core::ArithmeticAveragePriceOption::new(
        OptionType::Call,
        100.0,
        start,
        start,
        end,
        kiyosi_core::DEFAULT_REALIZED_AVERAGE,
    )?;
    let geometric_engine = kiyosi_core::AnalyticGeometricAveragePriceEngine;
    let arithmetic_engine = kiyosi_core::TurnbullWakemanArithmeticAveragePriceEngine;
    let asian_mc = kiyosi_core::MonteCarloAsianEngine::new(MonteCarloSettings {
        path_count: 16384,
        step_count: 32,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        ..Default::default()
    })?;
    let asian_result = asian_mc.price_with_greeks(
        &arithmetic,
        &context,
        GreeksRequest::all(),
        Default::default(),
    )?;
    assert!(asian_result.require(Greek::Delta)? > 0.0);
    assert!(!asian_result.has(Greek::Vega));
    assert!(
        (asian_mc.price(&geometric, &context)? - geometric_engine.price(&geometric, &context)?)
            .abs()
            < 0.01
    );
    assert!(
        (implied_volatility(
            &asian_mc,
            &arithmetic,
            &context,
            asian_result.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.4,
                ..Default::default()
            }
        )? - 0.2)
            .abs()
            < 1e-6
    );
    let geometric_price = geometric_engine.price(&geometric, &context)?;
    let arithmetic_price = arithmetic_engine.price(&arithmetic, &context)?;
    assert!(arithmetic_price > geometric_price);
    assert!(
        geometric_engine
            .price_with_greeks(
                &geometric,
                &context,
                GreeksRequest::new([Greek::Delta]),
                Default::default()
            )?
            .require(Greek::Delta)?
            > 0.0
    );
    assert!(
        (implied_volatility(
            &arithmetic_engine,
            &arithmetic,
            &context,
            arithmetic_price,
            Default::default()
        )? - 0.2)
            .abs()
            < 1e-7
    );
    assert_eq!(geometric.replace(110.0)?.realized_average(), 110.0);
    let price = AnalyticVanillaEngine.price(&option, &context)?;
    assert!((price - 9.227005508154036).abs() < 1e-12);
    let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    let american_mc = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        thread_count: 2,
        ..Default::default()
    })?;
    let american_result = american_mc.price_with_greeks(
        &put,
        &context,
        GreeksRequest::new([Greek::Delta]),
        NumericalShiftSettings {
            spot_shift: MonteCarloVanillaEngine::default_american_spot_shift(&context),
            ..Default::default()
        },
    )?;
    assert!((american_result.price() - 6.66).abs() < 0.3);
    assert!(american_result.require(Greek::Delta)? < 0.0);
    let mc = MonteCarloVanillaEngine::new(MonteCarloSettings {
        seed: Some(42),
        thread_count: 2,
        ..Default::default()
    })?;
    let estimate = mc.price_with_statistics(&option, &context)?;
    assert!((estimate.price() - price).abs() < 6.0 * estimate.standard_error().unwrap_or(0.0));
    let sobol = MonteCarloVanillaEngine::new(MonteCarloSettings {
        path_count: 65_536,
        sampling: MonteCarloSampling::Sobol,
        antithetic: true,
        ..mc.settings()
    })?;
    let sobol_estimate = sobol.price_with_statistics(&option, &context)?;
    assert!((sobol_estimate.price() - price).abs() < 0.002);
    assert!(
        sobol_estimate.standard_error().is_none() && sobol_estimate.confidence_interval().is_none()
    );
    assert!(
        (implied_volatility(
            &mc,
            &option,
            &context,
            estimate.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.4,
                ..Default::default()
            }
        )? - 0.2)
            .abs()
            < 1e-8
    );
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
    let digital_mc = MonteCarloDigitalEngine::new(sobol.settings())?;
    let digital_analysis = digital_mc.price_with_greeks(
        &digital,
        &context,
        GreeksRequest::all(),
        Default::default(),
    )?;
    assert!(
        (digital_analysis.price() - AnalyticDigitalEngine.price(&digital, &context)?).abs() < 0.01
    );
    assert!(digital_analysis.has(Greek::Delta) && digital_analysis.has(Greek::Gamma));
    assert!(!digital_analysis.has(Greek::Vega));
    assert!(
        (implied_volatility(
            &digital_mc,
            &digital,
            &context,
            digital_analysis.price(),
            ImpliedVolatilitySettings {
                lower_bound: 0.1,
                upper_bound: 0.22,
                ..Default::default()
            }
        )? - 0.2)
            .abs()
            < 0.001
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
    let integral = QuadratureVanillaEngine.price_with_greeks(
        &option,
        &context,
        GreeksRequest::new([Greek::Delta]),
        Default::default(),
    )?;
    assert!((integral.price() - price).abs() < 1e-9);
    assert!(integral.require(Greek::Delta)? > 0.0);
    assert!(
        (QuadratureDigitalEngine.price(&digital, &context)?
            - AnalyticDigitalEngine.price(&digital, &context)?)
        .abs()
            < 1e-9
    );
    assert!(
        (implied_volatility(
            &QuadratureVanillaEngine,
            &option,
            &context,
            integral.price(),
            Default::default()
        )? - 0.2)
            .abs()
            < 1e-7
    );
    let fd = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
        scheme: FiniteDifferenceScheme::CrankNicolson,
        ..Default::default()
    })?;
    let result =
        fd.price_with_greeks(&option, &context, GreeksRequest::all(), Default::default())?;
    assert!((result.price() - price).abs() < 0.002);
    assert!(result.require(Greek::Delta)? > 0.0);
    assert!((fd.price(&american, &context)? - price).abs() < 0.004);
    let digital_fd = FiniteDifferenceDigitalEngine::default();
    assert!(
        (digital_fd.price(&digital, &context)?
            - AnalyticDigitalEngine.price(&digital, &context)?)
        .abs()
            < 0.001
    );
    let scalar = numerical_greeks(
        &fd,
        &option,
        &context,
        GreeksRequest::new([Greek::Vega]),
        Default::default(),
    )?;
    assert!((scalar.require(Greek::Vega)? - result.require(Greek::Vega)?).abs() < 1e-9);
    Ok(())
}
