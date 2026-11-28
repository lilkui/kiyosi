use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::validate_rollback_work;
use super::{
    AmericanOption, CoxRossRubinsteinVanillaEngine, EuropeanOption, Payoff, PricingContext, Result,
    RollbackWork, flat_rates, parameters, rollback_nodes, time_step, unstable,
};

/// Borrows a contract for [`CoxRossRubinsteinVanillaEngine::price_many`].
#[derive(Clone, Copy, Debug)]
pub enum BinomialVanilla<'a> {
    /// Prices this contract without early exercise.
    European(&'a EuropeanOption),
    /// Prices this contract with early exercise.
    American(&'a AmericanOption),
}

impl CoxRossRubinsteinVanillaEngine {
    /// Prices vanilla contracts in input order under one market snapshot.
    ///
    /// Compatible American contracts can share lattice geometry and parallel
    /// work runs on a bounded library pool. Configured American rollback
    /// tolerances apply as in [`PricingEngine::price`](crate::PricingEngine::price).
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{AmericanOption, BlackScholesMertonParameters, CoxRossRubinsteinVanillaEngine,
    ///     Date, OptionType, PricingContext, engines::BinomialVanilla};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, start.add_days(365)?)?;
    /// let market = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start)?;
    /// let prices = CoxRossRubinsteinVanillaEngine::default()
    ///     .price_many(&[BinomialVanilla::American(&put)], &market)?;
    /// assert!(prices[0] > 0.0);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] above 1,048,576 contracts or for
    /// excessive combined terminal-node and rollback work.
    /// Propagates the first contract pricing error in input order or a cancellation error.
    /// A failed request returns no partial prices.
    pub fn price_many(
        &self,
        options: &[BinomialVanilla<'_>],
        context: &PricingContext,
    ) -> Result<Vec<f64>> {
        const LANES: usize = 4;
        crate::binding_support::validate_contract_batch_size(options.len())?;
        let budget = RollbackWork::default();
        let scalar = |option| match option {
            BinomialVanilla::European(option) => self.price_vanilla(
                option,
                Payoff::vanilla(option.option_type(), option.strike(), false),
                context,
                Some(&budget),
            ),
            BinomialVanilla::American(option) => self.price_vanilla(
                option,
                Payoff::vanilla(option.option_type(), option.strike(), true),
                context,
                Some(&budget),
            ),
        };
        let serial = || {
            options
                .iter()
                .map(|&option| {
                    crate::cancellation::check()?;
                    scalar(option)
                })
                .collect()
        };
        let eligible = self.settings.step_count >= 256
            && options
                .iter()
                .any(|option| matches!(option, BinomialVanilla::American(_)));
        if !eligible {
            return serial();
        }
        // A failed tiled or banded solve can be followed by a full rollback.
        // When workers might contend for the budget, charge it in input order.
        let steps = self.settings.step_count.unsigned_abs() as usize;
        let fallback_factor = if self.rollback_tolerance.is_some() {
            3
        } else {
            2
        };
        if validate_rollback_work(
            rollback_nodes(steps) * fallback_factor,
            options.len() as u64,
        )
        .is_err()
        {
            return serial();
        }
        let can_tile = self
            .rollback_tolerance
            .is_none()
            .then(|| flat_rates(context).ok())
            .flatten();
        let exercise_option = |option| match (can_tile, option) {
            (Some((rate, dividend)), BinomialVanilla::American(option))
                if option.option_type() != crate::OptionType::Call
                    || dividend > 0.0
                    || rate < 0.0 =>
            {
                Some(option)
            }
            _ => None,
        };
        let expiry =
            |index: usize| exercise_option(options[index]).map(AmericanOption::expiry_time);
        let mut ordered: Vec<_> = (0..options.len()).collect();
        ordered.sort_unstable_by_key(|&index| (expiry(index), index));
        crate::cancellation::check()?;
        let mut jobs: Vec<_> = ordered
            .chunk_by(|&a, &b| expiry(a) == expiry(b))
            .flat_map(|group| group.chunks(LANES))
            .collect();
        jobs.sort_unstable_by_key(|job| job[0]);

        let first_error = AtomicUsize::new(usize::MAX);
        let evaluate =
            |indices: &[usize]| -> std::result::Result<[f64; LANES], (usize, crate::Error)> {
                crate::cancellation::check().map_err(|error| (indices[0], error))?;
                if let &[a, b, c, d] = indices {
                    if let [
                        BinomialVanilla::American(a),
                        BinomialVanilla::American(b),
                        BinomialVanilla::American(c),
                        BinomialVanilla::American(d),
                    ] = [options[a], options[b], options[c], options[d]]
                    {
                        if expiry(indices[0]).is_some() {
                            match self.price_american_lanes([a, b, c, d], context, &budget) {
                                Ok(Some(prices)) => return Ok(prices),
                                Err(error @ crate::Error::Io(_)) => {
                                    return Err((indices[0], error));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                let mut prices = [0.0; LANES];
                for (price, &index) in prices.iter_mut().zip(indices) {
                    crate::cancellation::check().map_err(|error| (index, error))?;
                    *price = scalar(options[index]).map_err(|error| (index, error))?;
                }
                Ok(prices)
            };
        let evaluate = |indices: &[usize]| {
            (indices[0] <= first_error.load(Ordering::Relaxed)).then(|| {
                evaluate(indices).inspect_err(|(index, _)| {
                    first_error.fetch_min(*index, Ordering::Relaxed);
                })
            })
        };
        let mut prices = Vec::new();
        prices
            .try_reserve_exact(options.len())
            .map_err(|_| unstable())?;
        prices.resize(options.len(), 0.0);
        let mut error = None;
        let mut collect = |indices: &[usize], batch| match batch {
            Some(Ok(batch)) => {
                for (&index, price) in indices.iter().zip(batch) {
                    prices[index] = price;
                }
            }
            Some(Err((index, cause)))
                if error.as_ref().is_none_or(|(current, _)| index < *current) =>
            {
                error = Some((index, cause));
            }
            _ => {}
        };
        if let Some((pool, _)) = (jobs.len() > 1).then(super::super::rollback_pool).flatten() {
            let tokens = crate::cancellation::current();
            let batches: Vec<_> = pool.install(|| {
                jobs.par_iter()
                    .map(|indices| {
                        crate::cancellation::with_tokens(tokens.clone(), || Ok(evaluate(indices)))
                    })
                    .collect()
            });
            for (indices, batch) in jobs.into_iter().zip(batches) {
                collect(indices, batch?);
            }
        } else {
            for indices in jobs {
                collect(indices, evaluate(indices));
            }
        }
        if let Some((_, error)) = error {
            return Err(error);
        }
        Ok(prices)
    }

    fn price_american_lanes<const N: usize>(
        &self,
        options: [&AmericanOption; N],
        context: &PricingContext,
        budget: &RollbackWork,
    ) -> Result<Option<[f64; N]>> {
        let (rate, dividend) = flat_rates(context)?;
        let expiry = options[0].expiry_time();
        if super::super::deterministic::enabled(context)
            || options.iter().any(|option| {
                option.expiry_time() != expiry
                    || (option.option_type() == crate::OptionType::Call
                        && dividend <= 0.0
                        && rate >= 0.0)
            })
        {
            return Ok(None);
        }
        let dt = time_step(options[0], context, self.settings)?;
        for option in &options[1..] {
            time_step(*option, context, self.settings)?;
        }
        if dt == 0.0 {
            return Ok(None);
        }
        self.settings.validate_rollback()?;
        let (_, up, down, discount, probability) = parameters(context, dt)?;
        let spot = context.spot_price();
        let step_count = self.settings.step_count;
        let steps = step_count.unsigned_abs() as usize;
        let mut asset = spot * down.powi(step_count);
        let up_squared = up * up;
        if !discount.factor.is_normal() || !asset.is_normal() || !up_squared.is_finite() {
            return Ok(None);
        }
        budget.charge(rollback_nodes(steps) * N as u64)?;
        let payoff =
            options.map(|option| Payoff::vanilla(option.option_type(), option.strike(), true));
        let exercise =
            |asset: f64| payoff.map(|payoff| (payoff.sign * (asset - payoff.strike)).max(0.0));
        let mut values = Vec::with_capacity(steps + 1);
        for node in 0..=steps {
            if node % 256 == 0 {
                crate::cancellation::check()?;
            }
            if !asset.is_finite() {
                return Err(unstable());
            }
            values.push(exercise(asset));
            asset *= up_squared;
        }
        let rollback = |lower: [f64; N], upper: [f64; N]| -> [f64; N] {
            std::array::from_fn(|lane| {
                discount.rollback_normal(lower[lane], upper[lane], probability)
            })
        };
        let mut first_asset = spot * down.powi(step_count - 1);
        for level in (1..steps).rev() {
            crate::cancellation::check()?;
            if level < steps - 1 {
                first_asset *= up;
            }
            let mut asset = first_asset;
            for node in 0..=level {
                let continuation = rollback(values[node], values[node + 1]);
                let intrinsic = exercise(asset);
                values[node] = std::array::from_fn(|lane| continuation[lane].max(intrinsic[lane]));
                if values[node].iter().any(|v| !v.is_finite()) {
                    return Err(unstable());
                }
                asset *= up_squared;
            }
        }
        crate::cancellation::check()?;
        let continuation = rollback(values[0], values[1]);
        let intrinsic = exercise(spot);
        let lattice_intrinsic = exercise(first_asset * up);
        let prices = std::array::from_fn(|lane| {
            if intrinsic[lane] > continuation[lane] {
                intrinsic[lane]
            } else {
                continuation[lane].max(lattice_intrinsic[lane])
            }
        });
        if prices.iter().any(|v| !v.is_finite()) {
            return Err(unstable());
        }
        Ok(Some(prices))
    }
}
