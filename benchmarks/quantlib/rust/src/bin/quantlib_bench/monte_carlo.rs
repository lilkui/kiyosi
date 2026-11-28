use kiyosi::PricingEngine;
use kiyosi::{
    BarrierOption, BarrierOptionTerms, BarrierType, BlackScholesMertonParameters, EuropeanOption,
    MonteCarloBarrierEngine, MonteCarloGridSettings, MonteCarloSampling, MonteCarloSettings,
    MonteCarloVanillaEngine, PricingContext, configure_monte_carlo_pool,
};

use super::common::{Failure, Price, Request};

pub(super) fn prepare_workers(request: &mut Request) -> Result<i32, Failure> {
    let workers = request.number("workers")?;
    if workers < 1 {
        return Err("MC benchmark workers must be positive".into());
    }
    if workers > 1 {
        let available = std::thread::available_parallelism()?.get();
        if usize::try_from(workers)? > available {
            return Err(format!(
                "MC benchmark requests {workers} workers but only {available} CPUs are available"
            )
            .into());
        }
        configure_monte_carlo_pool(workers)?;
    }
    Ok(workers)
}

pub fn prepare(request: &mut Request, barrier: bool) -> Result<Price, Failure> {
    let side = request.option()?;
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    if expiry <= valuation {
        return Err("benchmark expiry must follow valuation".into());
    }
    let spot: f64 = request.number("spot")?;
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
    if barrier && seed == 5 {
        return Err(
            "barrier seed 5 couples QuantLib Gaussian and auxiliary crossing streams".into(),
        );
    }
    let workers = prepare_workers(request)?;
    if seed == 0
        || request.take("sampling")? != "pseudo_random"
        || request.take("antithetic")? != "false"
        || request.take("control_variate")? != "false"
    {
        return Err("MC benchmark requires a nonzero seed, pseudorandom sampling and no antithetic/control variate".into());
    }
    let sampling = MonteCarloSettings {
        path_count: request.number("path_count")?,
        seed: Some(u64::from(seed)),
        max_active_workers: workers,
        sampling: MonteCarloSampling::PseudoRandom,
        antithetic: false,
    };
    if barrier {
        let level: f64 = request.number("barrier")?;
        let direction = request.take("direction")?;
        let activation = request.take("activation")?;
        let kind = match (direction.as_str(), activation.as_str()) {
            ("down", "out") => BarrierType::DownAndOut,
            ("down", "in") => BarrierType::DownAndIn,
            ("up", "out") => BarrierType::UpAndOut,
            ("up", "in") => BarrierType::UpAndIn,
            _ => return Err("invalid barrier direction/activation".into()),
        };
        if !(if direction == "up" {
            spot < level
        } else {
            spot > level
        }) || request.number::<f64>("rebate")? != 0.0
        {
            return Err("MC benchmark requires an untouched zero-rebate barrier".into());
        }
        let option = BarrierOption::new(BarrierOptionTerms::new(
            side, strike, valuation, expiry, level, kind,
        ))?;
        let engine = MonteCarloBarrierEngine::new(sampling)?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    } else {
        let option = EuropeanOption::new(side, strike, valuation, expiry)?;
        // European BSM uses exact terminal sampling; the public grid setting is unused.
        let engine = MonteCarloVanillaEngine::new(MonteCarloGridSettings {
            sampling,
            step_count: 1,
        })?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    }
}
