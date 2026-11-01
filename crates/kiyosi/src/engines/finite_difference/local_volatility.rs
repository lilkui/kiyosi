use super::*;
use crate::{LocalVolatilityParameters, ModelParameters};

pub(super) fn validate_request(context: &PricingContext, request: GreeksRequest) -> Result<()> {
    if local_model(context).is_some()
        && Greek::ALL
            .into_iter()
            .any(|g| request.has(g) && !matches!(g, Greek::Delta | Greek::Gamma))
    {
        return Err(Error::UnsupportedOperation(
            "local volatility supports only delta and gamma".into(),
        ));
    }
    Ok(())
}

pub(super) fn price(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    model: &LocalVolatilityParameters,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
) -> Result<PricingResult> {
    validate_request(context, request)?;
    Ok(super::native_model(
        option,
        payoff,
        context,
        settings,
        request,
        Default::default(),
        LocalDiffusion::new(context, model.surface()),
    )?
    .0)
}

pub(super) fn local_model(context: &PricingContext) -> Option<&LocalVolatilityParameters> {
    match context.model_parameters() {
        ModelParameters::LocalVolatility(m) => Some(m),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{hint::black_box, time::Instant};

    #[test]
    #[ignore = "release timing comparison; run alone on an idle machine"]
    fn concrete_and_dispatched_local_volatility_benchmark() {
        let start = crate::Date::new(2025, 1, 1).unwrap();
        let end = crate::Date::new(2026, 1, 1).unwrap();
        let surface = crate::LocalVolatilitySurface::new(
            vec![start.into(), end.into()],
            vec![80.0, 120.0],
            vec![vec![0.3, 0.15], vec![0.35, 0.2]],
        )
        .unwrap();
        let context = PricingContext::new(
            LocalVolatilityParameters::new(0.05, 0.02, surface).unwrap(),
            100.0,
            start.into(),
        )
        .unwrap();
        let model = local_model(&context).unwrap();
        let option = EuropeanOption::new(OptionType::Call, 100.0, start, end).unwrap();
        let engine = FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings {
            asset_step_count: 400,
            time_step_count: 200,
            asset_upper_boundary: Some(400.0),
            ..Default::default()
        })
        .unwrap();
        let concrete = || {
            let option = black_box(&option);
            price(
                option,
                Payoff::vanilla(option.option_type(), option.strike(), false),
                black_box(&context),
                black_box(model),
                black_box(engine.settings()),
                GreeksRequest::default(),
            )
            .unwrap()
            .price()
        };
        let dispatched = || {
            black_box(engine)
                .price(black_box(&option), black_box(&context))
                .unwrap()
        };
        assert_eq!(concrete(), dispatched());
        fn measure(run: impl Fn() -> f64) -> f64 {
            let begin = Instant::now();
            for _ in 0..50 {
                black_box(run());
            }
            begin.elapsed().as_secs_f64() * 1000.0
        }
        let mut direct = Vec::new();
        let mut dispatch = Vec::new();
        for round in 0..9 {
            if round % 2 == 0 {
                direct.push(measure(concrete));
                dispatch.push(measure(dispatched));
            } else {
                dispatch.push(measure(dispatched));
                direct.push(measure(concrete));
            }
        }
        direct.sort_by(f64::total_cmp);
        dispatch.sort_by(f64::total_cmp);
        println!(
            "50 local-vol FD calls: concrete_ms={direct:?}, dispatched_ms={dispatch:?}; median_ratio={}",
            dispatch[4] / direct[4]
        );
    }
}
