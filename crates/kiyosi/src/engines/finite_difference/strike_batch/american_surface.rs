use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    AmericanOption, AmericanOptionChanges, FiniteDifferenceScheme, FiniteDifferenceVanillaEngine,
    Payoff, PricingContext, Result, SpatialGrid, american, diffusion, invalid_grid,
    local_volatility, payoff, strike_group_len, surface::fallback_work, validate_work,
};

#[derive(Default)]
pub(super) struct SurfaceWork(AtomicU64);

impl SurfaceWork {
    pub(super) fn charge(&self, work: u64) -> Result<()> {
        validate_work(1, work)?;
        let previous = self.0.fetch_add(work, Ordering::Relaxed);
        validate_work(1, previous + work)
    }
}

impl FiniteDifferenceVanillaEngine {
    /// Prices an American strike and expiry surface in input order.
    ///
    /// Compatible flat-rate calendar-time BSM strikes and tenors share a
    /// rollback per strike group on a normalized log-spot grid. Compatible local-volatility
    /// tenors share step factors on an absolute spot grid. Other strips use
    /// [`Self::price_american_strikes`]. Check convergence over the surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{AmericanOption, BlackScholesMertonParameters, Date,
    ///     FiniteDifferenceVanillaEngine, OptionType, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = start.add_days(365)?;
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    /// let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.25)?;
    /// let market = PricingContext::new(model, 100.0, start)?;
    /// let rows = FiniteDifferenceVanillaEngine::default().price_american_surface(
    ///     &put, &market, &[start.add_days(180)?, end], &[90.0, 100.0, 110.0],
    /// )?;
    /// assert!(rows[1][1] > rows[0][1]);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for invalid strikes, expiries outside
    /// contract life, oversized output, or excessive combined rollback work.
    /// Pricing and cancellation errors return no partial output.
    pub fn price_american_surface(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        expiries: &[crate::Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        local_volatility::validate_model(context)?;
        let valuation = context.valuation_time();
        super::super::validate_finite_difference_surface_size(expiries.len(), strikes.len())?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        for &expiry in expiries {
            crate::cancellation::check()?;
            if expiry < valuation || valuation < template.effective_time() {
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
        let end = ordered[ordered.len() - 1];
        context.validate_curve_coverage(end)?;
        if strikes.is_empty() {
            return Ok(vec![Vec::new(); expiries.len()]);
        }
        let contract = |expiry| {
            template.replace(AmericanOptionChanges {
                expiry_time: Some(expiry),
                ..Default::default()
            })
        };
        let mut unique = strikes.to_vec();
        unique.sort_unstable_by(f64::total_cmp);
        unique.dedup_by(|a, b| a.to_bits() == b.to_bits());
        let columns = strikes
            .iter()
            .map(|strike| {
                crate::cancellation::check()?;
                unique
                    .binary_search_by(|candidate| candidate.total_cmp(strike))
                    .map_err(|_| super::super::unstable())
            })
            .collect::<Result<Vec<_>>>()?;
        let budget = SurfaceWork::default();
        let local = local_volatility::local_model(context);
        let scalar = |dates: &[crate::Timestamp], strikes: &[f64]| {
            budget.charge(fallback_work(
                context,
                self.settings,
                dates,
                strikes,
                local.map(|_| american::LOCAL_LANES as u64),
            )?)?;
            dates
                .iter()
                .map(|&expiry| {
                    crate::cancellation::check()?;
                    self.price_american_strikes(&contract(expiry)?, context, strikes)
                })
                .collect::<Result<Vec<_>>>()
        };
        let shared = |sigma: f64, strikes: &[f64]| -> Result<Vec<Vec<f64>>> {
            let spot = context.spot_price();
            // A singleton can roll back its own payoff on the original spot grid.
            let reference = if let [strike] = strikes {
                *strike
            } else {
                spot
            };
            let mut low = f64::INFINITY;
            let mut high = 0.0_f64;
            let queries = strikes
                .iter()
                .map(|&strike| {
                    crate::cancellation::check()?;
                    let normalized = spot * (reference / strike);
                    let scale = strike / reference;
                    if !normalized.is_normal() || !scale.is_normal() {
                        return Err(super::super::unstable());
                    }
                    low = low.min(normalized);
                    high = high.max(normalized);
                    Ok((normalized, scale))
                })
                .collect::<Result<Vec<_>>>()?;
            if low.min(spot) < 0.5 * high.max(spot) {
                return scalar(&ordered, strikes);
            }
            let longest = valuation.years_until(end);
            let first = ordered.partition_point(|&date| {
                let tenor = valuation.years_until(date);
                tenor < 0.25 * longest || sigma * tenor.sqrt() < 0.04
            });
            let (short, live) = ordered.split_at(first);
            if live.len() < 2 {
                return scalar(&ordered, strikes);
            }
            let mut surface = AmericanSurface {
                times: live.iter().map(|&expiry| expiry.years_until(end)).collect(),
                queries,
                rows: Vec::with_capacity(live.len()),
                budget: &budget,
            };
            if surface.times.windows(2).any(|pair| pair[0] <= pair[1])
                || surface.times[0] >= longest
            {
                return scalar(&ordered, strikes);
            }
            let anchor = super::super::solution::at_spot(context, high)?;
            let payoff = Payoff::vanilla(template.option_type(), reference, true);
            super::super::solve_model(
                end,
                payoff,
                &anchor,
                self.settings,
                true,
                low,
                sigma,
                None,
                Some(&mut surface),
            )?;
            if surface.rows.len() != live.len() {
                return Err(super::super::unstable());
            }
            let mut rows = scalar(short, strikes)?;
            rows.extend(surface.rows);
            Ok(rows)
        };
        let settings = self.settings;
        let model = context.bsm_parameters().ok().filter(|model| {
            settings.asset_upper_boundary.is_none()
                && settings.scheme == FiniteDifferenceScheme::Rannacher
                && settings.asset_step_count >= 128
                && !diffusion::uses_business_time(context)
                && model.risk_free_curve().exact_flat_rate().is_some()
                && model.dividend_curve().exact_flat_rate().is_some()
        });
        let rows = if let Some(model) = model {
            let mut rows: Vec<_> = ordered
                .iter()
                .map(|_| Vec::with_capacity(unique.len()))
                .collect();
            let mut remaining = unique.as_slice();
            while !remaining.is_empty() {
                crate::cancellation::check()?;
                let (group, tail) =
                    remaining.split_at(strike_group_len(context.spot_price(), remaining, |&k| k));
                let prices = match shared(model.volatility(), group) {
                    Ok(prices) => prices,
                    Err(error @ crate::Error::Io(_)) => return Err(error),
                    Err(_) => scalar(&ordered, group)?,
                };
                for (row, prices) in rows.iter_mut().zip(prices) {
                    row.extend(prices);
                }
                remaining = tail;
            }
            rows
        } else if let Some(local) = local {
            american::local_surface(
                self,
                template,
                context,
                (local, &ordered, &unique),
                scalar,
                &budget,
            )?
        } else {
            scalar(&ordered, &unique)?
        };
        expiries
            .iter()
            .map(|expiry| {
                crate::cancellation::check()?;
                let index = ordered
                    .binary_search(expiry)
                    .map_err(|_| super::super::unstable())?;
                columns
                    .iter()
                    .map(|&column| {
                        crate::cancellation::check()?;
                        Ok(rows[index][column])
                    })
                    .collect()
            })
            .collect()
    }
}

pub(in crate::engines::finite_difference) struct AmericanSurface<'a> {
    times: Vec<f64>,
    queries: Vec<(f64, f64)>,
    rows: Vec<Vec<f64>>,
    budget: &'a SurfaceWork,
}

impl AmericanSurface<'_> {
    pub(in crate::engines::finite_difference) fn charge(
        &self,
        assets: u64,
        steps: u64,
    ) -> Result<()> {
        self.budget
            .charge(assets.saturating_mul(steps).saturating_add(
                (assets + self.queries.len() as u64).saturating_mul(self.times.len() as u64),
            ))
    }

    pub(in crate::engines::finite_difference) fn times(&self) -> &[f64] {
        &self.times
    }

    pub(in crate::engines::finite_difference) fn capture(
        &mut self,
        time: f64,
        space: &SpatialGrid,
        values: &[f64],
        payoff: Payoff,
    ) -> Result<()> {
        if self
            .times
            .get(self.rows.len())
            .is_some_and(|next| next.to_bits() == time.to_bits())
        {
            let spline = space.spline(values.to_vec());
            let row = self
                .queries
                .iter()
                .map(|&(spot, scale)| {
                    crate::cancellation::check()?;
                    let [mut price, ..] = space.sample(&spline, spot);
                    if space.exercise_cell(&spline, spot, |asset| payoff.value(asset)) {
                        price = payoff.value(spot);
                    }
                    let price = payoff::checked_price(payoff, spot, price, [0.0; 2], 0.0)?;
                    crate::numerics::checked(price * scale)
                })
                .collect::<Result<_>>()?;
            self.rows.push(row);
        }
        Ok(())
    }
}
