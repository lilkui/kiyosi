use kiyosi::{
    AnalyticBarrierEngine, AnalyticBinaryBarrierEngine, AnalyticDoubleBarrierEngine,
    AnalyticDoubleTouchEngine, BarrierOption, BarrierOptionTerms, BarrierType, BinaryBarrierOption,
    BinaryBarrierTerms, BlackScholesMertonParameters, DoubleBarrierOption,
    DoubleBarrierOptionTerms, DoubleBarrierType, DoubleTouchOption, DoubleTouchOptionTerms,
    PricingContext, RebateTiming, SettlementTiming, TouchOption, TouchOptionTerms,
};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request, family: &str) -> Result<Price, Failure> {
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    if expiry <= valuation {
        return Err("benchmark expiry must follow valuation".into());
    }
    let spot: f64 = request.number("spot")?;
    let rate: f64 = request.number("rate")?;
    let dividend: f64 = request.number("dividend")?;
    let volatility = request.number("volatility")?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility)?,
        spot,
        valuation,
    )?;
    let activation = request.take("activation")?;
    let knock_in = match activation.as_str() {
        "in" => true,
        "out" => false,
        _ => return Err("activation must be in or out".into()),
    };
    if family.starts_with("double_") {
        let lower: f64 = request.number("lower")?;
        let upper: f64 = request.number("upper")?;
        if !(lower < spot && spot < upper) {
            return Err("benchmark requires initially untouched barriers".into());
        }
        let barrier_type = if knock_in {
            DoubleBarrierType::KnockIn
        } else {
            DoubleBarrierType::KnockOut
        };
        if family == "double_barrier_bsm" {
            let side = request.option()?;
            let strike: f64 = request.number("strike")?;
            if !(lower < strike && strike < upper) {
                return Err("strike must be between barriers".into());
            }
            let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(
                side,
                strike,
                valuation,
                expiry,
                lower,
                upper,
                barrier_type,
            ))?;
            return Ok(Box::new(move || {
                AnalyticDoubleBarrierEngine.price(
                    std::hint::black_box(&option),
                    std::hint::black_box(&context),
                )
            }));
        }
        let payout = request.number("payout")?;
        let option = DoubleTouchOption::cash(
            DoubleTouchOptionTerms::new(valuation, expiry, lower, upper, barrier_type),
            payout,
        )?;
        return Ok(Box::new(move || {
            AnalyticDoubleTouchEngine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }));
    }
    let barrier: f64 = request.number("barrier")?;
    let direction = request.take("direction")?;
    let barrier_type = match (direction.as_str(), knock_in) {
        ("up", true) => BarrierType::UpAndIn,
        ("up", false) => BarrierType::UpAndOut,
        ("down", true) => BarrierType::DownAndIn,
        ("down", false) => BarrierType::DownAndOut,
        _ => return Err("direction must be up or down".into()),
    };
    if !(if direction == "up" {
        spot < barrier
    } else {
        spot > barrier
    }) {
        return Err("benchmark requires initially untouched barriers".into());
    }
    if family == "single_barrier_bsm" {
        let side = request.option()?;
        let strike = request.number("strike")?;
        let mut terms =
            BarrierOptionTerms::new(side, strike, valuation, expiry, barrier, barrier_type);
        terms.rebate = request.number("rebate")?;
        terms.rebate_timing = if knock_in {
            RebateTiming::AtExpiry
        } else {
            RebateTiming::AtHit
        };
        let option = BarrierOption::new(terms)?;
        return Ok(Box::new(move || {
            AnalyticBarrierEngine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }));
    }
    let payoff = request.take("payoff")?;
    let payout = match payoff.as_str() {
        "cash" => Some(request.number("payout")?),
        "asset" => None,
        _ => return Err("payoff must be cash or asset".into()),
    };
    if family == "binary_barrier_bsm" {
        let side = request.option()?;
        let strike = request.number("strike")?;
        let terms = BinaryBarrierTerms::new(side, strike, valuation, expiry, barrier, barrier_type);
        let option = if let Some(payout) = payout {
            BinaryBarrierOption::cash(terms, payout)?
        } else {
            BinaryBarrierOption::asset(terms)?
        };
        return Ok(Box::new(move || {
            AnalyticBinaryBarrierEngine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }));
    }
    let mut terms = TouchOptionTerms::new(valuation, expiry, barrier, barrier_type);
    terms.settlement_timing = match request.take("settlement")?.as_str() {
        "expiry" => SettlementTiming::AtExpiry,
        "hit" if knock_in => SettlementTiming::AtHit,
        _ => return Err("unmatched settlement".into()),
    };
    let option = if let Some(payout) = payout {
        TouchOption::cash(terms, payout)?
    } else {
        TouchOption::asset(terms)?
    };
    Ok(Box::new(move || {
        AnalyticBinaryBarrierEngine.price(
            std::hint::black_box(&option),
            std::hint::black_box(&context),
        )
    }))
}
