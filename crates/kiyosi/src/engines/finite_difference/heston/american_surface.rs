use super::{
    AmericanOption, FiniteDifferenceHestonVanillaEngine, Payoff, PricingContext, Result,
    SpatialGrid, Spline, Timestamp, WORKSPACE_BUDGET, grid_price, invalid_grid, prepare_batch,
    unstable,
};

impl FiniteDifferenceHestonVanillaEngine {
    /// Prices an American strike and expiry surface in input order.
    ///
    /// With flat rates, automatic asset bounds and time-homogeneous stochastic
    /// dynamics, one rollback supplies prices at each time to maturity. Other
    /// markets use independent expiry rollbacks. The longest maturity sets the
    /// shared mesh. Check convergence at the shortest maturity and across strikes.
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
        if (expiries.len() as u64).saturating_mul(64 + (strikes.len() as u64).saturating_mul(16))
            > WORKSPACE_BUDGET / 4
        {
            return Err(invalid_grid(
                "Heston surface output exceeds workspace budget",
            ));
        }
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
        let mut share = live.len() >= 2
            && self.settings.asset_upper_boundary.is_none()
            && model.risk_free_curve().exact_flat_rate().is_some()
            && model.dividend_curve().exact_flat_rate().is_some()
            && (constant_variance
                || (model.vol_of_vol() > 0.0
                    && (model.initial_variance() > 0.0
                        || (model.mean_reversion_speed() > 0.0
                            && model.long_run_variance() > 0.0))));
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
        let times = if share {
            live.iter()
                .map(|&expiry| expiry.years_until(end))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let rows = if share
            && times[0] < valuation.years_until(end)
            && times.windows(2).all(|pair| pair[0] > pair[1])
        {
            let mut surface = AmericanSurface {
                times,
                queries,
                rows: Vec::with_capacity(live.len()),
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
            if surface.rows.len() != live.len() {
                return Err(unstable());
            }
            surface.rows
        } else {
            live.iter()
                .map(|&expiry| {
                    crate::cancellation::check()?;
                    self.price_american_strikes(&contract(expiry)?, context, strikes)
                })
                .collect::<Result<Vec<_>>>()?
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
}

pub(super) struct AmericanSurface {
    // Elapsed calendar times in rollback order, corresponding to increasing tenors.
    times: Vec<f64>,
    queries: Vec<(f64, f64)>,
    rows: Vec<Vec<f64>>,
}

impl AmericanSurface {
    pub(super) fn bytes(&self) -> u64 {
        self.times.len() as u64 * (64 + 8 * self.queries.len() as u64)
            + 16 * self.queries.len() as u64
    }

    pub(super) fn work(&self, assets: u64) -> u64 {
        self.times.len() as u64 * (assets + self.queries.len() as u64)
    }

    pub(super) fn extend_grid(&self, events: &mut Vec<f64>, end: f64) {
        events.extend_from_slice(&self.times);
        let short = end - self.times[0];
        if short >= 24.0 / 31_536_000_000_000.0 {
            // Resolve the startup payoff kink even when the first tenor is short.
            events.extend((1..24).map(|i| end - (short / 24.0) * f64::from(i)));
        }
    }

    pub(super) fn capture(
        &mut self,
        time: f64,
        spline: impl FnOnce() -> Spline,
        asset: &SpatialGrid,
        payoff: Payoff,
        scale: f64,
    ) -> Result<()> {
        if self
            .times
            .get(self.rows.len())
            .is_some_and(|&next| next.to_bits() == time.to_bits())
        {
            let spline = spline();
            let mut row = Vec::with_capacity(self.queries.len());
            for &(spot, ratio) in &self.queries {
                crate::cancellation::check()?;
                row.push(crate::numerics::checked(
                    grid_price(asset, &spline, payoff, scale, spot)? / ratio,
                )?);
            }
            self.rows.push(row);
        }
        Ok(())
    }
}
