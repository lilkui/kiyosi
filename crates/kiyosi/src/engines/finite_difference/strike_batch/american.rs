use super::super::RollbackWork;
use super::{
    AmericanOption, Diffusion, FiniteDifferenceScheme, FiniteDifferenceVanillaEngine, Payoff,
    PricingContext, Result, SpatialGrid, concentrate, diffusion, diffusion_log_width, invalid_grid,
    payoff, project_payoff, spot_floor, strike_group_len, validate_work, validate_workspace,
};
use crate::engines::deterministic::ExerciseBoundaries;
use crate::{LocalVolatilityParameters, Timestamp};
use rayon::prelude::*;

pub(super) const LOCAL_LANES: usize = 8;

pub(super) fn local_strikes(
    engine: &FiniteDifferenceVanillaEngine,
    option: &AmericanOption,
    context: &PricingContext,
    group: &[(usize, f64)],
    prices: &mut [f64],
    budget: &RollbackWork,
) -> Result<()> {
    local_expiries(
        engine,
        option,
        context,
        &[option.expiry_time()],
        group,
        &mut [prices],
        budget,
    )
}

struct ExpiryRow<'a> {
    maturity: f64,
    finish: usize,
    bounds: Vec<ExerciseBoundaries<'a>>,
    old: Vec<[f64; LOCAL_LANES]>,
    next: Vec<[f64; LOCAL_LANES]>,
}

fn local_expiries(
    engine: &FiniteDifferenceVanillaEngine,
    option: &AmericanOption,
    context: &PricingContext,
    expiries: &[Timestamp],
    group: &[(usize, f64)],
    prices: &mut [&mut [f64]],
    budget: &RollbackWork,
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
    let expiry = expiries[expiries.len() - 1];
    let settings = engine.settings;
    let spot = context.spot_price();
    diffusion::with_diffusion!(context, Some(expiry), |model| {
        let valuation = context.valuation_time();
        let root = context
            .model_parameters()
            .variance_time_convention()
            .year_fraction(valuation, expiries[0])?
            .sqrt();
        let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
        let horizon_width = diffusion_log_width(context, expiry, &risk, &dividend)?;
        let times: Vec<_> = expiries
            .iter()
            .map(|&expiry| valuation.years_until(expiry))
            .collect();
        let grid = diffusion::time_grid(
            context,
            expiry,
            settings,
            &risk,
            &dividend,
            &times[..times.len() - 1],
        )?;
        let steps = grid.rollback_steps(settings.scheme, &times[..times.len() - 1])?;
        let partitions = times
            .iter()
            .map(|time| {
                grid.times
                    .binary_search_by(|node| node.total_cmp(time))
                    .map_err(|_| super::super::unstable())
            })
            .collect::<Result<Vec<_>>>()?;
        let nodes = u64::from(settings.asset_step_count.unsigned_abs()) + 1;
        let work = (nodes * unique.len().next_multiple_of(LOCAL_LANES) as u64)
            .saturating_mul(steps)
            .saturating_mul(expiries.len() as u64);
        validate_work(1, work)?;
        validate_workspace(
            settings.scheme,
            nodes * (3 + 2 * expiries.len() as u64) * LOCAL_LANES as u64,
            grid.widths.len() as u64,
            risk.workspace_bytes() + dividend.workspace_bytes(),
        )?;
        budget.charge(work)?;

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
            let terminal: Vec<_> = (0..=space.count)
                .map(|i| {
                    payoffs.map(|payoff| {
                        project_payoff(&space, &[payoff.strike], i, |asset| payoff.value(asset))
                    })
                })
                .collect();
            let mut rows = times
                .iter()
                .zip(&partitions)
                .map(|(&maturity, &finish)| {
                    let bounds = payoffs
                        .iter()
                        .take(tile.len())
                        .map(|&payoff| {
                            ExerciseBoundaries::new(
                                payoff,
                                [low, high],
                                maturity,
                                [&risk, &dividend],
                                finish,
                            )
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(ExpiryRow {
                        maturity,
                        finish,
                        bounds,
                        old: terminal.clone(),
                        next: terminal.clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let exercise: Vec<_> = space
                .spots
                .iter()
                .map(|&asset| payoffs.map(|payoff| payoff.value(asset)))
                .collect();
            let mut stepper = model.stepper(space.count);
            for step in (0..grid.widths.len()).rev() {
                crate::cancellation::check()?;
                for damped in [false, true] {
                    let active = |row: &ExpiryRow<'_>| {
                        step < row.finish && (row.finish - step <= 2) == damped
                    };
                    if !rows.iter().any(active) {
                        continue;
                    }
                    let mut damping = u32::from(damped);
                    for (current, dt, weight) in grid.substeps(step, settings.scheme, &mut damping)
                    {
                        let middle = current + 0.5 * dt;
                        let packed = stepper.prepare_packed(
                            &space,
                            model,
                            middle,
                            [dt, weight, risk.rate(middle), dividend.rate(middle)],
                        )?;
                        for row in rows.iter_mut().filter(|row| active(row)) {
                            let maturity = row.maturity;
                            let discount = [
                                risk.log_factor(current, maturity),
                                dividend.log_factor(current, maturity),
                            ];
                            let mut edges = [[0.0; LOCAL_LANES]; 2];
                            for (lane, (&payoff, boundary)) in
                                payoffs.iter().zip(&mut row.bounds).enumerate()
                            {
                                let floor = boundary.at(current)?;
                                for (edge, (asset, floor)) in
                                    edges.iter_mut().zip([low, high].into_iter().zip(floor))
                                {
                                    edge[lane] = payoff::boundary_value(
                                        payoff,
                                        asset,
                                        discount,
                                        model,
                                        maturity - current,
                                    )
                                    .max(floor);
                                }
                            }
                            for edge in &mut edges {
                                let last = edge[tile.len() - 1];
                                edge[tile.len()..].fill(last);
                            }
                            packed.apply(&row.old, &mut row.next, edges)?;
                            for (values, exercise) in row.next[1..space.count]
                                .iter_mut()
                                .zip(&exercise[1..space.count])
                            {
                                for (value, &intrinsic) in values.iter_mut().zip(exercise) {
                                    *value = value.max(intrinsic);
                                }
                            }
                            std::mem::swap(&mut row.old, &mut row.next);
                        }
                    }
                }
            }
            for (row, prices) in rows.into_iter().zip(&mut *prices) {
                let totals = [
                    risk.log_factor(0.0, row.maturity),
                    dividend.log_factor(0.0, row.maturity),
                ];
                for (lane, (&(index, _), payoff)) in tile.iter().zip(payoffs).enumerate() {
                    let spline = space.spline(row.old.iter().map(|node| node[lane]).collect());
                    let [mut value, ..] = space.sample(&spline, spot);
                    if space.exercise_cell(&spline, spot, |asset| payoff.value(asset)) {
                        value = payoff.value(spot);
                    }
                    prices[index] = payoff::checked_price(payoff, spot, value, totals, 0.0)?;
                }
            }
        }
        for prices in prices {
            for pair in group.windows(2) {
                if pair[0].1.to_bits() == pair[1].1.to_bits() {
                    prices[pair[1].0] = prices[pair[0].0];
                }
            }
        }
        Ok(())
    })
}

pub(super) fn local_surface(
    engine: &FiniteDifferenceVanillaEngine,
    option: &AmericanOption,
    context: &PricingContext,
    (model, expiries, strikes): (&LocalVolatilityParameters, &[Timestamp], &[f64]),
    scalar: impl Fn(&[Timestamp], &[f64]) -> Result<Vec<Vec<f64>>> + Sync,
    budget: &RollbackWork,
) -> Result<Vec<Vec<f64>>> {
    const EXPIRY_LANES: usize = 2;
    let settings = engine.settings;
    let valuation = context.valuation_time();
    let indexed: Vec<_> = strikes.iter().copied().enumerate().collect();
    let paired = |dates: &[Timestamp]| -> Result<Vec<Vec<f64>>> {
        let [first, last] = dates else {
            return scalar(dates, strikes);
        };
        if settings.scheme != FiniteDifferenceScheme::Rannacher
            || settings.asset_step_count < 128
            || valuation.years_until(*first) < 0.75 * valuation.years_until(*last)
            || *first == valuation
        {
            return scalar(dates, strikes);
        }
        let mut rows = vec![vec![0.0; strikes.len()]; dates.len()];
        let mut remaining = indexed.as_slice();
        while !remaining.is_empty() {
            crate::cancellation::check()?;
            let len = strike_group_len(context.spot_price(), remaining, |&(_, strike)| strike);
            let (group, tail) = remaining.split_at(len);
            let first = group[0].0;
            let mut outputs: Vec<_> = rows.iter_mut().map(Vec::as_mut_slice).collect();
            if len >= 4 {
                match local_expiries(engine, option, context, dates, group, &mut outputs, budget) {
                    Ok(()) => {
                        remaining = tail;
                        continue;
                    }
                    Err(error @ crate::Error::Io(_)) => return Err(error),
                    Err(_) => {}
                }
            }
            let strips = scalar(dates, &strikes[first..first + len])?;
            for (output, values) in outputs.into_iter().zip(strips) {
                output[first..first + len].copy_from_slice(&values);
            }
            remaining = tail;
        }
        Ok(rows)
    };
    // The pool has at most four workers. Large grids keep their per-request
    // workspace on one worker, including when a pair needs scalar fallback.
    let parallel = expiries.len() > EXPIRY_LANES
        && settings.asset_step_count <= 4096
        && settings.time_step_count <= 4096
        && valuation.years_until(expiries[expiries.len() - 1]) <= 10.0
        && model.surface().times().len() <= 4096
        && [model.risk_free_curve(), model.dividend_curve()]
            .iter()
            .all(|curve| curve.nodes().len() <= 4096);
    let groups = if let Some((pool, _)) = parallel.then(crate::engines::rollback_pool).flatten() {
        let tokens = crate::cancellation::current();
        pool.install(|| {
            expiries
                .par_chunks(EXPIRY_LANES)
                .map(|dates| crate::cancellation::with_tokens(tokens.clone(), || paired(dates)))
                .collect::<Vec<_>>()
        })
    } else {
        expiries.chunks(EXPIRY_LANES).map(paired).collect()
    };
    let mut rows = Vec::with_capacity(expiries.len());
    for group in groups {
        rows.extend(group?);
    }
    Ok(rows)
}
