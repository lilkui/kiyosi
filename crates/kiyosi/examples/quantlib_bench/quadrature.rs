use kiyosi::{
    BlackScholesMertonParameters, DigitalOption, EuropeanOption, PricingContext, PricingEngine,
    QuadratureDigitalEngine, QuadratureSettings, QuadratureVanillaEngine,
    QuadratureVanillaSettings,
};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request, digital: bool) -> Result<Price, Failure> {
    let side = request.option()?;
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    if expiry <= valuation {
        return Err("benchmark expiry must follow valuation".into());
    }
    let spot = request.number("spot")?;
    let strike = request.number("strike")?;
    let model = BlackScholesMertonParameters::new(
        request.number::<f64>("rate")?,
        request.number::<f64>("dividend")?,
        request.number("volatility")?,
    )?;
    let context = PricingContext::new(model, spot, valuation)?;
    let settings = QuadratureSettings {
        asset_step_count: request.number("asset_step_count")?,
        integration_standard_deviations: request.number("integration_standard_deviations")?,
    };
    if digital {
        let option = match request.take("payoff")?.as_str() {
            "cash" => DigitalOption::cash_or_nothing(
                side,
                strike,
                request.number("cash")?,
                valuation,
                expiry,
            )?,
            "asset" => DigitalOption::asset_or_nothing(side, strike, valuation, expiry)?,
            value => return Err(format!("invalid digital payoff: {value}").into()),
        };
        let engine = QuadratureDigitalEngine::new(settings)?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    } else {
        let option = EuropeanOption::new(side, strike, valuation, expiry)?;
        let engine = QuadratureVanillaEngine::new(QuadratureVanillaSettings {
            asset_step_count: settings.asset_step_count,
            time_step_count: request.number("time_step_count")?,
            integration_standard_deviations: settings.integration_standard_deviations,
        })?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    }
}
