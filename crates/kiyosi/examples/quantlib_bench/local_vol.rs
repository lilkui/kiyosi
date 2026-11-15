use super::common::{Failure, Price, Request};
use kiyosi::{LocalVolatilityParameters, LocalVolatilitySurface, PricingContext};

pub fn prepare(request: &mut Request) -> Result<Price, Failure> {
    let valuation = request.date("valuation")?;
    let expiry = request.date("expiry")?;
    let surface = LocalVolatilitySurface::new(
        vec![valuation, expiry],
        vec![
            request.number("surface_low")?,
            request.number("surface_high")?,
        ],
        vec![
            vec![
                request.number("surface_start_low")?,
                request.number("surface_start_high")?,
            ],
            vec![
                request.number("surface_end_low")?,
                request.number("surface_end_high")?,
            ],
        ],
    )?;
    match request.take("operation")?.as_str() {
        "surface_point" => {
            let time = request.date("query_date")?;
            let spot = request.number("query_spot")?;
            surface.volatility(time, spot)?;
            Ok(Box::new(move || surface.volatility(time, spot)))
        }
        "price" => {
            let context = PricingContext::new(
                LocalVolatilityParameters::new(
                    request.number::<f64>("rate")?,
                    request.number::<f64>("dividend")?,
                    surface,
                )?,
                request.number("spot")?,
                valuation,
            )?;
            super::fd::prepare_product(request, context, valuation, expiry)
        }
        value => Err(format!("invalid local volatility operation: {value}").into()),
    }
}
