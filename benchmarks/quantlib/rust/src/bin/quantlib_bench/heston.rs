use kiyosi::{
    AmericanOption, AnalyticHestonEuropeanEngine, EuropeanOption,
    FiniteDifferenceHestonVanillaEngine, HestonParameterInputs, HestonParameters, PricingContext,
    PricingEngine,
    engines::{HestonFiniteDifferenceSettings, HestonIntegrationSettings},
};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request, family: &str) -> Result<Price, Failure> {
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
    let model = HestonParameters::new(HestonParameterInputs {
        risk_free_rate: rate.into(),
        dividend_yield: dividend.into(),
        initial_variance: request.number("initial_variance")?,
        long_run_variance: request.number("long_run_variance")?,
        mean_reversion_speed: request.number("mean_reversion_speed")?,
        vol_of_vol: request.number("vol_of_vol")?,
        correlation: request.number("correlation")?,
    })?;
    let context = PricingContext::new(model, spot, valuation)?;
    if family == "heston_analytic" {
        let option = EuropeanOption::new(option_type, strike, valuation, expiry)?;
        let engine = AnalyticHestonEuropeanEngine::new(HestonIntegrationSettings {
            absolute_tolerance: request.number("absolute_tolerance")?,
            relative_tolerance: request.number("relative_tolerance")?,
            max_evaluations: request.number("max_evaluations")?,
        })?;
        return Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }));
    }
    let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings {
        asset_step_count: request.number("asset_steps")?,
        variance_step_count: request.number("variance_steps")?,
        time_step_count: request.number("time_steps")?,
        asset_upper_boundary: Some(request.number("asset_upper")?),
        variance_upper_boundary: Some(request.number("variance_upper")?),
    })?;
    if family == "heston_fd_american" {
        let option = AmericanOption::new(option_type, strike, valuation, expiry)?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    } else {
        let option = EuropeanOption::new(option_type, strike, valuation, expiry)?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    }
}
