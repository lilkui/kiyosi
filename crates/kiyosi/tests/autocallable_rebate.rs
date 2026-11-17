use kiyosi::{
    AnalyticDigitalEngine, AutocallableBarrierState, AutocallableNote, AutocallableObservation,
    AutocallableTerms, BinarySnowballOption, BinarySnowballTerms, BlackScholesMertonParameters,
    CouponObservation, CouponQuoteConvention, Date, DigitalOption, Error,
    FiniteDifferenceAutocallableEngine, KnockInObservation, KnockOutObservation,
    MonteCarloAutocallableEngine, MonteCarloSettings, OptionType, PhoenixOption,
    PhoenixOptionChanges, PhoenixTerms, PricingContext, PricingEngine,
    QuadratureAutocallableEngine, SnowballOption, SnowballOptionChanges, SnowballTerms,
    TernarySnowballOption, TernarySnowballTerms, Timestamp, TradingCalendar,
};

fn start() -> Timestamp {
    Timestamp::new(Date::new(2025, 1, 1).unwrap(), 12, 0, 0, 0).unwrap()
}

fn end() -> Timestamp {
    Timestamp::new(Date::new(2025, 4, 1).unwrap(), 18, 0, 0, 0).unwrap()
}

fn market(spot: f64, time: Timestamp, volatility: f64) -> PricingContext {
    PricingContext::new(
        BlackScholesMertonParameters::new(0.04, 0.01, volatility).unwrap(),
        spot,
        time,
    )
    .unwrap()
}

fn snowball() -> SnowballTerms {
    SnowballTerms::new(
        0.06,
        100.0,
        80.0,
        100.0,
        60.0,
        start(),
        end(),
        vec![AutocallableObservation::snowball(end(), 110.0, 0.08).unwrap()],
        KnockInObservation::AtExpiry,
        TradingCalendar::weekdays(),
    )
}

fn phoenix() -> PhoenixTerms {
    PhoenixTerms::new(
        0.01,
        100.0,
        80.0,
        100.0,
        60.0,
        start(),
        end(),
        vec![KnockOutObservation::new(end(), 110.0).unwrap()],
        vec![CouponObservation::new(end(), 90.0).unwrap()],
        KnockInObservation::AtExpiry,
        TradingCalendar::weekdays(),
    )
}

fn assert_prices<T: AutocallableTerms>(
    note: &AutocallableNote<T>,
    context: &PricingContext,
    expected: f64,
) {
    for engine in [
        &QuadratureAutocallableEngine::default() as &dyn PricingEngine<AutocallableNote<T>>,
        &FiniteDifferenceAutocallableEngine::default(),
        &MonteCarloAutocallableEngine::default(),
    ] {
        let actual = engine.price(note, context).unwrap();
        assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
    }
}

#[test]
fn rebates_add_to_every_terminal_redemption_and_coupon_outcome() {
    let years = 90.25 / 365.0;
    for (flat, annual) in [(0.0, 0.0), (0.02, 0.0), (0.0, 0.06), (0.02, 0.06)] {
        let rebate = flat + annual * years;
        let mut snow = snowball();
        snow.rebate_rate = flat;
        snow.annual_rebate_rate = annual;
        let snow = SnowballOption::new(snow).unwrap();
        let mut bird = phoenix();
        bird.rebate_rate = flat;
        bird.annual_rebate_rate = annual;
        let bird = PhoenixOption::new(bird).unwrap();
        let mut binary = BinarySnowballTerms::new(
            0.06,
            start(),
            end(),
            snow.autocall_observations().to_vec(),
            TradingCalendar::weekdays(),
        );
        binary.rebate_rate = flat;
        binary.annual_rebate_rate = annual;
        let binary = BinarySnowballOption::new(binary).unwrap();
        let mut ternary = TernarySnowballTerms::new(
            0.06,
            0.01,
            80.0,
            start(),
            end(),
            snow.autocall_observations().to_vec(),
            KnockInObservation::AtExpiry,
            TradingCalendar::weekdays(),
        );
        ternary.rebate_rate = flat;
        ternary.annual_rebate_rate = annual;
        let ternary = TernarySnowballOption::new(ternary).unwrap();
        for (spot, snow_payoff, phoenix_payoff, binary_coupon, ternary_coupon) in [
            (70.0, 0.7, 0.7, 0.06, 0.01),
            (90.0, 1.0 + 0.06 * years, 1.01, 0.06, 0.06),
            (120.0, 1.0 + 0.08 * years, 1.01, 0.08, 0.08),
        ] {
            let context = market(spot, end(), 0.3);
            assert_prices(&snow, &context, snow_payoff + rebate);
            assert_prices(&bird, &context, phoenix_payoff + rebate);
            assert_prices(&binary, &context, 1.0 + binary_coupon * years + rebate);
            assert_prices(&ternary, &context, 1.0 + ternary_coupon * years + rebate);
        }
    }
}

#[test]
fn rebates_settle_once_at_early_knockout_and_zero_lifetime() {
    let middle = start().add_days(33).unwrap();
    let mut terms = snowball();
    terms.rebate_rate = 0.02;
    terms.annual_rebate_rate = 0.06;
    terms.autocall_observations.insert(
        0,
        AutocallableObservation::snowball(middle, 110.0, 0.08).unwrap(),
    );
    let note = SnowballOption::new(terms).unwrap();
    assert_prices(
        &note,
        &market(120.0, middle, 0.3),
        1.02 + 0.14 * 33.0 / 365.0,
    );
    let settled = note
        .replace(SnowballOptionChanges {
            barrier_state: Some(Some(AutocallableBarrierState::KnockedOut)),
            ..Default::default()
        })
        .unwrap();
    assert_prices(
        &settled,
        &market(120.0, middle.add_micros(1).unwrap(), 0.3),
        0.0,
    );
    let immediate = note
        .replace(SnowballOptionChanges {
            effective_time: Some(end()),
            autocall_observations: Some(vec![note.autocall_observations()[1]]),
            ..Default::default()
        })
        .unwrap();
    for spot in [90.0, 120.0] {
        assert_prices(&immediate, &market(spot, end(), 0.3), 1.02);
    }
}

fn assert_rebate_increment<T: AutocallableTerms>(
    plain: &AutocallableNote<T>,
    rebated: &AutocallableNote<T>,
) {
    let context = market(80.0, start(), 0.3);
    let expected = (0.02 + 0.06 * 90.25 / 365.0) * (-0.04_f64 * 90.25 / 365.0).exp();
    let mc = MonteCarloAutocallableEngine::new(MonteCarloSettings {
        path_count: 4096,
        seed: Some(42),
        ..Default::default()
    })
    .unwrap();
    for engine in [
        &QuadratureAutocallableEngine::default() as &dyn PricingEngine<AutocallableNote<T>>,
        &FiniteDifferenceAutocallableEngine::default(),
        &mc,
    ] {
        let increment =
            engine.price(rebated, &context).unwrap() - engine.price(plain, &context).unwrap();
        assert!(
            (increment - expected).abs() < 2e-6,
            "{increment} != {expected}"
        );
    }
}

#[test]
fn stochastic_terminal_paths_discount_rebates_independently_of_downside() {
    for lower in [0.0, 60.0] {
        let mut terms = snowball();
        terms.lower_strike = lower;
        terms.principal_redemption_ratio = 0.4;
        let plain = SnowballOption::new(terms).unwrap();
        let rebated = plain
            .replace(SnowballOptionChanges {
                rebate_rate: Some(0.02),
                annual_rebate_rate: Some(0.06),
                ..Default::default()
            })
            .unwrap();
        assert_rebate_increment(&plain, &rebated);
        let mut terms = phoenix();
        terms.lower_strike = lower;
        terms.principal_redemption_ratio = 0.4;
        let plain = PhoenixOption::new(terms).unwrap();
        let rebated = plain
            .replace(PhoenixOptionChanges {
                rebate_rate: Some(0.02),
                annual_rebate_rate: Some(0.06),
                ..Default::default()
            })
            .unwrap();
        assert_rebate_increment(&plain, &rebated);
    }
}

#[test]
fn rebate_validation_and_replacement_preserve_the_original() {
    let note = SnowballOption::new(snowball()).unwrap();
    for value in [-0.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for changes in [
            SnowballOptionChanges {
                rebate_rate: Some(value),
                ..Default::default()
            },
            SnowballOptionChanges {
                annual_rebate_rate: Some(value),
                ..Default::default()
            },
        ] {
            assert!(matches!(note.replace(changes), Err(Error::InvalidInput(_))));
        }
    }
    let edited = note
        .replace(SnowballOptionChanges {
            rebate_rate: Some(2.0),
            annual_rebate_rate: Some(3.0),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        edited.replace(SnowballOptionChanges::default()).unwrap(),
        edited
    );
    assert_eq!(note, SnowballOption::new(snowball()).unwrap());
    let quoted = edited
        .with_coupon(0.2, CouponQuoteConvention::ShiftMaturityCoupon)
        .unwrap();
    assert!((quoted.terms().rebate_rate - 2.0).abs() < f64::EPSILON);
    assert!((quoted.terms().annual_rebate_rate - 3.0).abs() < f64::EPSILON);
}

#[test]
fn stochastic_early_exit_rebates_match_an_independent_digital_portfolio() {
    let middle = start().add_days(33).unwrap();
    let mut terms = BinarySnowballTerms::new(
        0.0,
        start(),
        end(),
        vec![AutocallableObservation::snowball(middle, 110.0, 0.0).unwrap()],
        TradingCalendar::weekdays(),
    );
    terms.rebate_rate = 0.02;
    terms.annual_rebate_rate = 0.06;
    let note = BinarySnowballOption::new(terms).unwrap();
    let context = market(100.0, start(), 0.3);
    let digital =
        DigitalOption::cash_or_nothing(OptionType::Call, 110.0, 1.0, start(), middle).unwrap();
    let digital_value = AnalyticDigitalEngine.price(&digital, &context).unwrap();
    let early_years = 33.0 / 365.0;
    let expiry_years = 90.25 / 365.0;
    let early_discount = (-0.04_f64 * early_years).exp();
    let maturity_value = (1.02 + 0.06 * expiry_years) * (-0.04_f64 * expiry_years).exp();
    let expected = maturity_value
        + digital_value * (1.02 + 0.06 * early_years - maturity_value / early_discount);
    for (engine, tolerance) in [
        (
            &QuadratureAutocallableEngine::default() as &dyn PricingEngine<BinarySnowballOption>,
            1e-8,
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
    assert!((estimate.price() - expected).abs() < 5.0 * estimate.standard_error().unwrap() + 1e-7);
}
