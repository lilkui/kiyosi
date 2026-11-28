use std::cell::Cell;

use super::{
    BusinessDiffusion, Diffusion, EuropeanOption, FiniteDifferenceScheme,
    FiniteDifferenceVanillaEngine, Payoff, PricingContext, Result, Stepper, StripModel, diffusion,
    indexed_value, invalid_grid, local_volatility, payoff, strike_group_len, strip_grid,
    terminal_moments, validate_work, validate_workspace,
};
use crate::engines::deterministic::{self, AmericanExerciseMarket};
use crate::engines::finite_difference::{
    Curve,
    edge_weights::{BYTES_PER_STEP, EdgeWeights},
    unstable, validate_finite_difference_surface_size,
};
use crate::{Error, ModelParameters, Timestamp, engines::FiniteDifferenceSettings};

impl FiniteDifferenceVanillaEngine {
    pub(in crate::engines::finite_difference) fn price_european_surface(
        &self,
        template: &EuropeanOption,
        context: &PricingContext,
        expiries: &[Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        local_volatility::validate_model(context)?;
        let valuation = context.valuation_time();
        validate_finite_difference_surface_size(expiries.len(), strikes.len())?;
        let mut limits = [f64::INFINITY, 0.0_f64];
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
            limits = [limits[0].min(strike), limits[1].max(strike)];
        }
        for &expiry in expiries {
            crate::cancellation::check()?;
            if expiry < valuation || expiry < template.effective_time() {
                return Err(invalid_grid(
                    "surface expiry precedes effective or valuation time",
                ));
            }
        }
        let mut ordered = expiries.to_vec();
        ordered.sort_unstable();
        ordered.dedup();
        let Some(&end) = ordered.last() else {
            return Ok(Vec::new());
        };
        context.validate_curve_coverage(end)?;
        if strikes.is_empty() {
            return Ok(vec![Vec::new(); expiries.len()]);
        }
        let anchor = context.spot_price();
        let live = ordered.partition_point(|&expiry| expiry == valuation);
        let active = &ordered[live..];
        let payoff = Payoff::vanilla(template.option_type(), anchor, false);
        let contract = |expiry| {
            template.replace(crate::VanillaOptionChanges {
                expiry_time: Some(expiry),
                ..Default::default()
            })
        };
        let scalar = |expiries: &[Timestamp]| {
            expiries
                .iter()
                .map(|&expiry| {
                    crate::cancellation::check()?;
                    self.price_european_strikes(&contract(expiry)?, context, strikes)
                })
                .collect::<Result<Vec<_>>>()
        };
        let settings = self.settings;
        let shared_work = Cell::new(0_u64);
        let fallback = |dates: &[Timestamp]| {
            let work = fallback_work(context, settings, dates, strikes, None, false)?;
            validate_work(1, shared_work.get().saturating_add(work))?;
            scalar(dates)
        };
        let rows = if active.len() >= 2
            && strikes.len() >= 4
            && settings.asset_step_count >= 128
            && settings.scheme == FiniteDifferenceScheme::Rannacher
            && limits[0] < limits[1]
            && 0.5 * anchor.max(limits[1]) <= anchor.min(limits[0])
        {
            let shared = || {
                let model = context.model_parameters();
                let (strip, sigma) = match model {
                    ModelParameters::BlackScholesMerton(model) => {
                        // A tight boundary can have material BSM continuation value.
                        if settings.asset_upper_boundary.is_some() {
                            return Ok(None);
                        }
                        let sigma = model.volatility();
                        (StripModel::Constant(sigma), sigma)
                    }
                    ModelParameters::LocalVolatility(model) => {
                        let surface = model.surface();
                        (
                            StripModel::Local(surface),
                            surface.volatility(valuation, anchor)?,
                        )
                    }
                    ModelParameters::Heston(_) => return Err(unstable()),
                };
                let width = |expiry| {
                    model
                        .variance_time_convention()
                        .year_fraction(valuation, expiry)
                        .map(|years| sigma * years.sqrt())
                };
                // Require at least 24 nominal intervals to resolve the initial point mass.
                let min_tenor =
                    valuation.years_until(end) * (24.0 / f64::from(settings.time_step_count));
                let (mut first, mut last) = (0, active.len());
                while first < last {
                    crate::cancellation::check()?;
                    let middle = first + (last - first) / 2;
                    if valuation.years_until(active[middle]) >= min_tenor
                        && width(active[middle])? >= 0.04
                    {
                        last = middle;
                    } else {
                        first = middle + 1;
                    }
                }
                let (short, compatible) = active.split_at(first);
                if compatible.len() < 2 {
                    return Ok(None);
                }
                let short_work = fallback_work(context, settings, short, strikes, None, false)?;
                let rows = strip_rows(
                    self,
                    context,
                    strip,
                    (compatible, strikes),
                    limits,
                    (payoff, width(compatible[0])?),
                    |work| {
                        validate_work(1, work.saturating_add(short_work))?;
                        shared_work.set(work);
                        Ok(())
                    },
                )?;
                Ok(Some((short, rows)))
            };
            match shared() {
                Ok(Some((short, long_rows))) => {
                    let mut rows = scalar(short)?;
                    rows.extend(long_rows);
                    rows
                }
                Err(error @ Error::Io(_)) => return Err(error),
                Ok(None) | Err(_) => fallback(active)?,
            }
        } else {
            fallback(active)?
        };
        expiries
            .iter()
            .map(|expiry| {
                crate::cancellation::check()?;
                let row = ordered.binary_search(expiry).map_err(|_| unstable())?;
                if row < live {
                    return strikes
                        .iter()
                        .map(|&strike| {
                            crate::cancellation::check()?;
                            Ok(Payoff { strike, ..payoff }.value(anchor))
                        })
                        .collect();
                }
                let values = &rows[row - live];
                let mut output = Vec::with_capacity(values.len());
                for chunk in values.chunks(1024) {
                    crate::cancellation::check()?;
                    output.extend_from_slice(chunk);
                }
                Ok(output)
            })
            .collect()
    }
}

pub(super) fn fallback_work(
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    expiries: &[Timestamp],
    strikes: &[f64],
    local_lanes: Option<u64>,
    american: bool,
) -> Result<u64> {
    if expiries.is_empty() || !american && deterministic::enabled(context) {
        return Ok(0);
    }
    let mut ordered = strikes.to_vec();
    ordered.sort_unstable_by(f64::total_cmp);
    crate::cancellation::check()?;
    let can_share =
        settings.scheme == FiniteDifferenceScheme::Rannacher && settings.asset_step_count >= 128;
    let mut rollbacks = 0_u64;
    let mut distinct = 0_u64;
    let mut remaining = ordered.as_slice();
    while let Some(&first) = remaining.first() {
        crate::cancellation::check()?;
        let (group, tail) = remaining.split_at(strike_group_len(
            context.spot_price(),
            remaining,
            |&strike| strike,
        ));
        let mut unique = 1;
        for (index, pair) in group.windows(2).enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            unique += u64::from(pair[0].to_bits() != pair[1].to_bits());
        }
        rollbacks += unique;
        distinct += unique;
        // A failed shared strike grid can be followed by a solve for each strike.
        if can_share && group.len() >= 4 && first < group[group.len() - 1] {
            rollbacks += local_lanes.map_or(1, |lanes| unique.next_multiple_of(lanes));
        }
        remaining = tail;
    }

    let mut work = 0_u64;
    let valuation = context.valuation_time();
    for &expiry in expiries {
        crate::cancellation::check()?;
        let expiry_work =
            if american && valuation < expiry && deterministic::enabled_until(context, expiry)? {
                AmericanExerciseMarket::estimate_intervals(context, expiry).saturating_mul(distinct)
            } else {
                strike_rollback_work(context, settings, expiry)?.saturating_mul(rollbacks)
            };
        work = work.saturating_add(expiry_work);
        validate_work(1, work)?;
    }
    Ok(work)
}

pub(super) fn strike_rollback_work(
    context: &PricingContext,
    settings: FiniteDifferenceSettings,
    expiry: Timestamp,
) -> Result<u64> {
    let valuation = context.valuation_time();
    if expiry == valuation || crate::engines::deterministic::enabled_until(context, expiry)? {
        return Ok(0);
    }
    let model = context.model_parameters();
    let local_times =
        local_volatility::local_model(context).map_or(&[][..], |model| model.surface().times());
    let local_start = local_times.partition_point(|&time| time <= valuation);
    let assets = u64::from(settings.asset_step_count.unsigned_abs());
    let base_steps = u64::from(settings.time_step_count.unsigned_abs())
        + if settings.scheme == FiniteDifferenceScheme::Rannacher {
            4
        } else {
            0
        };
    let knots = [model.risk_free_curve(), model.dividend_curve()]
        .map(|curve| Curve::interval_count(curve, valuation, expiry) - 1);
    let variance_knots = if diffusion::uses_business_time(context) {
        valuation.date().days_until(expiry.date()).unsigned_abs()
    } else {
        0
    };
    let local_knots = local_times[local_start..].partition_point(|&time| time < expiry) as u64;
    let steps = base_steps + knots[0] + knots[1] + local_knots + variance_knots;
    Ok(assets.saturating_mul(steps))
}

fn strip_rows(
    engine: &FiniteDifferenceVanillaEngine,
    context: &PricingContext,
    model: StripModel<'_>,
    (expiries, strikes): (&[Timestamp], &[f64]),
    limits: [f64; 2],
    (payoff, width): (Payoff, f64),
    charge_work: impl FnOnce(u64) -> Result<()>,
) -> Result<Vec<Vec<f64>>> {
    let valuation = context.valuation_time();
    let end = expiries[expiries.len() - 1];
    let spot = context.spot_price();
    if let StripModel::Local(surface) = model {
        if surface
            .constant_volatility_between(valuation, end)?
            .is_some()
        {
            return Err(unstable());
        }
    }
    let settings = engine.settings;
    let times: Vec<_> = expiries
        .iter()
        .map(|&expiry| valuation.years_until(expiry))
        .collect();
    let (risk, dividend) = diffusion::rate_curves(context, end, settings)?;
    let (space, node) = strip_grid(settings, context, end, [&risk, &dividend], limits, width)?;
    let grid = diffusion::time_grid(context, end, settings, &risk, &dividend, &times)?;
    let count = space.count as u64 + 1;
    let steps = grid.widths.len() as u64;
    let work = count.saturating_mul(steps + 5 * times.len() as u64);
    validate_workspace(
        settings.scheme,
        4 * count,
        steps,
        risk.workspace_bytes() + dividend.workspace_bytes() + BYTES_PER_STEP * steps,
    )?;
    charge_work(work)?;
    let bounds = [space.lower, space.upper];
    let sign = payoff.sign;
    let mut edges = EdgeWeights::new(
        &grid.times[..grid.widths.len()],
        [&risk, &dividend],
        bounds,
        sign,
    )?
    .ok_or_else(unstable)?;
    let mut pending = [[0.0; 2]; 2];
    let stepper = || match model {
        StripModel::Constant(_) => Stepper::new(),
        StripModel::Local(_) => Stepper::new_local(space.count),
    };
    let mut regular = stepper();
    let mut damped = stepper();
    let mut base: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.0; space.count + 1]);
    let mut current = base[0].clone();
    let mut next = base[0].clone();
    base[2][node] = 1.0;
    let offset = match model {
        StripModel::Constant(_) => 0.0,
        StripModel::Local(surface) => surface.times()[0].years_until(valuation),
    };
    let clock =
        diffusion::uses_business_time(context).then(|| BusinessDiffusion::new(context, 0.0));
    let advance = |stepper: &mut Stepper,
                   weights: &mut [f64],
                   next: &mut [f64],
                   (start, dt, theta): (f64, f64, f64)| {
        let middle = start + 0.5 * dt;
        let key = [dt, theta, risk.rate(middle), dividend.rate(middle)];
        let scale = clock.map_or(1.0, |clock| clock.variance_rate(middle).sqrt());
        match model {
            StripModel::Constant(sigma) => {
                stepper.transpose_constant(weights, next, &space, sigma * scale, key)
            }
            StripModel::Local(surface) => stepper.transpose_local(
                weights,
                next,
                &space,
                surface,
                [offset + middle, scale],
                key,
            ),
        }
    };
    let mut rows = Vec::with_capacity(times.len());
    for (step, &dt) in grid.widths.iter().enumerate() {
        crate::cancellation::check()?;
        current.copy_from_slice(&base[2]);
        let slot = step % 2;
        // The last two steps belong to the expiry's damping branch.
        if step >= 2 {
            edges.add(step - 2, pending[slot]).ok_or_else(unstable)?;
        }
        pending[slot] = advance(
            &mut regular,
            &mut current,
            &mut base[0],
            (grid.times[step], dt, 0.5),
        )?;
        base.rotate_left(1);
        let maturity = grid.times[step + 1];
        if times[rows.len()].to_bits() != maturity.to_bits() {
            continue;
        }
        let first = step.saturating_sub(1);
        current.copy_from_slice(&base[usize::from(step == 0)]);
        let mut boundary = Vec::with_capacity(8);
        for index in first..=step {
            let mut damping = 1;
            let mut substeps = grid.substeps(index, settings.scheme, &mut damping);
            let later = substeps.next().expect("each interval has a substep");
            for time @ (start, _, _) in substeps.chain([later]) {
                let edge = advance(&mut damped, &mut current, &mut next, time)?;
                let cash = risk.log_factor(start, maturity).exp();
                let asset = dividend.log_factor(start, maturity).exp();
                for (bound, weight) in bounds.into_iter().zip(edge) {
                    let present = bound * asset;
                    if !cash.is_normal() || !(present / cash).is_normal() {
                        return Err(unstable());
                    }
                    boundary.push([present / cash, weight * present, weight * cash]);
                }
                std::mem::swap(&mut current, &mut next);
            }
        }
        let terminal = terminal_moments(&space, &current, sign);
        let discount = [
            risk.log_factor(0.0, maturity),
            dividend.log_factor(0.0, maturity),
        ];
        let factors = discount.map(f64::exp);
        let row = strikes
            .iter()
            .map(|&strike| {
                crate::cancellation::check()?;
                let mut edge = edges.at(strike, factors).ok_or_else(unstable)?;
                for &[cutoff, asset, cash] in &boundary {
                    if sign * (cutoff - strike) > 0.0 {
                        edge[0] += sign * (-strike).mul_add(cash, asset);
                        edge[1] += strike.mul_add(cash.abs(), asset.abs());
                    }
                }
                let payoff = Payoff { strike, ..payoff };
                let value = indexed_value(&space, &current, &terminal, payoff, edge, 0.0)
                    .ok_or_else(unstable)?;
                payoff::checked_price(payoff, spot, value, discount, 0.0)
            })
            .collect::<Result<_>>()?;
        rows.push(row);
        if rows.len() == times.len() {
            return Ok(rows);
        }
    }
    Err(unstable())
}
