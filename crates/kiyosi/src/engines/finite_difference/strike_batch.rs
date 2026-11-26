use super::grid::{StrikeMoments, concentrate, project_payoff, spot_floor};
use super::{
    EuropeanOption, FiniteDifferenceScheme, FiniteDifferenceVanillaEngine, Payoff, PricingContext,
    PricingEngine, Result, SpatialGrid, Stepper, diffusion, diffusion_log_width, invalid_grid,
    local_volatility, payoff, validate_life, validate_work, validate_workspace,
};

impl FiniteDifferenceVanillaEngine {
    /// Prices European vanilla options across strikes in input order.
    ///
    /// Uses the template's direction and dates. Compatible calendar-time local-volatility
    /// requests propagate pricing weights on a common absolute-spot mesh. Other requests
    /// use scalar pricing. Check spatial and temporal convergence over the full strike range.
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
        let mut prices = Vec::new();
        prices
            .try_reserve_exact(strikes.len())
            .map_err(|_| super::unstable())?;
        if let Some(local) = local_volatility::local_model(context).filter(|_| {
            strikes.len() >= 4
                && self.settings.asset_step_count >= 128
                && self.settings.scheme == FiniteDifferenceScheme::Rannacher
                && context.valuation_time() < option.expiry_time()
                && !diffusion::uses_business_time(context)
        }) {
            let surface = local.surface();
            let spot = context.spot_price();
            let (low, high) = strikes.iter().fold((spot, spot), |(low, high), &strike| {
                (low.min(strike), high.max(strike))
            });
            let expiry = option.expiry_time();
            let width = surface.volatility(context.valuation_time(), spot)?
                * context.valuation_time().years_until(expiry).sqrt();
            if high * 0.5 <= low
                && width >= 0.04
                && surface
                    .constant_volatility_between(context.valuation_time(), expiry)?
                    .is_none()
            {
                let solve = MarketStrip::new(self, option, context, surface, [low, high], width)
                    .and_then(|batch| {
                        for &strike in strikes {
                            crate::cancellation::check()?;
                            let payoff = Payoff::vanilla(option.option_type(), strike, false);
                            prices.push(batch.price(payoff, spot)?);
                        }
                        Ok(())
                    });
                match solve {
                    Ok(()) => return Ok(prices),
                    Err(crate::Error::ComputationFailure(_)) => prices.clear(),
                    Err(error) => return Err(error),
                }
            }
        }
        for &strike in strikes {
            crate::cancellation::check()?;
            let contract = option.replace(crate::EuropeanOptionChanges {
                strike: Some(strike),
                ..Default::default()
            })?;
            prices.push(self.price(&contract, context)?);
        }
        Ok(prices)
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
