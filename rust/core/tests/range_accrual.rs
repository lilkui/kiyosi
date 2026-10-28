use chrono::{Datelike, NaiveDate};
use kiyosi_core::*;

fn day(day: u32) -> Date {
    Date::new(2025, 1, day).unwrap()
}

fn terms() -> RangeAccrualTerms {
    RangeAccrualTerms {
        notional: 1000.0,
        annual_coupon_rate: 0.12,
        lower_bound: 95.0,
        upper_bound: 110.0,
        effective_date: day(1),
        expiry_date: day(17),
        realized_hit_count: None,
    }
}

fn market(at: impl Into<Timestamp>, spot: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.2).unwrap(),
        spot,
        at.into(),
    )
    .unwrap()
    .replace(PricingContextChanges {
        calendar: Some(TradingCalendar::all_days()),
        ..Default::default()
    })
    .unwrap()
}

fn civil(date: Date) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).unwrap()
}

fn add(date: Date, days: i64) -> Date {
    let next = civil(date) + chrono::Duration::days(days);
    Date::new(next.year(), next.month(), next.day()).unwrap()
}

fn years(before: Timestamp, after: Timestamp) -> f64 {
    let days = (civil(after.date()) - civil(before.date())).num_days() as f64;
    (days
        + (after.micros_since_midnight() as f64 - before.micros_since_midnight() as f64)
            / 86_400_000_000.0)
        / 365.0
}

// Independent payoff reference using ordinary erfc marginals, outside extreme tails.
fn reference(option: &RangeAccrualOption, context: &PricingContext) -> f64 {
    let mut hits = f64::from(option.realized_hit_count().unwrap_or(0));
    let mut count = 0;
    let model = context.model_parameters();
    let valuation = context.valuation_time();
    let mut date = add(option.effective_date(), 1);
    loop {
        if context.calendar().is_trading_day(date) {
            count += 1;
            let time = date.start_of_day();
            if time == valuation {
                hits += f64::from(
                    option.lower_bound() <= context.spot_price()
                        && context.spot_price() <= option.upper_bound(),
                );
            } else if time > valuation {
                let t = years(valuation, time);
                let sigma = model.volatility();
                let mean = context.spot_price().ln()
                    - model.risk_free_curve().log_factor(valuation, time).unwrap()
                    + model.dividend_curve().log_factor(valuation, time).unwrap()
                    - 0.5 * sigma * sigma * t;
                let width = sigma * (2.0 * t).sqrt();
                let cdf = |bound: f64| {
                    if bound == 0.0 {
                        0.0
                    } else {
                        0.5 * libm::erfc((mean - bound.ln()) / width)
                    }
                };
                hits += cdf(option.upper_bound()) - cdf(option.lower_bound());
            }
        }
        if date == option.expiry_date() {
            break;
        }
        date = add(date, 1);
    }
    option.notional()
        * option.annual_coupon_rate()
        * years(option.effective_date().into(), option.expiry_date().into())
        * hits
        / f64::from(count)
        * model
            .risk_free_curve()
            .factor(valuation, option.expiry_date().into())
            .unwrap()
}

struct ReferenceEngine;
impl PricingEngine<RangeAccrualOption> for ReferenceEngine {
    fn price(&self, option: &RangeAccrualOption, context: &PricingContext) -> Result<f64> {
        Ok(reference(option, context))
    }
}

#[test]
fn exact_marginals_match_flat_negative_and_node_curves_and_recover_volatility() {
    let option = RangeAccrualOption::new(terms()).unwrap();
    let curve = RateTermStructure::new(
        day(1).into(),
        vec![day(6).into(), day(17).into()],
        vec![1.001, 0.997],
    )
    .unwrap();
    let dividend = RateTermStructure::new(
        add(day(1), -1).into(),
        vec![day(8).into(), day(17).into()],
        vec![0.998, 1.002],
    )
    .unwrap();
    for (rate, dividend) in [
        (Rate::Flat(0.04), Rate::Flat(0.01)),
        (Rate::Flat(-0.03), Rate::Flat(0.01)),
        (curve.into(), dividend.into()),
    ] {
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, dividend, 0.2).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(
            (AnalyticRangeAccrualEngine.price(&option, &context).unwrap()
                - reference(&option, &context))
            .abs()
                < 1e-10
        );
    }
    let annual = RangeAccrualOption::new(RangeAccrualTerms {
        expiry_date: add(day(1), 365),
        ..terms()
    })
    .unwrap();
    let context = market(day(1), 100.0);
    let quote = reference(&annual, &context);
    assert!((AnalyticRangeAccrualEngine.price(&annual, &context).unwrap() - quote).abs() < 1e-10);
    let sigma = implied_volatility(
        &AnalyticRangeAccrualEngine,
        &annual,
        &context,
        quote,
        ImpliedVolatilitySettings {
            lower_bound: 0.1,
            upper_bound: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-8);
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn numerical_greeks_match_independent_payoff_reference() {
    let curve = RateTermStructure::new(
        add(day(1), -5).into(),
        vec![day(4).into(), day(9).into()],
        vec![1.001, 0.997],
    )
    .unwrap();
    for (days, rate) in [(1, Rate::Flat(0.04)), (8, curve.into())] {
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            expiry_date: add(day(1), days),
            ..terms()
        })
        .unwrap();
        let context = market(add(day(1), -3), 99.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, -0.01, 0.2).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        let shifts = NumericalShiftSettings {
            spot_shift: 0.1,
            ..Default::default()
        };
        let expected = numerical_greeks(
            &ReferenceEngine,
            &option,
            &context,
            GreeksRequest::all(),
            shifts,
        )
        .unwrap();
        let actual = AnalyticRangeAccrualEngine
            .price_with_greeks(&option, &context, GreeksRequest::all(), shifts)
            .unwrap();
        for greek in Greek::ALL {
            assert!(
                (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs() < 2e-8,
                "{greek:?}"
            );
        }
    }
}

#[test]
fn terminal_coupon_counts_current_spot_inclusively() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        expiry_date: day(3),
        realized_hit_count: Some(1),
        ..terms()
    })
    .unwrap();
    for (spot, hits) in [(94.0, 1.0), (95.0, 2.0), (110.0, 2.0), (111.0, 1.0)] {
        let price = AnalyticRangeAccrualEngine
            .price(&option, &market(day(3), spot))
            .unwrap();
        assert!((price - 120.0 / 365.0 * hits).abs() < 1e-12);
    }
}

#[test]
fn greeks_respect_observations_and_allow_pre_effective_time() {
    let option = RangeAccrualOption::new(terms()).unwrap();
    let before = market(Date::new(2024, 12, 29).unwrap(), 99.0);
    let result = AnalyticRangeAccrualEngine
        .price_with_greeks(&option, &before, GreeksRequest::all(), Default::default())
        .unwrap();
    for greek in Greek::ALL {
        assert!(result.has(greek), "{greek:?}");
    }
    let current = market(day(2), 95.0);
    let result = AnalyticRangeAccrualEngine
        .price_with_greeks(&option, &current, GreeksRequest::all(), Default::default())
        .unwrap();
    for greek in Greek::ALL {
        assert_eq!(
            result.has(greek),
            matches!(greek, Greek::Vega | Greek::Rho),
            "{greek:?}"
        );
    }
    let weekend = RangeAccrualOption::new(RangeAccrualTerms {
        effective_date: day(3),
        expiry_date: day(6),
        ..terms()
    })
    .unwrap();
    let context = market(Timestamp::new(day(4), 12, 0, 0, 0).unwrap(), 100.0)
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::weekdays()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        AnalyticRangeAccrualEngine
            .price_with_greeks(
                &weekend,
                &context,
                GreeksRequest::new([Greek::Theta]),
                Default::default()
            )
            .unwrap()
            .has(Greek::Theta)
    );
}

#[test]
fn terms_validate_before_dates_and_replacement_preserves_original() {
    let option = RangeAccrualOption::new(terms()).unwrap();
    assert_eq!(option.realized_hit_count(), None);
    assert_eq!(
        RangeAccrualTerms::new(1000.0, 0.12, 95.0, 110.0, day(1), day(17)),
        terms()
    );
    let changed = option.replace(Some(3)).unwrap();
    assert_eq!(changed.realized_hit_count(), Some(3));
    assert_eq!(changed.replace(None).unwrap(), option);
    assert_eq!(
        option.replace(Some(-1)).unwrap_err().category,
        ErrorCategory::InvalidParameter
    );
    assert_eq!(option, RangeAccrualOption::new(terms()).unwrap());
    for bad in [-1.0, f64::INFINITY, f64::NAN] {
        for field in 0..4 {
            let mut input = terms();
            match field {
                0 => input.notional = bad,
                1 => input.annual_coupon_rate = bad,
                2 => input.lower_bound = bad,
                _ => input.upper_bound = bad,
            }
            input.expiry_date = input.effective_date;
            assert_eq!(
                RangeAccrualOption::new(input).unwrap_err().category,
                ErrorCategory::InvalidParameter
            );
        }
    }
    for input in [
        RangeAccrualTerms {
            notional: 0.0,
            ..terms()
        },
        RangeAccrualTerms {
            lower_bound: 110.0,
            ..terms()
        },
        RangeAccrualTerms {
            upper_bound: 90.0,
            ..terms()
        },
        RangeAccrualTerms {
            realized_hit_count: Some(-1),
            ..terms()
        },
    ] {
        assert_eq!(
            RangeAccrualOption::new(input).unwrap_err().category,
            ErrorCategory::InvalidParameter
        );
    }
    for end in [day(1), add(day(1), -1)] {
        assert_eq!(
            RangeAccrualOption::new(RangeAccrualTerms {
                expiry_date: end,
                ..terms()
            })
            .unwrap_err()
            .category,
            ErrorCategory::InvalidTimeRange
        );
    }
    assert!(
        RangeAccrualOption::new(RangeAccrualTerms {
            annual_coupon_rate: 0.0,
            lower_bound: 0.0,
            ..terms()
        })
        .is_ok()
    );
}

#[test]
fn history_is_strictly_prior_and_valuation_can_precede_effective_date() {
    let option = RangeAccrualOption::new(terms()).unwrap();
    for (at, hits) in [
        (add(day(1), -3).into(), None),
        (day(1).into(), None),
        (Timestamp::new(day(1), 12, 0, 0, 0).unwrap(), None),
        (day(2).into(), None),
        (Timestamp::new(day(2), 12, 0, 0, 0).unwrap(), Some(1)),
        (day(9).into(), Some(5)),
    ] {
        let live = option.replace(hits).unwrap();
        let context = market(at, 100.0);
        assert!(
            (AnalyticRangeAccrualEngine.price(&live, &context).unwrap()
                - reference(&live, &context))
            .abs()
                < 1e-10
        );
    }
    for (at, hits) in [
        (day(1).into(), Some(1)),
        (day(2).into(), Some(1)),
        (Timestamp::new(day(2), 12, 0, 0, 0).unwrap(), Some(2)),
        (day(3).into(), None),
    ] {
        let live = option.replace(hits).unwrap();
        assert_eq!(
            AnalyticRangeAccrualEngine
                .price(&live, &market(at, 100.0))
                .unwrap_err()
                .category,
            ErrorCategory::InvalidParameter
        );
    }
    let after = market(Timestamp::new(day(17), 0, 0, 0, 1).unwrap(), 100.0);
    assert_eq!(
        AnalyticRangeAccrualEngine
            .price(&option, &after)
            .unwrap_err()
            .category,
        ErrorCategory::InvalidTimeRange
    );
}

#[test]
fn calendar_and_history_validation_precede_zero_coupon_and_curve_coverage() {
    let option = RangeAccrualOption::new(terms()).unwrap();
    let curve = RateTermStructure::new(day(1).into(), vec![day(2).into()], vec![0.999]).unwrap();
    for (risk, dividend) in [
        (Rate::Curve(curve.clone()), Rate::Flat(0.01)),
        (Rate::Flat(0.04), Rate::Curve(curve)),
    ] {
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(risk, dividend, 0.2).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            AnalyticRangeAccrualEngine
                .price(&option, &context)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidTimeRange
        );
        let zero = RangeAccrualOption::new(RangeAccrualTerms {
            annual_coupon_rate: 0.0,
            ..terms()
        })
        .unwrap();
        assert_eq!(
            AnalyticRangeAccrualEngine.price(&zero, &context).unwrap(),
            0.0
        );
        assert_eq!(
            AnalyticRangeAccrualEngine
                .price(&zero, &market(day(4), 100.0))
                .unwrap_err()
                .category,
            ErrorCategory::InvalidParameter
        );
    }
    let context = market(day(3), 100.0)
        .replace(PricingContextChanges {
            calendar: Some(TradingCalendar::weekdays()),
            ..Default::default()
        })
        .unwrap();
    let weekend = RangeAccrualOption::new(RangeAccrualTerms {
        annual_coupon_rate: 0.0,
        expiry_date: day(5),
        ..terms()
    })
    .unwrap();
    assert_eq!(
        AnalyticRangeAccrualEngine
            .price(&weekend, &context)
            .unwrap_err()
            .category,
        ErrorCategory::InvalidDate
    );
    let monday = RangeAccrualOption::new(RangeAccrualTerms {
        effective_date: day(3),
        expiry_date: day(6),
        ..terms()
    })
    .unwrap();
    assert!(
        (AnalyticRangeAccrualEngine.price(&monday, &context).unwrap()
            - reference(&monday, &context))
        .abs()
            < 1e-12
    );
}

#[test]
fn tiny_volatility_and_coupon_scaling_preserve_representable_prices() {
    let below = f64::from_bits(100.0_f64.to_bits() - 1);
    let above = f64::from_bits(100.0_f64.to_bits() + 1);
    for (lower, upper, vol, expected, days) in [
        (100.0, above, 1e-16, 0.5, 1),
        (below, 100.0, 1e-16, 0.5, 1),
        (below, above, 1e-16, 1.0, 1),
        (100.0, above, 1e-16, 0.5, 2),
        (100.0, above, f64::from_bits(1), 0.5, 1),
        (below, 100.0, f64::from_bits(1), 0.5, 1),
        (below, above, f64::from_bits(1), 1.0, 1),
        (below, 110.0, 1e-14, 0.6069959804126283, 1),
        (90.0, above, 1e-14, 0.6069959804126285, 1),
    ] {
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            notional: 365.0 / f64::from(days),
            annual_coupon_rate: 1.0,
            lower_bound: lower,
            upper_bound: upper,
            expiry_date: add(day(1), days.into()),
            ..terms()
        })
        .unwrap();
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(BlackScholesMertonParameters::new(0.0, 0.0, vol).unwrap()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            (AnalyticRangeAccrualEngine.price(&option, &context).unwrap() - expected).abs() < 2e-14
        );
    }
    let mean = 100.0_f64.ln() + 0.01 / 365.0;
    let width = 0.2 / 365.0_f64.sqrt();
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        notional: 1e300,
        annual_coupon_rate: 1e100,
        lower_bound: (mean + 40.0 * width).exp(),
        upper_bound: (mean + 41.0 * width).exp(),
        expiry_date: day(2),
        ..terms()
    })
    .unwrap();
    // Independent normal-tail constant: log P(Z > 40); the >41 tail is negligible here.
    let expected =
        (1e300_f64.ln() + 1e100_f64.ln() - 365.0_f64.ln() - 0.04 / 365.0 - 804.6084420137538).exp();
    assert!(
        (AnalyticRangeAccrualEngine
            .price(&option, &market(day(1), 100.0))
            .unwrap()
            / expected
            - 1.0)
            .abs()
            < 2e-11
    );
    let overflow = RangeAccrualOption::new(RangeAccrualTerms {
        notional: 1e300,
        annual_coupon_rate: 1e100,
        ..terms()
    })
    .unwrap();
    assert_eq!(
        AnalyticRangeAccrualEngine
            .price(&overflow, &market(day(1), 100.0))
            .unwrap_err()
            .category,
        ErrorCategory::InvalidResult
    );
}

#[test]
fn request_and_solver_errors_preserve_inputs() {
    let engine = AnalyticRangeAccrualEngine;
    let option = RangeAccrualOption::new(terms()).unwrap();
    let context = market(day(1), 100.0);
    let shifts = NumericalShiftSettings::default();
    assert_eq!(
        engine
            .price_with_greeks(&option, &context, GreeksRequest::default(), shifts)
            .unwrap_err()
            .category,
        ErrorCategory::InvalidParameter
    );
    assert_eq!(
        engine
            .price_with_greeks(
                &option,
                &context,
                GreeksRequest::all(),
                NumericalShiftSettings {
                    volatility_shift: 0.0,
                    ..shifts
                }
            )
            .unwrap_err()
            .category,
        ErrorCategory::InvalidParameter
    );
    let unavailable = engine
        .price_with_greeks(
            &option,
            &context,
            GreeksRequest::new([Greek::Delta]),
            NumericalShiftSettings {
                spot_shift: 200.0,
                ..shifts
            },
        )
        .unwrap();
    assert!(!unavailable.has(Greek::Delta));
    let settings = ImpliedVolatilitySettings {
        lower_bound: 0.1,
        upper_bound: 0.4,
        ..Default::default()
    };
    assert_eq!(
        implied_volatility(&engine, &option, &context, -1.0, settings)
            .unwrap_err()
            .category,
        ErrorCategory::UnbracketedVolatility
    );
    let zero = RangeAccrualOption::new(RangeAccrualTerms {
        annual_coupon_rate: 0.0,
        ..terms()
    })
    .unwrap();
    assert_eq!(
        implied_volatility(&engine, &zero, &context, 0.0, settings)
            .unwrap_err()
            .category,
        ErrorCategory::UnsupportedOperation
    );
    let end = option.replace(Some(15)).unwrap();
    let terminal = market(day(17), 100.0);
    assert_eq!(
        implied_volatility(&engine, &end, &terminal, 0.0, settings)
            .unwrap_err()
            .category,
        ErrorCategory::UnsupportedOperation
    );
    let result = engine
        .price_with_greeks(&end, &terminal, GreeksRequest::all(), shifts)
        .unwrap();
    assert!(Greek::ALL.into_iter().all(|g| !result.has(g)));
    assert_eq!(option, RangeAccrualOption::new(terms()).unwrap());
    assert_eq!(context.model_parameters().volatility(), 0.2);
}

#[test]
fn full_term_coupon_uses_calendar_days_and_one_expiry_discount() {
    let option = RangeAccrualOption::new(terms()).unwrap();
    let model = BlackScholesMertonParameters::new(0.8, 0.8, 1e-9).unwrap();
    let context = market(day(1), 100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })
        .unwrap();
    let engine = AnalyticRangeAccrualEngine;
    let expected = 120.0 * 16.0 / 365.0 * (-0.8_f64 * 16.0 / 365.0).exp();
    assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-12);
    let seasoned = option.replace(Some(3)).unwrap();
    let live = context
        .replace(PricingContextChanges {
            valuation_time: Some(day(8).into()),
            ..Default::default()
        })
        .unwrap();
    let expected = 120.0 * 13.0 / 365.0 * (-0.8_f64 * 9.0 / 365.0).exp();
    assert!((engine.price(&seasoned, &live).unwrap() - expected).abs() < 1e-12);
    let rebased = context
        .replace(PricingContextChanges {
            calendar: Some(
                TradingCalendar::custom(TradingCalendar::all_days(), vec![], vec![], Some(252))
                    .unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.price(&option, &context).unwrap(),
        engine.price(&option, &rebased).unwrap()
    );
    let overrides = context
        .replace(PricingContextChanges {
            calendar: Some(
                TradingCalendar::custom(
                    TradingCalendar::weekdays(),
                    vec![day(2)],
                    vec![day(4)],
                    None,
                )
                .unwrap(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(
        (engine.price(&option, &overrides).unwrap() - reference(&option, &overrides)).abs() < 1e-12
    );
}

#[test]
fn long_and_zero_lower_bound_contracts_support_shared_concurrent_reads() {
    let engine = AnalyticRangeAccrualEngine;
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        expiry_date: add(day(1), 21204),
        ..terms()
    })
    .unwrap();
    let context = market(day(1), 100.0);
    let expected = reference(&option, &context);
    std::thread::scope(|scope| {
        let parallel = scope.spawn(|| engine.price(&option, &context).unwrap());
        assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-8);
        assert!((parallel.join().unwrap() - expected).abs() < 1e-8);
    });
    let seasoned = option.replace(Some(20000)).unwrap();
    let live = market(add(day(1), 20001), 100.0);
    assert!((engine.price(&seasoned, &live).unwrap() - reference(&seasoned, &live)).abs() < 1e-8);
    let lower_zero = RangeAccrualOption::new(RangeAccrualTerms {
        lower_bound: 0.0,
        ..terms()
    })
    .unwrap();
    assert!(
        (engine.price(&lower_zero, &context).unwrap() - reference(&lower_zero, &context)).abs()
            < 1e-10
    );
}

#[test]
fn unresolvable_curve_knots_fail_before_pricing_future_observations() {
    let expiry = Date::new(2500, 1, 2).unwrap();
    let knot = Date::new(2500, 1, 1).unwrap();
    let curve = RateTermStructure::new(
        day(1).into(),
        vec![
            knot.into(),
            Timestamp::new(knot, 0, 0, 0, 1).unwrap(),
            expiry.into(),
        ],
        vec![1.0; 3],
    )
    .unwrap();
    for (risk, dividend) in [
        (Rate::Curve(curve.clone()), Rate::Flat(0.0)),
        (Rate::Flat(0.0), Rate::Curve(curve)),
    ] {
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(risk, dividend, 0.2).unwrap(),
                ),
                ..Default::default()
            })
            .unwrap();
        let mut input = RangeAccrualTerms {
            effective_date: Date::new(2499, 12, 31).unwrap(),
            expiry_date: expiry,
            ..terms()
        };
        let option = RangeAccrualOption::new(input).unwrap();
        assert_eq!(
            AnalyticRangeAccrualEngine
                .price(&option, &context)
                .unwrap_err()
                .category,
            ErrorCategory::InvalidResult
        );
        input.annual_coupon_rate = 0.0;
        assert_eq!(
            AnalyticRangeAccrualEngine
                .price(&RangeAccrualOption::new(input).unwrap(), &context)
                .unwrap(),
            0.0
        );
    }
}
