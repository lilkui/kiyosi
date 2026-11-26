use super::grid::{StrikeMoments, concentrate, project_payoff, spot_floor};
use super::{
    EuropeanOption, FiniteDifferenceEngine, FiniteDifferenceScheme, FiniteDifferenceVanillaEngine,
    Payoff, PricingContext, PricingEngine, Result, SpatialGrid, Stepper, diffusion,
    diffusion_log_width, invalid_grid, local_volatility, payoff, validate_life, validate_work,
    validate_workspace,
};

impl FiniteDifferenceVanillaEngine {
    /// Prices European vanilla options across strikes in input order.
    ///
    /// Uses the template's direction and dates. Compatible BSM strikes share a
    /// rescaled solution with automatic boundaries. Compatible calendar-time
    /// local-volatility strikes share an absolute-spot mesh. Outliers can use
    /// individual solves. Check convergence over the full strike range.
    ///
    /// # Errors
    ///
    /// Returns invalid input for nonpositive or non-finite strikes and propagates pricing
    /// and cancellation errors without returning partial results.
    pub fn price_strikes(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        local_volatility::validate_model(context)?;
        validate_life(option, context)?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        let scalar = |strike| {
            crate::cancellation::check()?;
            let contract = option.replace(crate::EuropeanOptionChanges {
                strike: Some(strike),
                ..Default::default()
            })?;
            self.price(&contract, context)
        };
        if strikes.len() < 4
            || self.settings.asset_step_count < 128
            || self.settings.scheme != FiniteDifferenceScheme::Rannacher
            || context.valuation_time() == option.expiry_time()
            || diffusion::uses_business_time(context)
        {
            return strikes.iter().copied().map(scalar).collect();
        }
        let spot = context.spot_price();
        let expiry = option.expiry_time();
        let mut local_strip = None;
        if let Some(local) = local_volatility::local_model(context) {
            let surface = local.surface();
            let at = context.valuation_time();
            let width = surface.volatility(at, spot)? * at.years_until(expiry).sqrt();
            if width >= 0.04 && surface.constant_volatility_between(at, expiry)?.is_none() {
                local_strip = Some((surface, width));
            }
        }
        let normalized = self.settings.asset_upper_boundary.is_none()
            && matches!(context.model_parameters(), crate::ModelParameters::BlackScholesMerton(model)
                if model.volatility() * context.valuation_time().years_until(expiry).sqrt() >= 0.04);
        if !normalized && local_strip.is_none() {
            return strikes.iter().copied().map(scalar).collect();
        }

        let mut ordered = Vec::new();
        ordered
            .try_reserve_exact(strikes.len())
            .map_err(|_| super::unstable())?;
        ordered.extend(strikes.iter().copied().enumerate());
        ordered.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
        crate::cancellation::check()?;
        let mut prices = Vec::new();
        prices
            .try_reserve_exact(strikes.len())
            .map_err(|_| super::unstable())?;
        prices.resize(strikes.len(), 0.0);
        let mut remaining = ordered.as_slice();
        while let Some(&(_, first)) = remaining.first() {
            crate::cancellation::check()?;
            let low = spot.min(first);
            let count = if low >= 0.5 * spot {
                remaining
                    .partition_point(|&(_, strike)| 0.5 * strike <= low)
                    .max(1)
            } else {
                1
            };
            let (group, tail) = remaining.split_at(count);
            remaining = tail;
            let last = group[count - 1].1;
            if count >= 4 && first < last {
                let fast = if let Some((surface, width)) = local_strip {
                    MarketStrip::new(self, option, context, surface, [low, spot.max(last)], width)
                        .and_then(|batch| {
                            for &(i, strike) in group {
                                crate::cancellation::check()?;
                                let payoff = Payoff::vanilla(option.option_type(), strike, false);
                                prices[i] = batch.price(payoff, spot)?;
                            }
                            Ok(())
                        })
                } else {
                    self.normalized_strikes(option, context, group, &mut prices)
                };
                match fast {
                    Ok(()) => continue,
                    Err(error @ crate::Error::Io(_)) => return Err(error),
                    // A shared grid can fail even when individual grids are representable.
                    Err(_) => {}
                }
            }
            for &(i, strike) in group {
                prices[i] = scalar(strike)?;
            }
        }
        Ok(prices)
    }

    fn normalized_strikes(
        &self,
        template: &EuropeanOption,
        context: &PricingContext,
        group: &[(usize, f64)],
        prices: &mut [f64],
    ) -> Result<()> {
        let spot = context.spot_price();
        let option = template.replace(crate::EuropeanOptionChanges {
            strike: Some(spot),
            ..Default::default()
        })?;
        let normalized_spot = |strike| spot * (spot / strike);
        let low = normalized_spot(group[group.len() - 1].1);
        let high = normalized_spot(group[0].1);
        if !low.is_normal() || !high.is_normal() || low >= high {
            return Err(super::unstable());
        }
        let solved = self.solve(&option, context, low..=high)?;
        for &(i, strike) in group {
            prices[i] =
                crate::numerics::checked(solved.price(normalized_spot(strike))? * (strike / spot))?;
        }
        Ok(())
    }
}

struct MarketStrip {
    space: SpatialGrid,
    weights: Vec<f64>,
    edges: Vec<[f64; 3]>,
    terminal: StrikeMoments,
    boundary: StrikeMoments,
    discount: [f64; 2],
}

impl MarketStrip {
    fn new(
        engine: &FiniteDifferenceVanillaEngine,
        option: &EuropeanOption,
        context: &PricingContext,
        surface: &crate::LocalVolatilitySurface,
        [low, high]: [f64; 2],
        width: f64,
    ) -> Result<Self> {
        let expiry = option.expiry_time();
        let valuation = context.valuation_time();
        let time = valuation.years_until(expiry);
        let spot = context.spot_price();
        let settings = engine.settings;
        let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
        let domain_width =
            (1.5 * diffusion_log_width(context, expiry, &risk, &dividend)?).max(4.0_f64.ln());
        let lower = spot_floor(low, low, domain_width);
        let upper = settings
            .asset_upper_boundary
            .unwrap_or_else(|| super::scaled(high, domain_width));
        if upper <= high {
            return Err(invalid_grid(
                "finite-difference upper bound must exceed spot and strike",
            ));
        }
        let mut space = SpatialGrid::new(settings.asset_step_count, [lower, upper], spot)?;
        concentrate(&mut space, vec![spot], 0.3 * width)?;
        let node = space.nearest(spot);
        if space.asset(node).to_bits() != spot.to_bits() {
            return Err(super::unstable());
        }
        let time_grid = diffusion::time_grid(context, expiry, settings, &risk, &dividend, &[])?;
        let steps = time_grid.rollback_steps(settings.scheme, &[])?;
        validate_work(space.count as u64, steps)?;
        validate_workspace(
            settings.scheme,
            2 * (space.count as u64 + 1),
            steps,
            risk.workspace_bytes() + dividend.workspace_bytes() + 256 * steps,
        )?;
        let mut weights = vec![0.0; space.count + 1];
        let mut previous = weights.clone();
        weights[node] = 1.0;
        let mut edges = Vec::new();
        let mut stepper = Stepper::new_local(space.count);
        let offset = surface.times()[0].years_until(valuation);
        for step in 0..time_grid.widths.len() {
            crate::cancellation::check()?;
            let mut damping = u32::try_from(time_grid.widths.len() - step).unwrap_or(u32::MAX);
            // The first two intervals rolled back from expiry consist of two half-steps.
            damping = if damping <= 2 { damping } else { 0 };
            let mut substeps = time_grid.substeps(step, settings.scheme, &mut damping);
            let later = substeps.next().expect("each interval has a substep");
            for (current, dt, theta) in substeps.chain([later]) {
                let middle = current + 0.5 * dt;
                let contribution = stepper.transpose_local(
                    &mut weights,
                    &mut previous,
                    &space,
                    surface,
                    offset + middle,
                    [dt, theta, risk.rate(middle), dividend.rate(middle)],
                )?;
                let cash = risk.log_factor(current, time).exp();
                let asset = dividend.log_factor(current, time).exp();
                for (bound, weight) in [lower, upper].into_iter().zip(contribution) {
                    let present = bound * asset;
                    if !present.is_normal() || !cash.is_normal() || !(present / cash).is_normal() {
                        return Err(super::unstable());
                    }
                    edges.push([present / cash, weight * present, weight * cash]);
                }
                std::mem::swap(&mut weights, &mut previous);
            }
        }
        let sign = Payoff::vanilla(option.option_type(), option.strike(), false).sign;
        let terminal = StrikeMoments::new(
            space
                .spots
                .iter()
                .zip(&weights)
                .map(|(&spot, &weight)| [spot, spot * weight, weight])
                .collect(),
            sign,
        );
        let boundary = StrikeMoments::new(edges.clone(), sign);
        Ok(Self {
            space,
            weights,
            edges,
            terminal,
            boundary,
            discount: [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)],
        })
    }

    fn price(&self, payoff: Payoff, spot: f64) -> Result<f64> {
        let strike = payoff.strike;
        if let Some(([mut value, scale], [edge, edge_scale])) =
            self.terminal.at(strike).zip(self.boundary.at(strike))
        {
            let coordinate = self.space.coordinate(strike);
            let right = self.space.nodes.partition_point(|&node| node < coordinate);
            for index in right.saturating_sub(1)..=(right.min(self.space.count)) {
                let payment =
                    project_payoff(&self.space, &[strike], index, |spot| payoff.value(spot));
                value += self.weights[index] * (payment - payoff.value(self.space.asset(index)));
            }
            value += edge;
            if value.is_finite()
                && (scale + edge_scale == 0.0 || value.abs() > 1e-6 * (scale + edge_scale))
            {
                return payoff::checked_price(payoff, spot, value, self.discount, 0.0);
            }
        }
        let mut value = crate::numerics::Sum::default();
        for (index, &weight) in self.weights.iter().enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            value.add(
                weight * project_payoff(&self.space, &[strike], index, |spot| payoff.value(spot)),
            );
        }
        for (index, &[cutoff, asset, cash]) in self.edges.iter().enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            if payoff.sign * (cutoff - strike) > 0.0 {
                value.add(payoff.sign * (-strike).mul_add(cash, asset));
            }
        }
        payoff::checked_price(payoff, spot, value.total(), self.discount, 0.0)
    }
}
