use super::{
    AmericanOption, AmericanOptionChanges, FiniteDifferenceScheme, FiniteDifferenceVanillaEngine,
    Payoff, PricingContext, Result, SpatialGrid, diffusion, invalid_grid, local_volatility, payoff,
};

impl FiniteDifferenceVanillaEngine {
    /// Prices an American strike and expiry surface in input order.
    ///
    /// Compatible flat-rate calendar-time BSM tenors share one rollback on
    /// a normalized log-spot grid. Short tenors and other markets use
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
    /// contract life, or oversized output. Pricing and cancellation errors return
    /// no partial output.
    pub fn price_american_surface(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        expiries: &[crate::Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        local_volatility::validate_model(context)?;
        let valuation = context.valuation_time();
        let budget = super::super::WORKSPACE_BUDGET;
        if (expiries.len() as u64).saturating_mul(64 + (strikes.len() as u64).saturating_mul(16))
            > budget / 4
        {
            return Err(invalid_grid(
                "American surface output exceeds workspace budget",
            ));
        }
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
        let scalar = |dates: &[crate::Timestamp]| {
            dates
                .iter()
                .map(|&expiry| {
                    crate::cancellation::check()?;
                    self.price_american_strikes(&contract(expiry)?, context, strikes)
                })
                .collect::<Result<Vec<_>>>()
        };
        let shared = |sigma: f64| -> Result<Vec<Vec<f64>>> {
            let spot = context.spot_price();
            let mut low = f64::INFINITY;
            let mut high = 0.0_f64;
            let queries = strikes
                .iter()
                .map(|&strike| {
                    crate::cancellation::check()?;
                    let normalized = spot * (spot / strike);
                    let scale = strike / spot;
                    if !normalized.is_normal() || !scale.is_normal() {
                        return Err(super::super::unstable());
                    }
                    low = low.min(normalized);
                    high = high.max(normalized);
                    Ok((normalized, scale))
                })
                .collect::<Result<Vec<_>>>()?;
            if low.min(spot) < 0.5 * high.max(spot) {
                return scalar(&ordered);
            }
            let longest = valuation.years_until(end);
            let first = ordered.partition_point(|&date| {
                let tenor = valuation.years_until(date);
                tenor < 0.25 * longest || sigma * tenor.sqrt() < 0.04
            });
            let (short, live) = ordered.split_at(first);
            if live.len() < 2 {
                return scalar(&ordered);
            }
            let mut surface = AmericanSurface {
                times: live.iter().map(|&expiry| expiry.years_until(end)).collect(),
                queries,
                rows: Vec::with_capacity(live.len()),
            };
            if surface.times.windows(2).any(|pair| pair[0] <= pair[1])
                || surface.times[0] >= longest
            {
                return scalar(&ordered);
            }
            let anchor = super::super::solution::at_spot(context, high)?;
            let payoff = Payoff::vanilla(template.option_type(), spot, true);
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
            let mut rows = scalar(short)?;
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
            match shared(model.volatility()) {
                Ok(rows) => rows,
                Err(error @ crate::Error::Io(_)) => return Err(error),
                Err(_) => scalar(&ordered)?,
            }
        } else {
            scalar(&ordered)?
        };
        expiries
            .iter()
            .map(|expiry| {
                crate::cancellation::check()?;
                let index = ordered
                    .binary_search(expiry)
                    .map_err(|_| super::super::unstable())?;
                let mut output = Vec::with_capacity(strikes.len());
                for chunk in rows[index].chunks(1024) {
                    crate::cancellation::check()?;
                    output.extend_from_slice(chunk);
                }
                Ok(output)
            })
            .collect()
    }
}

pub(in crate::engines::finite_difference) struct AmericanSurface {
    times: Vec<f64>,
    queries: Vec<(f64, f64)>,
    rows: Vec<Vec<f64>>,
}

impl AmericanSurface {
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
