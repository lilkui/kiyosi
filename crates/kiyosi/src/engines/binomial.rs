use super::Payoff;

mod barrier;
pub use barrier::CoxRossRubinsteinBarrierEngine;

use crate::{
    AmericanOption, DigitalOption, Error, EuropeanOption, Greek, GreeksRequest,
    ImpliedVolatilitySettings, NumericalShiftSettings, OptionContract, OptionType, PricingContext,
    PricingEngine, PricingResult, Result,
};

/// Configures the number of CRR time steps. See [`CoxRossRubinsteinVanillaEngine::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinomialSettings {
    /// Sets the number of steps, from 1 through 1,000,000 inclusive; defaults to 256.
    pub step_count: i32,
}

impl Default for BinomialSettings {
    fn default() -> Self {
        Self { step_count: 256 }
    }
}

impl BinomialSettings {
    fn validate(self) -> Result<()> {
        if !(1..=1_000_000).contains(&self.step_count) {
            return Err(Error::InvalidInput("invalid binomial step count".into()));
        }
        Ok(())
    }
}

/// Prices European and American vanilla contracts with a flat-rate CRR tree.
///
/// Returns node delta/gamma and completes other requested Greeks numerically.
/// Values own their settings; shared instances support concurrent pricing.
/// See [`Self::price_with_greeks`] for an example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoxRossRubinsteinVanillaEngine {
    settings: BinomialSettings,
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
        Ok(Self { settings })
    }

    /// Returns owned settings. See [`Self::new`] for an example.
    pub fn settings(&self) -> BinomialSettings {
        self.settings
    }

    /// Prices a supported vanilla contract without changing inputs.
    ///
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for node curves before life
    /// validation, [`Error::InvalidInput`] outside contract life, and
    /// [`Error::ComputationFailure`] for unstable probabilities, asset nodes or values.
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Returns node delta/gamma and numerically completes missing requested Greeks.
    ///
    /// At expiry every Greek is absent. One-step gamma uses scalar differences.
    /// Bumps use [`NumericalShiftSettings`]; feasible bumped failures propagate.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request/shift validation, then reuses
    /// [`Self::price`] and [`crate::numerical_greeks`] errors. Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AmericanOption, CoxRossRubinsteinVanillaEngine,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let result = CoxRossRubinsteinVanillaEngine::default().price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price_with_greeks(self, option, context, request, settings)
    }
}

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.model_parameters();
    Ok((model.risk_free_rate()?, model.dividend_yield()?))
}

fn time_step(
    option: &impl OptionContract,
    context: &PricingContext,
    settings: BinomialSettings,
) -> Result<f64> {
    let valuation = context.valuation_time();
    let expiry = option.expiry_date().start_of_day();
    if valuation < option.effective_date().start_of_day() || valuation > expiry {
        return Err(Error::InvalidInput(
            "valuation must be within contract life".into(),
        ));
    }
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
        // Round into the admissible domain so the endpoint probability stays in [0, 1].
        let minimum = libm::nextafter(((rate - dividend) * dt).abs() / dt.sqrt(), f64::INFINITY);
        if minimum.is_finite() {
            settings.lower_bound = settings.lower_bound.max(minimum);
        }
        if settings.lower_bound >= settings.upper_bound {
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

fn scaled(value: f64, exponent: f64) -> f64 {
    let factor = exponent.exp();
    let result = value * factor;
    if value == 0.0 {
        0.0
    } else if factor.is_normal() && result.is_normal() {
        result
    } else {
        (value.ln() + exponent).exp()
    }
}

// Relative binomial weights start at the mode, preserving underflowed tail probabilities.
fn european_levels(values: &[f64], p: f64, discount: f64, level: usize) -> (f64, [f64; 3]) {
    let steps = values.len() - 1;
    let log_discount = discount.ln();
    if p == 0.0 || p == 1.0 {
        let first = if p == 1.0 { steps - level } else { 0 };
        return (
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
        );
    }
    let mode = steps.min(((steps + 1) as f64 * p) as usize);
    let log_odds = p.ln() - (-p).ln_1p();
    let mut weights = vec![0.0; steps + 1];
    for node in (1..=mode).rev() {
        weights[node - 1] =
            weights[node] + (node as f64 / (steps - node + 1) as f64).ln() - log_odds;
    }
    for node in mode..steps {
        weights[node + 1] =
            weights[node] + ((steps - node) as f64 / (node + 1) as f64).ln() + log_odds;
    }
    let normalization: f64 = weights.iter().map(|w| w.exp()).sum();
    // Sum positive terms in log space so neither large totals nor tiny weights lose a finite price.
    let weighted = |offset: usize, removed: usize| {
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
        let maximum = (0..=steps).map(term).fold(f64::NEG_INFINITY, f64::max);
        if maximum == f64::NEG_INFINITY {
            return 0.0;
        }
        let sum: f64 = (0..=steps).map(|i| (term(i) - maximum).exp()).sum();
        (maximum
            + (sum / normalization).ln()
            + removed as f64 * std::f64::consts::LN_2
            + (steps - removed) as f64 * log_discount)
            .exp()
    };
    (
        weighted(0, 0),
        std::array::from_fn(|node| {
            if level > 0 && node <= level {
                weighted(node, level)
            } else {
                0.0
            }
        }),
    )
}

fn native(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    settings: BinomialSettings,
    request: GreeksRequest,
) -> Result<PricingResult> {
    let (rate, dividend) = flat_rates(context)?;
    let dt = time_step(option, context, settings)?;
    let spot = context.spot_price();
    let exercise = |s: f64| (payoff.sign * (s - payoff.strike)).max(0.0);
    if dt == 0.0 {
        return PricingResult::new(payoff.value(spot));
    }
    let (width, up, down, discount, p) = parameters(context, dt)?;
    let steps = settings.step_count as usize;
    let up_squared = up * up;
    let mut node_spot = spot * down.powi(settings.step_count);
    if !up_squared.is_finite() || !node_spot.is_normal() {
        return Err(unstable());
    }
    let mut values = Vec::with_capacity(steps + 1);
    let relative = (spot - payoff.strike) / payoff.strike;
    let log_moneyness = if relative.abs() < 0.5 {
        relative.ln_1p()
    } else {
        spot.ln() - payoff.strike.ln()
    };
    for node in 0..=steps {
        if !node_spot.is_finite() {
            return Err(unstable());
        }
        values.push(if payoff.digital {
            // Log coordinates keep exact equality at the central strike node out of the money.
            if payoff.sign * (log_moneyness + (2.0 * node as f64 - steps as f64) * width) > 0.0 {
                payoff.payout.unwrap_or(node_spot)
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
    let no_early_exercise =
        !payoff.american || (payoff.sign > 0.0 && dividend <= 0.0 && rate >= 0.0);
    let direct = no_early_exercise
        && steps >= 256
        && scaled(
            values.iter().copied().fold(0.0, f64::max),
            steps as f64 * discount.ln(),
        )
        .is_finite();
    if direct {
        let level = if request.has(Greek::Gamma) {
            2
        } else {
            usize::from(request.has(Greek::Delta))
        };
        let (price, nodes) = european_levels(&values, p, discount, level);
        second = nodes;
        first = if level == 2 {
            [
                discount * ((1.0 - p) * nodes[0] + p * nodes[1]),
                discount * ((1.0 - p) * nodes[1] + p * nodes[2]),
            ]
        } else {
            [nodes[0], nodes[1]]
        };
        values[0] = price.max(if payoff.american { exercise(spot) } else { 0.0 });
    } else {
        node_spot = spot * down.powi(settings.step_count - 1);
        for level in (0..steps).rev() {
            if level + 1 == 1 {
                first.copy_from_slice(&values[..2]);
            }
            if level + 1 == 2 {
                second.copy_from_slice(&values[..3]);
            }
            if level < steps - 1 {
                node_spot *= up;
            }
            if payoff.american && level == 0 {
                let continuation = discount * ((1.0 - p) * values[0] + p * values[1]);
                if exercise(spot) > 0.0 && exercise(spot) > continuation {
                    let mut result = PricingResult::new(exercise(spot))?;
                    result.set(request, Greek::Delta, payoff.sign)?;
                    result.set(request, Greek::Gamma, 0.0)?;
                    return Ok(result);
                }
            }
            let mut asset = node_spot;
            for index in 0..=level {
                let continuation = discount * ((1.0 - p) * values[index] + p * values[index + 1]);
                values[index] = if payoff.american {
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
    let mut result = PricingResult::new(values[0])?;
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
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                Ok(native(
                    option,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    context,
                    self.settings,
                    GreeksRequest::default(),
                )?
                .price())
            }
            fn price_with_greeks(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                settings: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                flat_rates(context)?;
                request.validate()?;
                settings.validate()?;
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
/// Supports only node delta/gamma; other requested Greeks remain absent.
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
        Ok(self
            .native(option, context, GreeksRequest::default())?
            .price())
    }

    /// Returns requested node delta/gamma; all other Greeks remain absent.
    ///
    /// One-step gamma and all expiry Greeks are absent.
    ///
    /// # Errors
    ///
    /// Checks flat-rate applicability before request validation, then reuses
    /// [`Self::price`] errors. Empty requests return
    /// [`Error::InvalidInput`]. Inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{CoxRossRubinsteinDigitalEngine, DigitalOption,
    ///     BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?)?;
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
        flat_rates(context)?;
        request.validate()?;
        self.native(option, context, request)
    }

    fn native(
        &self,
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
    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
        _settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request)
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

fn parameters(context: &PricingContext, dt: f64) -> Result<(f64, f64, f64, f64, f64)> {
    let (rate, dividend) = flat_rates(context)?;
    let width = context.model_parameters().volatility() * dt.sqrt();
    let up = width.exp();
    let down = 1.0 / up;
    let discount = (-rate * dt).exp();
    let p = ((rate - dividend) * dt + width).exp_m1() / (2.0 * width).exp_m1();
    if !up.is_finite()
        || !down.is_finite()
        || up == down
        || !discount.is_finite()
        || !(0.0..=1.0).contains(&p)
    {
        return Err(unstable());
    }
    Ok((width, up, down, discount, p))
}
