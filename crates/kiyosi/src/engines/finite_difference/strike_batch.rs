mod american;
mod american_surface;
mod surface;

pub(super) use american_surface::AmericanSurface;

use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

use crate::{
    AmericanOption, AmericanOptionChanges, FiniteDifferenceHestonVanillaEngine,
    FiniteDifferenceSolution, Timestamp,
};

use super::diffusion::{BusinessDiffusion, Diffusion};
use super::grid::{StrikeMoments, concentrate, project_payoff, spot_floor};
use super::{
    EuropeanOption, FiniteDifferenceEngine, FiniteDifferenceScheme, FiniteDifferenceVanillaEngine,
    Payoff, PricingContext, PricingEngine, Result, RollbackWork, SpatialGrid, Stepper, diffusion,
    diffusion_log_width, invalid_grid, local_volatility, payoff, validate_life, validate_work,
    validate_workspace,
};

/// Prices batches of vanilla options under one market snapshot.
///
/// The template selects European or American exercise. See [`Self::price_surface`]
/// for an example.
pub trait VanillaBatchEngine<I = EuropeanOption>: PricingEngine<I> {
    /// Prices a template across strikes in input order, including duplicates.
    ///
    /// The template supplies direction and contract dates. Compatible strikes
    /// share numerical work. Check convergence over the requested strike range.
    /// The example in [`Self::price_surface`] also works for a single expiry with
    /// `engine.price_strikes(&template, &market, &strikes)?`.
    ///
    /// # Errors
    ///
    /// Rejects nonpositive or non-finite strikes and requests exceeding work or
    /// workspace limits. Pricing and cancellation failures return no partial prices.
    fn price_strikes(
        &self,
        template: &I,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>>;

    /// Prices a strike–expiry surface with one row per expiry and one column per strike.
    ///
    /// Preserves input order and duplicate dates and strikes. The template supplies
    /// direction and effective time. Expiries must be at or after valuation and
    /// the effective time. American valuation must be at or after the effective time.
    /// Compatible expiries share grids or rollback work. Check convergence over
    /// the requested surface.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{AmericanOption, BlackScholesMertonParameters, Date,
    ///     FiniteDifferenceVanillaEngine, OptionType, PricingContext, VanillaBatchEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 2)?.start_of_day();
    /// let expiries = [start.add_days(90)?, start.add_days(180)?];
    /// let strikes = [95.0, 100.0, 105.0];
    /// let template = AmericanOption::new(OptionType::Put, 100.0, start, expiries[1])?;
    /// let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    /// let market = PricingContext::new(model, 100.0, start)?;
    /// let engine = FiniteDifferenceVanillaEngine::default();
    /// let prices = engine.price_strikes(&template, &market, &strikes)?;
    /// assert!(prices[0] < prices[2]);
    /// let rows = engine.price_surface(&template, &market, &expiries, &strikes)?;
    /// assert_eq!(rows.len(), expiries.len());
    /// assert_eq!(rows[0].len(), strikes.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Rejects invalid dates or strikes and requests exceeding work or workspace
    /// limits. Pricing and cancellation failures return no partial surface.
    fn price_surface(
        &self,
        template: &I,
        context: &PricingContext,
        expiries: &[Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>>;
}

macro_rules! vanilla_batch {
    ($engine:ty, $option:ty, $strikes:ident, $surface:ident) => {
        impl VanillaBatchEngine<$option> for $engine {
            #[inline]
            fn price_strikes(
                &self,
                template: &$option,
                context: &PricingContext,
                strikes: &[f64],
            ) -> Result<Vec<f64>> {
                self.$strikes(template, context, strikes)
            }

            #[inline]
            fn price_surface(
                &self,
                template: &$option,
                context: &PricingContext,
                expiries: &[Timestamp],
                strikes: &[f64],
            ) -> Result<Vec<Vec<f64>>> {
                self.$surface(template, context, expiries, strikes)
            }
        }
    };
}

vanilla_batch!(
    FiniteDifferenceVanillaEngine,
    EuropeanOption,
    price_european_strikes,
    price_european_surface
);
vanilla_batch!(
    FiniteDifferenceVanillaEngine,
    AmericanOption,
    price_american_strikes,
    price_american_surface
);
vanilla_batch!(
    FiniteDifferenceHestonVanillaEngine,
    EuropeanOption,
    price_european_strikes,
    price_european_surface
);
vanilla_batch!(
    FiniteDifferenceHestonVanillaEngine,
    AmericanOption,
    price_american_strikes,
    price_american_surface
);

impl FiniteDifferenceVanillaEngine {
    pub(super) fn price_european_strikes(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        local_volatility::validate_model(context)?;
        validate_life(option, context)?;
        super::validate_finite_difference_strike_batch_size(strikes.len())?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        if strikes.is_empty() {
            return Ok(Vec::new());
        }
        let budget = RollbackWork::default();
        let work = surface::strike_rollback_work(context, self.settings, option.expiry_time())?;
        let scalar = |strike| {
            crate::cancellation::check()?;
            budget.charge(work)?;
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
        {
            return scalar_strikes(strikes, work, scalar);
        }
        let spot = context.spot_price();
        let expiry = option.expiry_time();
        let valuation = context.valuation_time();
        let root_time = valuation.years_until(expiry).sqrt();
        let business_time = diffusion::uses_business_time(context);
        let mut strip = None;
        if let Some(local) = local_volatility::local_model(context) {
            let surface = local.surface();
            let root = if business_time {
                local
                    .variance_time_convention()
                    .year_fraction(valuation, expiry)?
                    .sqrt()
            } else {
                root_time
            };
            let width = surface.volatility(valuation, spot)? * root;
            if width >= 0.04
                && (business_time
                    || surface
                        .constant_volatility_between(valuation, expiry)?
                        .is_none())
            {
                strip = Some((StripModel::Local(surface), width));
            }
        }
        let mut normalized = false;
        if let crate::ModelParameters::BlackScholesMerton(model) = context.model_parameters() {
            let sigma = model.volatility();
            let root_variance_time =
                if business_time && self.settings.asset_upper_boundary.is_none() {
                    model
                        .variance_time_convention()
                        .year_fraction(valuation, expiry)?
                        .sqrt()
                } else {
                    root_time
                };
            let width = sigma * root_variance_time;
            if width >= 0.04 {
                normalized = self.settings.asset_upper_boundary.is_none();
                if !normalized && !business_time {
                    strip = Some((StripModel::Constant(sigma), width));
                }
            }
        }
        if !normalized && strip.is_none() {
            return scalar_strikes(strikes, work, scalar);
        }

        batch_strikes(spot, strikes, scalar, |group, prices| {
            if let Some((model, width)) = strip {
                let range = [group[0].1, group[group.len() - 1].1];
                let batch = MarketStrip::new(self, option, context, model, range, width, &budget)?;
                for &(i, strike) in group {
                    crate::cancellation::check()?;
                    let payoff = Payoff::vanilla(option.option_type(), strike, false);
                    prices[i] = batch.price(payoff, spot)?;
                }
                Ok(())
            } else {
                normalized_strikes(spot, group, prices, |range| {
                    budget.charge(work)?;
                    let option = option.replace(crate::EuropeanOptionChanges {
                        strike: Some(spot),
                        ..Default::default()
                    })?;
                    self.solve(&option, context, range)
                })
            }
        })
    }

    pub(super) fn price_american_strikes(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        local_volatility::validate_model(context)?;
        validate_life(option, context)?;
        super::validate_finite_difference_strike_batch_size(strikes.len())?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        if strikes.is_empty() {
            return Ok(Vec::new());
        }
        let budget = RollbackWork::default();
        let work = surface::strike_rollback_work(context, self.settings, option.expiry_time())?;
        let at_strike = |strike| {
            option.replace(AmericanOptionChanges {
                strike: Some(strike),
                ..Default::default()
            })
        };
        let scalar = |strike| {
            crate::cancellation::check()?;
            budget.charge(work)?;
            self.price(&at_strike(strike)?, context)
        };
        if strikes.len() < 4
            || self.settings.asset_step_count < 128
            || self.settings.scheme != FiniteDifferenceScheme::Rannacher
            || context.valuation_time() == option.expiry_time()
        {
            return scalar_strikes(strikes, work, scalar);
        }
        let spot = context.spot_price();
        if local_volatility::local_model(context).is_some() {
            return batch_strikes(spot, strikes, scalar, |group, prices| {
                american::local_strikes(self, option, context, group, prices, &budget)
            });
        }
        if self.settings.asset_upper_boundary.is_some() {
            return scalar_strikes(strikes, work, scalar);
        }
        let crate::ModelParameters::BlackScholesMerton(model) = context.model_parameters() else {
            return scalar_strikes(strikes, work, scalar);
        };
        let time = model
            .variance_time_convention()
            .year_fraction(context.valuation_time(), option.expiry_time())?;
        if model.volatility() * time.sqrt() < 0.04 {
            return scalar_strikes(strikes, work, scalar);
        }
        batch_strikes(spot, strikes, scalar, |group, prices| {
            normalized_strikes(spot, group, prices, |range| {
                budget.charge(work)?;
                self.solve(&at_strike(spot)?, context, range)
            })
        })
    }
}

fn scalar_strikes(
    strikes: &[f64],
    work: u64,
    scalar: impl Fn(f64) -> Result<f64>,
) -> Result<Vec<f64>> {
    if validate_work(work, strikes.len() as u64).is_err() {
        let mut distinct = HashSet::new();
        for &strike in strikes {
            crate::cancellation::check()?;
            if distinct.insert(strike.to_bits()) {
                validate_work(work, distinct.len() as u64)?;
            }
        }
    }
    let mut quoted = HashMap::new();
    strikes
        .iter()
        .map(|&strike| {
            crate::cancellation::check()?;
            match quoted.entry(strike.to_bits()) {
                std::collections::hash_map::Entry::Occupied(entry) => Ok(*entry.get()),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    Ok(*entry.insert(scalar(strike)?))
                }
            }
        })
        .collect()
}

// A grid is sized for this strike group and the quoted spot.
fn strike_group_len<T>(spot: f64, remaining: &[T], strike: impl Fn(&T) -> f64) -> usize {
    let first = strike(&remaining[0]);
    let low = spot.min(first);
    if low >= 0.5 * spot && first <= 2.0 * low {
        remaining.partition_point(|item| 0.5 * strike(item) <= low)
    } else {
        remaining.partition_point(|item| strike(item).to_bits() == first.to_bits())
    }
}

fn batch_strikes(
    spot: f64,
    strikes: &[f64],
    scalar: impl Fn(f64) -> Result<f64>,
    shared: impl Fn(&[(usize, f64)], &mut [f64]) -> Result<()>,
) -> Result<Vec<f64>> {
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
        let count = strike_group_len(spot, remaining, |&(_, strike)| strike);
        let (group, tail) = remaining.split_at(count);
        remaining = tail;
        let last = group[count - 1].1;
        if count >= 4 && first < last {
            match shared(group, &mut prices) {
                Ok(()) => continue,
                Err(error @ crate::Error::Io(_)) => return Err(error),
                // A shared grid can fail even when individual grids are representable.
                Err(_) => {}
            }
        }
        for same in group.chunk_by(|a, b| a.1.to_bits() == b.1.to_bits()) {
            crate::cancellation::check()?;
            let price = scalar(same[0].1)?;
            for &(i, _) in same {
                prices[i] = price;
            }
        }
    }
    Ok(prices)
}

fn normalized_strikes(
    spot: f64,
    group: &[(usize, f64)],
    prices: &mut [f64],
    solve: impl FnOnce(RangeInclusive<f64>) -> Result<FiniteDifferenceSolution>,
) -> Result<()> {
    let normalized_spot = |strike| spot * (spot / strike);
    let low = normalized_spot(group[group.len() - 1].1);
    let high = normalized_spot(group[0].1);
    if !low.is_normal() || !high.is_normal() || low >= high {
        return Err(super::unstable());
    }
    let solved = solve(low..=high)?;
    for &(i, strike) in group {
        prices[i] =
            crate::numerics::checked(solved.price(normalized_spot(strike))? * (strike / spot))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum StripModel<'a> {
    Constant(f64),
    Local(&'a crate::LocalVolatilitySurface),
}

struct BoundaryTerm {
    spot: f64,
    weight: f64,
    discount: [f64; 2],
    remaining: f64,
}

impl BoundaryTerm {
    fn value(&self, payoff: Payoff, sigma: f64) -> f64 {
        self.weight
            * payoff::boundary_value(payoff, self.spot, self.discount, sigma, self.remaining)
    }

    fn bounded_intrinsic(&self, sigma: f64, range: [f64; 2], budget: f64) -> Option<[f64; 3]> {
        let [risk, dividend] = self.discount;
        let cash = risk.exp();
        let asset = super::scaled(self.spot, dividend);
        let forward = asset / cash;
        let weighted = [self.weight * asset, self.weight * cash];
        if !asset.is_normal()
            || !cash.is_normal()
            || !forward.is_normal()
            || !weighted.iter().all(|x| x.is_finite())
        {
            return None;
        }
        // Time value is an OTM put below the forward and an OTM call above it.
        // Both are largest at the strike nearest the forward. Dropping the
        // subtracted leg bounds the error without subtracting nearly equal terms.
        let strike = forward.clamp(range[0], range[1]);
        let moneyness = crate::numerics::log_ratio(self.spot, strike) + (dividend - risk);
        let (d1, d2) = super::black_scholes_arguments(moneyness, sigma, self.remaining.sqrt());
        let limit = if d2.is_nan() || d1.is_nan() {
            return None;
        } else if strike < forward {
            super::weighted_cdf(strike, risk, -d2)
        } else {
            super::weighted_cdf(self.spot, dividend, d1)
        };
        // Leave room for rounding in the forward and bound evaluation.
        (self.weight.abs() * limit <= budget).then_some([forward, weighted[0], weighted[1]])
    }
}

enum Boundary {
    Constant {
        sigma: f64,
        exact: Vec<BoundaryTerm>,
        approximated: Vec<BoundaryTerm>,
        moments: StrikeMoments,
        error_budget: f64,
    },
    Local {
        terms: Vec<[f64; 3]>,
        moments: StrikeMoments,
    },
}

struct MarketStrip {
    space: SpatialGrid,
    weights: Vec<f64>,
    terminal: StrikeMoments,
    boundary: Boundary,
    discount: [f64; 2],
}

impl MarketStrip {
    fn new(
        engine: &FiniteDifferenceVanillaEngine,
        option: &EuropeanOption,
        context: &PricingContext,
        model: StripModel<'_>,
        strikes: [f64; 2],
        width: f64,
        budget: &RollbackWork,
    ) -> Result<Self> {
        let expiry = option.expiry_time();
        let valuation = context.valuation_time();
        let time = valuation.years_until(expiry);
        let spot = context.spot_price();
        let settings = engine.settings;
        let (risk, dividend) = diffusion::rate_curves(context, expiry, settings)?;
        let (space, node) = strip_grid(
            settings,
            context,
            expiry,
            [&risk, &dividend],
            strikes,
            width,
        )?;
        let [lower, upper] = [space.lower, space.upper];
        let time_grid = diffusion::time_grid(context, expiry, settings, &risk, &dividend, &[])?;
        let steps = time_grid.rollback_steps(settings.scheme, &[])?;
        validate_work(space.count as u64, steps)?;
        // Constant boundaries retain the exact fallback terms alongside the
        // prefix sums. Include both growing vectors while building the index.
        let bytes_per_step = match model {
            StripModel::Constant(_) => 384,
            StripModel::Local(_) => 256,
        };
        validate_workspace(
            settings.scheme,
            2 * (space.count as u64 + 1),
            steps,
            risk.workspace_bytes() + dividend.workspace_bytes() + bytes_per_step * steps,
        )?;
        budget.charge((space.count as u64).saturating_mul(steps))?;
        let mut weights = vec![0.0; space.count + 1];
        let mut previous = weights.clone();
        weights[node] = 1.0;
        let sign = Payoff::vanilla(option.option_type(), option.strike(), false).sign;
        let mut local_terms = Vec::new();
        let mut exact = Vec::new();
        let mut approximated = Vec::new();
        let mut constant_terms = Vec::new();
        let error_budget = spot * 1e-8;
        #[expect(
            clippy::cast_precision_loss,
            reason = "bounded time step count fits exactly"
        )]
        let term_budget = 0.25 * error_budget / steps as f64;
        let (mut stepper, offset) = match model {
            StripModel::Constant(_) => (Stepper::new(), 0.0),
            StripModel::Local(surface) => (
                Stepper::new_local(space.count),
                surface.times()[0].years_until(valuation),
            ),
        };
        let clock =
            diffusion::uses_business_time(context).then(|| BusinessDiffusion::new(context, 0.0));
        for step in 0..time_grid.widths.len() {
            crate::cancellation::check()?;
            let mut damping = u32::try_from(time_grid.widths.len() - step).unwrap_or(u32::MAX);
            // The first two intervals rolled back from expiry consist of two half-steps.
            damping = if damping <= 2 { damping } else { 0 };
            let mut substeps = time_grid.substeps(step, settings.scheme, &mut damping);
            let later = substeps.next().expect("each interval has a substep");
            for (current, dt, theta) in substeps.chain([later]) {
                let middle = current + 0.5 * dt;
                let key = [dt, theta, risk.rate(middle), dividend.rate(middle)];
                let discount = [
                    risk.log_factor(current, time),
                    dividend.log_factor(current, time),
                ];
                match model {
                    StripModel::Constant(sigma) => {
                        let weights = stepper.transpose_constant(
                            &mut weights,
                            &mut previous,
                            &space,
                            sigma,
                            key,
                        )?;
                        for (spot, weight) in [lower, upper].into_iter().zip(weights) {
                            if weight == 0.0 {
                                continue;
                            }
                            let term = BoundaryTerm {
                                spot,
                                weight,
                                discount,
                                remaining: time - current,
                            };
                            if let Some(intrinsic) =
                                term.bounded_intrinsic(sigma, strikes, term_budget)
                            {
                                constant_terms.push(intrinsic);
                                approximated.push(term);
                            } else {
                                exact.push(term);
                            }
                        }
                    }
                    StripModel::Local(surface) => {
                        let contribution = stepper.transpose_local(
                            &mut weights,
                            &mut previous,
                            &space,
                            surface,
                            [
                                offset + middle,
                                clock.map_or(1.0, |clock| clock.variance_rate(middle).sqrt()),
                            ],
                            key,
                        )?;
                        let [cash, asset] = discount.map(f64::exp);
                        for (bound, weight) in [lower, upper].into_iter().zip(contribution) {
                            let present = bound * asset;
                            if !present.is_normal()
                                || !cash.is_normal()
                                || !(present / cash).is_normal()
                            {
                                return Err(super::unstable());
                            }
                            local_terms.push([present / cash, weight * present, weight * cash]);
                        }
                    }
                }
                std::mem::swap(&mut weights, &mut previous);
            }
        }
        let terminal = terminal_moments(&space, &weights, sign);
        let boundary = match model {
            StripModel::Constant(sigma) => Boundary::Constant {
                sigma,
                exact,
                error_budget: if approximated.is_empty() {
                    0.0
                } else {
                    error_budget
                },
                approximated,
                moments: StrikeMoments::new(constant_terms, sign),
            },
            StripModel::Local(_) => Boundary::Local {
                moments: StrikeMoments::new(local_terms.clone(), sign),
                terms: local_terms,
            },
        };
        Ok(Self {
            space,
            weights,
            terminal,
            boundary,
            discount: [risk.log_factor(0.0, time), dividend.log_factor(0.0, time)],
        })
    }

    fn price(&self, payoff: Payoff, spot: f64) -> Result<f64> {
        let strike = payoff.strike;
        let mut exact_sum = crate::numerics::Sum::default();
        let (boundary, error_budget) = match &self.boundary {
            Boundary::Constant {
                sigma,
                exact,
                moments,
                error_budget,
                ..
            } => {
                let mut scale = 0.0;
                for (index, term) in exact.iter().enumerate() {
                    if index % 1024 == 0 {
                        crate::cancellation::check()?;
                    }
                    let term = term.value(payoff, *sigma);
                    exact_sum.add(term);
                    scale += term.abs();
                }
                (
                    moments
                        .at(strike)
                        .map(|[a, b]| [a + exact_sum.total(), b + scale]),
                    *error_budget,
                )
            }
            Boundary::Local { moments, .. } => (moments.at(strike), 0.0),
        };
        if let Some(value) = boundary.and_then(|boundary| {
            indexed_value(
                &self.space,
                &self.weights,
                &self.terminal,
                payoff,
                boundary,
                error_budget,
            )
        }) {
            return payoff::checked_price(payoff, spot, value, self.discount, 0.0);
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
        if let Boundary::Local { terms, .. } = &self.boundary {
            for (index, &[cutoff, asset, cash]) in terms.iter().enumerate() {
                if index % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                if payoff.sign * (cutoff - strike) > 0.0 {
                    value.add(payoff.sign * (-strike).mul_add(cash, asset));
                }
            }
        } else if let Boundary::Constant {
            sigma,
            approximated,
            ..
        } = &self.boundary
        {
            for (index, term) in approximated.iter().enumerate() {
                if index % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                exact_sum.add(term.value(payoff, *sigma));
            }
            value.add(exact_sum.total());
        }
        payoff::checked_price(payoff, spot, value.total(), self.discount, 0.0)
    }
}

fn strip_grid(
    settings: super::FiniteDifferenceSettings,
    context: &PricingContext,
    expiry: crate::Timestamp,
    [risk, dividend]: [&super::Curve; 2],
    strikes: [f64; 2],
    width: f64,
) -> Result<(SpatialGrid, usize)> {
    let spot = context.spot_price();
    let [low, high] = [spot.min(strikes[0]), spot.max(strikes[1])];
    let domain_width =
        (1.5 * diffusion_log_width(context, expiry, risk, dividend)?).max(4.0_f64.ln());
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
    Ok((space, node))
}

fn terminal_moments(space: &SpatialGrid, weights: &[f64], sign: f64) -> StrikeMoments {
    StrikeMoments::new(
        space
            .spots
            .iter()
            .zip(weights)
            .map(|(&spot, &weight)| [spot, spot * weight, weight])
            .collect(),
        sign,
    )
}

fn indexed_value(
    space: &SpatialGrid,
    weights: &[f64],
    terminal: &StrikeMoments,
    payoff: Payoff,
    [edge, edge_scale]: [f64; 2],
    error_budget: f64,
) -> Option<f64> {
    let strike = payoff.strike;
    let [mut value, scale] = terminal.at(strike)?;
    let coordinate = space.coordinate(strike);
    let right = space.nodes.partition_point(|&node| node < coordinate);
    let start = right.saturating_sub(1);
    for (offset, &weight) in weights[start..=right.min(space.count)].iter().enumerate() {
        let index = start + offset;
        let payment = project_payoff(space, &[strike], index, |spot| payoff.value(spot));
        value += weight * (payment - payoff.value(space.asset(index)));
    }
    value += edge;
    (value.is_finite()
        && (error_budget == 0.0 || value > error_budget)
        && (scale + edge_scale == 0.0 || value.abs() > 1e-6 * (scale + edge_scale)))
        .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_boundary_uses_intrinsic_only_within_its_budget() {
        let term = BoundaryTerm {
            spot: 200.0,
            weight: -0.002,
            discount: [-0.05, -0.02],
            remaining: 0.25,
        };
        let budget = 1e-8;
        let intrinsic = term.bounded_intrinsic(0.2, [85.0, 115.0], budget).unwrap();
        assert!(term.bounded_intrinsic(0.2, [85.0, 220.0], budget).is_none());
        for direction in [crate::OptionType::Call, crate::OptionType::Put] {
            for strike in [85.0, 100.0, 115.0] {
                let payoff = Payoff::vanilla(direction, strike, false);
                let [value, _] = StrikeMoments::new(vec![intrinsic], payoff.sign)
                    .at(strike)
                    .unwrap();
                assert!((term.value(payoff, 0.2) - value).abs() <= budget);
            }
        }
    }
}
