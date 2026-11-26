use super::{
    AmericanOption, Diffusion, FiniteDifferenceVanillaEngine, Payoff, PricingContext, Result,
    SpatialGrid, concentrate, diffusion, diffusion_log_width, invalid_grid, payoff, project_payoff,
    spot_floor, validate_work, validate_workspace,
};
use crate::engines::deterministic::ExerciseBoundaries;

const LOCAL_LANES: usize = 8;

pub(super) fn local_strikes(
    engine: &FiniteDifferenceVanillaEngine,
    option: &AmericanOption,
    context: &PricingContext,
    group: &[(usize, f64)],
    prices: &mut [f64],
) -> Result<()> {
    let mut unique = Vec::new();
    unique
        .try_reserve_exact(group.len())
        .map_err(|_| super::super::unstable())?;
    for (position, &item) in group.iter().enumerate() {
        if position % 1024 == 0 {
            crate::cancellation::check()?;
        }
        if position == 0 || item.1.to_bits() != group[position - 1].1.to_bits() {
            unique.push(item);
        }
    }
    let expiry = option.expiry_time();
    let settings = engine.settings;
    let spot = context.spot_price();
    diffusion::with_diffusion!(context, Some(expiry), |model| {
        let valuation = context.valuation_time();
        let root = context
            .model_parameters()
            .variance_time_convention()
            .year_fraction(valuation, expiry)?
            .sqrt();
        let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
        let horizon_width = diffusion_log_width(context, expiry, &risk, &dividend)?;
        let grid = diffusion::time_grid(context, expiry, settings, &risk, &dividend, &[])?;
        let steps = grid.rollback_steps(settings.scheme, &[])?;
        let time = valuation.years_until(expiry);
        let totals = [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)];
        let nodes = u64::from(settings.asset_step_count.unsigned_abs()) + 1;
        validate_work(
            nodes * unique.len().next_multiple_of(LOCAL_LANES) as u64,
            steps,
        )?;
        validate_workspace(
            settings.scheme,
            nodes * 3 * LOCAL_LANES as u64,
            grid.widths.len() as u64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )?;

        for tile in unique.chunks(LOCAL_LANES) {
            crate::cancellation::check()?;
            let anchor = tile[tile.len() / 2].1;
            let width = tile
                .iter()
                .map(|&(_, strike)| model.sigma(0.0, strike) * root)
                .fold(f64::INFINITY, f64::min);
            if width < 0.04 {
                return Err(super::super::unstable());
            }
            let extent = (1.5 * horizon_width).max(4.0_f64.ln());
            let low = spot_floor(spot.min(tile[0].1), anchor, extent);
            let high = super::super::scaled(spot.max(tile[tile.len() - 1].1), extent);
            let high = settings.asset_upper_boundary.unwrap_or(high);
            if high <= spot.max(tile[tile.len() - 1].1) {
                return Err(invalid_grid(
                    "finite-difference upper bound must exceed spot and strike",
                ));
            }
            let mut space = SpatialGrid::new(settings.asset_step_count, [low, high], anchor)?;
            concentrate(&mut space, vec![anchor], 0.3 * width)?;
            let payoffs: [Payoff; LOCAL_LANES] = std::array::from_fn(|lane| {
                let strike = tile[lane.min(tile.len() - 1)].1;
                Payoff::vanilla(option.option_type(), strike, true)
            });
            let mut bounds = payoffs
                .iter()
                .take(tile.len())
                .map(|&payoff| {
                    ExerciseBoundaries::new(
                        payoff,
                        [low, high],
                        time,
                        [&risk, &dividend],
                        grid.widths.len(),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let mut old: Vec<_> = (0..=space.count)
                .map(|i| {
                    payoffs.map(|payoff| {
                        project_payoff(&space, &[payoff.strike], i, |asset| payoff.value(asset))
                    })
                })
                .collect();
            let exercise: Vec<_> = space
                .spots
                .iter()
                .map(|&asset| payoffs.map(|payoff| payoff.value(asset)))
                .collect();
            let mut next = old.clone();
            let mut stepper = model.stepper(space.count);
            let mut damping = 2;
            for step in (0..grid.widths.len()).rev() {
                crate::cancellation::check()?;
                for (current, dt, weight) in grid.substeps(step, settings.scheme, &mut damping) {
                    let middle = current + 0.5 * dt;
                    let rates = [risk.rate(middle), dividend.rate(middle)];
                    let discount = [
                        risk.log_factor(current, time),
                        dividend.log_factor(current, time),
                    ];
                    let mut edges = [[0.0; LOCAL_LANES]; 2];
                    for (lane, (&payoff, boundary)) in payoffs.iter().zip(&mut bounds).enumerate() {
                        let floor = boundary.at(current)?;
                        for (edge, (asset, floor)) in
                            edges.iter_mut().zip([low, high].into_iter().zip(floor))
                        {
                            edge[lane] = payoff::boundary_value(
                                payoff,
                                asset,
                                discount,
                                model,
                                time - current,
                            )
                            .max(floor);
                        }
                    }
                    for edge in &mut edges {
                        let last = edge[tile.len() - 1];
                        edge[tile.len()..].fill(last);
                    }
                    stepper.advance_packed(
                        &old,
                        &mut next,
                        &space,
                        model,
                        middle,
                        [dt, weight, rates[0], rates[1]],
                        edges,
                    )?;
                    for (values, exercise) in next[1..space.count]
                        .iter_mut()
                        .zip(&exercise[1..space.count])
                    {
                        for (value, &intrinsic) in values.iter_mut().zip(exercise) {
                            *value = value.max(intrinsic);
                        }
                    }
                    std::mem::swap(&mut old, &mut next);
                }
            }
            for (lane, (&(index, _), payoff)) in tile.iter().zip(payoffs).enumerate() {
                let spline = space.spline(old.iter().map(|node| node[lane]).collect());
                let [mut value, ..] = space.sample(&spline, spot);
                if space.exercise_cell(&spline, spot, |asset| payoff.value(asset)) {
                    value = payoff.value(spot);
                }
                prices[index] = payoff::checked_price(payoff, spot, value, totals, 0.0)?;
            }
        }
        for pair in group.windows(2) {
            if pair[0].1.to_bits() == pair[1].1.to_bits() {
                prices[pair[1].0] = prices[pair[0].0];
            }
        }
        Ok(())
    })
}
