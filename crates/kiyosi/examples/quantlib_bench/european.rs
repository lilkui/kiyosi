use kiyosi::{
    AnalyticEuropeanEngine, BlackScholesMertonParameters, EuropeanOption, PricingContext,
};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request) -> Result<Price, Failure> {
    let option_type = request.option()?;
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    if expiry <= valuation {
        return Err("benchmark expiry must follow valuation".into());
    }
    let spot = request.number("spot")?;
    let strike = request.number("strike")?;
    let rate: f64 = request.number("rate")?;
    let dividend: f64 = request.number("dividend")?;
    let volatility = request.number("volatility")?;
    let option = EuropeanOption::new(option_type, strike, valuation, expiry)?;
    let model = BlackScholesMertonParameters::new(rate, dividend, volatility)?;
    let context = PricingContext::new(model, spot, valuation)?;
    let engine = AnalyticEuropeanEngine;
    Ok(Box::new(move || {
        engine.price(
            std::hint::black_box(&option),
            std::hint::black_box(&context),
        )
    }))
}
