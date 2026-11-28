use kiyosi::PricingEngine;
use kiyosi::{
    AnalyticGeometricAveragePriceEngine, ArithmeticAveragePriceOption,
    BlackScholesMertonParameters, GeometricAveragePriceOption, PricingContext,
    TurnbullWakemanArithmeticAveragePriceEngine,
};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request, arithmetic: bool) -> Result<Price, Failure> {
    let option_type = request.option()?;
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    let averaging_start = request.date("averaging_start")?;
    if expiry <= valuation || averaging_start != valuation {
        return Err(
            "Asian benchmark requires future expiry and averaging_start equal to valuation".into(),
        );
    }
    let spot: f64 = request.number("spot")?;
    let strike: f64 = request.number("strike")?;
    let rate: f64 = request.number("rate")?;
    let dividend: f64 = request.number("dividend")?;
    let volatility: f64 = request.number("volatility")?;
    if !spot.is_finite()
        || spot <= 0.0
        || !strike.is_finite()
        || strike <= 0.0
        || !volatility.is_finite()
        || volatility <= 0.0
    {
        return Err("positive finite spot, strike and volatility required".into());
    }
    let carry = rate - dividend;
    if arithmetic
        && (carry.abs() < 1e-6
            || (carry + volatility * volatility).abs() < 1e-6
            || (2.0 * carry + volatility * volatility).abs() < 1e-6)
    {
        return Err(
            "arithmetic Asian benchmark requires ordinary carry and nonsingular Levy denominators"
                .into(),
        );
    }
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility)?,
        spot,
        valuation,
    )?;
    if arithmetic {
        let option = ArithmeticAveragePriceOption::new(
            option_type,
            strike,
            valuation,
            averaging_start,
            expiry,
            None,
        )?;
        Ok(Box::new(move || {
            TurnbullWakemanArithmeticAveragePriceEngine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    } else {
        let option = GeometricAveragePriceOption::new(
            option_type,
            strike,
            valuation,
            averaging_start,
            expiry,
            None,
        )?;
        Ok(Box::new(move || {
            AnalyticGeometricAveragePriceEngine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    }
}
