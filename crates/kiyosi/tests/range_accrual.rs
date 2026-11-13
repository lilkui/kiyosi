use chrono::{Datelike, NaiveDate};
use kiyosi::*;

fn day(day: u32) -> Timestamp {
    Date::new(2025, 1, day).unwrap().start_of_day()
}

fn terms() -> RangeAccrualTerms {
    RangeAccrualTerms {
        observation_clock_time: (day(17)).time_of_day(),
        calendar: TradingCalendar::default(),
        notional: 1000.0,
        annual_coupon_rate: 0.12,
        lower_bound: 95.0,
        upper_bound: 110.0,
        effective_time: day(1),
        expiry_time: day(17),
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
}

#[test]
fn observation_schedules_retain_valid_storage_across_contract_replacements() -> Result<()> {
    let option = RangeAccrualOption::new(terms())?;
    let schedule = option.observation_schedule();
    let closed = schedule.times()[0].date();
    let calendar = TradingCalendar::custom(option.calendar().clone(), vec![closed], vec![], None)?;
    let updated = option.replace(RangeAccrualOptionChanges {
        calendar: Some(calendar.clone()),
        ..Default::default()
    })?;
    assert_eq!(
        updated.observation_schedule().times(),
        &schedule.times()[1..]
    );
    assert_eq!(updated.observation_schedule().calendar(), &calendar);
    assert!(schedule.calendar().is_trading_day(closed));
    assert_eq!(schedule.times()[0].date(), closed);
    let snapshot = schedule.clone();
    drop(option);
    drop(updated);
    assert_eq!(snapshot.times()[0].date(), closed);
    Ok(())
}

#[test]
fn range_pricing_observes_cancellation_inside_an_active_request() -> Result<()> {
    let option = RangeAccrualOption::new(terms())?;
    let context = market(day(1), 100.0);
    let token = CancellationToken::default();
    let result = token.run(|| {
        token.cancel();
        assert!(matches!(
            AnalyticRangeAccrualEngine.price(&option, &context),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted
        ));
        Ok(())
    });
    assert!(
        matches!(result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
    );
    assert!(AnalyticRangeAccrualEngine.price(&option, &context)? > 0.0);
    Ok(())
}

#[test]
fn automatic_time_bumps_recover_daily_risk_without_crossing_observations() -> Result<()> {
    let clock = TimeOfDay::new(15, 0, 0, 0)?;
    let mut terms = RangeAccrualTerms::new(1000.0, 0.12, 90.0, 110.0, day(1), day(5), clock);
    terms.realized_hit_count = Some(1);

    let option = RangeAccrualOption::new(terms)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.0, 0.0)?,
        100.0,
        day(2).date().at_time(TimeOfDay::new(12, 0, 0, 0)?),
    )?;
    let request = GreeksRequest::new([Greek::Theta, Greek::Charm, Greek::Color]);
    let auto = AnalyticRangeAccrualEngine.price_with_greeks(&option, &context, request)?;
    assert!((auto.require(Greek::Theta)? - 0.04 * auto.price()).abs() < 1e-9);
    assert_eq!(auto.require(Greek::Charm)?, 0.0);
    assert_eq!(auto.require(Greek::Color)?, 0.0);
    let explicit = numerical_greeks(
        &AnalyticRangeAccrualEngine,
        &option,
        &context,
        request,
        NumericalShiftSettings {
            time_shift: Some(std::time::Duration::from_secs(86_400)),
            ..Default::default()
        },
    )?;
    assert_eq!(
        explicit.status(Greek::Theta),
        GreekStatus::StencilUnavailable
    );
    let at_observation = context.replace(PricingContextChanges {
        valuation_time: Some(day(2).date().at_time(clock)),
        ..Default::default()
    })?;
    assert_eq!(
        AnalyticRangeAccrualEngine
            .price_with_greeks(&option, &at_observation, request,)?
            .status(Greek::Theta),
        GreekStatus::StencilUnavailable
    );
    Ok(())
}

#[test]
fn dividend_coverage_ends_at_the_last_observation() -> Result<()> {
    let start = Timestamp::new(day(2).date(), 16, 0, 0, 0)?;
    let last = Timestamp::new(day(3).date(), 9, 0, 0, 0)?;
    let expiry = Timestamp::new(day(3).date(), 17, 0, 0, 0)?;
    let option = RangeAccrualOption::new(RangeAccrualTerms::new(
        1000.0,
        0.12,
        90.0,
        110.0,
        start,
        expiry,
        last.time_of_day(),
    ))?;
    let price = |risk_end, dividend_end| {
        AnalyticRangeAccrualEngine.price(
            &option,
            &PricingContext::new(
                BlackScholesMertonParameters::new(
                    RateTermStructure::bounded_flat(0.04, start, risk_end)?,
                    RateTermStructure::bounded_flat(0.01, start, dividend_end)?,
                    0.2,
                )?,
                100.0,
                start,
            )?,
        )
    };
    assert_eq!(price(expiry, last)?, price(expiry, expiry)?);
    assert!(matches!(price(last, last), Err(Error::InvalidInput(_))));
    assert!(matches!(price(expiry, start), Err(Error::InvalidInput(_))));
    Ok(())
}

fn civil(date: Date) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).unwrap()
}

fn add(date: Timestamp, days: i64) -> Timestamp {
    let next = civil(date.date()) + chrono::Duration::days(days);
    Date::new(next.year(), next.month(), next.day())
        .unwrap()
        .start_of_day()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "test times and counts are within exact f64 integer precision."
)]
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
    let model = context.model_parameters().as_bsm().unwrap();
    let valuation = context.valuation_time();
    let mut date = add(option.effective_time(), 1);
    loop {
        if option.calendar().is_trading_day(date.date()) {
            count += 1;
            let time = date;
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
        if date == option.expiry_time() {
            break;
        }
        date = add(date, 1);
    }
    option.notional()
        * option.annual_coupon_rate()
        * years(option.effective_time(), option.expiry_time())
        * hits
        / f64::from(count)
        * model
            .risk_free_curve()
            .factor(valuation, option.expiry_time())
            .unwrap()
}

struct ReferenceEngine;
impl PricingEngine<RangeAccrualOption> for ReferenceEngine {
    fn price(&self, option: &RangeAccrualOption, context: &PricingContext) -> Result<f64> {
        Ok(reference(option, context))
    }
}

#[test]
fn deterministic_flat_curves_include_exact_bounds_after_many_observations() -> Result<()> {
    for lower in [110.0_f64, 120.0] {
        let rate = ((lower - 100.0) / 100.0).ln_1p();
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            notional: 1000.0,
            annual_coupon_rate: 0.1,
            lower_bound: lower,
            upper_bound: lower + 10.0,
            expiry_time: day(1).add_days(365)?,
            ..terms()
        })?;
        let context = PricingContext::new(
            BlackScholesMertonParameters::new(rate, 0.0, 0.0)?,
            100.0,
            day(1),
        )?;
        let expected = 100.0 / 365.0 * (-rate).exp();
        assert!((AnalyticRangeAccrualEngine.price(&option, &context)? - expected).abs() < 1e-14);
    }
    Ok(())
}

#[test]
fn dense_shifted_curves_match_independent_marginals_from_intraday_valuation() -> Result<()> {
    let start = day(1);
    let end = start.add_days(730)?;
    let reference_time = start.add_days(-2)?;
    let nodes = (0..=730)
        .map(|n| start.add_days(n)?.add_micros(43_200_000_000))
        .collect::<Result<Vec<_>>>()?;
    let curve = |rate: f64, amplitude: f64, shift| {
        RateTermStructure::new(
            reference_time,
            nodes.clone(),
            nodes
                .iter()
                .map(|&node| {
                    let time = reference_time.years_until(node);
                    (-rate * time + amplitude * (7.0 * time).sin()).exp()
                })
                .collect(),
        )?
        .shifted(shift)
    };
    let risk = curve(0.03, 0.005, 0.002)?;
    let dividend = curve(-0.01, 0.003, -0.001)?;
    for valuation in [start, start.add_days(5)?.add_micros(32_400_000_000)?] {
        let mut input = RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            expiry_time: end,
            ..terms()
        };
        if valuation != start {
            input.realized_hit_count = Some(3);
        }
        let option = RangeAccrualOption::new(input)?;
        for volatility in [0.2, 1e-10] {
            let context = PricingContext::new(
                BlackScholesMertonParameters::new(risk.clone(), dividend.clone(), volatility)?,
                100.0,
                valuation,
            )?;
            let expected = reference(&option, &context);
            let actual = AnalyticRangeAccrualEngine.price(&option, &context)?;
            assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
        }
    }
    Ok(())
}

#[test]
fn exact_marginals_match_flat_negative_and_node_curves_and_recover_volatility() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    let curve = RateTermStructure::new(day(1), vec![day(6), day(17)], vec![1.001, 0.997]).unwrap();
    let dividend =
        RateTermStructure::new(add(day(1), -1), vec![day(8), day(17)], vec![0.998, 1.002]).unwrap();
    for (rate, dividend) in [
        (Rate::Flat(0.04), Rate::Flat(0.01)),
        (Rate::Flat(-0.03), Rate::Flat(0.01)),
        (curve.into(), dividend.into()),
    ] {
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, dividend, 0.2)
                        .unwrap()
                        .into(),
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
        calendar: TradingCalendar::all_days(),
        expiry_time: add(day(1), 365),
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
        ImpliedVolatilitySettings::new(0.1, 0.4),
    )
    .unwrap();
    assert!((sigma - 0.2).abs() < 1e-8);
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
}

#[test]
fn numerical_greeks_match_independent_payoff_reference() {
    let curve =
        RateTermStructure::new(add(day(1), -5), vec![day(8), day(16)], vec![1.001, 0.997]).unwrap();
    for (days, rate) in [(8, Rate::Flat(0.04)), (15, curve.into())] {
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            effective_time: day(3),
            expiry_time: add(day(1), days),
            ..terms()
        })
        .unwrap();
        let context = market(day(4), 99.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(rate, -0.01, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let option = option
            .replace(RangeAccrualOptionChanges {
                calendar: Some(TradingCalendar::weekdays()),
                ..Default::default()
            })
            .unwrap();
        let shifts = NumericalShiftSettings {
            spot_shift: Some(0.1),
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
        let actual = kiyosi::numerical_greeks(
            &AnalyticRangeAccrualEngine,
            &option,
            &context,
            GreeksRequest::all(),
            shifts,
        )
        .unwrap();
        for greek in Greek::ALL {
            let tolerance = match greek {
                Greek::Delta | Greek::Gamma | Greek::Speed => 2e-8,
                Greek::Theta | Greek::Charm | Greek::Color => 7.3e-6,
                Greek::Volga => 0.0002,
                _ => 2e-6,
            };
            assert!(
                (actual.require(greek).unwrap() - expected.require(greek).unwrap()).abs()
                    < tolerance,
                "{greek:?}"
            );
        }
    }
}

#[test]
fn terminal_coupon_counts_current_spot_inclusively() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        expiry_time: day(3),
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
fn greeks_respect_contract_life_and_observations() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    let before = market(Date::new(2024, 12, 29).unwrap().start_of_day(), 99.0);
    assert!(matches!(
        AnalyticRangeAccrualEngine.price_with_greeks(&option, &before, GreeksRequest::all()),
        Err(Error::InvalidInput(_))
    ));
    let current = market(day(2), 95.0);
    let result = AnalyticRangeAccrualEngine
        .price_with_greeks(&option, &current, GreeksRequest::all())
        .unwrap();
    for greek in Greek::ALL {
        assert_eq!(
            result.has(greek),
            matches!(
                greek,
                Greek::Vega | Greek::Volga | Greek::Rho | Greek::DividendRho
            ),
            "{greek:?}"
        );
    }
    let weekend = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::weekdays(),
        effective_time: day(3),
        expiry_time: day(6),
        ..terms()
    })
    .unwrap();
    let context = market(Timestamp::new(day(4).date(), 12, 0, 0, 0).unwrap(), 100.0);
    assert!(
        AnalyticRangeAccrualEngine
            .price_with_greeks(&weekend, &context, GreeksRequest::new([Greek::Theta]))
            .unwrap()
            .has(Greek::Theta)
    );
}

#[test]
fn terms_validate_before_dates_and_replacement_preserves_original() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    assert_eq!(option.realized_hit_count(), None);
    assert_eq!(
        RangeAccrualTerms::new(
            1000.0,
            0.12,
            95.0,
            110.0,
            day(1),
            day(17),
            (day(17)).time_of_day()
        ),
        terms()
    );
    let changed = option
        .replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(Some(3)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.realized_hit_count(), Some(3));
    assert_eq!(
        changed
            .replace(RangeAccrualOptionChanges {
                realized_hit_count: Some(None),
                ..Default::default()
            })
            .unwrap(),
        option
    );
    assert!(matches!(
        option
            .replace(RangeAccrualOptionChanges {
                realized_hit_count: Some(Some(-1)),
                ..Default::default()
            })
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    assert_eq!(
        option,
        RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            ..terms()
        })
        .unwrap()
    );
    for bad in [-1.0, f64::INFINITY, f64::NAN] {
        for (field, name) in [
            "notional",
            "annual_coupon_rate",
            "lower_bound",
            "upper_bound",
        ]
        .into_iter()
        .enumerate()
        {
            let mut input = terms();
            match field {
                0 => input.notional = bad,
                1 => input.annual_coupon_rate = bad,
                2 => input.lower_bound = bad,
                _ => input.upper_bound = bad,
            }
            input.expiry_time = input.effective_time;
            let error = RangeAccrualOption::new(input).unwrap_err();
            assert!(matches!(error, Error::InvalidInput(_)));
            assert!(error.to_string().contains(name), "{name}: {error}");
        }
    }
    for (input, name) in [
        (
            RangeAccrualTerms {
                notional: 0.0,
                ..terms()
            },
            "notional",
        ),
        (
            RangeAccrualTerms {
                lower_bound: 110.0,
                ..terms()
            },
            "lower_bound",
        ),
        (
            RangeAccrualTerms {
                upper_bound: 90.0,
                ..terms()
            },
            "upper_bound",
        ),
        (
            RangeAccrualTerms {
                realized_hit_count: Some(-1),
                ..terms()
            },
            "realized_hit_count",
        ),
    ] {
        let error = RangeAccrualOption::new(input).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)));
        assert!(error.to_string().contains(name), "{name}: {error}");
    }
    for end in [day(1), add(day(1), -1)] {
        assert!(matches!(
            RangeAccrualOption::new(RangeAccrualTerms {
                expiry_time: end,
                ..terms()
            })
            .unwrap_err(),
            Error::InvalidInput(_)
        ));
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
fn history_must_fit_observations_strictly_before_expiry() -> Result<()> {
    let clock = TimeOfDay::new(15, 0, 0, 0)?;
    let first = day(2).date().at_time(clock);
    let last = day(3).date().at_time(clock);
    for (expiry, maximum) in [(last, 1), (last.add_micros(1)?, 2)] {
        let input = RangeAccrualTerms {
            effective_time: day(2),
            expiry_time: expiry,
            observation_clock_time: clock,
            ..terms()
        };
        for hits in [None, Some(0), Some(maximum)] {
            let option = RangeAccrualOption::new(RangeAccrualTerms {
                realized_hit_count: hits,
                ..input.clone()
            })?;
            assert_eq!(option.realized_hit_count(), hits);
        }
        assert!(matches!(
            RangeAccrualOption::new(RangeAccrualTerms {
                realized_hit_count: Some(maximum + 1),
                ..input.clone()
            }),
            Err(Error::InvalidInput(_))
        ));
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            realized_hit_count: Some(maximum),
            ..input
        })?;
        for changes in [
            RangeAccrualOptionChanges {
                realized_hit_count: Some(Some(maximum + 1)),
                ..Default::default()
            },
            RangeAccrualOptionChanges {
                expiry_time: Some(first),
                ..Default::default()
            },
            RangeAccrualOptionChanges {
                calendar: Some(TradingCalendar::custom(
                    option.calendar().clone(),
                    vec![first.date()],
                    vec![],
                    None,
                )?),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                option.replace(changes),
                Err(Error::InvalidInput(_))
            ));
        }
        assert_eq!(option.realized_hit_count(), Some(maximum));
        assert_eq!(option.expiry_time(), expiry);
        assert_eq!(option.observation_schedule().times(), &[first, last]);
        let shortened = option.replace(RangeAccrualOptionChanges {
            expiry_time: Some(first),
            realized_hit_count: Some(Some(0)),
            ..Default::default()
        })?;
        assert_eq!(shortened.realized_hit_count(), Some(0));
        assert_eq!(shortened.observation_schedule().times(), &[first]);
    }
    Ok(())
}

#[test]
fn history_is_strictly_prior_within_contract_life() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    for (at, hits) in [
        (day(1), None),
        (Timestamp::new(day(1).date(), 12, 0, 0, 0).unwrap(), None),
        (day(2), None),
        (Timestamp::new(day(2).date(), 12, 0, 0, 0).unwrap(), Some(1)),
        (day(9), Some(5)),
    ] {
        let live = option
            .replace(RangeAccrualOptionChanges {
                realized_hit_count: Some(hits),
                ..Default::default()
            })
            .unwrap();
        let context = market(at, 100.0);
        assert!(
            (AnalyticRangeAccrualEngine.price(&live, &context).unwrap()
                - reference(&live, &context))
            .abs()
                < 1e-10
        );
    }
    for (at, hits) in [
        (day(1), Some(1)),
        (day(2), Some(1)),
        (Timestamp::new(day(2).date(), 12, 0, 0, 0).unwrap(), Some(2)),
    ] {
        assert!(matches!(
            AnalyticRangeAccrualEngine.price(
                &option
                    .replace(RangeAccrualOptionChanges {
                        realized_hit_count: Some(hits),
                        ..Default::default()
                    })
                    .unwrap(),
                &market(at, 100.0)
            ),
            Err(Error::InvalidInput(_))
        ));
    }
    let missing = market(day(3), 100.0);
    assert!(matches!(
        AnalyticRangeAccrualEngine.price(&option, &missing),
        Err(Error::MissingHistory(_))
    ));
    let after = market(Timestamp::new(day(17).date(), 0, 0, 0, 1).unwrap(), 100.0);
    assert!(matches!(
        AnalyticRangeAccrualEngine
            .price(&option, &after)
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
}

#[test]
fn calendar_and_history_validation_precede_zero_coupon_and_curve_coverage() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    let curve = RateTermStructure::new(day(1), vec![day(2)], vec![0.999]).unwrap();
    for (risk, dividend) in [
        (Rate::Curve(curve.clone()), Rate::Flat(0.01)),
        (Rate::Flat(0.04), Rate::Curve(curve)),
    ] {
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(risk, dividend, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            AnalyticRangeAccrualEngine
                .price(&option, &context)
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        let zero = RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            annual_coupon_rate: 0.0,
            ..terms()
        })
        .unwrap();
        assert_eq!(
            AnalyticRangeAccrualEngine.price(&zero, &context).unwrap(),
            0.0
        );
        assert!(matches!(
            AnalyticRangeAccrualEngine
                .price(&zero, &market(day(4), 100.0))
                .unwrap_err(),
            Error::MissingHistory(_)
        ));
    }
    let context = market(day(3), 100.0);
    let weekend = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        annual_coupon_rate: 0.0,
        expiry_time: day(5),
        ..terms()
    })
    .unwrap();
    assert!(matches!(
        AnalyticRangeAccrualEngine
            .price(&weekend, &context)
            .unwrap_err(),
        Error::MissingHistory(_)
    ));
    let monday = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        effective_time: day(3),
        expiry_time: day(6),
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
        (below, 110.0, 1e-14, 0.606_995_980_412_628_3, 1),
        (90.0, above, 1e-14, 0.606_995_980_412_628_5, 1),
    ] {
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            notional: 365.0 / f64::from(days),
            annual_coupon_rate: 1.0,
            lower_bound: lower,
            upper_bound: upper,
            expiry_time: add(day(1), days.into()),
            ..terms()
        })
        .unwrap();
        let context = market(day(1), 100.0)
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(0.0, 0.0, vol)
                        .unwrap()
                        .into(),
                ),
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
        calendar: TradingCalendar::all_days(),
        notional: 1e300,
        annual_coupon_rate: 1e100,
        lower_bound: (mean + 40.0 * width).exp(),
        upper_bound: (mean + 41.0 * width).exp(),
        expiry_time: day(2),
        ..terms()
    })
    .unwrap();
    // Independent normal-tail constant: log P(Z > 40); the >41 tail is negligible here.
    let expected =
        (1e300_f64.ln() + 1e100_f64.ln() - 365.0_f64.ln() - 0.04 / 365.0 - 804.608_442_013_753_8)
            .exp();
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
        calendar: TradingCalendar::all_days(),
        notional: 1e300,
        annual_coupon_rate: 1e100,
        ..terms()
    })
    .unwrap();
    assert!(matches!(
        AnalyticRangeAccrualEngine
            .price(&overflow, &market(day(1), 100.0))
            .unwrap_err(),
        Error::ComputationFailure(_)
    ));
}

#[test]
fn request_and_solver_errors_preserve_inputs() {
    let engine = AnalyticRangeAccrualEngine;
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    let context = market(day(1), 100.0);
    let shifts = NumericalShiftSettings::default();

    assert!(matches!(
        kiyosi::numerical_greeks(
            &engine,
            &option,
            &context,
            GreeksRequest::all(),
            NumericalShiftSettings {
                volatility_shift: Some(0.0),
                ..shifts
            }
        )
        .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let unavailable = kiyosi::numerical_greeks(
        &engine,
        &option,
        &context,
        GreeksRequest::new([Greek::Delta]),
        NumericalShiftSettings {
            spot_shift: Some(200.0),
            ..shifts
        },
    )
    .unwrap();
    assert!(!unavailable.has(Greek::Delta));
    let settings = ImpliedVolatilitySettings::new(0.1, 0.4);
    assert!(matches!(
        implied_volatility(&engine, &option, &context, -1.0, settings).unwrap_err(),
        Error::RootNotBracketed(_)
    ));
    let zero = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        annual_coupon_rate: 0.0,
        ..terms()
    })
    .unwrap();
    assert!(matches!(
        implied_volatility(&engine, &zero, &context, 0.0, settings).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let end = option
        .replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(Some(15)),
            ..Default::default()
        })
        .unwrap();
    let terminal = market(day(17), 100.0);
    assert!(matches!(
        implied_volatility(&engine, &end, &terminal, 0.0, settings).unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
    let result =
        kiyosi::numerical_greeks(&engine, &end, &terminal, GreeksRequest::all(), shifts).unwrap();
    for greek in [Greek::Theta, Greek::Charm, Greek::Color] {
        assert_eq!(result.get(greek), None);
    }
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        if result.supported().has(greek) {
            assert_eq!(result.require(greek).unwrap(), 0.0);
        }
    }
    assert_eq!(
        option,
        RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            ..terms()
        })
        .unwrap()
    );
    assert_eq!(
        context.model_parameters().as_bsm().unwrap().volatility(),
        0.2
    );
}

#[test]
fn full_term_coupon_uses_calendar_days_and_one_expiry_discount() {
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        ..terms()
    })
    .unwrap();
    let model = BlackScholesMertonParameters::new(0.8, 0.8, 1e-9).unwrap();
    let context = market(day(1), 100.0)
        .replace(PricingContextChanges {
            model_parameters: Some(model.into()),
            ..Default::default()
        })
        .unwrap();
    let engine = AnalyticRangeAccrualEngine;
    let expected = 120.0 * 16.0 / 365.0 * (-0.8_f64 * 16.0 / 365.0).exp();
    assert!((engine.price(&option, &context).unwrap() - expected).abs() < 1e-12);
    let seasoned = option
        .replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(Some(3)),
            ..Default::default()
        })
        .unwrap();
    let live = context
        .replace(PricingContextChanges {
            valuation_time: Some(day(8)),
            ..Default::default()
        })
        .unwrap();
    let expected = 120.0 * 13.0 / 365.0 * (-0.8_f64 * 9.0 / 365.0).exp();
    assert!((engine.price(&seasoned, &live).unwrap() - expected).abs() < 1e-12);
}

#[test]
fn long_and_zero_lower_bound_contracts_support_shared_concurrent_reads() {
    let engine = AnalyticRangeAccrualEngine;
    let option = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
        expiry_time: add(day(1), 21204),
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
    let seasoned = option
        .replace(RangeAccrualOptionChanges {
            realized_hit_count: Some(Some(20000)),
            ..Default::default()
        })
        .unwrap();
    let live = market(add(day(1), 20001), 100.0);
    assert!((engine.price(&seasoned, &live).unwrap() - reference(&seasoned, &live)).abs() < 1e-8);
    let lower_zero = RangeAccrualOption::new(RangeAccrualTerms {
        calendar: TradingCalendar::all_days(),
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
    let expiry = Date::new(2500, 1, 2).unwrap().start_of_day();
    let knot = Date::new(2500, 1, 1).unwrap().start_of_day();
    let curve = RateTermStructure::new(
        day(1),
        vec![
            knot,
            Timestamp::new(knot.date(), 0, 0, 0, 1).unwrap(),
            expiry,
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
                    BlackScholesMertonParameters::new(risk, dividend, 0.2)
                        .unwrap()
                        .into(),
                ),
                ..Default::default()
            })
            .unwrap();
        let mut input = RangeAccrualTerms {
            effective_time: day(1),
            expiry_time: expiry,
            ..terms()
        };
        let option = RangeAccrualOption::new(RangeAccrualTerms {
            calendar: TradingCalendar::all_days(),
            ..input.clone()
        })
        .unwrap();
        assert!(matches!(
            AnalyticRangeAccrualEngine
                .price(&option, &context)
                .unwrap_err(),
            Error::ComputationFailure(_)
        ));
        input.annual_coupon_rate = 0.0;
        assert_eq!(
            AnalyticRangeAccrualEngine
                .price(
                    &RangeAccrualOption::new(RangeAccrualTerms {
                        calendar: TradingCalendar::all_days(),
                        ..input
                    })
                    .unwrap(),
                    &context
                )
                .unwrap(),
            0.0
        );
    }
}
