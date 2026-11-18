//! Offline domain, space and time refinement for the benchmark American put.

use kiyosi::{
    AmericanOption, BlackScholesMertonParameters, Date, FiniteDifferenceScheme,
    FiniteDifferenceSettings, FiniteDifferenceVanillaEngine, OptionType, PricingContext,
    PricingEngine,
};

fn main() -> kiyosi::Result<()> {
    let at = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let option = AmericanOption::new(OptionType::Put, 100.0, at, end)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
        100.0,
        at,
    )?;
    for (space, time, upper) in [
        (1600, 6400, 400.0),
        (3200, 6400, 400.0),
        (6400, 6400, 400.0),
        (6400, 12800, 400.0),
        (6400, 25600, 400.0),
        (8000, 25600, 500.0),
        (9600, 25600, 600.0),
    ] {
        let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_step_count: space,
            time_step_count: time,
            scheme: FiniteDifferenceScheme::Rannacher,
            asset_upper_boundary: Some(upper),
        })?;
        println!(
            "{{\"space\":{space},\"time\":{time},\"upper\":{upper},\"price\":{:.17}}}",
            engine.price(&option, &context)?
        );
    }
    Ok(())
}
