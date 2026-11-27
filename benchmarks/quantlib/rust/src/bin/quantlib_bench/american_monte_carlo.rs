use kiyosi::{
    AmericanOption, BlackScholesMertonParameters, MonteCarloGridSettings, MonteCarloSampling,
    MonteCarloSettings, MonteCarloVanillaEngine, PricingContext,
};

use super::common::{Failure, Price, Request};
use super::monte_carlo::prepare_workers;

pub fn prepare(request: &mut Request) -> Result<Price, Failure> {
    let side = request.option()?;
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
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(rate, dividend, volatility)?,
        spot,
        valuation,
    )?;
    let seed: u32 = request.number("seed")?;
    let workers = prepare_workers(request)?;
    let step_count = request.number("step_count")?;
    let path_count = request.number("path_count")?;
    if seed == 0
        || !(2..=10_000).contains(&step_count)
        || !(1..=10_000_000).contains(&path_count)
        || request.take("sampling")? != "pseudo_random"
        || request.take("antithetic")? != "false"
        || request.take("control_variate")? != "false"
        || request.take("basis")? != "quadratic_monomial"
    {
        return Err("American MC requires a nonzero seed, pseudorandom sampling, quadratic monomial, no antithetic/control variate and bounded paths/steps".into());
    }
    let engine = MonteCarloVanillaEngine::new(MonteCarloGridSettings {
        sampling: MonteCarloSettings {
            path_count,
            seed: Some(u64::from(seed)),
            max_active_workers: workers,
            sampling: MonteCarloSampling::PseudoRandom,
            antithetic: false,
        },
        step_count,
    })?;
    let option = AmericanOption::new(side, strike, valuation, expiry)?;
    Ok(Box::new(move || {
        engine.price(
            std::hint::black_box(&option),
            std::hint::black_box(&context),
        )
    }))
}
