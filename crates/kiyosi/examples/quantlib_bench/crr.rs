use kiyosi::{
    AmericanOption, BarrierOption, BarrierOptionTerms, BarrierType, BinomialSettings,
    BlackScholesMertonParameters, CoxRossRubinsteinBarrierEngine, CoxRossRubinsteinVanillaEngine,
    EuropeanOption, PricingContext,
};

use super::common::{Failure, Price, Request};

pub fn prepare(request: &mut Request, barrier: bool) -> Result<Price, Failure> {
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
    let settings = BinomialSettings {
        step_count: request.number("step_count")?,
    };
    if barrier {
        let level = request.number("barrier")?;
        let kind = match request.take("barrier_type")?.as_str() {
            "down_out" => BarrierType::DownAndOut,
            "down_in" => BarrierType::DownAndIn,
            "up_out" => BarrierType::UpAndOut,
            "up_in" => BarrierType::UpAndIn,
            value => return Err(format!("invalid barrier_type: {value}").into()),
        };
        let option = BarrierOption::new(BarrierOptionTerms::new(
            side, strike, valuation, expiry, level, kind,
        ))?;
        let engine = CoxRossRubinsteinBarrierEngine::new(settings)?;
        Ok(Box::new(move || {
            engine.price(
                std::hint::black_box(&option),
                std::hint::black_box(&context),
            )
        }))
    } else {
        let engine = CoxRossRubinsteinVanillaEngine::new(settings)?;
        match request.take("exercise")?.as_str() {
            "european" => {
                let option = EuropeanOption::new(side, strike, valuation, expiry)?;
                Ok(Box::new(move || {
                    engine.price(
                        std::hint::black_box(&option),
                        std::hint::black_box(&context),
                    )
                }))
            }
            "american" => {
                let option = AmericanOption::new(side, strike, valuation, expiry)?;
                Ok(Box::new(move || {
                    engine.price(
                        std::hint::black_box(&option),
                        std::hint::black_box(&context),
                    )
                }))
            }
            value => Err(format!("invalid exercise: {value}").into()),
        }
    }
}
