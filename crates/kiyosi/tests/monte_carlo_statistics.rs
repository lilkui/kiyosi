use kiyosi::*;

fn dates() -> (Timestamp, Timestamp) {
    (
        Date::new(2025, 1, 2).unwrap().start_of_day(),
        Date::new(2026, 1, 2).unwrap().start_of_day(),
    )
}

fn market() -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
        100.0,
        dates().0,
    )
    .unwrap()
}

fn settings() -> MonteCarloSettings {
    MonteCarloSettings {
        path_count: 4096,
        seed: Some(42),
        max_active_workers: 1,
        ..Default::default()
    }
}

fn sample<I: OptionContract>(
    engine: &impl PricingEngine<I>,
    option: &I,
) -> (f64, MonteCarloEstimate) {
    let context = market();
    (
        engine.price(option, &context).unwrap(),
        engine.price_with_statistics(option, &context).unwrap(),
    )
}

fn samples(settings: MonteCarloSettings) -> Vec<(&'static str, f64, MonteCarloEstimate)> {
    let (start, end) = dates();
    let digital = MonteCarloDigitalEngine::new(settings).unwrap();
    let asian = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: settings,
        step_count: 8,
    })
    .unwrap();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let asset = DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))
    .unwrap();
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))
    .unwrap();
    [
        ("cash", sample(&digital, &cash)),
        ("asset", sample(&digital, &asset)),
        ("geometric", sample(&asian, &geometric)),
        ("arithmetic", sample(&asian, &arithmetic)),
        (
            "barrier",
            sample(&MonteCarloBarrierEngine::new(settings).unwrap(), &barrier),
        ),
        (
            "double",
            sample(
                &MonteCarloDoubleBarrierEngine::new(settings).unwrap(),
                &double,
            ),
        ),
    ]
    .into_iter()
    .map(|(name, (price, estimate))| (name, price, estimate))
    .collect()
}

#[test]
fn statistics_preserve_prices_and_report_uncertainty_across_products_and_threads() {
    for antithetic in [false, true] {
        let settings = MonteCarloSettings {
            antithetic,
            ..settings()
        };
        let serial = samples(settings);
        let parallel = samples(MonteCarloSettings {
            max_active_workers: 4,
            ..settings
        });
        assert_eq!(serial, parallel);
        for (name, price, estimate) in serial {
            assert_eq!(estimate.price(), price, "{name}");
            let error = estimate.standard_error().unwrap();
            assert!(error > 0.0, "{name}: {estimate:?}");
            let (lower, upper) = estimate.confidence_interval().unwrap();
            assert!(lower < price && price < upper, "{name}");
        }
    }
}

#[test]
fn digital_uncertainty_matches_independent_bernoulli_and_pair_formulas() {
    let (start, end) = dates();
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let amount = 10.0 * (-0.05_f64).exp();
    for antithetic in [false, true] {
        let settings = MonteCarloSettings {
            antithetic,
            ..settings()
        };
        let estimate = MonteCarloDigitalEngine::new(settings)
            .unwrap()
            .price_with_statistics(&option, &market())
            .unwrap();
        let (probability, scale, observations) = if antithetic {
            (
                2.0 * estimate.price() / amount - 1.0,
                amount / 2.0,
                f64::from(settings.path_count / 2),
            )
        } else {
            (
                estimate.price() / amount,
                amount,
                f64::from(settings.path_count),
            )
        };
        let error = scale * (probability * (1.0 - probability) / (observations - 1.0)).sqrt();
        assert!((estimate.standard_error().unwrap() - error).abs() < 1e-13);
        let (lower, upper) = estimate.confidence_interval().unwrap();
        assert!((lower - (estimate.price() - 1.959_963_984_540_054 * error)).abs() < 1e-12);
        assert!((upper - (estimate.price() + 1.959_963_984_540_054 * error)).abs() < 1e-12);
    }
}

#[test]
fn reported_uncertainty_agrees_with_analytic_prices() {
    let estimates = samples(settings());
    let (start, end) = dates();
    let context = market();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let asset = DigitalOption::asset_or_nothing(OptionType::Call, 100.0, start, end).unwrap();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let barrier = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))
    .unwrap();
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))
    .unwrap();
    for (index, reference) in [
        (0, AnalyticDigitalEngine.price(&cash, &context).unwrap()),
        (1, AnalyticDigitalEngine.price(&asset, &context).unwrap()),
        (
            2,
            AnalyticGeometricAveragePriceEngine
                .price(&geometric, &context)
                .unwrap(),
        ),
        (4, AnalyticBarrierEngine.price(&barrier, &context).unwrap()),
        (
            5,
            AnalyticDoubleBarrierEngine
                .price(&double, &context)
                .unwrap(),
        ),
    ] {
        let (name, _, estimate) = estimates[index];
        assert!(
            (estimate.price() - reference).abs() < 6.0 * estimate.standard_error().unwrap(),
            "{name}: {estimate:?} vs {reference}"
        );
    }
}

#[test]
fn uncertainty_omission_follows_sampling_and_independent_counts() {
    for settings in [
        MonteCarloSettings {
            path_count: 1,
            ..settings()
        },
        MonteCarloSettings {
            path_count: 2,
            antithetic: true,
            ..settings()
        },
        MonteCarloSettings {
            path_count: 3,
            antithetic: true,
            ..settings()
        },
        MonteCarloSettings {
            path_count: 16,
            sampling: MonteCarloSampling::Sobol,
            ..settings()
        },
    ] {
        for (name, price, estimate) in samples(settings) {
            assert_eq!(estimate.price(), price, "{name}");
            assert_eq!(estimate.standard_error(), None, "{name}");
            assert_eq!(estimate.confidence_interval(), None, "{name}");
        }
    }
}

#[test]
fn terminal_statistics_report_known_values_for_each_new_engine() {
    let (start, end) = dates();
    let context = market()
        .replace(PricingContextChanges {
            spot_price: Some(110.0),
            valuation_time: Some(end),
            ..Default::default()
        })
        .unwrap();
    let cash = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, Some(110.0))
            .unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, Some(110.0))
            .unwrap();
    let mut barrier = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    barrier.touch_state = Some(BarrierTouchState::Untouched);
    let barrier = BarrierOption::new(barrier).unwrap();
    let mut double = DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    );
    double.touch_state = Some(BarrierTouchState::Untouched);
    let double = DoubleBarrierOption::new(double).unwrap();
    for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
        let settings = MonteCarloSettings {
            path_count: 1,
            sampling,
            ..settings()
        };
        let asian = MonteCarloAsianEngine::new(settings.into()).unwrap();
        let estimates = [
            MonteCarloDigitalEngine::new(settings)
                .unwrap()
                .price_with_statistics(&cash, &context)
                .unwrap(),
            asian.price_with_statistics(&geometric, &context).unwrap(),
            asian.price_with_statistics(&arithmetic, &context).unwrap(),
            MonteCarloBarrierEngine::new(settings)
                .unwrap()
                .price_with_statistics(&barrier, &context)
                .unwrap(),
            MonteCarloDoubleBarrierEngine::new(settings)
                .unwrap()
                .price_with_statistics(&double, &context)
                .unwrap(),
        ];
        for estimate in estimates {
            assert_eq!(estimate.price(), 10.0);
            let expected = (sampling == MonteCarloSampling::PseudoRandom).then_some(0.0);
            assert_eq!(estimate.standard_error(), expected);
            assert_eq!(
                estimate.confidence_interval(),
                expected.map(|_| (10.0, 10.0))
            );
        }
    }
}

#[test]
fn analytic_at_hit_rebates_shift_prices_without_adding_sampling_variance() {
    let (start, end) = dates();
    let context = market();
    let mut single = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    single.rebate_timing = RebateTiming::AtHit;
    let zero = BarrierOption::new(single.clone()).unwrap();
    single.rebate = 7.0;
    let rebate = BarrierOption::new(single).unwrap();
    let engine = MonteCarloBarrierEngine::new(settings()).unwrap();
    let base = engine.price_with_statistics(&zero, &context).unwrap();
    let actual = engine.price_with_statistics(&rebate, &context).unwrap();
    let shift = AnalyticBarrierEngine.price(&rebate, &context).unwrap()
        - AnalyticBarrierEngine.price(&zero, &context).unwrap();
    assert_eq!(actual.standard_error(), base.standard_error());
    assert!((actual.price() - base.price() - shift).abs() < 1e-12);
    let mut double = DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    );
    double.rebate_timing = RebateTiming::AtHit;
    let zero = DoubleBarrierOption::new(double.clone()).unwrap();
    double.rebate = 7.0;
    let rebate = DoubleBarrierOption::new(double).unwrap();
    let engine = MonteCarloDoubleBarrierEngine::new(settings()).unwrap();
    let base = engine.price_with_statistics(&zero, &context).unwrap();
    let actual = engine.price_with_statistics(&rebate, &context).unwrap();
    let shift = AnalyticDoubleBarrierEngine
        .price(&rebate, &context)
        .unwrap()
        - AnalyticDoubleBarrierEngine.price(&zero, &context).unwrap();
    assert_eq!(actual.standard_error(), base.standard_error());
    assert!((actual.price() - base.price() - shift).abs() < 1e-12);
}

#[test]
fn generic_statistics_reject_engines_without_sampling_estimators() {
    let (start, end) = dates();
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    assert!(matches!(
        AnalyticEuropeanEngine.price_with_statistics(&option, &market()),
        Err(Error::UnsupportedOperation(_))
    ));
    let american = AmericanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    assert!(matches!(
        PricingEngine::price_with_statistics(
            &MonteCarloVanillaEngine::new(settings().into()).unwrap(),
            &american,
            &market()
        ),
        Err(Error::UnsupportedOperation(_))
    ));
}

#[test]
fn statistics_preserve_scheduled_barrier_prices() {
    let (start, end) = dates();
    let mut single = BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    );
    single.observation_times = Some(vec![start.add_days(182).unwrap(), end]);
    let mut double = DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    );

    double
        .observation_times
        .clone_from(&single.observation_times);
    for (price, estimate) in [
        sample(
            &MonteCarloBarrierEngine::new(settings()).unwrap(),
            &BarrierOption::new(single).unwrap(),
        ),
        sample(
            &MonteCarloDoubleBarrierEngine::new(settings()).unwrap(),
            &DoubleBarrierOption::new(double).unwrap(),
        ),
    ] {
        assert_eq!(price, estimate.price());
        assert!(estimate.standard_error().unwrap() > 0.0);
    }
}

#[test]
fn arithmetic_statistics_support_curves_and_known_history() {
    let (start, end) = dates();
    let option =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    let curve = RateTermStructure::new(start, vec![end], vec![0.95]).unwrap();
    let context = market()
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(curve, 0.02, 0.2)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let engine = MonteCarloAsianEngine::new(MonteCarloGridSettings {
        sampling: settings(),
        step_count: 8,
    })
    .unwrap();
    let estimate = engine.price_with_statistics(&option, &context).unwrap();
    assert_eq!(estimate.price(), engine.price(&option, &context).unwrap());
    assert!(estimate.standard_error().unwrap() > 0.0);
    let valuation = Timestamp::new(start.date().add_days(182).unwrap(), 12, 0, 0, 0).unwrap();
    let context = market()
        .replace(PricingContextChanges {
            valuation_time: Some(valuation),
            ..Default::default()
        })
        .unwrap();
    for direction in [OptionType::Call, OptionType::Put] {
        let option = option
            .replace(AveragePriceOptionChanges {
                option_type: Some(direction),
                realized_average: Some(Some(1000.0)),
                ..Default::default()
            })
            .unwrap();
        let estimate = engine.price_with_statistics(&option, &context).unwrap();
        let expected = if direction == OptionType::Call {
            (400.0 + 50.0 * 0.015_f64.exp_m1() / 0.015) * (-0.025_f64).exp()
        } else {
            0.0
        };
        assert!((estimate.price() - expected).abs() < 1e-12);
        assert_eq!(estimate.standard_error(), Some(0.0));
    }
}

#[test]
fn resolved_barriers_report_zero_error_before_expiry() {
    let (start, end) = dates();
    let context = market()
        .replace(PricingContextChanges {
            spot_price: Some(120.0),
            ..Default::default()
        })
        .unwrap();
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let single = MonteCarloBarrierEngine::new(settings()).unwrap();
    let double = MonteCarloDoubleBarrierEngine::new(settings()).unwrap();
    for knock_in in [false, true] {
        let mut terms = BarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            120.0,
            if knock_in {
                BarrierType::UpAndIn
            } else {
                BarrierType::UpAndOut
            },
        );
        terms.rebate = 7.0;
        let mut double_terms = DoubleBarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            80.0,
            120.0,
            if knock_in {
                DoubleBarrierType::KnockIn
            } else {
                DoubleBarrierType::KnockOut
            },
        );
        double_terms.rebate = 7.0;
        let expected = if knock_in {
            AnalyticEuropeanEngine.price(&vanilla, &context).unwrap()
        } else {
            7.0 * (-0.05_f64).exp()
        };
        for estimate in [
            single
                .price_with_statistics(&BarrierOption::new(terms).unwrap(), &context)
                .unwrap(),
            double
                .price_with_statistics(&DoubleBarrierOption::new(double_terms).unwrap(), &context)
                .unwrap(),
        ] {
            assert!((estimate.price() - expected).abs() < 1e-12);
            assert_eq!(estimate.standard_error(), Some(0.0));
            assert_eq!(
                estimate.confidence_interval(),
                Some((estimate.price(), estimate.price()))
            );
        }
    }
}

#[test]
fn statistics_preserve_life_history_and_model_rejections() {
    let (start, end) = dates();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end).unwrap();
    let geometric =
        GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None).unwrap();
    let arithmetic =
        ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)
            .unwrap();
    let single = BarrierOption::new(BarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        120.0,
        BarrierType::UpAndOut,
    ))
    .unwrap();
    let double = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
        OptionType::Call,
        100.0,
        start,
        end,
        80.0,
        120.0,
        DoubleBarrierType::KnockOut,
    ))
    .unwrap();
    let digital_engine = MonteCarloDigitalEngine::new(settings()).unwrap();
    let asian_engine = MonteCarloAsianEngine::new(settings().into()).unwrap();
    let single_engine = MonteCarloBarrierEngine::new(settings()).unwrap();
    let double_engine = MonteCarloDoubleBarrierEngine::new(settings()).unwrap();
    let after = market()
        .replace(PricingContextChanges {
            valuation_time: Some(end.add_days(1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    let heston = market()
        .replace(PricingContextChanges {
            model_parameters: Some(
                HestonParameters::new(kiyosi::HestonParameterInputs {
                    risk_free_rate: 0.05.into(),
                    dividend_yield: 0.02.into(),
                    initial_variance: 0.04,
                    long_run_variance: 0.04,
                    mean_reversion_speed: 1.0,
                    vol_of_vol: 0.2,
                    correlation: -0.5,
                })
                .unwrap()
                .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    for context in [&after, &heston] {
        for result in [
            digital_engine.price_with_statistics(&digital, context),
            asian_engine.price_with_statistics(&geometric, context),
            asian_engine.price_with_statistics(&arithmetic, context),
            single_engine.price_with_statistics(&single, context),
            double_engine.price_with_statistics(&double, context),
        ] {
            if context == &after {
                assert!(matches!(result, Err(Error::InvalidInput(_))));
            } else {
                assert!(matches!(result, Err(Error::UnsupportedOperation(_))));
            }
        }
    }
    let seasoned = market()
        .replace(PricingContextChanges {
            valuation_time: Some(start.add_days(1).unwrap()),
            ..Default::default()
        })
        .unwrap();
    for result in [
        asian_engine.price_with_statistics(&geometric, &seasoned),
        asian_engine.price_with_statistics(&arithmetic, &seasoned),
        single_engine.price_with_statistics(&single, &seasoned),
        double_engine.price_with_statistics(&double, &seasoned),
    ] {
        assert!(matches!(result, Err(Error::MissingHistory(_))));
    }
}

#[test]
fn uncertainty_failure_does_not_prevent_scalar_pricing() {
    let (start, end) = dates();
    let option =
        DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 1.6e308, start, end).unwrap();
    let engine = MonteCarloDigitalEngine::new(MonteCarloSettings {
        path_count: 2,
        seed: Some(0),
        ..settings()
    })
    .unwrap();
    let price = engine.price(&option, &market()).unwrap();
    assert!((price / (8e307 * (-0.05_f64).exp()) - 1.0).abs() < 1e-13);
    assert!(matches!(
        engine.price_with_statistics(&option, &market()),
        Err(Error::ComputationFailure(_))
    ));
    assert_eq!(engine.price(&option, &market()).unwrap(), price);
}

#[test]
fn generic_statistics_keep_existing_monte_carlo_estimators_available() {
    let (start, end) = dates();
    let settings = MonteCarloSettings {
        path_count: 16,
        ..settings()
    };
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
    let accumulator = Accumulator::new(AccumulatorTerms::new(
        100.0,
        110.0,
        1.0,
        2.0,
        start,
        end,
        (end).time_of_day(),
    ))
    .unwrap();
    let note = BinarySnowballOption::new(BinarySnowballTerms::new(
        0.1,
        start,
        end,
        autocallable_observations(&[end], &[110.0], Some(&[0.1]), None).unwrap(),
        TradingCalendar::weekdays(),
    ))
    .unwrap();
    for (price, estimate) in [
        sample(
            &MonteCarloVanillaEngine::new(settings.into()).unwrap(),
            &vanilla,
        ),
        sample(
            &MonteCarloAccumulatorEngine::new(settings).unwrap(),
            &accumulator,
        ),
        sample(&MonteCarloAutocallableEngine::new(settings).unwrap(), &note),
    ] {
        assert_eq!(estimate.price(), price);
        assert!(estimate.standard_error().is_some());
    }
}

#[test]
fn reported_seeds_replay_unseeded_and_configured_estimates() -> Result<()> {
    fn check<I: OptionContract, E: PricingEngine<I>>(
        build: impl Fn(MonteCarloSettings) -> Result<E>,
        option: &I,
    ) -> Result<()> {
        let context = market();
        let deterministic = context.replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.05, 0.02, 0.0)?.into()),
            ..Default::default()
        })?;
        for sampling in [MonteCarloSampling::PseudoRandom, MonteCarloSampling::Sobol] {
            for seed in [None, Some(u64::MAX)] {
                let settings = MonteCarloSettings {
                    path_count: 256,
                    sampling,
                    seed,
                    max_active_workers: 1,
                    antithetic: true,
                };
                let original = build(settings)?;
                let estimate = original.price_with_statistics(option, &context)?;
                let actual = estimate
                    .seed()
                    .expect("sampled estimate must identify its draws");
                if let Some(seed) = seed {
                    assert_eq!(actual, seed);
                }
                let replay = build(MonteCarloSettings {
                    seed: Some(actual),
                    max_active_workers: 4,
                    ..settings
                })?;
                assert_eq!(replay.price_with_statistics(option, &context)?, estimate);
                assert_eq!(replay.price(option, &context)?, estimate.price());
                assert_eq!(
                    original
                        .price_with_statistics(option, &deterministic)?
                        .seed(),
                    None
                );
            }
        }
        Ok(())
    }
    let (start, end) = dates();
    let vanilla = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    check(
        |sampling| MonteCarloVanillaEngine::new(sampling.into()),
        &vanilla,
    )?;
    check(
        MonteCarloDigitalEngine::new,
        &DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?,
    )?;
    check(
        |sampling| MonteCarloAsianEngine::new(sampling.into()),
        &GeometricAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)?,
    )?;
    check(
        |sampling| MonteCarloAsianEngine::new(sampling.into()),
        &ArithmeticAveragePriceOption::new(OptionType::Call, 100.0, start, start, end, None)?,
    )?;
    check(
        MonteCarloBarrierEngine::new,
        &BarrierOption::new(BarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            120.0,
            BarrierType::UpAndOut,
        ))?,
    )?;
    check(
        MonteCarloDoubleBarrierEngine::new,
        &DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            80.0,
            120.0,
            DoubleBarrierType::KnockOut,
        ))?,
    )?;
    let clock = TimeOfDay::new(0, 0, 0, 0)?;
    check(
        MonteCarloAutocallableEngine::new,
        &SnowballPresetTerms::new(0.1, 100.0, 80.0, 110.0, start, end, vec![end], Some(clock))
            .expand(SnowballPreset::Standard)?,
    )?;
    check(
        MonteCarloAccumulatorEngine::new,
        &Accumulator::new(AccumulatorTerms::new(
            100.0, 120.0, 1.0, 2.0, start, end, clock,
        ))?,
    )?;
    Ok(())
}

#[test]
fn greek_results_retain_seeds_for_native_and_scalar_replay() -> Result<()> {
    fn check<I: OptionContract, E: PricingEngine<I>>(
        option: &I,
        factory: impl Fn(Option<u64>) -> Result<E>,
    ) -> Result<()> {
        let context = market();
        let request = GreeksRequest::new([Greek::Delta]);
        let calculate = |engine: &E, numerical| {
            if numerical {
                numerical_greeks(engine, option, &context, request, Default::default())
            } else {
                engine.price_with_greeks_with_settings(
                    option,
                    &context,
                    request,
                    engine.greek_settings(option),
                )
            }
        };
        for numerical in [false, true] {
            let result = calculate(&factory(None)?, numerical)?;
            assert!(result.seed().is_some());
            let replay = factory(result.seed())?;
            assert_eq!(calculate(&replay, numerical)?, result);
            assert_eq!(replay.price(option, &context)?, result.price());
        }
        let deterministic = context.replace(PricingContextChanges {
            model_parameters: Some(BlackScholesMertonParameters::new(0.05, 0.02, 0.0)?.into()),
            ..Default::default()
        })?;
        let engine = factory(None)?;
        assert_eq!(
            engine
                .price_with_greeks_with_settings(
                    option,
                    &deterministic,
                    request,
                    engine.greek_settings(option)
                )?
                .seed(),
            None
        );
        Ok(())
    }
    let (start, end) = dates();
    let settings = |seed| MonteCarloSettings {
        path_count: 32,
        seed,
        ..settings()
    };
    check(
        &EuropeanOption::new(OptionType::Call, 100.0, start, end)?,
        |seed| MonteCarloVanillaEngine::new(settings(seed).into()),
    )?;
    check(
        &AmericanOption::new(OptionType::Put, 100.0, start, end)?,
        |seed| {
            MonteCarloVanillaEngine::new(MonteCarloGridSettings {
                sampling: settings(seed),
                step_count: 4,
            })
        },
    )?;
    check(
        &DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0, start, end)?,
        |seed| MonteCarloDigitalEngine::new(settings(seed)),
    )?;
    check(
        &BarrierOption::new(BarrierOptionTerms::new(
            OptionType::Call,
            100.0,
            start,
            end,
            120.0,
            BarrierType::UpAndOut,
        ))?,
        |seed| MonteCarloBarrierEngine::new(settings(seed)),
    )?;
    Ok(())
}
