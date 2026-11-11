use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 1).unwrap().start_of_day(),
        Date::new(2026, 1, 1).unwrap().start_of_day(),
    )
}
fn model() -> HestonParameters {
    HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.05.into(),
        dividend_yield: 0.02.into(),
        initial_variance: 0.04,
        long_run_variance: 0.04,
        mean_reversion_speed: 1.5,
        vol_of_vol: 0.3,
        correlation: -0.7,
    })
    .unwrap()
}
fn context(model: HestonParameters) -> PricingContext {
    PricingContext::new(model, 100.0, dates().0).unwrap()
}
fn option(kind: OptionType) -> EuropeanOption {
    EuropeanOption::new(kind, 100.0, dates().0, dates().1).unwrap()
}

#[test]
fn heston_matches_quantlib_and_put_call_parity() {
    // QuantLib 1.43, adaptive Gatheral integration at relative tolerance 1e-12.
    for (model, reference) in [
        (model(), 9.011_278_407_541_74),
        (
            HestonParameters::new(kiyosi::HestonParameterInputs {
                risk_free_rate: 0.05.into(),
                dividend_yield: 0.02.into(),
                initial_variance: 0.04,
                long_run_variance: 0.04,
                mean_reversion_speed: 0.2,
                vol_of_vol: 1.0,
                correlation: -0.9,
            })
            .unwrap(),
            6.256_279_042_682_394,
        ),
    ] {
        let context = context(model);
        let engine = AnalyticHestonEuropeanEngine::default();
        let call = engine.price(&option(OptionType::Call), &context).unwrap();
        let put = engine.price(&option(OptionType::Put), &context).unwrap();
        assert!(
            (call - reference).abs() <= 1e-8 + 1e-8 * reference,
            "{call} != {reference}"
        );
        assert!((call - put - 100.0 * ((-0.02_f64).exp() - (-0.05_f64).exp())).abs() < 1e-12);
    }
}

#[test]
fn deterministic_variance_and_absorbing_zero_have_independent_prices() {
    let engine = AnalyticHestonEuropeanEngine::new(HestonIntegrationSettings {
        max_evaluations: 1,
        ..Default::default()
    })
    .unwrap();
    for (v0, theta, kappa) in [
        (0.04_f64, 0.04_f64, 1.5_f64),
        (0.09, 0.01, 2.0),
        (0.04, 0.09, 0.0),
        (0.0, 0.04, 1.5),
    ] {
        let variance = if kappa == 0.0 {
            v0
        } else {
            theta + (v0 - theta) * (1.0 - (-kappa).exp()) / kappa
        };
        let bsm = PricingContext::new(
            BlackScholesMertonParameters::new(0.05, 0.02, variance.sqrt()).unwrap(),
            100.0,
            dates().0,
        )
        .unwrap();
        let heston = context(
            HestonParameters::new(kiyosi::HestonParameterInputs {
                risk_free_rate: 0.05.into(),
                dividend_yield: 0.02.into(),
                initial_variance: v0,
                long_run_variance: theta,
                mean_reversion_speed: kappa,
                vol_of_vol: 0.0,
                correlation: 1.0,
            })
            .unwrap(),
        );
        for kind in [OptionType::Call, OptionType::Put] {
            let expected = AnalyticEuropeanEngine.price(&option(kind), &bsm).unwrap();
            assert!((engine.price(&option(kind), &heston).unwrap() - expected).abs() < 1e-12);
        }
    }
    let zero = context(
        HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: 0.0,
            long_run_variance: 0.0,
            mean_reversion_speed: 0.0,
            vol_of_vol: 0.3,
            correlation: -1.0,
        })
        .unwrap(),
    );
    let expected = 100.0 * ((-0.02_f64).exp() - (-0.05_f64).exp());
    assert!((engine.price(&option(OptionType::Call), &zero).unwrap() - expected).abs() < 1e-12);
    assert_eq!(engine.price(&option(OptionType::Put), &zero).unwrap(), 0.0);
}

#[test]
fn variance_boundaries_are_valid_and_replacements_are_atomic() {
    let original = model();
    for value in [-f64::EPSILON, f64::NAN, f64::INFINITY] {
        for changes in [
            HestonParameterChanges {
                initial_variance: Some(value),
                ..Default::default()
            },
            HestonParameterChanges {
                long_run_variance: Some(value),
                ..Default::default()
            },
            HestonParameterChanges {
                mean_reversion_speed: Some(value),
                ..Default::default()
            },
            HestonParameterChanges {
                vol_of_vol: Some(value),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                original.replace(changes),
                Err(Error::InvalidInput(_))
            ));
        }
    }
    for correlation in [
        -1.0 - f64::EPSILON,
        1.0 + f64::EPSILON,
        f64::NAN,
        f64::INFINITY,
    ] {
        assert!(matches!(
            original.replace(HestonParameterChanges {
                correlation: Some(correlation),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    let replaced = original
        .replace(HestonParameterChanges {
            initial_variance: Some(0.09),
            correlation: Some(1.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(original, model());
    assert_eq!(replaced.initial_variance(), 0.09);
    assert_eq!(replaced.correlation(), 1.0);
    assert_eq!(replaced.long_run_variance(), 0.04);
}

#[test]
fn settings_validation_budget_and_request_state_are_explicit() {
    for settings in [
        HestonIntegrationSettings {
            absolute_tolerance: -1.0,
            ..Default::default()
        },
        HestonIntegrationSettings {
            relative_tolerance: f64::NAN,
            ..Default::default()
        },
        HestonIntegrationSettings {
            absolute_tolerance: 0.0,
            relative_tolerance: 0.0,
            ..Default::default()
        },
        HestonIntegrationSettings {
            max_evaluations: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            AnalyticHestonEuropeanEngine::new(settings),
            Err(Error::InvalidInput(_))
        ));
    }
    let engine = AnalyticHestonEuropeanEngine::new(HestonIntegrationSettings {
        max_evaluations: 1,
        ..Default::default()
    })
    .unwrap();
    let context = context(model());
    let saved = context.clone();
    assert!(matches!(
        engine.price(&option(OptionType::Call), &context),
        Err(Error::NonConvergence(_))
    ));
    assert_eq!(engine.settings().max_evaluations, 1);
    assert_eq!(context, saved);
    assert!(
        (AnalyticHestonEuropeanEngine::default()
            .price(&option(OptionType::Call), &context)
            .unwrap()
            - 9.011_278_407_541_74)
            .abs()
            < 1e-7
    );
}

#[test]
fn zero_variance_parameters_and_correlation_endpoints_match_independent_limits() {
    // QuantLib 1.43: exact rho endpoints use Gatheral/exp-sinh (1e-11 and
    // 1e-13 agree); zero model parameters use positive limits 1e-10 -> 1e-12.
    for (changes, expected) in [
        (
            HestonParameterChanges {
                correlation: Some(1.0),
                ..Default::default()
            },
            8.794_471_051_466_95,
        ),
        (
            HestonParameterChanges {
                correlation: Some(-1.0),
                ..Default::default()
            },
            9.001_988_480_441_852,
        ),
        (
            HestonParameterChanges {
                initial_variance: Some(0.0),
                correlation: Some(0.0),
                ..Default::default()
            },
            6.700_977_765_814_732_5,
        ),
        (
            HestonParameterChanges {
                mean_reversion_speed: Some(0.0),
                long_run_variance: Some(0.0),
                correlation: Some(0.0),
                ..Default::default()
            },
            8.577_592_757_927_446,
        ),
        (
            HestonParameterChanges {
                mean_reversion_speed: Some(0.0),
                ..Default::default()
            },
            8.695_615_806_894_695,
        ),
        (
            HestonParameterChanges {
                long_run_variance: Some(0.0),
                ..Default::default()
            },
            6.835_650_794_549_077,
        ),
    ] {
        let context = context(model().replace(changes).unwrap());
        let actual = AnalyticHestonEuropeanEngine::default()
            .price(&option(OptionType::Call), &context)
            .unwrap();
        assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
    }
}

#[test]
fn relative_tail_accuracy_convergence_and_monetary_scaling() {
    let tight = AnalyticHestonEuropeanEngine::new(HestonIntegrationSettings {
        absolute_tolerance: 0.0,
        relative_tolerance: 1e-8,
        max_evaluations: 40_000,
    })
    .unwrap();
    let tail = EuropeanOption::new(
        OptionType::Put,
        70.0,
        dates().0,
        Date::new(2025, 1, 31).unwrap().start_of_day(),
    )
    .unwrap();
    // QuantLib 1.43 adaptive Gatheral at 1e-10 and 1e-12 agrees within 2e-14.
    let expected = 3.105_513_031_411_244e-6;
    let actual = tight.price(&tail, &context(model())).unwrap();
    assert!((actual / expected - 1.0).abs() < 1e-8);
    for scale in [1e-100, 1.0, 1e100] {
        let context = PricingContext::new(model(), 100.0 * scale, dates().0).unwrap();
        let option =
            EuropeanOption::new(OptionType::Call, 100.0 * scale, dates().0, dates().1).unwrap();
        let actual = tight.price(&option, &context).unwrap() / scale;
        assert!((actual - 9.011_278_407_541_74).abs() < 1e-7);
    }
    let coarse = AnalyticHestonEuropeanEngine::default()
        .price(&option(OptionType::Call), &context(model()))
        .unwrap();
    let fine = tight
        .price(&option(OptionType::Call), &context(model()))
        .unwrap();
    assert!((fine - 9.011_278_407_541_74).abs() <= (coarse - 9.011_278_407_541_74).abs() + 1e-12);
}

#[test]
fn curves_life_and_expiry_reuse_european_contracts() {
    let (start, end) = dates();
    let risk = RateTermStructure::new(
        start,
        vec![Date::new(2025, 7, 1).unwrap().start_of_day(), end],
        vec![0.99, (-0.05_f64).exp()],
    )
    .unwrap();
    let dividend = RateTermStructure::new(start, vec![end], vec![(-0.02_f64).exp()]).unwrap();
    let parameters = model()
        .replace(HestonParameterChanges {
            risk_free_rate: Some(risk.clone().into()),
            dividend_yield: Some(dividend.into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(parameters.risk_free_curve(), &risk);
    assert!(matches!(
        parameters.flat_risk_free_rate(),
        Err(Error::UnsupportedOperation(_))
    ));
    let context = context(parameters);
    let engine = AnalyticHestonEuropeanEngine::default();
    assert!(
        (engine.price(&option(OptionType::Call), &context).unwrap() - 9.011_278_407_541_74).abs()
            < 1e-7
    );
    let terminal = context
        .replace(PricingContextChanges {
            valuation_time: Some(end),
            spot_price: Some(110.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.price(&option(OptionType::Call), &terminal).unwrap(),
        10.0
    );
    assert_eq!(
        engine.price(&option(OptionType::Put), &terminal).unwrap(),
        0.0
    );
    let outside = context.replace(PricingContextChanges {
        valuation_time: Some(Timestamp::new(end.date(), 0, 0, 0, 1).unwrap()),
        ..Default::default()
    });
    assert!(matches!(outside, Err(Error::InvalidInput(_))));
    let too_long = EuropeanOption::new(
        OptionType::Call,
        100.0,
        start,
        Date::new(2026, 1, 2).unwrap().start_of_day(),
    )
    .unwrap();
    assert!(matches!(
        engine.price(&too_long, &context),
        Err(Error::InvalidInput(_))
    ));
    let dated = PricingContext::new(
        model(),
        100.0,
        Timestamp::new(end.date(), 0, 0, 0, 1).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        engine.price(&option(OptionType::Call), &dated),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn unsupported_requests_and_invalid_computations_preserve_categories() {
    fn send_sync<T: Send + Sync>() {}
    let engine = AnalyticHestonEuropeanEngine::default();
    let context = context(model());
    let option = option(OptionType::Call);
    let request = GreeksRequest::new([Greek::Delta]);
    let result = engine
        .price_with_greeks(&option, &context, request)
        .unwrap();
    assert_eq!(result.price(), engine.price(&option, &context).unwrap());
    assert_eq!(
        result.status(Greek::Delta),
        kiyosi::GreekStatus::Unsupported
    );
    assert!(matches!(
        numerical_greeks(&engine, &option, &context, request, Default::default()),
        Err(Error::UnsupportedOperation(_))
    ));
    assert!(matches!(
        implied_volatility(
            &engine,
            &option,
            &context,
            9.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        ),
        Err(Error::UnsupportedOperation(_))
    ));
    assert!(matches!(
        FiniteDifferenceVanillaEngine::default().price(&option, &context),
        Err(Error::UnsupportedOperation(_))
    ));
    assert!(matches!(
        AnalyticEuropeanEngine.price(&option, &context),
        Err(Error::UnsupportedOperation(_))
    ));
    let bsm = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        dates().1,
    )
    .unwrap();
    assert!(matches!(
        engine.price(&option, &bsm),
        Err(Error::UnsupportedOperation(_))
    ));
    let extreme = context
        .replace(PricingContextChanges {
            model_parameters: Some(
                model()
                    .replace(HestonParameterChanges {
                        initial_variance: Some(1e308),
                        long_run_variance: Some(1e308),
                        ..Default::default()
                    })
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let distant = EuropeanOption::new(
        OptionType::Call,
        100.0,
        dates().0,
        Date::new(2027, 1, 1).unwrap().start_of_day(),
    )
    .unwrap();
    assert!(matches!(
        engine.price(&distant, &extreme),
        Err(Error::ComputationFailure(_))
    ));
    send_sync::<HestonParameters>();
    send_sync::<AnalyticHestonEuropeanEngine>();
}

#[test]
fn small_vol_of_vol_converges_to_bsm_without_cancellation() {
    for xi in [1e-8, 1e-10, 1e-100] {
        let parameters = model()
            .replace(HestonParameterChanges {
                vol_of_vol: Some(xi),
                ..Default::default()
            })
            .unwrap();
        let price = AnalyticHestonEuropeanEngine::default()
            .price(&option(OptionType::Call), &context(parameters))
            .unwrap();
        assert!((price - 9.227_005_508_154_036).abs() < 1e-8);
    }
}

#[test]
fn absorbing_variance_preserves_small_discounted_payoff_differences() {
    let engine = AnalyticHestonEuropeanEngine::new(HestonIntegrationSettings {
        absolute_tolerance: 0.0,
        relative_tolerance: 1e-8,
        max_evaluations: 1,
    })
    .unwrap();
    for (kind, risk, dividend) in [
        (OptionType::Call, 1e-18, 0.0),
        (OptionType::Put, 0.0, 1e-18),
    ] {
        let parameters = HestonParameters::new(kiyosi::HestonParameterInputs {
            risk_free_rate: risk.into(),
            dividend_yield: dividend.into(),
            initial_variance: 0.0,
            long_run_variance: 0.0,
            mean_reversion_speed: 1.0,
            vol_of_vol: 0.3,
            correlation: 0.0,
        })
        .unwrap();
        let context = PricingContext::new(parameters, 1e18, dates().0).unwrap();
        let option = EuropeanOption::new(kind, 1e18, dates().0, dates().1).unwrap();
        assert!((engine.price(&option, &context).unwrap() - 1.0).abs() < 1e-12);
    }
}

#[test]
fn rare_large_variance_does_not_hide_the_fourier_tail() {
    // QuantLib 1.43 Gatheral/exp-sinh at 1e-10 and 1e-12; independently
    // confirmed from the limiting characteristic function exp(-sqrt(u*u+0.25)).
    let expected = 59.021_179_583_659_89;
    let parameters = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.0.into(),
        dividend_yield: 0.0.into(),
        initial_variance: 0.04,
        long_run_variance: 1e40,
        mean_reversion_speed: 1.0,
        vol_of_vol: 1e40,
        correlation: 0.0,
    })
    .unwrap();
    let context = context(parameters);
    for settings in [
        HestonIntegrationSettings::default(),
        HestonIntegrationSettings {
            absolute_tolerance: 1e-12,
            relative_tolerance: 1e-12,
            max_evaluations: 100_000,
        },
    ] {
        let actual = AnalyticHestonEuropeanEngine::new(settings)
            .unwrap()
            .price(&option(OptionType::Call), &context)
            .unwrap();
        assert!(
            (actual - expected).abs()
                <= settings.absolute_tolerance + settings.relative_tolerance * expected,
            "{actual} != {expected}"
        );
    }
}

#[test]
fn subnormal_reversion_preserves_long_run_variance_growth() {
    // With kappa*T this small, V = v0*T + kappa*theta*T*T/2 to binary64
    // accuracy. The ATM price is 100*erf(sqrt(V)/(2*sqrt(2))).
    let parameters = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.0.into(),
        dividend_yield: 0.0.into(),
        initial_variance: 1e-20,
        long_run_variance: 1e308,
        mean_reversion_speed: f64::from_bits(1),
        vol_of_vol: 0.0,
        correlation: 0.0,
    })
    .unwrap();
    let context = context(parameters);
    let expected = 6.270_413_576_726_258e-7;
    for kind in [OptionType::Call, OptionType::Put] {
        let actual = AnalyticHestonEuropeanEngine::default()
            .price(&option(kind), &context)
            .unwrap();
        assert!(
            (actual / expected - 1.0).abs() < 1e-10,
            "{actual} != {expected}"
        );
    }
}

#[test]
fn reversion_growth_avoids_intermediate_product_overflow() {
    let parameters = HestonParameters::new(kiyosi::HestonParameterInputs {
        risk_free_rate: 0.0.into(),
        dividend_yield: 0.0.into(),
        initial_variance: 0.0,
        long_run_variance: 1e308,
        mean_reversion_speed: 1e10,
        vol_of_vol: 0.0,
        correlation: 0.0,
    })
    .unwrap();
    let time = Timestamp::new(Date::new(2025, 12, 31).unwrap(), 23, 59, 59, 999_999).unwrap();
    let context = PricingContext::new(parameters, 100.0, time).unwrap();
    // Over one microsecond, kappa*theta overflows but the integrated variance
    // is finite (about 5e290); both ATM prices approach 100.
    for kind in [OptionType::Call, OptionType::Put] {
        assert_eq!(
            AnalyticHestonEuropeanEngine::default()
                .price(&option(kind), &context)
                .unwrap(),
            100.0
        );
    }
}
