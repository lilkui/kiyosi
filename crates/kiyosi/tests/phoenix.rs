use kiyosi::*;

fn date(month: u32, day: u32) -> Timestamp {
    Date::new(2025, month, day).unwrap().start_of_day()
}
fn terms() -> PhoenixTerms {
    let mut terms = PhoenixTerms::new(
        0.01,
        100.0,
        80.0,
        100.0,
        60.0,
        date(1, 1),
        date(4, 1),
        [date(2, 3), date(4, 1)]
            .iter()
            .zip([110.0, 110.0].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        [date(2, 3), date(4, 1)]
            .iter()
            .zip([90.0, 90.0].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap(),
        kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: (date(4, 1)).time_of_day(),
        },
        TradingCalendar::weekdays(),
    );
    terms.knock_in_observation = KnockInObservation::AtExpiry;
    terms.barrier_state = Some(AutocallableBarrierState::NoEvent);

    terms
}
fn market(spot: f64, valuation: Timestamp) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, 0.3).unwrap(),
        spot,
        valuation,
    )
    .unwrap()
}

#[test]
fn closed_horizon_coupon_jump_has_no_spot_greeks() -> Result<()> {
    let start = Date::new(2026, 11, 14)?.start_of_day();
    let end = start.add_days(1)?;
    let mut input = terms();
    input.effective_time = start;
    input.expiry_time = end;
    input.calendar = TradingCalendar::all_days();
    input.knock_out_observations = vec![KnockOutObservation::new(end, 200.0)?];
    input.coupon_observations = vec![CouponObservation::new(end, 100.0)?];
    let note = PhoenixOption::new(input)?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.0,
        0.0,
        0.2,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
    )?;
    let context = PricingContext::new(model, 100.0, start)?;
    let engine = FiniteDifferenceAutocallableEngine::default();
    let solution = engine.solve(&note, &context, 99.0..=101.0)?;
    let request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    let shifts = NumericalShiftSettings {
        spot_shift: Some(0.125),
        ..Default::default()
    };
    for (spot, status) in [
        (100.0, GreekStatus::Undefined),
        (100.0625, GreekStatus::StencilUnavailable),
    ] {
        let at_spot = context.replace(PricingContextChanges {
            spot_price: Some(spot),
            ..Default::default()
        })?;
        for result in [
            engine.price_with_greeks_with_settings(&note, &at_spot, request, shifts)?,
            solution.price_with_greeks(spot, request, shifts.spot_shift)?,
        ] {
            assert_eq!(result.status(Greek::Delta), status);
            assert_eq!(result.status(Greek::Gamma), status);
        }
    }
    Ok(())
}

#[test]
fn closed_prefix_pays_coupon_and_knock_out_exactly() -> Result<()> {
    let start = Date::new(2026, 11, 14)?.start_of_day();
    let sunday = start.add_days(1)?;
    let end = sunday.add_days(1)?.add_micros(43_200_000_000)?;
    let mut input = terms();
    input.effective_time = start;
    input.expiry_time = end;
    input.calendar = TradingCalendar::all_days();
    input.rebate_rate = 0.02;
    input.knock_out_observations = vec![KnockOutObservation::new(sunday, 110.0)?];
    input.coupon_observations = vec![CouponObservation::new(sunday, 100.0)?];
    let paid = PhoenixOption::new(input.clone())?;
    input.coupon_per_period = 0.0;
    let plain = PhoenixOption::new(input)?;
    let model = BlackScholesMertonParameters::new_with_variance_time_convention(
        0.04,
        0.04,
        0.2,
        VarianceTimeConvention::Business(TradingCalendar::weekdays()),
    )?;
    let context = PricingContext::new(model, 100.0, start)?;
    let engine = FiniteDifferenceAutocallableEngine::default();
    let a = engine.solve(&paid, &context, 99.0..=111.0)?;
    let b = engine.solve(&plain, &context, 99.0..=111.0)?;
    let discount = (-0.04 / 365.0_f64).exp();
    for (spot, expected) in [(99.0, 0.0), (100.0, 0.01 * discount)] {
        let at_spot = context.replace(PricingContextChanges {
            spot_price: Some(spot),
            ..Default::default()
        })?;
        assert!(
            (engine.price(&paid, &at_spot)? - engine.price(&plain, &at_spot)? - expected).abs()
                < 1e-12
        );
        assert!((a.price(spot)? - b.price(spot)? - expected).abs() < 1e-12);
    }
    let at_knock_out = context.replace(PricingContextChanges {
        spot_price: Some(110.0),
        ..Default::default()
    })?;
    assert!((engine.price(&paid, &at_knock_out)? - 1.03 * discount).abs() < 1e-12);
    assert!((a.price(110.0)? - 1.03 * discount).abs() < 1e-12);
    assert!(matches!(
        implied_volatility(
            &engine,
            &paid,
            &at_knock_out,
            1.03 * discount,
            ImpliedVolatilitySettings::new(0.0, 1.0)
        ),
        Err(Error::UnsupportedOperation(_))
    ));
    Ok(())
}

#[test]
fn lockout_preserves_conditional_coupons_and_continuation_in_all_engines() {
    let context = market(100.0, date(1, 1));
    let mut input = terms();
    input.lower_strike = 100.0;
    input.knock_out_observations = ([date(1, 1), date(2, 3), date(4, 1)]
        .iter()
        .zip([100.0; 3].iter())
        .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap())
    .into_iter()
    .skip(2)
    .collect();
    input.coupon_observations = [date(1, 1), date(2, 3), date(4, 1)]
        .iter()
        .zip([90.0; 3].iter())
        .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let mut expected = (-0.04 * date(1, 1).years_until(date(4, 1))).exp() + 0.01;
    for time in [date(2, 3), date(4, 1)] {
        let digital =
            DigitalOption::cash_or_nothing(OptionType::Call, 90.0, 0.01, date(1, 1), time).unwrap();
        expected += AnalyticDigitalEngine.price(&digital, &context).unwrap();
    }
    for (engine, tolerance) in [
        (
            &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
            1e-9,
        ),
        (&FiniteDifferenceAutocallableEngine::default(), 2e-5),
    ] {
        let actual = engine.price(&note, &context).unwrap();
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} != {expected}"
        );
    }
    let engine = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let estimate = engine.price_with_statistics(&note, &context).unwrap();
    assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap() + 1e-6);
}

#[test]
fn lockout_preserves_daily_knock_in_and_first_eligible_knock_out() {
    let mut input = terms();
    input.lower_strike = 0.0;
    input.knock_in_observation = KnockInObservation::EveryTradingDay {
        observation_clock_time: date(1, 1).time_of_day(),
    };
    input.knock_out_observations = ([date(1, 1), date(4, 1)]
        .iter()
        .zip([50.0, 200.0].iter())
        .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap())
    .into_iter()
    .skip(1)
    .collect();
    input.coupon_observations = [date(1, 1), date(4, 1)]
        .iter()
        .zip([0.0; 2].iter())
        .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let horizon = date(1, 1).years_until(date(4, 1));
    let rate = (90.0_f64 / 70.0).ln() / horizon;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rate, 0.0, 0.0).unwrap(),
        70.0,
        date(1, 1),
    )
    .unwrap();
    for engine in [
        &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
        &FiniteDifferenceAutocallableEngine::default(),
        &MonteCarloAutocallableEngine::default(),
    ] {
        let expected = 0.01 + (0.9 + 0.01) * (-rate * horizon).exp();
        assert!((engine.price(&note, &context).unwrap() - expected).abs() < 1e-12);
        let context = market(200.0, date(4, 1));
        assert!((engine.price(&note, &context).unwrap() - 1.01).abs() < 1e-14);
    }
}

#[test]
fn lockout_daily_layers_agree_across_engines_and_constant_local_volatility() {
    let end = date(1, 10);
    let mut input = terms();
    input.expiry_time = end;
    input.knock_in_level = 99.0;
    input.lower_strike = 0.0;
    input.knock_in_observation = KnockInObservation::EveryTradingDay {
        observation_clock_time: date(1, 1).time_of_day(),
    };
    input.knock_out_observations = ([date(1, 2), date(1, 6), end]
        .iter()
        .zip([100.0; 3].iter())
        .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap())
    .into_iter()
    .skip(2)
    .collect();
    input.coupon_observations = [date(1, 2), date(1, 6), end]
        .iter()
        .zip([90.0; 3].iter())
        .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let context = market(100.0, date(1, 1));
    let expected = QuadratureAutocallableEngine::default()
        .price(&note, &context)
        .unwrap();
    let fd = FiniteDifferenceAutocallableEngine::new(FiniteDifferenceSettings {
        asset_step_count: 800,
        time_step_count: 800,
        ..Default::default()
    })
    .unwrap();
    let actual = fd.price(&note, &context).unwrap();
    assert!((actual - expected).abs() < 2e-5, "{actual} != {expected}");
    let surface = LocalVolatilitySurface::new(
        vec![date(1, 1), end],
        vec![50.0, 150.0],
        vec![vec![0.3; 2]; 2],
    )
    .unwrap();
    let local = PricingContext::new(
        LocalVolatilityParameters::new(0.04, 0.01, surface).unwrap(),
        100.0,
        date(1, 1),
    )
    .unwrap();
    assert!((fd.price(&note, &local).unwrap() - actual).abs() < 1e-12);
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    let estimate = mc.price_with_statistics(&note, &context).unwrap();
    assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap());
}

#[test]
fn lockout_history_depends_only_on_eligible_knock_out_observations() {
    let mut input = terms();
    input.barrier_state = None;

    input.knock_out_observations = [date(3, 3), date(4, 1)]
        .into_iter()
        .map(|time| KnockOutObservation::new(time, 110.0))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    input.coupon_observations = [date(2, 3), date(3, 3), date(4, 1)]
        .into_iter()
        .map(|time| CouponObservation::new(time, 90.0))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    let note = PhoenixOption::new(input).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    // The locked coupon is past. Today's coupon and the maturity payment remain.
    let at_first_eligible = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0).unwrap(),
        100.0,
        date(3, 3),
    )
    .unwrap();
    let price = engine.price(&note, &at_first_eligible).unwrap();
    assert!((price - 1.02).abs() < 1e-12, "Phoenix price {price}");
    assert!(matches!(
        engine.price(&note, &market(100.0, date(3, 4))),
        Err(Error::MissingHistory(_))
    ));
    let known = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        known
            .replace(PhoenixOptionChanges {
                knock_out_observations: Some(
                    known
                        .coupon_observations()
                        .iter()
                        .map(|r| KnockOutObservation::new(r.time(), 110.0).unwrap())
                        .collect()
                ),
                ..Default::default()
            })
            .unwrap()
            .barrier_state(),
        known.barrier_state()
    );
    assert_eq!(known.knock_out_observations().len(), 2);
    assert_eq!(
        known
            .replace(PhoenixOptionChanges {
                knock_out_observations: Some(known.knock_out_observations()[1..].to_vec()),
                ..Default::default()
            })
            .unwrap()
            .knock_out_observations()
            .len(),
        1
    );
    assert!(matches!(
        engine.price(
            &note
                .replace(PhoenixOptionChanges {
                    barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
                    ..Default::default()
                })
                .unwrap(),
            &market(100.0, date(2, 4))
        ),
        Err(Error::InvalidInput(_))
    ));
    let possible = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.price(&possible, &market(100.0, date(3, 4))).unwrap(),
        0.0
    );
    let mut daily = note.terms().clone();
    daily.knock_in_observation = KnockInObservation::EveryTradingDay {
        observation_clock_time: date(1, 1).time_of_day(),
    };
    assert!(matches!(
        engine.price(
            &PhoenixOption::new(daily).unwrap(),
            &market(100.0, date(1, 2))
        ),
        Err(Error::MissingHistory(_))
    ));
}

#[test]
fn coupon_only_dates_do_not_create_knock_out_greek_discontinuities() {
    let mut input = terms();
    input.knock_out_observations.remove(0);
    let note = PhoenixOption::new(input).unwrap();
    let context = market(110.0, date(2, 3));
    let greeks = QuadratureAutocallableEngine::default()
        .price_with_greeks(&note, &context, GreeksRequest::new([Greek::Delta]))
        .unwrap();
    assert!(greeks.require(Greek::Delta).unwrap().is_finite());
    assert_eq!(note.knock_out_observations().len(), 1);
}
#[test]
fn quoted_coupons_preserve_units_and_reprice_solved_quotes() {
    let engine = QuadratureAutocallableEngine::default();
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.0, 0.0, 0.0).unwrap(),
        100.0,
        date(1, 1),
    )
    .unwrap();
    let mut input = terms();
    input.knock_out_observations = [date(4, 1)]
        .iter()
        .zip([110.0].iter())
        .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    input.coupon_observations = [date(4, 1)]
        .iter()
        .zip([0.0].iter())
        .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    let note = PhoenixOption::new(input.clone()).unwrap();
    for coupon in [-0.02, 0.0, 0.02] {
        let quoted = note.with_coupon(coupon).unwrap();
        let mut expected = input.clone();
        expected.coupon_per_period = coupon;
        assert_eq!(quoted.terms(), &expected);
        let quote = 1.0 + coupon;
        assert!((engine.price(&quoted, &context).unwrap() - quote).abs() < 1e-14);
        let solved = implied_phoenix_coupon(
            &engine,
            &note,
            &context,
            quote,
            ImpliedCouponSettings {
                lower_bound: -1.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!((solved - coupon).abs() < 1e-12);
        let repriced = note
            .replace(PhoenixOptionChanges {
                coupon_per_period: Some(solved),
                ..Default::default()
            })
            .unwrap();
        assert!((engine.price(&repriced, &context).unwrap() - quote).abs() < 1e-12);
    }
    let quoted = note.with_coupon(0.12).unwrap();
    assert_eq!(quoted.terms().coupon_per_period, 0.12);
    assert_eq!(quoted.barrier_state(), note.barrier_state());
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            note.with_coupon(invalid),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(note.terms(), &input);
}

#[test]
fn conditional_period_coupons_are_inclusive_and_independent_of_principal_and_loss() {
    let engine = QuadratureAutocallableEngine::default();
    for coupon in [0.365, -0.365] {
        let mut terms = terms();
        terms.coupon_per_period = coupon;
        terms.principal_redemption_ratio = 0.25;
        let note = PhoenixOption::new(terms).unwrap();
        for (spot, base, paid) in [
            (60.0, -0.15, false),
            (70.0, -0.05, false),
            (80.0, 0.25, false),
            (90.0, 0.25, true),
            (110.0, 0.25, true),
        ] {
            let expected = base + if paid { coupon } else { 0.0 };
            assert!(
                (engine.price(&note, &market(spot, date(4, 1))).unwrap() - expected).abs() < 1e-14
            );
        }
        assert!(
            (engine.price(&note, &market(110.0, date(2, 3))).unwrap() - (0.25 + coupon)).abs()
                < 1e-14
        );
    }
}

#[test]
fn periodic_coupons_ignore_unequal_and_zero_length_periods_in_all_engines() {
    for coupon in [-0.01, 0.0, 0.01] {
        let mut input = terms();
        input.coupon_per_period = coupon;
        input.knock_out_observations = [date(1, 1), date(2, 3), date(4, 1)]
            .iter()
            .zip([110.0; 3].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap();
        input.coupon_observations = [date(1, 1), date(2, 3), date(4, 1)]
            .iter()
            .zip([90.0; 3].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap();
        let note = PhoenixOption::new(input).unwrap();
        for at in [date(1, 1), date(2, 3), date(4, 1)] {
            let context = market(110.0, at);
            for engine in [
                &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
                &FiniteDifferenceAutocallableEngine::default(),
                &MonteCarloAutocallableEngine::default(),
            ] {
                assert!((engine.price(&note, &context).unwrap() - (1.0 + coupon)).abs() < 1e-14);
            }
        }
    }
}

#[test]
fn terminal_coupon_and_downside_match_an_independent_vanilla_digital_portfolio() {
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let ctx = market(100.0, date(1, 1));
    for coupon in [-4.0, 0.1] {
        for lower in [0.0, 60.0] {
            let mut t = terms();
            t.expiry_time = end;
            t.knock_in_level = 100.0;
            t.lower_strike = lower;
            t.principal_redemption_ratio = 0.25;
            t.coupon_per_period = coupon;
            t.knock_out_observations = [end]
                .iter()
                .zip([100.0].iter())
                .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
                .collect::<kiyosi::Result<Vec<_>>>()
                .unwrap();
            t.coupon_observations = [end]
                .iter()
                .zip([100.0].iter())
                .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
                .collect::<kiyosi::Result<Vec<_>>>()
                .unwrap();
            let put = |strike| {
                AnalyticEuropeanEngine
                    .price(
                        &EuropeanOption::new(OptionType::Put, strike, t.effective_time, end)
                            .unwrap(),
                        &ctx,
                    )
                    .unwrap()
            };
            let digital = DigitalOption::cash_or_nothing(
                OptionType::Call,
                100.0,
                coupon.abs(),
                t.effective_time,
                end,
            )
            .unwrap();
            let expected = 0.25 * (-0.04_f64).exp()
                + coupon.signum() * AnalyticDigitalEngine.price(&digital, &ctx).unwrap()
                - (put(100.0) - if lower == 0.0 { 0.0 } else { put(lower) }) / 100.0;
            let actual = QuadratureAutocallableEngine::default()
                .price(&PhoenixOption::new(t).unwrap(), &ctx)
                .unwrap();
            assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
        }
    }
}

#[test]
fn phoenix_validation_defaults_and_history_share_core_categories() {
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        let t = terms();
        assert!(matches!(
            t.coupon_observations[0].replace(None, Some(value)),
            Err(Error::InvalidInput(_))
        ));
    }
    let mut t = terms();
    t.knock_out_observations.clear();
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.coupon_per_period = f64::NAN;
    assert!(matches!(
        PhoenixOption::new(t).unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.knock_in_observation = KnockInObservation::EveryTradingDay {
        observation_clock_time: t.expiry_time.time_of_day(),
    };
    t.barrier_state = None;
    let note = PhoenixOption::new(t).unwrap();
    let engine = QuadratureAutocallableEngine::default();
    let ctx = market(100.0, date(2, 3));
    assert!(matches!(
        engine.price(&note, &ctx).unwrap_err(),
        Error::MissingHistory(_)
    ));
    let known = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::NoEvent)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        known.knock_out_observations(),
        note.knock_out_observations()
    );
    assert_eq!(note.barrier_state(), None);
    assert!(engine.price(&known, &ctx).unwrap().is_finite());
    let impossible = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        engine
            .price(&impossible, &market(100.0, date(1, 1)))
            .unwrap_err(),
        Error::InvalidInput(_)
    ));
    let mut t = terms();
    t.barrier_state = Some(AutocallableBarrierState::KnockedIn);

    assert!(matches!(
        engine.price(&PhoenixOption::new(t).unwrap(), &market(100.0, date(1, 1))),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn phoenix_greeks_and_volatility_preserve_thresholds_and_failure_state() {
    let note = PhoenixOption::new(terms()).unwrap();
    let ctx = market(103.0, date(1, 1));
    let engine = QuadratureAutocallableEngine::default();
    let result = kiyosi::numerical_greeks(
        &engine,
        &note,
        &ctx,
        GreeksRequest::all(),
        NumericalShiftSettings {
            spot_shift: Some(0.25),
            ..Default::default()
        },
    )
    .unwrap();
    for greek in Greek::ALL {
        assert!(result.has(greek), "{greek:?}");
    }
    let sigma = implied_volatility(
        &engine,
        &note,
        &ctx,
        result.price(),
        ImpliedVolatilitySettings::new(0.2, 0.4),
    )
    .unwrap();
    assert!((sigma - 0.3).abs() < 1e-6);
    assert_eq!(ctx.model_parameters().as_bsm().unwrap().volatility(), 0.3);
    let ctx = market(90.0, date(2, 3));
    let result = engine
        .price_with_greeks(&note, &ctx, GreeksRequest::all())
        .unwrap();
    assert!(!result.has(Greek::Delta));
    assert!(!result.has(Greek::Theta));
    assert!(result.has(Greek::Vega));
    assert!(result.has(Greek::Rho));
    let settled = note
        .replace(PhoenixOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.price(&settled, &market(100.0, date(3, 3))).unwrap(),
        0.0
    );
    assert!(matches!(
        implied_volatility(
            &engine,
            &settled,
            &market(100.0, date(3, 3)),
            0.0,
            kiyosi::ImpliedVolatilitySettings::new(0.0, 4.0)
        )
        .unwrap_err(),
        Error::UnsupportedOperation(_)
    ));
}

#[test]
fn unconditional_coupon_tails_match_a_two_date_cashflow_portfolio() {
    let middle = date(7, 1);
    let end = Date::new(2026, 1, 1).unwrap().start_of_day();
    let mut t = terms();
    t.coupon_per_period = -0.5;
    t.lower_strike = 100.0;
    t.principal_redemption_ratio = 0.25;
    t.expiry_time = end;
    t.knock_out_observations = [middle, end]
        .iter()
        .zip([100.0; 2].iter())
        .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    t.coupon_observations = [middle, end]
        .iter()
        .zip([0.0; 2].iter())
        .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
        .collect::<kiyosi::Result<Vec<_>>>()
        .unwrap();
    let note = PhoenixOption::new(t).unwrap();
    let ctx = market(100.0, date(1, 1))
        .replace(PricingContextChanges {
            model_parameters: Some(
                BlackScholesMertonParameters::new(0.04, 0.01, 2.0)
                    .unwrap()
                    .into(),
            ),
            ..Default::default()
        })
        .unwrap();
    let time: f64 = 181.0 / 365.0;
    let survival = 0.5 * libm::erfc((0.03 - 2.0) * time.sqrt() / (2.0 * 2.0_f64.sqrt()));
    let expected = (-0.5 + 0.25 * (1.0 - survival)) * (-0.04 * time).exp()
        + (0.25 - 0.5) * survival * (-0.04_f64).exp();
    for settings in [
        QuadratureSettings::default(),
        QuadratureSettings {
            asset_step_count: 128,
            integration_standard_deviations: 0.5,
        },
    ] {
        let actual = QuadratureAutocallableEngine::new(settings)
            .unwrap()
            .price(&note, &ctx)
            .unwrap();
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }
}

#[test]
fn phoenix_daily_layers_converge_with_conditional_coupons() {
    let mut prices = Vec::new();
    for mode in [
        KnockInObservation::AtExpiry,
        KnockInObservation::EveryTradingDay {
            observation_clock_time: kiyosi::TimeOfDay::new(0, 0, 0, 0).unwrap(),
        },
    ] {
        let mut t = terms();
        t.knock_in_level = 95.0;
        t.knock_in_observation = mode;

        t.knock_out_observations = [date(2, 3), date(3, 3), date(4, 1)]
            .iter()
            .zip([110.0, 108.0, 105.0].iter())
            .map(|(&time, &level)| kiyosi::KnockOutObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap();
        t.coupon_observations = [date(2, 3), date(3, 3), date(4, 1)]
            .iter()
            .zip([95.0; 3].iter())
            .map(|(&time, &level)| kiyosi::CouponObservation::new(time, level))
            .collect::<kiyosi::Result<Vec<_>>>()
            .unwrap();
        let note = PhoenixOption::new(t).unwrap();
        let values: Vec<_> = [(512, 8.0), (1024, 8.0), (1024, 10.0)]
            .into_iter()
            .map(|(n, e)| {
                QuadratureAutocallableEngine::new(QuadratureSettings {
                    asset_step_count: n,
                    integration_standard_deviations: e,
                })
                .unwrap()
                .price(&note, &market(100.0, date(1, 1)))
                .unwrap()
            })
            .collect();
        assert!((values[0] - values[1]).abs() < 1e-4, "{values:?}");
        assert!((values[1] - values[2]).abs() < 1e-4, "{values:?}");
        prices.push(values[0]);
    }
    assert!(prices[1] < prices[0] - 0.001, "{prices:?}");
}

#[test]
fn independent_schedules_match_an_early_redemption_cashflow_portfolio() -> Result<()> {
    let mut input = terms();
    input.lower_strike = input.upper_strike;
    input.knock_out_observations = vec![KnockOutObservation::new(date(3, 3), 110.0)?];
    input.coupon_observations = vec![
        CouponObservation::new(date(2, 3), 0.0)?,
        CouponObservation::new(date(4, 1), 0.0)?,
    ];
    let note = PhoenixOption::new(input)?;
    let context = market(100.0, date(1, 1));
    let discount = |time| (-0.04 * date(1, 1).years_until(time)).exp();
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 110.0, 1.0, date(1, 1), date(3, 3))?;
    let probability = AnalyticDigitalEngine.price(&digital, &context)? / discount(date(3, 3));
    let expected = 0.01 * discount(date(2, 3))
        + probability * discount(date(3, 3))
        + (1.0 - probability) * 1.01 * discount(date(4, 1));
    for (engine, tolerance) in [
        (
            &QuadratureAutocallableEngine::default() as &dyn PricingEngine<PhoenixOption>,
            1e-9,
        ),
        (&FiniteDifferenceAutocallableEngine::default(), 2e-5),
    ] {
        let actual = engine.price(&note, &context)?;
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} != {expected}"
        );
        assert_eq!(engine.price(&note, &market(110.0, date(3, 3)))?, 1.0);
    }
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 32768,
        seed: Some(42),
        ..Default::default()
    })?;
    let estimate = mc.price_with_statistics(&note, &context)?;
    assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap());
    assert_eq!(mc.price(&note, &market(110.0, date(3, 3)))?, 1.0);
    Ok(())
}

#[test]
fn each_schedule_validates_order_life_calendar_and_replacement_atomically() {
    let note = PhoenixOption::new(terms()).unwrap();
    for times in [
        vec![],
        vec![date(4, 1), date(2, 3)],
        vec![date(2, 3); 2],
        vec![date(1, 4)],
        vec![date(1, 1).add_days(-1).unwrap()],
        vec![date(4, 2)],
    ] {
        let knock_out = times
            .iter()
            .map(|&time| KnockOutObservation::new(time, 110.0).unwrap())
            .collect();
        let coupons = times
            .iter()
            .map(|&time| CouponObservation::new(time, 0.0).unwrap())
            .collect();
        assert!(matches!(
            note.replace(PhoenixOptionChanges {
                knock_out_observations: Some(knock_out),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            note.replace(PhoenixOptionChanges {
                coupon_observations: Some(coupons),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(note.terms(), &terms());
    }
}
