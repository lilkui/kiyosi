use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    AmericanOption, CoxRossRubinsteinVanillaEngine, EuropeanOption, Payoff, PricingContext, Result,
    flat_rates, parameters, time_step, unstable,
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
    /// tolerances apply as in [`Self::price`].
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{AmericanOption, BinomialVanilla, BlackScholesMertonParameters,
    ///     CoxRossRubinsteinVanillaEngine, Date, OptionType, PricingContext};
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
    /// Propagates the first pricing error in input order or a cancellation error.
    /// A failed request returns no partial prices.
    pub fn price_many<'a>(
        &self,
        options: &[BinomialVanilla<'a>],
        context: &PricingContext,
    ) -> Result<Vec<f64>> {
        const LANES: usize = 4;
        let scalar = |option| match option {
            BinomialVanilla::European(option) => self.price(option, context),
            BinomialVanilla::American(option) => self.price(option, context),
        };
        let eligible = self.settings.step_count >= 256
            && options
                .iter()
                .any(|option| matches!(option, BinomialVanilla::American(_)));
        if !eligible {
            return options
                .iter()
                .map(|&option| {
                    crate::cancellation::check()?;
                    scalar(option)
                })
                .collect();
        }
        let evaluate = |options: &[BinomialVanilla<'a>]| -> Result<[f64; LANES]> {
            crate::cancellation::check()?;
            if let Some(
                [
                    BinomialVanilla::American(a),
                    BinomialVanilla::American(b),
                    BinomialVanilla::American(c),
                    BinomialVanilla::American(d),
                ],
            ) = self.rollback_tolerance.is_none().then_some(options)
            {
                match self.price_american_lanes([a, b, c, d], context) {
                    Ok(Some(prices)) => return Ok(prices),
                    Err(error @ crate::Error::Io(_)) => return Err(error),
                    _ => {}
                }
            }
            let mut prices = [0.0; LANES];
            for (price, &option) in prices.iter_mut().zip(options) {
                crate::cancellation::check()?;
                *price = scalar(option)?;
            }
            Ok(prices)
        };
        let mut prices = Vec::new();
        prices
            .try_reserve_exact(options.len())
            .map_err(|_| unstable())?;
        if let Some((pool, _)) = (options.len() > LANES)
            .then(super::super::rollback_pool)
            .flatten()
        {
            let tokens = crate::cancellation::current();
            let first_error = AtomicUsize::new(usize::MAX);
            let batches = pool.install(|| {
                options
                    .par_chunks(LANES)
                    .enumerate()
                    .map(|(index, options)| {
                        if index > first_error.load(Ordering::Relaxed) {
                            return None;
                        }
                        Some(
                            crate::cancellation::with_tokens(tokens.clone(), || evaluate(options))
                                .inspect_err(|_| {
                                    first_error.fetch_min(index, Ordering::Relaxed);
                                }),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            for (options, batch) in options.chunks(LANES).zip(batches) {
                let batch = batch.ok_or_else(unstable)??;
                prices.extend_from_slice(&batch[..options.len()]);
            }
        } else {
            for options in options.chunks(LANES) {
                prices.extend_from_slice(&evaluate(options)?[..options.len()]);
            }
        }
        Ok(prices)
    }

    fn price_american_lanes<const N: usize>(
        &self,
        options: [&AmericanOption; N],
        context: &PricingContext,
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
