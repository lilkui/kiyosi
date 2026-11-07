mod support;
use support::dated;

use kiyosi::*;

#[test]
fn settled_binary_and_touch_payoffs_do_not_require_future_curve_coverage() -> Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let expiry = start.add_days(365)?;
    let curve = RateTermStructure::new(start, vec![start.add_days(31)?], vec![1.0])?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(curve.clone(), curve, 0.2)?,
        100.0,
        start.add_days(1)?,
    )?;
    let mut binary = BinaryBarrierTerms::new(
        OptionType::Call,
        100.0,
        start,
        expiry,
        120.0,
        BarrierType::UpAndOut,
    );
    binary.touch_state = Some(BarrierTouchState::Touched);
    let binary = BinaryBarrierOption::cash(binary, 10.0)?;
    let mut touch = TouchOptionTerms::new(start, expiry, 120.0, BarrierType::UpAndIn);
    touch.touch_state = Some(BarrierTouchState::Touched);
    touch.settlement_timing = SettlementTiming::AtHit;
    let touch = TouchOption::cash(touch, 10.0)?;
    let mut double =
        DoubleTouchOptionTerms::new(start, expiry, 80.0, 120.0, DoubleBarrierType::KnockIn);
    double.touch_state = Some(BarrierTouchState::Touched);
    double.settlement_timing = SettlementTiming::AtHit;
    let double = DoubleTouchOption::cash(double, 10.0)?;
    assert_eq!(
        AnalyticBinaryBarrierEngine.price(&dated(&binary, &context), &context)?,
        0.0
    );
    assert_eq!(
        AnalyticBinaryBarrierEngine.price(&dated(&touch, &context), &context)?,
        0.0
    );
    assert_eq!(
        AnalyticDoubleTouchEngine.price(&dated(&double, &context), &context)?,
        0.0
    );
    for value in [
        AnalyticBinaryBarrierEngine
            .price(&dated(&binary.replace_history(None), &context), &context),
        AnalyticBinaryBarrierEngine.price(
            &dated(
                &touch.replace(TouchOptionChanges {
                    settlement_timing: Some(SettlementTiming::AtExpiry),
                    ..Default::default()
                })?,
                &context,
            ),
            &context,
        ),
        AnalyticDoubleTouchEngine.price(
            &dated(
                &double.replace(DoubleTouchOptionChanges {
                    settlement_timing: Some(SettlementTiming::AtExpiry),
                    ..Default::default()
                })?,
                &context,
            ),
            &context,
        ),
    ] {
        assert!(matches!(value, Err(Error::InvalidInput(_))));
    }
    Ok(())
}
