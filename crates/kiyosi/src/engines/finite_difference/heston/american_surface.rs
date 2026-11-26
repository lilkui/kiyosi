use std::sync::Arc;

use rayon::prelude::*;

use super::{
    AmericanOption, FiniteDifferenceHestonVanillaEngine, Payoff, PricingContext, Result,
    SpatialGrid, Spline, Timestamp, WORKSPACE_BUDGET, grid_price, integrated_variance,
    invalid_grid, prepare_batch, rollback_pool, unstable,
};

impl FiniteDifferenceHestonVanillaEngine {
    /// Prices an American strike and expiry surface in input order.
    ///
    /// With flat rates and time-homogeneous dynamics, one rollback supplies each
    /// time to maturity. Fixed asset bounds use strike tiles on absolute meshes.
    /// Compatible dated curves with automatic bounds share factored operators in
    /// expiry tiles. Other markets use independent expiry rollbacks.
    /// The longest maturity sets each shared mesh. Flat-rate short tenors use
    /// individual expiry meshes. Check convergence across the surface.
    /// See [`Self::price_american_strikes`] for the contract and market inputs.
    ///
    /// # Errors
    ///
    /// Propagates strike-batch errors and rejects oversized output or expiries
    /// before valuation or the effective time. Cancellation returns no partial
    /// surface.
    pub fn price_american_surface(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        expiries: &[Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        let model = context.model_parameters().as_heston()?;
        let valuation = context.valuation_time();
        super::super::validate_finite_difference_surface_size(expiries.len(), strikes.len())?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        for &expiry in expiries {
            crate::cancellation::check()?;
            if expiry < template.effective_time()
                || expiry < valuation
                || valuation < template.effective_time()
            {
                return Err(invalid_grid(
                    "American surface requires valuation within contract life",
                ));
            }
        }
        if expiries.is_empty() {
            return Ok(Vec::new());
        }
        let mut ordered = expiries.to_vec();
        ordered.sort_unstable();
        ordered.dedup();
        let end = ordered.last().copied().ok_or_else(unstable)?;
        context.validate_curve_coverage(end)?;
        if strikes.is_empty() {
            return Ok(vec![Vec::new(); expiries.len()]);
        }

        let spot = context.spot_price();
        let payoff = Payoff::vanilla(template.option_type(), spot, true);
        let active = ordered.partition_point(|&expiry| expiry <= valuation);
        let live = &ordered[active..];
        let constant_variance = model.initial_variance() > 0.0
            && (model.mean_reversion_speed() == 0.0
                || model.initial_variance().to_bits() == model.long_run_variance().to_bits());
        let flat = model.risk_free_curve().exact_flat_rate().is_some()
            && model.dividend_curve().exact_flat_rate().is_some();
        let homogeneous = live.len() >= 2
            && flat
            && (constant_variance
                || (model.vol_of_vol() > 0.0
                    && (model.initial_variance() > 0.0
                        || (model.mean_reversion_speed() > 0.0
                            && model.long_run_variance() > 0.0))));
        let mut share = homogeneous && self.settings.asset_upper_boundary.is_none();
        let mut queries = Vec::new();
        let (mut low, mut high) = (f64::INFINITY, 0.0_f64);
        if share {
            queries.reserve_exact(strikes.len());
            for &strike in strikes {
                crate::cancellation::check()?;
                let ratio = payoff.strike / strike;
                let normalized = spot * ratio;
                share &= normalized.is_normal() && ratio.is_normal() && ratio.recip().is_normal();
                low = low.min(normalized);
                high = high.max(normalized);
                queries.push((normalized, ratio));
            }
        }
        let contract = |expiry| {
            template.replace(crate::AmericanOptionChanges {
                expiry_time: Some(expiry),
                ..Default::default()
            })
        };
        let times = if homogeneous {
            live.iter()
                .map(|&expiry| expiry.years_until(end))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let shared_times = !times.is_empty()
            && times[0] < valuation.years_until(end)
            && times.windows(2).all(|pair| pair[0] > pair[1]);
        // A common variance and spot mesh can underresolve very short tenors.
        let first = live.partition_point(|&expiry| {
            integrated_variance(model, valuation.years_until(expiry)).sqrt() < 0.04
        });
        let rows = if share && shared_times && live.len() - first >= 2 {
            let mut surface = AmericanSurface {
                times: times[first..].to_vec(),
                queries: SurfaceQueries::Normalized(queries),
                rows: Vec::with_capacity(live.len() - first),
            };
            let anchor = super::super::solution::at_spot(context, high)?;
            prepare_batch(
                &contract(end)?,
                &anchor,
                payoff,
                self.settings,
                low,
                &[],
                &[],
                Some(&mut surface),
            )?;
            if surface.rows.len() != live.len() - first {
                return Err(unstable());
            }
            let mut rows = self.expiry_rows(template, context, &live[..first], strikes)?;
            rows.extend(surface.rows);
            rows
        } else if let Some(upper) = self
            .settings
            .asset_upper_boundary
            .filter(|_| shared_times && self.settings.asset_step_count >= 128)
        {
            if live.len() - first < 2 {
                self.expiry_rows(template, context, live, strikes)?
            } else {
                let mut rows = self.expiry_rows(template, context, &live[..first], strikes)?;
                rows.extend(self.fixed_surface(
                    template,
                    context,
                    &live[first..],
                    &times[first..],
                    strikes,
                    upper,
                )?);
                rows
            }
        } else if !flat
            && self.settings.asset_upper_boundary.is_none()
            && self.settings.asset_step_count >= 128
            && model.vol_of_vol() > 0.0
            && (model.initial_variance() > 0.0
                || model.mean_reversion_speed() > 0.0 && model.long_run_variance() > 0.0)
        {
            self.dated_surface(template, context, live, strikes)?
        } else {
            self.expiry_rows(template, context, live, strikes)?
        };
        expiries
            .iter()
            .map(|expiry| {
                crate::cancellation::check()?;
                let index = ordered.binary_search(expiry).map_err(|_| unstable())?;
                let mut row = Vec::with_capacity(strikes.len());
                if index < active {
                    for &strike in strikes {
                        crate::cancellation::check()?;
                        row.push(Payoff { strike, ..payoff }.value(spot));
                    }
                } else {
                    for chunk in rows[index - active].chunks(1024) {
                        crate::cancellation::check()?;
                        row.extend_from_slice(chunk);
                    }
                }
                Ok(row)
            })
            .collect()
    }
    fn dated_surface(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        live: &[Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        let model = context.model_parameters().as_heston()?;
        let spot = context.spot_price();
        let valuation = context.valuation_time();
        let mut low = f64::INFINITY;
        let mut high = 0.0_f64;
        let mut queries = Vec::with_capacity(strikes.len());
        for &strike in strikes {
            crate::cancellation::check()?;
            let ratio = spot / strike;
            let normalized = spot * ratio;
            if !normalized.is_normal() || !ratio.is_normal() || !ratio.recip().is_normal() {
                return self.expiry_rows(template, context, live, strikes);
            }
            low = low.min(normalized);
            high = high.max(normalized);
            queries.push((normalized, ratio));
        }
        let anchor = super::super::solution::at_spot(context, high)?;
        let first = live.partition_point(|&expiry| {
            integrated_variance(model, valuation.years_until(expiry)).sqrt() < 0.04
        });
        if live.len() - first < 2 {
            return self.expiry_rows(template, context, live, strikes);
        }
        let settings = self.settings;
        let assets = u64::from(settings.asset_step_count.unsigned_abs()) + 1;
        let variances = u64::from(settings.variance_step_count.unsigned_abs()) + 1;
        let knots: u64 = [model.risk_free_curve(), model.dividend_curve()]
            .iter()
            .map(|curve| curve.nodes().len() as u64)
            .sum();
        let bytes = assets
            .saturating_mul(variances + 8)
            .saturating_mul(320)
            .saturating_add(
                (u64::from(settings.time_step_count.unsigned_abs()) + knots).saturating_mul(256),
            )
            .saturating_add(64 << 20);
        let pool = rollback_pool();
        if pool.is_some_and(|(_, workers)| live.len() - first < 2 * workers) {
            return self.expiry_rows(template, context, live, strikes);
        }
        let parallel = pool.filter(|(_, workers)| bytes <= WORKSPACE_BUDGET / *workers as u64);
        let mut rows = self.expiry_rows(template, context, &live[..first], strikes)?;
        let queries: Arc<[(f64, f64)]> = queries.into();
        let mut jobs = Vec::new();
        let mut remaining = &live[first..];
        while let Some(&first) = remaining.first() {
            crate::cancellation::check()?;
            let maturity = valuation.years_until(first);
            let count = remaining
                .partition_point(|&expiry| 0.5 * valuation.years_until(expiry) <= maturity)
                .clamp(1, 2);
            let (tile, tail) = remaining.split_at(count);
            remaining = tail;
            jobs.push(tile);
        }
        let solve = |tile: &[Timestamp]| {
            if tile.len() == 1 {
                return self.expiry_rows(template, context, tile, strikes);
            }
            let option = template.replace(crate::AmericanOptionChanges {
                expiry_time: tile.last().copied(),
                ..Default::default()
            })?;
            let mut surface = AmericanSurface {
                times: tile
                    .iter()
                    .map(|&expiry| valuation.years_until(expiry))
                    .collect(),
                queries: SurfaceQueries::Dated(Arc::clone(&queries)),
                rows: Vec::with_capacity(tile.len()),
            };
            let payoff = Payoff::vanilla(template.option_type(), spot, true);
            let batch = prepare_batch(
                &option,
                &anchor,
                payoff,
                self.settings,
                low,
                &[spot],
                &[],
                Some(&mut surface),
            );
            match batch {
                Ok(_) if surface.rows.len() == tile.len() => Ok(surface.rows),
                Err(error @ crate::Error::Io(_)) => Err(error),
                _ => self.expiry_rows(template, context, tile, strikes),
            }
        };
        if let Some((pool, _)) = parallel {
            let tokens = crate::cancellation::current();
            let batches: Vec<_> = pool.install(|| {
                jobs.par_iter()
                    .map(|tile| crate::cancellation::with_tokens(tokens.clone(), || solve(tile)))
                    .collect()
            });
            for batch in batches {
                rows.extend(batch?);
            }
        } else {
            for tile in jobs {
                rows.extend(solve(tile)?);
            }
        }
        Ok(rows)
    }

    fn fixed_surface(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        live: &[Timestamp],
        times: &[f64],
        strikes: &[f64],
        upper: f64,
    ) -> Result<Vec<Vec<f64>>> {
        let spot = context.spot_price();
        let mut ordered = strikes.to_vec();
        ordered.sort_unstable_by(f64::total_cmp);
        ordered.dedup();
        if spot.max(ordered[ordered.len() - 1]) >= upper {
            return self.expiry_rows(template, context, live, strikes);
        }
        let last = template.replace(crate::AmericanOptionChanges {
            expiry_time: live.last().copied(),
            ..Default::default()
        })?;
        let mut rows: Vec<_> = live
            .iter()
            .map(|_| Vec::with_capacity(ordered.len()))
            .collect();
        let mut remaining = ordered.as_slice();
        while let Some(&first) = remaining.first() {
            crate::cancellation::check()?;
            let low = first.min(spot);
            let count = remaining
                .partition_point(|&strike| 0.5 * strike.max(spot) <= low)
                .clamp(1, 8);
            let (tile, tail) = remaining.split_at(count);
            remaining = tail;
            let mut surface = AmericanSurface {
                times: times.to_vec(),
                queries: SurfaceQueries::Fixed { spot, count },
                rows: Vec::with_capacity(live.len()),
            };
            let payoff = Payoff::vanilla(template.option_type(), first, true);
            let batch = prepare_batch(
                &last,
                context,
                payoff,
                self.settings,
                low,
                &tile[1..],
                &[],
                Some(&mut surface),
            );
            match batch {
                Ok(_) if surface.rows.len() == rows.len() => {
                    for (row, tile) in rows.iter_mut().zip(surface.rows) {
                        row.extend(tile);
                    }
                }
                Err(error @ crate::Error::Io(_)) => return Err(error),
                _ => {
                    for (row, tile) in rows
                        .iter_mut()
                        .zip(self.expiry_rows(template, context, live, tile)?)
                    {
                        row.extend(tile);
                    }
                }
            }
        }
        let indices = strikes
            .iter()
            .map(|strike| {
                crate::cancellation::check()?;
                ordered
                    .binary_search_by(|part| part.total_cmp(strike))
                    .map_err(|_| unstable())
            })
            .collect::<Result<Vec<_>>>()?;
        rows.iter()
            .map(|row| {
                indices
                    .iter()
                    .map(|&index| {
                        crate::cancellation::check()?;
                        Ok(row[index])
                    })
                    .collect()
            })
            .collect()
    }

    fn expiry_rows(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        live: &[Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        let price = |&expiry| {
            crate::cancellation::check()?;
            let option = template.replace(crate::AmericanOptionChanges {
                expiry_time: Some(expiry),
                ..Default::default()
            })?;
            self.price_american_strikes(&option, context, strikes)
        };
        let settings = self.settings;
        let model = context.model_parameters();
        let curves = [model.risk_free_curve(), model.dividend_curve()];
        let knots: u64 = curves.iter().map(|curve| curve.nodes().len() as u64).sum();
        let assets = u64::from(settings.asset_step_count.unsigned_abs()) + 1;
        let variances = u64::from(settings.variance_step_count.unsigned_abs()) + 1;
        let steps = u64::from(settings.time_step_count.unsigned_abs()) + knots;
        // Only parallelize single-lane jobs with ample room for all workers'
        // grids, curve storage and the per-job recurring-calendar factor cache.
        let bytes = assets
            .saturating_mul(variances + 8)
            .saturating_mul(256)
            .saturating_add(steps.saturating_mul(256))
            .saturating_add(64 << 20);
        let parallel = live.len() >= 2
            && settings.asset_upper_boundary.is_none()
            && bytes <= WORKSPACE_BUDGET / 8
            && assets * variances < 64_000;
        if let Some((pool, _)) = parallel.then(rollback_pool).flatten() {
            let tokens = crate::cancellation::current();
            let results: Vec<_> = pool.install(|| {
                live.par_iter()
                    .map(|expiry| {
                        crate::cancellation::with_tokens(tokens.clone(), || price(expiry))
                    })
                    .collect()
            });
            return results.into_iter().collect();
        }
        live.iter().map(price).collect()
    }
}

pub(super) struct AmericanSurface {
    // Elapsed calendar times in rollback order, corresponding to increasing tenors.
    times: Vec<f64>,
    queries: SurfaceQueries,
    rows: Vec<Vec<f64>>,
}

enum SurfaceQueries {
    Normalized(Vec<(f64, f64)>),
    Dated(Arc<[(f64, f64)]>),
    Fixed { spot: f64, count: usize },
}

impl AmericanSurface {
    pub(super) fn bytes(&self) -> u64 {
        let quotes = match &self.queries {
            SurfaceQueries::Normalized(queries) => queries.len() as u64,
            SurfaceQueries::Dated(queries) => queries.len() as u64,
            SurfaceQueries::Fixed { count, .. } => *count as u64,
        };
        self.times.len() as u64 * (64 + 8 * quotes) + 16 * quotes
    }

    pub(super) fn work(&self, assets: u64) -> u64 {
        let work = match &self.queries {
            SurfaceQueries::Normalized(queries) => assets + queries.len() as u64,
            SurfaceQueries::Dated(queries) => assets + queries.len() as u64,
            SurfaceQueries::Fixed { count, .. } => assets * *count as u64,
        };
        self.times.len() as u64 * work
    }

    pub(super) fn maturities(&self) -> Option<&[f64]> {
        matches!(self.queries, SurfaceQueries::Dated(_)).then_some(&self.times)
    }

    pub(super) fn extend_grid(&self, events: &mut Vec<f64>, end: f64) {
        events.extend_from_slice(&self.times);
        if self.maturities().is_some() {
            return;
        }
        let short = end - self.times[0];
        if short >= 24.0 / 31_536_000_000_000.0 {
            // Resolve the startup payoff kink even when the first tenor is short.
            events.extend((1..24).map(|i| end - (short / 24.0) * f64::from(i)));
        }
    }

    pub(super) fn capture(
        &mut self,
        time: f64,
        mut spline: impl FnMut(usize) -> Spline,
        asset: &SpatialGrid,
        payoffs: &[Payoff],
        scale: f64,
    ) -> Result<()> {
        let normalized_row = |spline: &Spline, payoff: Payoff, queries: &[(f64, f64)]| {
            queries
                .iter()
                .map(|&(spot, ratio)| {
                    crate::cancellation::check()?;
                    crate::numerics::checked(
                        grid_price(asset, spline, payoff, scale, spot)? / ratio,
                    )
                })
                .collect::<Result<Vec<_>>>()
        };
        if let SurfaceQueries::Dated(queries) = &self.queries {
            if time == 0.0 {
                for (lane, &payoff) in payoffs.iter().enumerate() {
                    self.rows
                        .push(normalized_row(&spline(lane), payoff, queries)?);
                }
            }
            return Ok(());
        }
        if self
            .times
            .get(self.rows.len())
            .is_some_and(|&next| next.to_bits() == time.to_bits())
        {
            let row = match &self.queries {
                SurfaceQueries::Normalized(queries) => {
                    normalized_row(&spline(0), payoffs[0], queries)?
                }
                SurfaceQueries::Dated(_) => unreachable!("dated surfaces are sampled at valuation"),
                SurfaceQueries::Fixed { spot, .. } => payoffs
                    .iter()
                    .enumerate()
                    .map(|(lane, &payoff)| {
                        crate::cancellation::check()?;
                        grid_price(asset, &spline(lane), payoff, scale, *spot)
                    })
                    .collect::<Result<_>>()?,
            };
            self.rows.push(row);
        }
        Ok(())
    }
}
