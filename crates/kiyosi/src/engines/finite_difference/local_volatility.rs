use super::*;
use crate::{LocalVolatilityParameters, ModelParameters};

pub(super) fn applicable(context: &PricingContext, american: bool) -> Result<()> {
    if american {
        context.bsm_parameters()?;
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
    if Greek::ALL
        .into_iter()
        .any(|g| request.has(g) && !matches!(g, Greek::Delta | Greek::Gamma))
    {
        return Err(Error::UnsupportedOperation(
            "local volatility supports only delta and gamma".into(),
        ));
    }
    let time = time_to_expiry(option, context)?;
    let expiry = option.expiry_date().start_of_day();
    let surface = model.surface();
    surface.validate_time(expiry)?;
    let risk = Curve::new(model.risk_free_curve(), context.valuation_time(), expiry)?;
    let dividend = Curve::new(model.dividend_curve(), context.valuation_time(), expiry)?;
    let spot = context.spot_price();
    if time == 0.0 {
        return PricingResult::new(payoff.value(spot));
    }
    let relevant = spot.max(payoff.strike);
    let upper = settings
        .asset_upper_boundary
        .unwrap_or_else(|| diffusion_upper(relevant, context, time, &risk, &dividend));
    let space = SpatialGrid::new(settings, upper, 0.0, relevant)?;
    let offset = surface.times()[0].years_until(context.valuation_time());
    let knots: Vec<_> = surface
        .times()
        .iter()
        .filter(|&&t| t > context.valuation_time() && t < expiry)
        .map(|&t| context.valuation_time().years_until(t))
        .collect();
    let time_grid = TimeGrid::with_events(time, settings.time_step_count, &risk, &dividend, &knots);
    let mut old: Vec<_> = (0..=space.count)
        .map(|i| payoff.value(space.asset(i)))
        .collect();
    let mut next = vec![0.0; old.len()];
    let mut stepper = Stepper::new_local(space.count);
    let mut damping = 2;
    for step in (0..time_grid.widths.len()).rev() {
        for (current, dt, theta) in time_grid.substeps(step, settings.scheme, &mut damping) {
            let middle = current + 0.5 * dt;
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let dr = scaled(payoff.strike, risk.log_factor(current, time));
            let dq = scaled(upper, dividend.log_factor(current, time));
            let edges = if payoff.sign > 0.0 {
                [0.0, dq - dr]
            } else {
                [dr, 0.0]
            };
            stepper.advance_local(
                &old,
                &mut next,
                |i| {
                    let sigma = surface.at_year(offset + middle, space.asset(i));
                    space.coefficients(i, rate, q, sigma)
                },
                [dt, theta],
                edges,
            )?;
            std::mem::swap(&mut old, &mut next);
        }
    }
    let curvature = space.curvature(&old);
    let sample = space.sample(&old, &curvature, spot);
    let scale = scaled(spot, dividend.log_factor(0.0, time))
        .max(scaled(payoff.strike, risk.log_factor(0.0, time)));
    let price = crate::numerics::checked_with_scale(sample[0], scale)?;
    let mut result = PricingResult::new(price)?;
    result.set(request, Greek::Delta, sample[1])?;
    result.set(request, Greek::Gamma, sample[2])?;
    Ok(result)
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
