use kiyosi::PricingEngine;
use kiyosi::{AnalyticDigitalEngine, BlackScholesMertonParameters, DigitalOption, PricingContext};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request) -> Result<Price, Failure> {
    let option_type = request.option()?;
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    if expiry <= valuation {
        return Err("benchmark expiry must follow valuation".into());
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
    let option = match request.take("payout")?.as_str() {
        "cash" => DigitalOption::cash_or_nothing(
            option_type,
            strike,
            request.number("cash")?,
            valuation,
            expiry,
        )?,
        "asset" => DigitalOption::asset_or_nothing(option_type, strike, valuation, expiry)?,
        value => return Err(format!("invalid payout: {value}").into()),
    };
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility)?,
        spot,
        valuation,
    )?;
    Ok(Box::new(move || {
        AnalyticDigitalEngine.price(
            std::hint::black_box(&option),
            std::hint::black_box(&context),
        )
    }))
}
