use super::{Payoff, flat_rates};
use crate::numerics::{log_ratio, scaled_cash as scaled};

mod barrier;
mod batch;
pub use barrier::CoxRossRubinsteinBarrierEngine;
pub use batch::BinomialVanilla;

use crate::{
    AmericanOption, DigitalOption, Error, EuropeanOption, Greek, GreeksRequest,
    ImpliedVolatilitySettings, OptionContract, OptionType, PricingContext, PricingEngine,
    PricingResult, Result,
};

/// Configures the number of CRR time steps. See [`CoxRossRubinsteinVanillaEngine::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinomialSettings {
    /// Sets the number of steps, from 1 through 1,000,000 inclusive. Defaults to 256.
    /// Pricing paths that roll back every tree level allow at most 16,000 steps.
    pub step_count: i32,
}

impl Default for BinomialSettings {
    fn default() -> Self {
        Self { step_count: 256 }
    }
}

impl BinomialSettings {
    /// Validates the step count.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside 1 through 1,000,000 steps.
    /// Leaves the settings unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::BinomialSettings;
    /// # fn main() -> kiyosi::Result<()> {
    /// let settings = BinomialSettings { step_count: 1024 };
    /// settings.validate()?;
    /// # Ok(()) }
    /// ```
    pub fn validate(&self) -> Result<()> {
        if !(1..=1_000_000).contains(&self.step_count) {
            return Err(Error::InvalidInput(
                "step_count must be between 1 and 1000000".into(),
            ));
        }
        Ok(())
    }

    fn validate_rollback(self) -> Result<()> {
        // A 16,000-step rollback visits just over 128 million nodes per layer.
        if self.step_count > 16_000 {
            return Err(Error::InvalidInput(
                "binomial rollback requires step_count at most 16000".into(),
            ));
        }
        Ok(())
    }
}

/// Prices European and American vanilla contracts with a flat-rate CRR tree.
///
/// Returns node delta/gamma and completes other requested Greeks numerically.
/// Values own their settings; shared instances support concurrent pricing.
/// See [`Self::price`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CoxRossRubinsteinVanillaEngine {
    settings: BinomialSettings,
    rollback_tolerance: Option<f64>,
}

impl CoxRossRubinsteinVanillaEngine {
    /// Creates an engine after validating its step count.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside 1 through 1,000,000 steps.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BinomialSettings, CoxRossRubinsteinVanillaEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let engine = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 1024 })?;
    /// assert_eq!(engine.settings().step_count, 1024);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: BinomialSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self {
            settings,
            rollback_tolerance: None,
        })
    }

    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> BinomialSettings {
        self.settings
    }

    /// Returns the configured American option truncation tolerance in price units.
    ///
    /// See [`Self::with_rollback_tolerance`] for configuration.
    pub fn rollback_tolerance(&self) -> Option<f64> {
        self.rollback_tolerance
    }

    /// Configures tolerance-bounded rollback for American options.
    ///
    /// Ordinary price, Greek and implied-volatility requests use the bound.
    /// Pass `None` to use full rollback. See
    /// [`Self::price_with_rollback_tolerance`] for the truncation error bound.
    ///
    /// # Examples
    ///
    /// ```
    /// # use kiyosi::{BinomialSettings, CoxRossRubinsteinVanillaEngine, Result};
    /// # fn main() -> Result<()> {
    /// let engine = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 20_000 })?
    ///     .with_rollback_tolerance(Some(1e-6))?;
    /// assert_eq!(engine.rollback_tolerance(), Some(1e-6));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a nonpositive or nonfinite tolerance.
    pub fn with_rollback_tolerance(mut self, tolerance: Option<f64>) -> Result<Self> {
        if let Some(tolerance) = tolerance {
            crate::error::finite(tolerance, true, "absolute_tolerance")?;
        }
        self.rollback_tolerance = tolerance;
        Ok(self)
    }

    /// Prices a supported vanilla contract.
    ///
    /// See [`Self::price`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves before life
    /// validation, [`Error::InvalidInput`] outside permitted valuation bounds or above
    /// 16,000 steps for a tree requiring rollback, and
    /// [`Error::ComputationFailure`] for unstable probabilities, asset nodes or values.
    ///
    /// # Greeks
    ///
    /// Returns node delta/gamma and numerically completes missing requested Greeks.
    ///
    /// Expiry Greeks follow terminal payoffs; time Greeks are absent. One-step gamma uses scalar differences.
    /// Bumps use [`crate::NumericalShiftSettings`]; feasible bumped failures propagate.
    ///
    /// ## Errors
    ///
    /// Checks flat-rate applicability before shift validation, then reuses
    /// [`Self::price`] and [`crate::numerical_greeks`] errors.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use kiyosi::PricingEngine;
    /// use kiyosi::{AmericanOption, CoxRossRubinsteinVanillaEngine,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let result = CoxRossRubinsteinVanillaEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]))?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        context.natural_bsm_parameters()?;
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Prices an American option with an absolute tolerance for discarding lattice tails.
    ///
    /// The tolerance is in price units. The omitted paths' contribution is bounded
    /// using their exit probability and a discounted payoff bound. Calls use
    /// share-weighted probabilities to bound payouts that grow with spot.
    /// The bound concerns lattice truncation. The usual discretization and
    /// floating-point errors still apply. A wide band falls back to full rollback.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a nonpositive or nonfinite tolerance,
    /// out-of-life valuation, or excessive rollback work.
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves.
    /// Propagates the numerical and cancellation errors of [`Self::price`].
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{AmericanOption, BinomialSettings, BlackScholesMertonParameters,
    ///     CoxRossRubinsteinVanillaEngine, Date, OptionType, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    /// let market = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start)?;
    /// let engine = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 20_000 })?;
    /// assert!(engine.price_with_rollback_tolerance(&put, &market, 1e-6)? > 0.0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn price_with_rollback_tolerance(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        absolute_tolerance: f64,
    ) -> Result<f64> {
        crate::error::finite(absolute_tolerance, true, "absolute_tolerance")?;
        flat_rates(context)?;
        Ok(self
            .banded_native(
                option,
                Payoff::vanilla(option.option_type(), option.strike(), true),
                context,
                absolute_tolerance,
                GreeksRequest::new([]),
            )?
            .price())
    }

    /// Prices an American option and its requested Greeks with tolerance-bounded rollback.
    ///
    /// The tolerance caps discarded tail value in each price calculation. The
    /// engine tightens it for sensitivities whose differences amplify price errors.
    /// Grid discretization and rounding errors still apply. See
    /// [`Self::price_with_rollback_tolerance`] for the rollback contract.
    ///
    /// # Errors
    ///
    /// Propagates rollback, numerical shift and bumped pricing errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{AmericanOption, BinomialSettings, BlackScholesMertonParameters,
    ///     CoxRossRubinsteinVanillaEngine, Date, Greek, GreeksRequest,
    ///     NumericalShiftSettings, OptionType, PricingContext};
    ///
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let put = AmericanOption::new(OptionType::Put, 100.0, start, start.add_days(365)?)?;
    /// let market = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start)?;
    /// let engine = CoxRossRubinsteinVanillaEngine::new(BinomialSettings { step_count: 20_000 })?;
    /// let result = engine.price_with_greeks_with_rollback_tolerance(
    ///     &put, &market, GreeksRequest::new([Greek::Delta]), NumericalShiftSettings::default(), 1e-6)?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn price_with_greeks_with_rollback_tolerance(
        &self,
        option: &AmericanOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
        absolute_tolerance: f64,
    ) -> Result<PricingResult> {
        crate::error::finite(absolute_tolerance, true, "absolute_tolerance")?;
        flat_rates(context)?;
        shifts.validate()?;
        time_step(option, context, self.settings)?;
        self.banded_greeks(
            option,
            Payoff::vanilla(option.option_type(), option.strike(), true),
            context,
            request,
            shifts,
            absolute_tolerance,
        )
    }

    fn banded_greeks(
        &self,
        option: &impl OptionContract,
        payoff: Payoff,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
        absolute_tolerance: f64,
    ) -> Result<PricingResult> {
        let tolerance = stencil_tolerance(option, context, request, shifts, absolute_tolerance)?;
        let evaluate = |context: &PricingContext, stencil| {
            self.banded_native(option, payoff, context, tolerance, stencil)
        };
        crate::risk::numerical::complete_greeks(
            option,
            context,
            request,
            shifts,
            true,
            evaluate(context, request)?,
            evaluate,
        )
    }

    fn banded_native(
        self,
        option: &impl OptionContract,
        payoff: Payoff,
        context: &PricingContext,
        tolerance: f64,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let exact = || native(option, payoff, context, self.settings, request);
        let dt = time_step(option, context, self.settings)?;
        let (rate, dividend) = flat_rates(context)?;
        if dt == 0.0
            || crate::engines::deterministic::enabled(context)
            || tolerance == 0.0
            || (payoff.sign > 0.0 && dividend <= 0.0 && rate >= 0.0)
        {
            return exact();
        }
        let spot = context.spot_price();
        let steps = self.settings.step_count.unsigned_abs() as usize;
        let (width, up, down, discount, p) = parameters(context, dt)?;
        let h = spot * (up - 1.0);
        let tolerance = if request.has(Greek::Gamma) {
            tolerance * h.min(1.0).powi(2) * 0.125
        } else if request.has(Greek::Delta) {
            tolerance * h.min(1.0) * 0.25
        } else {
            tolerance
        };
        let (center, log_payoff_bound, log_step_discount) = if payoff.sign > 0.0 {
            // In share units, a call is bounded by one share. The tilted
            // up probability is p * up / forward_growth. Keep the original
            // exercise amounts and transition probabilities for the solve.
            let carry = (rate - dividend) * dt;
            let center = (p.ln() + (width - carry)).exp().min(1.0);
            (center, spot.ln() + 2.0 * width, -dividend * dt)
        } else {
            (p, payoff.strike.ln(), discount.log_factor)
        };
        let band = [
            center,
            std::f64::consts::LN_2
                + log_payoff_bound
                + f64::from(self.settings.step_count) * log_step_discount.max(0.0)
                - tolerance.ln(),
        ];
        let Some(BandedValues {
            price,
            first,
            second,
            exercised,
        }) = banded_american(steps, payoff, [spot, width, up, p], discount, band)?
        else {
            return exact();
        };
        if exercised {
            let mut result = PricingResult::new(price)?;
            result.set(request, Greek::Delta, payoff.sign)?;
            result.set(request, Greek::Gamma, 0.0)?;
            return Ok(result);
        }
        tree_result(
            price,
            first,
            second,
            spot,
            [up, down, up * up],
            request,
            steps,
        )
    }
}

fn stencil_tolerance(
    option: &impl OptionContract,
    context: &PricingContext,
    request: GreeksRequest,
    shifts: crate::NumericalShiftSettings,
    tolerance: f64,
) -> Result<f64> {
    let model = context.natural_bsm_parameters()?;
    let sigma = model.volatility();
    let vol = crate::risk::numerical::symmetric_shift(sigma, shifts.volatility_shift_at(sigma));
    let spot = context.spot_price();
    let bump = crate::risk::numerical::symmetric_shift(spot, shifts.spot_shift_at(spot));
    let time = if [Greek::Theta, Greek::Charm, Greek::Color]
        .into_iter()
        .any(|g| request.has(g))
        && context.valuation_time() < option.expiry_time()
    {
        crate::risk::numerical::time_stencil(option, context, shifts)?
            .map_or(0.0, |(before, after)| before.years_until(after))
    } else {
        0.0
    };
    let scale = [
        (Greek::Speed, bump),
        (Greek::Theta, time),
        (Greek::Charm, time),
        (Greek::Color, time),
        (Greek::Vega, vol),
        (Greek::Vanna, vol),
        (Greek::Zomma, vol),
        (Greek::Volga, vol * vol),
        (
            Greek::Rho,
            crate::risk::numerical::symmetric_shift(
                model.risk_free_curve().shift_origin(),
                shifts.rate_shift,
            ),
        ),
        (
            Greek::DividendRho,
            crate::risk::numerical::symmetric_shift(
                model.dividend_curve().shift_origin(),
                shifts.rate_shift,
            ),
        ),
    ]
    .into_iter()
    .filter(|&(greek, span)| request.has(greek) && span > 0.0)
    .fold(1.0_f64, |smallest, (_, span)| smallest.min(0.25 * span));
    Ok(tolerance * scale)
}

struct BandedValues {
    price: f64,
    first: [f64; 2],
    second: [f64; 3],
    exercised: bool,
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The configured maximum of one million tree steps bounds the lattice indices."
)]
fn banded_american(
    steps: usize,
    payoff: Payoff,
    [spot, width, up, p]: [f64; 4],
    discount: StepDiscount,
    [center, log_bound]: [f64; 2],
) -> Result<Option<BandedValues>> {
    // Each side of the centered walk has a maximal Hoeffding bound of
    // exp(-2*r²/steps). Two extra nodes cover the first two Greek levels.
    let radius = (0.5 * steps as f64 * log_bound.max(0.0)).sqrt().ceil() + 2.0;
    if !radius.is_finite() || 2.0 * radius >= steps as f64 {
        return Ok(None);
    }
    let radius = radius as usize;
    let capacity = 2 * radius + 2;
    if steps.saturating_mul(capacity) > 128_000_000 {
        return Ok(None);
    }
    let range = |level: usize| {
        let center = (center * level as f64) as usize;
        center.saturating_sub(radius)..(center + radius + 1).min(level + 1)
    };
    let mut later = range(steps);
    let mut values = vec![0.0; capacity];
    let mut next = vec![0.0; capacity];
    let asset =
        |level: usize, lower: usize| scaled(spot, (2.0 * lower as f64 - level as f64) * width);
    let step_up = up * up;
    let mut node_spot = asset(steps, later.start);
    if !step_up.is_normal() || !node_spot.is_normal() {
        return Ok(None);
    }
    for value in &mut values[..later.len()] {
        *value = payoff.value(node_spot);
        node_spot *= step_up;
    }
    let mut first = [0.0; 2];
    let mut second = [0.0; 3];
    let mut exercised = false;
    for level in (0..steps).rev() {
        if level + 1 == 1 {
            first.copy_from_slice(&values[..2]);
        } else if level + 1 == 2 {
            second.copy_from_slice(&values[..3]);
        }
        crate::cancellation::check()?;
        let current = range(level);
        node_spot = asset(level, current.start);
        if !node_spot.is_normal() {
            return Ok(None);
        }
        let child = |node| {
            if later.contains(&node) {
                values[node - later.start]
            } else {
                0.0
            }
        };
        for (node, value) in current.clone().zip(&mut next) {
            let continuation = discount.rollback(child(node), child(node + 1), p);
            let exercise = payoff.value(node_spot);
            if level == 0 {
                // Use the original spot at the root, as in the full rollback.
                exercised = exercise > 0.0 && exercise > continuation;
            }
            *value = continuation.max(exercise);
            if !value.is_finite() {
                return Err(unstable());
            }
            node_spot *= step_up;
        }
        std::mem::swap(&mut values, &mut next);
        later = current;
    }
    Ok(Some(BandedValues {
        price: values[0],
        first,
        second,
        exercised,
    }))
}

fn time_step(
    option: &impl OptionContract,
    context: &PricingContext,
    settings: BinomialSettings,
) -> Result<f64> {
    context.validate_curve_horizon(option.expiry_time())?;
    let valuation = context.valuation_time();
    let expiry = option.expiry_time();
    crate::instruments::monitoring::validate_life(option, context)?;
    Ok(valuation.years_until(expiry) / f64::from(settings.step_count))
}

fn prepare_implied(
    option: &impl OptionContract,
    context: &PricingContext,
    tree: BinomialSettings,
    mut settings: ImpliedVolatilitySettings,
) -> Result<ImpliedVolatilitySettings> {
    let (rate, dividend) = flat_rates(context)?;
    let dt = time_step(option, context, tree)?;
    if dt > 0.0 {
        let carry = (rate - dividend) * dt;
        let root = dt.sqrt();
        let valid = |volatility| tree_parameters(rate, carry, volatility * root, dt).is_ok();
        if valid(settings.lower_bound) {
            return Ok(settings);
        }
        // The solver checks zero separately; positive trials need distinct factors.
        let minimum = carry.abs().max(f64::EPSILON) / root;
        if minimum.is_finite() {
            let minimum = if valid(minimum) {
                minimum
            } else {
                libm::nextafter(minimum, f64::INFINITY)
            };
            settings.lower_bound = settings.lower_bound.max(minimum);
        }
        if settings.lower_bound > settings.upper_bound && valid(settings.upper_bound) {
            settings.lower_bound = settings.upper_bound;
        }
        if settings.lower_bound > settings.upper_bound {
            return Err(Error::RootNotBracketed(
                "volatility bounds exclude the tree domain".into(),
            ));
        }
    }
    Ok(settings)
}

fn unstable() -> Error {
    Error::ComputationFailure("binomial tree produced unstable parameters, nodes or values".into())
}

// Relative binomial weights start at the mode, preserving underflowed tail probabilities.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "Bounded step counts fit f64 and the binomial mode is floored."
)]
fn binomial_log_weights(steps: usize, p: f64) -> Result<(Vec<f64>, f64)> {
    let mode = steps.min(((steps + 1) as f64 * p) as usize);
    let log_odds = p.ln() - (-p).ln_1p();
    let mut weights = vec![0.0; steps + 1];
    for node in (1..=mode).rev() {
        if node % 256 == 0 {
            crate::cancellation::check()?;
        }
        weights[node - 1] =
            weights[node] + (node as f64 / (steps - node + 1) as f64).ln() - log_odds;
    }
    for node in mode..steps {
        if node % 256 == 0 {
            crate::cancellation::check()?;
        }
        weights[node + 1] =
            weights[node] + ((steps - node) as f64 / (node + 1) as f64).ln() + log_odds;
    }
    let normalization: f64 = weights.iter().map(|w| w.exp()).sum();
    Ok((weights, normalization))
}

#[expect(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    reason = "Exact probability endpoints select degenerate trees and bounded node counts fit f64."
)]
fn european_levels(
    values: &[f64],
    p: f64,
    log_discount: f64,
    level: usize,
) -> Result<(f64, [f64; 3])> {
    let steps = values.len() - 1;
    if p == 0.0 || p == 1.0 {
        let first = if p == 1.0 { steps - level } else { 0 };
        return Ok((
            scaled(
                values[if p == 1.0 { steps } else { 0 }],
                steps as f64 * log_discount,
            ),
            std::array::from_fn(|node| {
                if node <= level {
                    scaled(values[first + node], (steps - level) as f64 * log_discount)
                } else {
                    0.0
                }
            }),
        ));
    }
    let (weights, normalization) = binomial_log_weights(steps, p)?;
    // Sum positive terms in log space so neither large totals nor tiny weights lose a finite price.
    let weighted = |offset: usize, removed: usize| -> Result<f64> {
        let term = |index: usize| {
            let from_lower = p <= 0.5;
            let first = if from_lower {
                Some(index)
            } else {
                index.checked_sub(removed)
            };
            let Some(first) = first.filter(|&i| i <= steps - removed) else {
                return f64::NEG_INFINITY;
            };
            let mut log_ratio = 0.0;
            for r in 0..removed {
                let count = if from_lower {
                    steps - index - r
                } else {
                    index - r
                };
                log_ratio += (count as f64 / (steps - r) as f64
                    * (0.5 / if from_lower { 1.0 - p } else { p }))
                .ln();
            }
            values[first + offset].ln() + weights[index] + log_ratio
        };
        let maximum = (0..=steps).try_fold(f64::NEG_INFINITY, |maximum, i| -> Result<f64> {
            if i % 256 == 0 {
                crate::cancellation::check()?;
            }
            Ok(maximum.max(term(i)))
        })?;
        if maximum == f64::NEG_INFINITY {
            return Ok(0.0);
        }
        let sum = (0..=steps).try_fold(0.0, |sum, i| -> Result<f64> {
            if i % 256 == 0 {
                crate::cancellation::check()?;
            }
            Ok(sum + (term(i) - maximum).exp())
        })?;
        Ok((maximum
            + (sum / normalization).ln()
            + removed as f64 * std::f64::consts::LN_2
            + (steps - removed) as f64 * log_discount)
            .exp())
    };
    let mut nodes = [0.0; 3];
    for (node, value) in nodes.iter_mut().enumerate() {
        if level > 0 && node <= level {
            *value = weighted(node, level)?;
        }
    }
    Ok((weighted(0, 0)?, nodes))
}

// Normalize relative to the mode, so a million-step tree only visits its
// probability band. Successive outward weight ratios decrease monotonically.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "Tree sizes and floored binomial modes are bounded and fit exactly in f64."
)]
fn binomial_upper_tail(steps: usize, probability: f64, first: usize) -> Option<f64> {
    if first == 0 {
        return Some(1.0);
    }
    if first > steps {
        return Some(0.0);
    }
    let complement = 1.0 - probability;
    let mode = (((steps + 1) as f64 * probability) as usize).min(steps);
    let radius = 8.0 * (steps as f64 * probability * complement).sqrt();
    if first.abs_diff(mode) as f64 > radius {
        return None;
    }
    let mut total = crate::numerics::Sum::default();
    let mut tail = crate::numerics::Sum::default();
    total.add(1.0);
    if mode >= first {
        tail.add(1.0);
    }
    let mut visit = |indices: &mut dyn Iterator<Item = usize>, ratio: &dyn Fn(usize) -> f64| {
        let mut weight = 1.0;
        for i in indices {
            let ratio = ratio(i);
            weight *= ratio;
            // Bound all remaining terms by a geometric series. The small
            // tail controls the absolute tolerance when evaluating a rare event.
            if ratio < 1.0 && weight <= f64::EPSILON * (1.0 - ratio) * tail.total() {
                break;
            }
            total.add(weight);
            if i >= first {
                tail.add(weight);
            }
        }
    };
    visit(&mut (mode + 1..=steps), &|i| {
        (steps - i + 1) as f64 * probability / (i as f64 * complement)
    });
    visit(&mut (0..mode).rev(), &|i| {
        (i + 1) as f64 * complement / ((steps - i) as f64 * probability)
    });
    Some(tail.total() / total.total())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Validated binomial counts fit exactly in f64."
)]
fn terminal_tail_levels(
    payoff: Payoff,
    spot: f64,
    steps: usize,
    [width, p, log_discount]: [f64; 3],
    level: usize,
) -> Option<(f64, [f64; 3])> {
    // Use the full log-space sum for rare tails or extreme asset ranges.
    if steps < 256 || !(0.1..=0.9).contains(&p) || steps as f64 * width + spot.ln().abs() > 600.0 {
        return None;
    }
    let moneyness = log_ratio(spot, payoff.strike);
    let log_growth = p
        .mul_add(width.exp_m1(), (1.0 - p) * (-width).exp_m1())
        .ln_1p();
    let tilted = p * (width - log_growth).exp();
    let call = payoff.sign > 0.0;
    let price_at = |offset: usize, removed: usize| {
        let n = steps - removed;
        // Locate the paying suffix, reversing the node order for a put.
        let mut low = 0;
        let mut high = n + 1;
        while low < high {
            let mid = low.midpoint(high);
            let node = offset + if call { mid } else { n - mid };
            let distance = moneyness + (2.0 * node as f64 - steps as f64) * width;
            if payoff.sign * distance > 0.0 {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        if low > n {
            return Some(0.0);
        }
        let tail = |prob| binomial_upper_tail(n, if call { prob } else { 1.0 - prob }, low);
        let discount = n as f64 * log_discount;
        if let Some(payout) = payoff.payout() {
            return Some(scaled(payout, discount + tail(p)?.ln())).filter(|x| x.is_finite());
        }
        let forward = (2.0 * offset as f64 - removed as f64) * width + n as f64 * log_growth;
        let asset = scaled(spot * tail(tilted)?, discount + forward);
        if payoff.is_digital() {
            return Some(asset).filter(|x| x.is_finite());
        }
        let cash = scaled(payoff.strike * tail(p)?, discount);
        let price = payoff.sign * (asset - cash);
        (price.is_finite() && price > 1e-5 * asset.max(cash)).then_some(price)
    };
    let price = price_at(0, 0)?;
    let mut nodes = [0.0; 3];
    if level > 0 {
        for (offset, node) in nodes.iter_mut().enumerate().take(level + 1) {
            *node = price_at(offset, level)?;
        }
    }
    Some((price, nodes))
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "Validated positive i32 step counts bound tree indices and fit exactly in f64."
)]
fn native(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    settings: BinomialSettings,
    request: GreeksRequest,
) -> Result<PricingResult> {
    let (rate, dividend) = flat_rates(context)?;
    let dt = time_step(option, context, settings)?;
    if crate::engines::deterministic::enabled(context) {
        return PricingResult::new(crate::engines::deterministic::vanilla(
            option, payoff, context,
        )?);
    }
    let spot = context.spot_price();
    let exercise = |s: f64| (payoff.sign * (s - payoff.strike)).max(0.0);
    if dt == 0.0 {
        return PricingResult::new(payoff.value(spot));
    }
    let no_early_exercise =
        !payoff.is_american() || (payoff.sign > 0.0 && dividend <= 0.0 && rate >= 0.0);
    if !no_early_exercise {
        settings.validate_rollback()?;
    }
    let (width, up, down, discount, p) = parameters(context, dt)?;
    let steps = settings.step_count as usize;
    let tail_level = if request.has(Greek::Gamma) {
        2.min(steps)
    } else {
        usize::from(request.has(Greek::Delta))
    };
    if no_early_exercise {
        crate::cancellation::check()?;
        if let Some((price, second)) = terminal_tail_levels(
            payoff,
            spot,
            steps,
            [width, p, discount.log_factor],
            tail_level,
        ) {
            let first = if tail_level == 2 {
                [
                    discount.rollback(second[0], second[1], p),
                    discount.rollback(second[1], second[2], p),
                ]
            } else {
                [second[0], second[1]]
            };
            let price = if payoff.is_american() {
                price.max(exercise(spot))
            } else {
                price
            };
            return tree_result(
                price,
                first,
                second,
                spot,
                [up, down, up * up],
                request,
                steps,
            );
        }
    }
    let up_squared = up * up;
    let mut node_spot = spot * down.powi(settings.step_count);
    if !up_squared.is_finite() || !node_spot.is_normal() {
        return Err(unstable());
    }
    let mut values = Vec::with_capacity(steps + 1);
    let log_moneyness = log_ratio(spot, payoff.strike);
    for node in 0..=steps {
        if node % 256 == 0 {
            crate::cancellation::check()?;
        }
        if !node_spot.is_finite() {
            return Err(unstable());
        }
        values.push(if payoff.is_digital() {
            // Log coordinates keep exact equality at the central strike node out of the money.
            if payoff.sign * (log_moneyness + (2.0 * node as f64 - steps as f64) * width) > 0.0 {
                payoff.payout().unwrap_or(node_spot)
            } else {
                0.0
            }
        } else {
            exercise(node_spot)
        });
        node_spot *= up_squared;
    }
    let mut first = [0.0; 2];
    let mut second = [0.0; 3];
    // The log-space sum can price rare large payoffs whose discounted values overflow.
    let direct = no_early_exercise
        && (steps >= 256
            || !discount.factor.is_normal()
            || !scaled(
                values.iter().copied().fold(0.0, f64::max),
                steps as f64 * discount.log_factor,
            )
            .is_finite());
    if direct {
        let level = if request.has(Greek::Gamma) {
            2.min(steps)
        } else {
            usize::from(request.has(Greek::Delta))
        };
        let (price, nodes) = european_levels(&values, p, discount.log_factor, level)?;
        second = nodes;
        first = if level == 2 {
            [
                discount.rollback(nodes[0], nodes[1], p),
                discount.rollback(nodes[1], nodes[2], p),
            ]
        } else {
            [nodes[0], nodes[1]]
        };
        values[0] = price.max(if payoff.is_american() {
            exercise(spot)
        } else {
            0.0
        });
    } else {
        settings.validate_rollback()?;
        node_spot = spot * down.powi(settings.step_count - 1);
        for level in (0..steps).rev() {
            crate::cancellation::check()?;
            if level + 1 == 1 {
                first.copy_from_slice(&values[..2]);
            }
            if level + 1 == 2 {
                second.copy_from_slice(&values[..3]);
            }
            if level < steps - 1 {
                node_spot *= up;
            }
            if payoff.is_american() && level == 0 {
                let continuation = discount.rollback(values[0], values[1], p);
                if exercise(spot) > 0.0 && exercise(spot) > continuation {
                    let mut result = PricingResult::new(exercise(spot))?;
                    result.set(request, Greek::Delta, payoff.sign)?;
                    result.set(request, Greek::Gamma, 0.0)?;
                    return Ok(result);
                }
            }
            let mut asset = node_spot;
            for index in 0..=level {
                let continuation = discount.rollback(values[index], values[index + 1], p);
                values[index] = if payoff.is_american() {
                    continuation.max(exercise(asset))
                } else {
                    continuation
                };
                if !values[index].is_finite() {
                    return Err(unstable());
                }
                asset *= up_squared;
            }
        }
    }
    tree_result(
        values[0],
        first,
        second,
        spot,
        [up, down, up_squared],
        request,
        steps,
    )
}

fn tree_result(
    price: f64,
    first: [f64; 2],
    second: [f64; 3],
    spot: f64,
    [up, down, up_squared]: [f64; 3],
    request: GreeksRequest,
    steps: usize,
) -> Result<PricingResult> {
    let mut result = PricingResult::new(price)?;
    if request.has(Greek::Delta) {
        let denominator = spot * (up - down);
        if denominator.is_finite() && denominator != 0.0 {
            result.set(request, Greek::Delta, (first[1] - first[0]) / denominator)?;
        }
    }
    if request.has(Greek::Gamma) && steps >= 2 {
        let upper = spot * (up_squared - 1.0);
        let lower = spot * (1.0 - down * down);
        let denominator = 0.5 * spot * (up_squared - down * down);
        if upper != 0.0 && lower != 0.0 && denominator != 0.0 {
            let gamma =
                ((second[2] - second[1]) / upper - (second[1] - second[0]) / lower) / denominator;
            if gamma.is_finite() {
                result.set(request, Greek::Gamma, gamma)?;
            }
        }
    }
    Ok(result)
}

macro_rules! vanilla_pricing {
    ($option:ty, $american:expr) => {
        impl PricingEngine<$option> for CoxRossRubinsteinVanillaEngine {
            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                flat_rates(context).map(|_| ())
            }

            fn greek_method(
                &self,
                option: &$option,
                context: &PricingContext,
            ) -> Result<crate::GreekMethod> {
                <Self as PricingEngine<$option>>::validate_applicability(self, context)?;
                Ok(crate::GreekMethod::for_valuation(
                    option,
                    context,
                    if crate::engines::deterministic::enabled(context) {
                        crate::GreekMethod::Numerical
                    } else {
                        crate::GreekMethod::Mixed
                    },
                ))
            }

            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                context.natural_bsm_parameters()?;
                if let Some(tolerance) = self.rollback_tolerance.filter(|_| $american) {
                    flat_rates(context)?;
                    return Ok(self
                        .banded_native(
                            option,
                            Payoff::vanilla(option.option_type(), option.strike(), true),
                            context,
                            tolerance,
                            GreeksRequest::new([]),
                        )?
                        .price());
                }
                Ok(native(
                    option,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    context,
                    self.settings,
                    GreeksRequest::new([]),
                )?
                .price())
            }

            fn price_with_greeks_with_settings(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                settings: crate::NumericalShiftSettings,
            ) -> Result<PricingResult> {
                context.natural_bsm_parameters()?;
                flat_rates(context)?;
                settings.validate()?;
                if let Some(tolerance) = self.rollback_tolerance.filter(|_| $american) {
                    time_step(option, context, self.settings)?;
                    return self.banded_greeks(
                        option,
                        Payoff::vanilla(option.option_type(), option.strike(), true),
                        context,
                        request,
                        settings,
                        tolerance,
                    );
                }
                let evaluate = |context: &PricingContext, request| {
                    native(
                        option,
                        Payoff::vanilla(option.option_type(), option.strike(), $american),
                        context,
                        self.settings,
                        request,
                    )
                };
                crate::risk::numerical::complete_greeks(
                    option,
                    context,
                    request,
                    settings,
                    true,
                    evaluate(context, request)?,
                    evaluate,
                )
            }
            fn prepare_implied_volatility(
                &self,
                option: &$option,
                context: &PricingContext,
                settings: ImpliedVolatilitySettings,
            ) -> Result<ImpliedVolatilitySettings> {
                prepare_implied(option, context, self.settings, settings)
            }
        }
    };
}
vanilla_pricing!(EuropeanOption, false);
vanilla_pricing!(AmericanOption, true);

/// Prices European cash-or-nothing and asset-or-nothing contracts with a CRR tree.
///
/// Supports only node delta/gamma; other requested Greeks are absent.
/// Values own their settings; shared instances support concurrent pricing.
/// See [`Self::price_with_greeks`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoxRossRubinsteinDigitalEngine {
    settings: BinomialSettings,
}

impl CoxRossRubinsteinDigitalEngine {
    /// Creates an engine after validating the step count.
    ///
    /// See [`CoxRossRubinsteinVanillaEngine::new`] for a settings example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside 1 through 1,000,000 steps.
    pub fn new(settings: BinomialSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> BinomialSettings {
        self.settings
    }

    /// Prices a European digital, settling strictly in the money at expiry.
    ///
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Reuses [`CoxRossRubinsteinVanillaEngine::price`] errors and validation order.
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        context.natural_bsm_parameters()?;
        Ok(self
            .native(option, context, GreeksRequest::new([]))?
            .price())
    }

    /// Returns requested node delta/gamma; all other Greeks are absent.
    ///
    /// Uses engine-default estimation controls; see [`crate::numerical_greeks`] for custom bumps.
    /// Terminal stencils use engine-default spot bumps.
    /// One-step gamma is absent before expiry. Expiry spot Greeks follow terminal payoffs.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before shift validation, then reuses
    /// [`Self::price`] errors. Requested measures other than delta and gamma
    /// have [`crate::GreekStatus::Unsupported`]; empty requests select price only.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{CoxRossRubinsteinDigitalEngine, DigitalOption,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let result = CoxRossRubinsteinDigitalEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::all())?;
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// assert!(!result.has(Greek::Vega));
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.greeks_with_settings(option, context, request, Default::default())
    }

    fn greeks_with_settings(
        self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        super::greeks(&self, option, context, request, settings, || {
            self.native(option, context, request)
        })
    }

    fn native(
        self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        native(
            option,
            Payoff::digital(option),
            context,
            self.settings,
            request,
        )
    }
}

impl PricingEngine<DigitalOption> for CoxRossRubinsteinDigitalEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }

    fn greek_method(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<DigitalOption>>::validate_applicability(self, context)?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            if crate::engines::deterministic::enabled(context) {
                crate::GreekMethod::NativeWithSpotControl
            } else {
                crate::GreekMethod::Native
            },
        ))
    }

    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.greeks_with_settings(option, context, request, settings)
    }
    fn prepare_implied_volatility(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        settings: ImpliedVolatilitySettings,
    ) -> Result<ImpliedVolatilitySettings> {
        prepare_implied(option, context, self.settings, settings)
    }
}

#[derive(Clone, Copy)]
struct StepDiscount {
    log_factor: f64,
    factor: f64,
}

impl StepDiscount {
    #[inline]
    fn rollback(self, lower: f64, upper: f64, probability: f64) -> f64 {
        if self.factor.is_normal() {
            self.rollback_normal(lower, upper, probability)
        } else {
            self.rollback_scaled(lower, upper, probability)
        }
    }

    fn rollback_normal(self, lower: f64, upper: f64, probability: f64) -> f64 {
        self.factor * ((1.0 - probability) * lower + probability * upper)
    }

    #[cold]
    #[inline(never)]
    fn rollback_scaled(self, lower: f64, upper: f64, probability: f64) -> f64 {
        // Combine each node with its probability and discount before restoring its scale.
        scaled(lower, self.log_factor + (-probability).ln_1p())
            + scaled(upper, self.log_factor + probability.ln())
    }
}

fn parameters(context: &PricingContext, dt: f64) -> Result<(f64, f64, f64, StepDiscount, f64)> {
    let (rate, dividend) = flat_rates(context)?;
    let width = context.natural_bsm_parameters()?.volatility() * dt.sqrt();
    tree_parameters(rate, (rate - dividend) * dt, width, dt)
}

#[expect(
    clippy::float_cmp,
    reason = "Exactly collapsed up/down factors make the tree unrepresentable."
)]
fn tree_parameters(
    rate: f64,
    carry: f64,
    width: f64,
    dt: f64,
) -> Result<(f64, f64, f64, StepDiscount, f64)> {
    let up = width.exp();
    let down = 1.0 / up;
    let log_factor = -rate * dt;
    let p = (carry + width).exp_m1() / (2.0 * width).exp_m1();
    if !up.is_finite()
        || !down.is_finite()
        || up == down
        || !log_factor.is_finite()
        || !(0.0..=1.0).contains(&p)
    {
        return Err(unstable());
    }
    Ok((
        width,
        up,
        down,
        StepDiscount {
            log_factor,
            factor: log_factor.exp(),
        },
        p,
    ))
}
