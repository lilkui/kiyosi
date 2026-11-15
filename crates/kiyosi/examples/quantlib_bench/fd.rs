use super::common::{Failure, Price, Request};
use kiyosi::{
    AmericanOption, BarrierOption, BarrierOptionTerms, BarrierType, BinaryBarrierOption,
    BinaryBarrierTerms, BlackScholesMertonParameters, DigitalOption, EuropeanOption,
    FiniteDifferenceBarrierEngine, FiniteDifferenceBinaryBarrierEngine,
    FiniteDifferenceDigitalEngine, FiniteDifferenceScheme, FiniteDifferenceSettings,
    FiniteDifferenceVanillaEngine, PricingContext, PricingEngine, SettlementTiming, Timestamp,
    TouchOption, TouchOptionTerms,
};

pub fn prepare(request: &mut Request) -> Result<Price, Failure> {
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    let spot: f64 = request.number("spot")?;
    let volatility: f64 = request.number("volatility")?;
    if !volatility.is_finite() || volatility <= 0.0 {
        return Err("positive volatility required".into());
    }
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(
            request.number::<f64>("rate")?,
            request.number::<f64>("dividend")?,
            volatility,
        )?,
        spot,
        valuation,
    )?;
    prepare_product(request, context, valuation, expiry)
}

pub fn prepare_product(
    request: &mut Request,
    context: PricingContext,
    valuation: Timestamp,
    expiry: Timestamp,
) -> Result<Price, Failure> {
    if expiry <= valuation {
        return Err("benchmark expiry must follow valuation".into());
    }
    let settings = FiniteDifferenceSettings {
        asset_step_count: request.number("space")?,
        time_step_count: request.number("time")?,
        scheme: match request.take("scheme")?.as_str() {
            "rannacher" => FiniteDifferenceScheme::Rannacher,
            "crank_nicolson" => FiniteDifferenceScheme::CrankNicolson,
            "implicit_euler" => FiniteDifferenceScheme::ImplicitEuler,
            value => return Err(format!("invalid FD scheme: {value}").into()),
        },
        asset_upper_boundary: Some(request.number("upper")?),
    };
    let product = request.take("product")?;
    if product == "vanilla" {
        let option_type = request.option()?;
        let strike = request.number("strike")?;
        let engine = FiniteDifferenceVanillaEngine::new(settings)?;
        return match request.take("exercise")?.as_str() {
            "european" => {
                let option = EuropeanOption::new(option_type, strike, valuation, expiry)?;
                Ok(Box::new(move || engine.price(&option, &context)))
            }
            "american" => {
                let option = AmericanOption::new(option_type, strike, valuation, expiry)?;
                Ok(Box::new(move || engine.price(&option, &context)))
            }
            value => Err(format!("invalid exercise: {value}").into()),
        };
    }
    let payout = if product == "barrier" {
        "vanilla".to_owned()
    } else {
        request.take("payout")?
    };
    let cash = match payout.as_str() {
        "cash" => Some(request.number("cash")?),
        "asset" | "vanilla" => None,
        value => return Err(format!("invalid payout: {value}").into()),
    };
    if product == "digital" {
        let option_type = request.option()?;
        let strike = request.number("strike")?;
        let option = if let Some(cash) = cash {
            DigitalOption::cash_or_nothing(option_type, strike, cash, valuation, expiry)?
        } else {
            DigitalOption::asset_or_nothing(option_type, strike, valuation, expiry)?
        };
        let engine = FiniteDifferenceDigitalEngine::new(settings)?;
        return Ok(Box::new(move || engine.price(&option, &context)));
    }
    let barrier: f64 = request.number("barrier")?;
    let barrier_type = match request.take("barrier_type")?.as_str() {
        "down_out" => BarrierType::DownAndOut,
        "down_in" => BarrierType::DownAndIn,
        "up_out" => BarrierType::UpAndOut,
        "up_in" => BarrierType::UpAndIn,
        value => return Err(format!("invalid barrier_type: {value}").into()),
    };
    let up = matches!(barrier_type, BarrierType::UpAndIn | BarrierType::UpAndOut);
    if barrier <= 0.0
        || !barrier.is_finite()
        || (if up {
            context.spot_price() >= barrier
        } else {
            context.spot_price() <= barrier
        })
    {
        return Err("positive initially untouched barrier required".into());
    }
    if product == "touch" {
        let mut terms = TouchOptionTerms::new(valuation, expiry, barrier, barrier_type);
        terms.settlement_timing = match request.take("settlement")?.as_str() {
            "at_hit" if matches!(barrier_type, BarrierType::UpAndIn | BarrierType::DownAndIn) => {
                SettlementTiming::AtHit
            }
            "at_expiry"
                if !matches!(barrier_type, BarrierType::UpAndIn | BarrierType::DownAndIn) =>
            {
                SettlementTiming::AtExpiry
            }
            _ => return Err("FD subset requires one-touch at_hit or no-touch at_expiry".into()),
        };
        let option = if let Some(cash) = cash {
            TouchOption::cash(terms, cash)?
        } else {
            TouchOption::asset(terms)?
        };
        let engine = FiniteDifferenceBinaryBarrierEngine::new(settings)?;
        return Ok(Box::new(move || engine.price(&option, &context)));
    }
    let option_type = request.option()?;
    let strike = request.number("strike")?;
    match product.as_str() {
        "barrier" => {
            let option = BarrierOption::new(BarrierOptionTerms::new(
                option_type,
                strike,
                valuation,
                expiry,
                barrier,
                barrier_type,
            ))?;
            let engine = FiniteDifferenceBarrierEngine::new(settings)?;
            Ok(Box::new(move || engine.price(&option, &context)))
        }
        "binary_barrier" => {
            let terms = BinaryBarrierTerms::new(
                option_type,
                strike,
                valuation,
                expiry,
                barrier,
                barrier_type,
            );
            let option = if let Some(cash) = cash {
                BinaryBarrierOption::cash(terms, cash)?
            } else {
                BinaryBarrierOption::asset(terms)?
            };
            let engine = FiniteDifferenceBinaryBarrierEngine::new(settings)?;
            Ok(Box::new(move || engine.price(&option, &context)))
        }
        value => Err(format!("invalid FD product: {value}").into()),
    }
}
