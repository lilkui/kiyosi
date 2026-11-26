use rayon::prelude::*;
use std::sync::{
    OnceLock,
    atomic::{AtomicU32, Ordering},
};

use super::grid::{
    SpatialGrid, Spline, StrikeMoments, TimeGrid, concentrate, project_payoff, spot_floor,
};
use super::{Payoff, WORKSPACE_BUDGET, invalid_grid, prepare_rate_curves, unstable, validate_work};
use crate::engines::analytic::heston::integrated_variance;
use crate::engines::deterministic::{self, AmericanExercise, ExerciseBoundaries};
use crate::instruments::monitoring::validate_life;
use crate::numerics::{checked_with_scale, scaled_cash};
use crate::{
    AmericanOption, EuropeanOption, EuropeanOptionChanges, GreeksRequest, HestonParameters,
    OptionContract, PricingContext, PricingEngine, PricingResult, Result, Timestamp,
};

mod edge_weights;
use edge_weights::EdgeWeights;

/// Configures fixed Heston grids; see [`FiniteDifferenceHestonVanillaEngine::new`].
///
/// Counts specify intervals, not nodes. Curve knots are added to the time grid.
/// Pricing limits estimated workspace and rollback work. Successful pricing
/// does not guarantee a particular accuracy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HestonFiniteDifferenceSettings {
    /// Sets at least three asset intervals; defaults to 400.
    pub asset_step_count: i32,
    /// Sets at least three variance intervals; defaults to 100.
    pub variance_step_count: i32,
    /// Sets at least one base time interval; defaults to 400.
    pub time_step_count: i32,
    /// Sets a finite positive asset upper bound, or selects it automatically.
    ///
    /// When a numerical grid is required, must exceed valuation spot and strike.
    pub asset_upper_boundary: Option<f64>,
    /// Sets a finite positive variance upper bound, or selects it automatically.
    ///
    /// When a variance grid is required, must exceed initial variance.
    pub variance_upper_boundary: Option<f64>,
}

impl Default for HestonFiniteDifferenceSettings {
    fn default() -> Self {
        Self {
            asset_step_count: 400,
            variance_step_count: 100,
            time_step_count: 400,
            asset_upper_boundary: None,
            variance_upper_boundary: None,
        }
    }
}

impl HestonFiniteDifferenceSettings {
    /// Validates context-independent controls; see [`FiniteDifferenceHestonVanillaEngine::new`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] for insufficient counts or nonpositive or
    /// non-finite explicit bounds.
    pub fn validate(&self) -> Result<()> {
        for (name, count, minimum) in [
            ("asset_step_count", self.asset_step_count, 3),
            ("variance_step_count", self.variance_step_count, 3),
            ("time_step_count", self.time_step_count, 1),
        ] {
            if count < minimum {
                return Err(crate::Error::InvalidInput(
                    format!("{name} must be at least {minimum}").into(),
                ));
            }
        }
        for (name, boundary) in [
            ("asset_upper_boundary", self.asset_upper_boundary),
            ("variance_upper_boundary", self.variance_upper_boundary),
        ] {
            if let Some(value) = boundary {
                crate::error::finite(value, true, name)?;
            }
        }
        Ok(())
    }
}

/// Prices European and American vanilla options under the Heston model.
///
/// The asset grid uses log spot and an automatically selected positive lower boundary.
/// Uses the modified Craig-Sneyd (MCS) alternating direction implicit (ADI)
/// scheme with a fixed splitting parameter of 1/3, nine-point mixed derivatives,
/// nonuniform price/variance grids and Ikonen-Toivanen American splitting.
/// Accepts deterministic rate and dividend-yield curves, parameters that violate
/// the Feller condition, zero parameters and correlation endpoints. With zero
/// volatility of volatility, pricing uses the deterministic variance path in
/// one dimension. Absorbing zero variance uses optimal
/// deterministic exercise. Requested Greeks have [`crate::GreekStatus::Unsupported`];
/// implied-parameter solvers return [`crate::Error::UnsupportedOperation`].
///
/// Domain truncation and discretization both require convergence checks. No
/// automatic error estimate or refinement is performed. Shared and independent
/// instances support concurrent calls with request-local state. See [`Self::new`].
///
/// # References
///
/// In 't Hout and Foulon (2010), [ADI finite difference schemes for option pricing
/// in the Heston model with correlation][heston-adi].
///
/// [heston-adi]: https://www.math.ualberta.ca/ijnam/Volume-7-2010/No-2-10/2010-02-06.pdf
///
/// # Pricing
///
/// Prices a European or American call or put; see [`Self::new`].
///
/// Validates the model, permitted valuation bounds and curve coverage before expiry payoff.
/// European valuation may precede the effective time; American valuation must
/// be within the inclusive contract life. Both require rate/dividend coverage
/// from valuation through expiry; see [`OptionContract::valuation_start`].
/// Grid coverage is checked only when that grid is needed. Initial variance
/// is at valuation. Explicit domains must be checked by enlargement, as must
/// automatic domains. Only rounding-sized negative prices may be clipped.
///
/// ## Errors
///
/// Returns [`crate::Error::UnsupportedOperation`] for other models,
/// [`crate::Error::InvalidInput`] outside permitted valuation bounds or curve coverage, or for explicit
/// bounds that exclude spot, strike or initial variance, estimated workspace above 1 GiB,
/// more than one billion spatial grid points times time intervals, or American
/// exercise-boundary work estimated above 25 million curve-knot visits, and
/// [`crate::Error::ComputationFailure`] for unrepresentable grids, intermediates or
/// invalid prices. Fixed grids do not report error-target nonconvergence.
/// Inputs and settings remain unchanged on success and failure.
///
/// # Greeks
///
/// Returns the price with requested Greeks marked unsupported; see [`Self::new`].
///
/// Use [`PricingResult::require`] to reject an unavailable Greek.
///
/// ## Errors
///
/// Checks Heston applicability and propagates [`Self::price`] errors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FiniteDifferenceHestonVanillaEngine {
    settings: HestonFiniteDifferenceSettings,
}

impl FiniteDifferenceHestonVanillaEngine {
    /// Creates an engine with validated fixed-grid controls.
    ///
    /// # Errors
    ///
    /// Propagates [`HestonFiniteDifferenceSettings::validate`] errors; inputs remain unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AmericanOption, Date, FiniteDifferenceHestonVanillaEngine,
    ///     HestonFiniteDifferenceSettings, HestonParameters, OptionType, PricingContext, PricingEngine};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let model = HestonParameters::new(kiyosi::HestonParameterInputs {
    ///     risk_free_rate: 0.05.into(), dividend_yield: 0.02.into(),
    ///     initial_variance: 0.04, long_run_variance: 0.04,
    ///     mean_reversion_speed: 1.5, vol_of_vol: 0.3, correlation: -0.7,
    /// })?;
    /// let context = PricingContext::new(model, 100.0, start.into())?;
    /// let engine = FiniteDifferenceHestonVanillaEngine::new(HestonFiniteDifferenceSettings::default())?;
    /// assert_eq!(engine.settings().variance_step_count, 100);
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: HestonFiniteDifferenceSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned grid controls; see [`Self::new`].
    pub fn settings(&self) -> HestonFiniteDifferenceSettings {
        self.settings
    }

    /// Prices European vanilla options across strikes using the template's direction and dates.
    ///
    /// The result preserves input order and duplicates. The automatic domain covers
    /// the entire spot-to-strike range in a single rollback. Compatible strikes
    /// with an explicit asset boundary share an absolute grid and its factors.
    /// A European batch with the spot on a grid node can value all payoffs
    /// using weights from one transposed rollback.
    /// Check convergence with mesh refinement across the full range.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::{Date, EuropeanOption, FiniteDifferenceHestonVanillaEngine,
    ///     HestonParameterInputs, HestonParameters, OptionType, PricingContext};
    ///
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    /// let model = HestonParameters::new(HestonParameterInputs {
    ///     risk_free_rate: 0.05.into(),
    ///     dividend_yield: 0.02.into(),
    ///     initial_variance: 0.04,
    ///     long_run_variance: 0.04,
    ///     mean_reversion_speed: 1.5,
    ///     vol_of_vol: 0.3,
    ///     correlation: -0.7,
    /// })?;
    /// let market = PricingContext::new(model, 100.0, start)?;
    /// let prices = FiniteDifferenceHestonVanillaEngine::default()
    ///     .price_european_strikes(&option, &market, &[90.0, 110.0])?;
    /// assert!(prices[0] > prices[1]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Propagates scalar pricing errors for the supplied strikes and returns
    /// [`crate::Error::InvalidInput`] if a strike is nonpositive or non-finite.
    /// An error or cancellation returns no partial prices.
    pub fn price_european_strikes(
        &self,
        template: &EuropeanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        price_strikes(
            template,
            context,
            Payoff::vanilla(template.option_type(), template.strike(), false),
            self.settings,
            strikes,
        )
    }

    /// Prices American vanilla options across strikes using the template's direction and dates.
    ///
    /// Uses the batch semantics of [`Self::price_european_strikes`] with early exercise.
    ///
    /// # Errors
    ///
    /// Propagates the validation and pricing errors of [`Self::price_european_strikes`].
    pub fn price_american_strikes(
        &self,
        template: &AmericanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        price_strikes(
            template,
            context,
            Payoff::vanilla(template.option_type(), template.strike(), true),
            self.settings,
            strikes,
        )
    }

    /// Prices a European strike and expiry surface on a shared forward grid.
    ///
    /// The template supplies the payoff direction and effective time. Outer rows
    /// follow `expiries`, and each row follows `strikes`, including duplicates.
    /// Time steps span the longest maturity and include every requested expiry.
    /// Check mesh and domain convergence, especially for short maturities and
    /// wide strike ranges. See [`Self::price_european_strikes`] for the inputs.
    ///
    /// # Examples
    ///
    /// With the template, market and dates in [`Self::price_european_strikes`],
    /// pass `&[start.add_days(90)?, end]` and `&[90.0, 110.0]` to price both
    /// strikes at both expiries. The first row contains the 90-day prices.
    ///
    /// # Errors
    ///
    /// Propagates scalar validation and pricing errors and rejects excessive
    /// output size or grid work. Cancellation returns no partial prices.
    pub fn price_european_surface(
        &self,
        template: &EuropeanOption,
        context: &PricingContext,
        expiries: &[Timestamp],
        strikes: &[f64],
    ) -> Result<Vec<Vec<f64>>> {
        let model = context.model_parameters().as_heston()?;
        let valuation = context.valuation_time();
        let mut strike_range = [f64::INFINITY, 0.0_f64];
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
            strike_range = [strike_range[0].min(strike), strike_range[1].max(strike)];
        }
        let mut ordered = Vec::new();
        if (expiries.len() as u64).saturating_mul(64 + (strikes.len() as u64).saturating_mul(16))
            > WORKSPACE_BUDGET / 4
        {
            return Err(invalid_grid(
                "Heston surface output exceeds workspace budget",
            ));
        }
        for &expiry in expiries {
            crate::cancellation::check()?;
            if expiry < template.effective_time() || expiry < valuation {
                return Err(invalid_grid(
                    "Heston surface expiry precedes effective or valuation time",
                ));
            }
            ordered.push(expiry);
        }
        let contract = |expiry, strike| {
            template.replace(EuropeanOptionChanges {
                expiry_time: Some(expiry),
                strike: Some(strike),
                ..Default::default()
            })
        };
        let Some(end) = ordered.iter().copied().max() else {
            return Ok(Vec::new());
        };
        context.validate_curve_coverage(end)?;
        let Some((&first, rest)) = strikes.split_first() else {
            return Ok(vec![Vec::new(); expiries.len()]);
        };
        ordered.sort_unstable();
        ordered.dedup();
        let live = ordered.partition_point(|&expiry| expiry <= valuation);
        let absorbing = model.initial_variance() == 0.0
            && (model.mean_reversion_speed() == 0.0 || model.long_run_variance() == 0.0);
        let surface = if ordered.len() - live < 2 || absorbing {
            ordered[live..]
                .iter()
                .try_fold(Vec::new(), |mut prices, &expiry| {
                    prices.extend(self.price_european_strikes(
                        &contract(expiry, first)?,
                        context,
                        strikes,
                    )?);
                    Ok::<_, crate::Error>(prices)
                })?
        } else {
            if self
                .settings
                .asset_upper_boundary
                .is_some_and(|upper| upper <= context.spot_price().max(strike_range[1]))
            {
                return Err(invalid_grid(
                    "Heston asset boundary must exceed spot and strike",
                ));
            }
            let times = ordered[live..]
                .iter()
                .map(|&expiry| valuation.years_until(expiry))
                .collect::<Vec<_>>();
            let option = contract(end, first)?;
            let payoff = Payoff::vanilla(option.option_type(), first, false);
            prepare_batch(
                &option,
                context,
                payoff,
                self.settings,
                context.spot_price().min(strike_range[0]),
                rest,
                &times,
            )?
            .1
        };
        expiries
            .iter()
            .map(|expiry| {
                crate::cancellation::check()?;
                let row = ordered.binary_search(expiry).map_err(|_| unstable())?;
                if row < live {
                    return strikes
                        .iter()
                        .map(|&strike| {
                            crate::cancellation::check()?;
                            Ok(Payoff::vanilla(template.option_type(), strike, false)
                                .value(context.spot_price()))
                        })
                        .collect();
                }
                let start = (row - live) * strikes.len();
                let values = surface
                    .get(start..start + strikes.len())
                    .ok_or_else(unstable)?;
                let mut result = Vec::with_capacity(values.len());
                for chunk in values.chunks(1024) {
                    crate::cancellation::check()?;
                    result.extend_from_slice(chunk);
                }
                Ok(result)
            })
            .collect()
    }
}

macro_rules! pricing {
    ($instrument:ty, $american:literal) => {
        impl PricingEngine<$instrument> for FiniteDifferenceHestonVanillaEngine {
            fn greek_method(
                &self,
                option: &$instrument,
                context: &PricingContext,
            ) -> Result<crate::GreekMethod> {
                <Self as PricingEngine<$instrument>>::validate_applicability(self, context)?;
                Ok(crate::GreekMethod::for_valuation(
                    option,
                    context,
                    crate::GreekMethod::Unsupported,
                ))
            }

            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                context.model_parameters().as_heston()?;
                Ok(())
            }
            fn price(&self, option: &$instrument, context: &PricingContext) -> Result<f64> {
                price(
                    option,
                    context,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    self.settings,
                )
            }

            fn price_with_greeks_with_settings(
                &self,
                option: &$instrument,
                context: &PricingContext,
                request: GreeksRequest,
                settings: crate::NumericalShiftSettings,
            ) -> Result<PricingResult> {
                <Self as PricingEngine<$instrument>>::validate_applicability(self, context)?;
                settings.validate()?;
                PricingResult::new(self.price(option, context)?)
                    .map(|result| result.with_request(request, GreeksRequest::new([])))
            }
        }
    };
}
pricing!(EuropeanOption, false);
pricing!(AmericanOption, true);

fn variance_at(model: &HestonParameters, time: f64) -> f64 {
    let x = model.mean_reversion_speed() * time;
    let rise = -(-x).exp_m1();
    let growth = if rise == 0.0 && time > 0.0 && model.mean_reversion_speed() > 0.0 {
        scaled_cash(
            model.long_run_variance(),
            model.mean_reversion_speed().ln() + time.ln(),
        )
    } else {
        model.long_run_variance() * rise
    };
    model.initial_variance() * (-x).exp() + growth
}

fn variance_nodes(count: i32, upper: f64, initial: f64, scale: f64) -> Result<Vec<f64>> {
    if !upper.is_finite() || upper <= initial {
        return Err(unstable());
    }
    let end = (upper / scale).asinh();
    let spacing = end / f64::from(count);
    let mut nodes = Vec::new();
    for i in 0..count {
        if i % 1024 == 0 {
            crate::cancellation::check()?;
        }
        nodes.push(scale * (spacing * f64::from(i)).sinh());
    }
    nodes.push(upper);
    if nodes
        .windows(2)
        .any(|pair| !pair[0].is_finite() || pair[0] >= pair[1])
    {
        return Err(unstable());
    }
    Ok(nodes)
}

fn price(
    option: &impl OptionContract,
    context: &PricingContext,
    payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
) -> Result<f64> {
    prepare(option, context, payoff, settings, context.spot_price())?.price(option, context)
}

fn price_strikes(
    option: &impl OptionContract,
    context: &PricingContext,
    payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
    strikes: &[f64],
) -> Result<Vec<f64>> {
    let model = context.model_parameters().as_heston()?;
    validate_life(option, context)?;
    context.validate_curve_coverage(option.expiry_time())?;
    let spot = context.spot_price();
    let mut low = f64::INFINITY;
    let mut high = 0.0_f64;
    let active = context.valuation_time() < option.expiry_time()
        && (model.initial_variance() > 0.0
            || (model.mean_reversion_speed() > 0.0 && model.long_run_variance() > 0.0));
    let mut share = settings.asset_upper_boundary.is_none() && strikes.len() > 1 && active;
    for &strike in strikes {
        crate::cancellation::check()?;
        crate::error::finite(strike, true, "strike")?;
        let ratio = payoff.strike / strike;
        let normalized = spot * ratio;
        share &= normalized.is_normal() && ratio.is_normal() && ratio.recip().is_normal();
        low = low.min(normalized);
        high = high.max(normalized);
    }
    if !share {
        if let Some(upper) = settings
            .asset_upper_boundary
            .filter(|_| active && strikes.len() > 1)
        {
            return fixed_boundary_prices(option, context, payoff, settings, upper, strikes);
        }
        return strikes
            .iter()
            .map(|&strike| {
                crate::cancellation::check()?;
                price(option, context, Payoff { strike, ..payoff }, settings)
            })
            .collect();
    }
    let anchor = super::solution::at_spot(context, high)?;
    let state = prepare(option, &anchor, payoff, settings, low)?;
    strikes
        .iter()
        .map(|&strike| {
            crate::cancellation::check()?;
            let ratio = payoff.strike / strike;
            let query = super::solution::at_spot(context, spot * ratio)?;
            crate::numerics::checked(state.price(option, &query)? / ratio)
        })
        .collect()
}

fn fixed_boundary_prices(
    option: &impl OptionContract,
    context: &PricingContext,
    payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
    upper: f64,
    strikes: &[f64],
) -> Result<Vec<f64>> {
    let mut ordered = strikes.to_vec();
    ordered.sort_unstable_by(f64::total_cmp);
    ordered.dedup();
    let spot = context.spot_price();
    if spot.max(ordered[ordered.len() - 1]) >= upper {
        return Err(invalid_grid(
            "Heston asset boundary must exceed spot and strike",
        ));
    }
    let mut prices = Vec::with_capacity(ordered.len());
    let mut remaining = ordered.as_slice();
    while let Some(&first) = remaining.first() {
        crate::cancellation::check()?;
        let low = spot.min(first);
        let count = if payoff.is_american() {
            remaining.len().min(8)
        } else if low >= 0.5 * spot {
            remaining.partition_point(|&strike| 0.5 * strike <= low)
        } else {
            1
        };
        let (chunk, tail) = remaining.split_at(count);
        remaining = tail;
        let rest = &chunk[1..];
        let high = spot.max(chunk[count - 1]);
        if !rest.is_empty() && low >= 0.5 * high && settings.asset_step_count >= 128 {
            let template = Payoff {
                strike: first,
                ..payoff
            };
            match prepare_batch(option, context, template, settings, low, rest, &[]) {
                Ok((state, values)) => {
                    if let Some(state) = state {
                        prices.push(state.price(option, context)?);
                    }
                    prices.extend(values);
                    continue;
                }
                Err(error @ crate::Error::Io(_)) => return Err(error),
                Err(_) => {}
            }
        }
        for &strike in chunk {
            crate::cancellation::check()?;
            prices.push(price(
                option,
                context,
                Payoff { strike, ..payoff },
                settings,
            )?);
        }
    }
    strikes
        .iter()
        .map(|strike| {
            crate::cancellation::check()?;
            let index = ordered
                .binary_search_by(|k| k.total_cmp(strike))
                .expect("validated strikes are present");
            Ok(prices[index])
        })
        .collect()
}

fn prepare(
    option: &impl OptionContract,
    context: &PricingContext,
    payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
    minimum_spot: f64,
) -> Result<HestonState> {
    prepare_batch(option, context, payoff, settings, minimum_spot, &[], &[])
        .map(|(state, _)| state.expect("scalar preparation returns a saved state"))
}

struct RollbackLane<'a> {
    payoff: Payoff,
    values: Vec<f64>,
    intrinsic: Vec<f64>,
    multiplier: Vec<f64>,
    predictor: Vec<f64>,
    stage: Vec<f64>,
    boundaries: ExerciseBoundaries<'a>,
}

fn adjoint_work(assets: u64, rows: u64, steps: u64, expiries: u64, strikes: u64) -> u64 {
    let edge_terms = 2 * steps;
    let edge_levels = u64::from(edge_terms.max(1).ilog2() + 1);
    let asset_levels = u64::from(assets.ilog2() + 1);
    let quotes = expiries.saturating_mul(strikes);
    (assets * rows)
        .saturating_mul(steps + expiries)
        .saturating_add((edge_terms + quotes).saturating_mul(edge_levels))
        .saturating_add((expiries * assets + quotes).saturating_mul(asset_levels))
}

fn prepare_batch(
    option: &impl OptionContract,
    context: &PricingContext,
    mut payoff: Payoff,
    settings: HestonFiniteDifferenceSettings,
    minimum_spot: f64,
    additional_strikes: &[f64],
    expiries: &[f64],
) -> Result<(Option<HestonState>, Vec<f64>)> {
    // Live values, multiplier, ADI stage buffers, operator coefficients and factors.
    const BYTES_PER_GRID_POINT: u64 = 112;
    // Axis geometry, model coefficients, and terminal and intrinsic payoffs.
    const BYTES_PER_AXIS_NODE: u64 = 184;
    const BYTES_PER_TIME_STEP: u64 = 24;
    let model = context.model_parameters().as_heston()?;
    validate_life(option, context)?;
    let end = option.expiry_time();
    let time = context.valuation_time().years_until(end);
    context.validate_curve_coverage(end)?;
    if time == 0.0 {
        return Ok((Some(HestonState::Expiry(payoff)), Vec::new()));
    }
    let absorbing = model.initial_variance() == 0.0
        && (model.mean_reversion_speed() == 0.0 || model.long_run_variance() == 0.0);
    if absorbing {
        if payoff.is_american() {
            return Ok((
                Some(HestonState::Exercise(AmericanExercise::new(
                    payoff, context, end,
                )?)),
                Vec::new(),
            ));
        }
        context.validate_curve_horizon(end)?;
        return Ok((Some(HestonState::Direct(payoff)), Vec::new()));
    }
    let scale = if expiries.is_empty() {
        payoff.strike.max(context.spot_price())
    } else {
        context.spot_price()
    };
    let minimum_spot = minimum_spot / scale;
    payoff.strike /= scale;
    if minimum_spot == 0.0 || payoff.strike == 0.0 {
        return Err(unstable());
    }
    let integrated = integrated_variance(model, time);
    if !integrated.is_finite() || integrated < 0.0 {
        return Err(unstable());
    }
    if settings.asset_upper_boundary.is_some_and(|x| x <= scale) {
        return Err(invalid_grid(
            "Heston asset boundary must exceed spot and strike",
        ));
    }
    let deterministic = model.vol_of_vol() == 0.0;
    let assets = u64::try_from(settings.asset_step_count).map_err(|_| unstable())? + 1;
    let rows = if deterministic {
        1
    } else {
        u64::try_from(settings.variance_step_count).map_err(|_| unstable())? + 1
    };
    let steps = u64::try_from(settings.time_step_count).map_err(|_| unstable())?;
    let factor_rows = if deterministic { rows } else { rows - 1 };
    let padded = factor_rows.next_multiple_of(ASSET_LANES as u64) - factor_rows;
    let adjoint = !payoff.is_american() && (!additional_strikes.is_empty() || !expiries.is_empty());
    let extra_rhs = if adjoint {
        0
    } else {
        additional_strikes.len() as u64
    };
    let strikes_count = additional_strikes.len() as u64 + 1;
    let expiry_count = expiries.len().max(1) as u64;
    let batch_bytes = if adjoint {
        // Terminal weights, hinge terms and their cumulative sums.
        assets * 80 + expiry_count * strikes_count * 16 + assets * rows * size_of::<f64>() as u64
    } else {
        0
    };
    let grid_bytes = (assets * rows)
        .saturating_mul(BYTES_PER_GRID_POINT + extra_rhs * 4 * size_of::<f64>() as u64)
        .saturating_add(padded * assets * size_of::<[f64; 3]>() as u64)
        .saturating_add((assets + rows) * BYTES_PER_AXIS_NODE)
        .saturating_add(batch_bytes);
    let validate_grid = |time_steps, curve_bytes| {
        let time_bytes = BYTES_PER_TIME_STEP
            + if adjoint {
                edge_weights::BYTES_PER_STEP
            } else {
                0
            };
        if grid_bytes
            .saturating_add(time_steps * time_bytes)
            .saturating_add(curve_bytes)
            > WORKSPACE_BUDGET
        {
            return Err(invalid_grid(
                "Heston finite-difference workspace exceeds 1 GiB; reduce grid step counts",
            ));
        }
        if adjoint {
            let work = adjoint_work(assets, rows, time_steps, expiry_count, strikes_count);
            validate_work(work, 1)
        } else {
            validate_work((assets * rows).saturating_mul(1 + extra_rhs), time_steps)
        }
    };
    let (risk, dividend) = prepare_rate_curves(context, end, |knots, bytes| {
        validate_grid(steps.max(knots), bytes)
    })?;
    let mut drift: f64 = 0.0;
    let mut lower_drift: f64 = 0.0;
    for t in risk.ends().chain(dividend.ends()) {
        crate::cancellation::check()?;
        let carry = dividend.log_factor(0.0, t) - risk.log_factor(0.0, t);
        drift = drift.max(carry);
        lower_drift = lower_drift.max(-carry);
    }
    let upper = settings.asset_upper_boundary.map_or_else(
        || {
            let mesh = (drift + 6.0 * integrated.sqrt()).exp().max(4.0);
            if expiries.is_empty() {
                mesh
            } else {
                let farthest = additional_strikes
                    .iter()
                    .copied()
                    .fold(payoff.strike, |high, strike| high.max(strike / scale));
                mesh.max(2.0 * farthest)
            }
        },
        |x| x / scale,
    );
    let curve_bytes = risk.workspace_bytes() + dividend.workspace_bytes();
    let calendar_curve = !deterministic
        && [model.risk_free_curve(), model.dividend_curve()]
            .iter()
            .any(|curve| curve.calendar_quote().is_some());
    let time_grid = if calendar_curve {
        TimeGrid::aligned
    } else {
        TimeGrid::with_events
    };
    let mut events = expiries.to_vec();
    if let Some(&first) = expiries
        .first()
        .filter(|&&first| first >= 24.0 / 31_536_000_000_000.0)
    {
        // Preserve resolution of the initial point mass for short first expiries.
        events.extend((1..24).map(|index| first * f64::from(index) / 24.0));
    }
    let times = time_grid(
        time,
        settings.time_step_count,
        &risk,
        &dividend,
        &events,
        |steps| validate_grid(steps, curve_bytes),
    )?;
    let width = integrated.sqrt();
    let tail_width = width.max(0.35 * model.vol_of_vol() * time);
    let lower = spot_floor(
        minimum_spot,
        payoff.strike,
        lower_drift + 0.5 * integrated + 6.0 * tail_width,
    );
    let anchor = if expiries.is_empty() {
        payoff.strike
    } else {
        1.0
    };
    let mut asset = SpatialGrid::new(settings.asset_step_count, [lower, upper], anchor)?;
    let carry = dividend.log_factor(0.0, time) - risk.log_factor(0.0, time);
    if width > 0.0 && width < 0.04 && carry.abs() > 4.0 * width {
        asset.resolve_drift_band(anchor, carry, width)?;
    } else {
        let front_width = expiries
            .first()
            .map_or(width, |&first| integrated_variance(model, first).sqrt());
        let asset_scale = if front_width < 0.04 {
            (0.3 * front_width).max(f64::EPSILON)
        } else {
            0.06
        };
        let levels = if adjoint {
            // Project each terminal kink onto the same mesh. Clustering every
            // strike can leave most of the diffusion region under-resolved.
            vec![context.spot_price() / scale]
        } else {
            let mut levels = vec![payoff.strike];
            if !additional_strikes.is_empty() {
                levels.push(context.spot_price() / scale);
                levels.extend(additional_strikes.iter().map(|strike| strike / scale));
            }
            levels
        };
        concentrate(&mut asset, levels, asset_scale)?;
    }
    let mut valuation_row = 0;
    let variance = if deterministic {
        None
    } else {
        if settings
            .variance_upper_boundary
            .is_some_and(|x| x <= model.initial_variance())
        {
            return Err(invalid_grid(
                "Heston variance boundary must exceed initial variance",
            ));
        }
        let mean = model.initial_variance().max(variance_at(model, time));
        let kappa = model.mean_reversion_speed();
        let horizon = if kappa * time == 0.0 {
            time
        } else {
            -(-2.0 * kappa * time).exp_m1() / kappa / 2.0
        };
        let spread = model.vol_of_vol() * (mean * horizon).sqrt();
        let upper = settings
            .variance_upper_boundary
            .unwrap_or_else(|| (mean + 8.0 * spread).max(0.25));
        let variance_scale = (0.1 * mean).max(0.05 * spread);
        let mut nodes = variance_nodes(
            settings.variance_step_count,
            upper,
            model.initial_variance(),
            variance_scale.max(0.0001),
        )?;
        if model.initial_variance() > 0.0 {
            valuation_row = (1..nodes.len() - 1)
                .min_by(|&a, &b| {
                    (nodes[a] - model.initial_variance())
                        .abs()
                        .total_cmp(&(nodes[b] - model.initial_variance()).abs())
                })
                .ok_or_else(unstable)?;
            nodes[valuation_row] = model.initial_variance();
        }
        Some(nodes)
    };
    let nx = asset.count + 1;
    let rows = variance.as_ref().map_or(1, Vec::len);
    let length = nx
        .checked_mul(rows)
        .filter(|&n| n <= isize::MAX as usize / size_of::<[f64; 3]>())
        .ok_or_else(unstable)?;
    let strikes = || {
        std::iter::once(payoff.strike).chain(additional_strikes.iter().map(|strike| strike / scale))
    };
    let mut op = Operator::new(&asset, variance.as_deref(), model)?;
    if calendar_curve {
        const MAX_CACHE_BYTES: u64 = 64 << 20;
        let time_bytes = BYTES_PER_TIME_STEP
            + if adjoint {
                edge_weights::BYTES_PER_STEP
            } else {
                0
            };
        let used = grid_bytes + curve_bytes + times.widths.len() as u64 * time_bytes;
        let rows = rows as u64;
        let spare_bytes = ((2 * rows + padded) * assets + 2 * rows) * size_of::<[f64; 3]>() as u64;
        let available = MAX_CACHE_BYTES.min(WORKSPACE_BUDGET - used);
        op.cache_limit = if spare_bytes <= available / 2 {
            2
        } else {
            usize::from(spare_bytes <= available)
        };
    }
    if adjoint {
        let spot = context.spot_price() / scale;
        let node = asset
            .spots
            .binary_search_by(|s| s.total_cmp(&spot))
            .map_err(|_| invalid_grid("Heston strike batch requires a valuation node"))?;
        return european_strike_weights(
            &asset,
            variance.as_deref(),
            &times,
            [&risk, &dividend],
            model,
            &mut op,
            [lower, upper],
            valuation_row * nx + node,
            payoff,
            &strikes().collect::<Vec<_>>(),
            expiries,
            scale,
        )
        .map(|prices| (None, prices));
    }
    let mut lanes = Vec::with_capacity(additional_strikes.len() + 1);
    for strike in strikes() {
        crate::cancellation::check()?;
        let payoff = Payoff { strike, ..payoff };
        let terminal: Vec<_> = (0..nx)
            .map(|i| project_payoff(&asset, &[strike], i, |s| payoff.value(s)))
            .collect();
        let values: Vec<_> = (0..length).map(|k| terminal[k % nx]).collect();
        let intrinsic: Vec<_> = (0..nx).map(|i| payoff.value(asset.asset(i))).collect();
        let multiplier = if payoff.is_american() {
            vec![0.0; length]
        } else {
            Vec::new()
        };
        let boundaries = ExerciseBoundaries::new(
            payoff,
            [lower, upper],
            time,
            [&risk, &dividend],
            times.widths.len(),
        )?;
        lanes.push(RollbackLane {
            payoff,
            values,
            intrinsic,
            multiplier,
            predictor: vec![0.0; length],
            stage: vec![0.0; length],
            boundaries,
        });
    }
    let rows = op.dynamic_rows();
    let cells = nx * rows;
    let work = cells.saturating_mul(times.widths.len());
    let batched = lanes.len() > 1;
    let pool = ((cells >= 24_000 && work >= 1_000_000 && rows >= 16)
        || (batched && work.saturating_mul(lanes.len()) >= 1_000_000))
        .then(heston_pool)
        .flatten();
    let lane_workers = batched && pool.is_some();
    let tokens = crate::cancellation::current();
    let operator_workers = pool
        .filter(|_| cells >= 24_000 && work >= 1_000_000 && rows >= 16)
        .map(|(_, count)| AssetWorkers {
            count,
            tokens: tokens.clone(),
            rows_per_task: rows.div_ceil(count).max(ASSET_LANES),
        });
    let workers = operator_workers
        .as_ref()
        .filter(|_| !batched && cells >= 64_000 && work >= 8_000_000);
    let mut rollback = || {
        for step in (0..times.widths.len()).rev() {
            crate::cancellation::check()?;
            let a = times.times[step];
            let b = times.times[step + 1];
            let dt = times.widths[step];
            let middle = a.midpoint(b);
            let rate = risk.rate(middle);
            let q = dividend.rate(middle);
            let var = if deterministic {
                variance_at(model, middle)
            } else {
                0.0
            };
            op.prepare(&asset, [var, rate, q, dt / 3.0], operator_workers.as_ref())?;
            let advance = |lane: &mut RollbackLane<'_>| -> Result<()> {
                let RollbackLane {
                    values,
                    intrinsic,
                    multiplier,
                    predictor,
                    stage,
                    boundaries,
                    ..
                } = lane;
                let edges = boundaries.at(a)?;
                if edges.iter().any(|x| !x.is_finite()) {
                    return Err(unstable());
                }
                // Both ADI stages solve for increments with the same directional factors.
                let increment_edges = [edges[0] - values[0], edges[1] - values[nx - 1]];
                op.predict(values, multiplier, [predictor, stage], dt, workers)?;
                op.solve(stage, increment_edges, workers)?;
                op.correct(stage, predictor, dt, workers)?;
                op.solve(predictor, increment_edges, workers)?;
                if payoff.is_american() {
                    for ((row, old), correction) in predictor
                        .chunks_exact_mut(nx)
                        .zip(values.chunks_exact(nx))
                        .zip(multiplier.chunks_exact_mut(nx))
                    {
                        for (((value, &old), multiplier), &intrinsic) in row
                            .iter_mut()
                            .zip(old)
                            .zip(correction)
                            .zip(intrinsic.iter())
                        {
                            let raw = old + *value;
                            *value = (raw - dt * *multiplier).max(intrinsic);
                            *multiplier = (*multiplier + (intrinsic - raw) / dt).max(0.0);
                        }
                    }
                } else {
                    for (value, old) in predictor.iter_mut().zip(values.iter()) {
                        *value += old;
                    }
                }
                op.boundaries(predictor, edges);
                if predictor
                    .iter()
                    .chain(multiplier.iter())
                    .any(|x| !x.is_finite())
                {
                    return Err(unstable());
                }
                std::mem::swap(values, predictor);
                Ok(())
            };
            if lane_workers {
                lanes.par_iter_mut().try_for_each(|lane| {
                    crate::cancellation::with_tokens(tokens.clone(), || advance(lane))
                })?;
            } else {
                for lane in &mut lanes {
                    advance(lane)?;
                }
            }
        }
        Ok(())
    };
    if let Some((pool, _)) = pool {
        pool.install(|| crate::cancellation::with_tokens(tokens.clone(), rollback))?;
    } else {
        rollback()?;
    }
    let mut results = lanes
        .into_iter()
        .map(|RollbackLane { payoff, values, .. }| {
            let spline = asset.spline(if variance.is_some() {
                values[valuation_row * nx..][..nx].to_vec()
            } else {
                values
            });
            (payoff, spline)
        });
    let (payoff, spline) = results.next().expect("the template has a payoff");
    let prices = results
        .map(|(payoff, spline)| grid_price(&asset, &spline, payoff, scale, context.spot_price()))
        .collect::<Result<Vec<_>>>()?;
    Ok((
        Some(HestonState::Grid {
            scale,
            asset,
            payoff,
            spline,
        }),
        prices,
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Reuses the primal grid, curves, and factored operators."
)]
fn european_strike_weights(
    asset: &SpatialGrid,
    variance: Option<&[f64]>,
    times: &TimeGrid,
    [risk, dividend]: [&super::Curve; 2],
    model: &HestonParameters,
    op: &mut Operator,
    bounds: [f64; 2],
    valuation_node: usize,
    payoff: Payoff,
    strikes: &[f64],
    expiries: &[f64],
    scale: f64,
) -> Result<Vec<f64>> {
    let nx = op.nx;
    let length = nx * op.rows;
    let end = times.times[times.times.len() - 1];
    let mut remaining = if expiries.is_empty() {
        std::slice::from_ref(&end)
    } else {
        expiries
    };
    let mut prices = Vec::with_capacity(remaining.len() * strikes.len());
    let mut edge_weights = Vec::with_capacity(times.widths.len());
    let mut edge_index = EdgeWeights::new(
        &times.times[..times.widths.len()],
        [risk, dividend],
        bounds,
        payoff.sign,
    )?;
    let mut quote_work = adjoint_work(
        nx as u64,
        op.rows as u64,
        times.widths.len() as u64,
        remaining.len() as u64,
        strikes.len() as u64,
    );
    let mut weights = vec![0.0; length];
    let mut previous = vec![0.0; length];
    let mut stage = vec![0.0; length];
    weights[valuation_node] = 1.0;
    let mut mixed = vec![0.0; length];
    let mut contributions = vec![[0.0; 2]; op.dynamic_rows().div_ceil(ASSET_LANES)];
    let active = op.dynamic_rows() * nx;
    let work = active.saturating_mul(times.widths.len());
    let pool = (active >= 24_000 && work >= 1_000_000 && op.dynamic_rows() >= 16)
        .then(heston_pool)
        .flatten();
    let tokens = crate::cancellation::current();
    let operator_workers = pool.map(|(_, count)| AssetWorkers {
        count,
        tokens: tokens.clone(),
        rows_per_task: op.dynamic_rows().div_ceil(count).max(ASSET_LANES),
    });
    let workers = operator_workers
        .as_ref()
        .filter(|_| active >= 64_000 && work >= 8_000_000);
    // Reverse each linear rollback stage, carrying weights from valuation to expiry.
    let mut propagate = || {
        for (step, &dt) in times.widths.iter().enumerate() {
            crate::cancellation::check()?;
            let a = times.times[step];
            let middle = a.midpoint(times.times[step + 1]);
            let var = if variance.is_none() {
                variance_at(model, middle)
            } else {
                0.0
            };
            op.prepare(
                asset,
                [var, risk.rate(middle), dividend.rate(middle), dt / 3.0],
                operator_workers.as_ref(),
            )?;
            let mut edges = op.take_boundary_weights(&mut weights);
            previous.copy_from_slice(&weights);
            let inc1 = op.transpose_solve(&mut weights, &mut contributions, workers)?;
            stage.fill(0.0);
            op.transpose_explicit(
                &weights,
                &mut stage,
                &mut mixed,
                [dt / 6.0, dt * 0.5],
                workers,
            )?;
            let inc2 = op.transpose_solve(&mut stage, &mut contributions, workers)?;
            for (value, correction) in weights.iter_mut().zip(&stage) {
                *value += correction;
            }
            op.transpose_explicit(&weights, &mut previous, &mut mixed, [dt, dt], workers)?;
            for (i, column) in [0, nx - 1].into_iter().enumerate() {
                let increment = inc1[i] + inc2[i];
                previous[column] -= increment;
                edges[i] += increment;
            }
            edge_weights.push(edges);
            if edge_index
                .as_mut()
                .is_some_and(|index| index.add(step, edges).is_none())
            {
                edge_index = None;
            }
            std::mem::swap(&mut previous, &mut weights);
            let next = times.times[step + 1];
            while remaining
                .first()
                .is_some_and(|expiry| expiry.to_bits() == next.to_bits())
            {
                european_weighted_prices(
                    asset,
                    &weights,
                    &times.times[..=step],
                    &edge_weights,
                    edge_index.as_ref(),
                    &mut quote_work,
                    [risk, dividend],
                    bounds,
                    next,
                    payoff,
                    strikes,
                    scale,
                    &mut prices,
                )?;
                remaining = &remaining[1..];
            }
        }
        Ok(())
    };
    if let Some((pool, _)) = pool {
        pool.install(|| crate::cancellation::with_tokens(tokens, propagate))?;
    } else {
        propagate()?;
    }
    if !remaining.is_empty() {
        return Err(invalid_grid(
            "Heston surface expiry is missing from the time grid",
        ));
    }
    Ok(prices)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Values a payoff strip using the current grid and boundary weights."
)]
fn european_weighted_prices(
    asset: &SpatialGrid,
    weights: &[f64],
    times: &[f64],
    edge_weights: &[[f64; 2]],
    edge_index: Option<&EdgeWeights>,
    quote_work: &mut u64,
    [risk, dividend]: [&super::Curve; 2],
    bounds: [f64; 2],
    end: f64,
    payoff: Payoff,
    strikes: &[f64],
    scale: f64,
    prices: &mut Vec<f64>,
) -> Result<()> {
    let nx = asset.count + 1;
    let mut terminal = vec![0.0; nx];
    for row in weights.chunks_exact(nx) {
        for (weight, &value) in terminal.iter_mut().zip(row) {
            *weight += value;
        }
    }
    let discount = [risk, dividend].map(|curve| curve.log_factor(0.0, end).exp());
    let terminal_moments = StrikeMoments::new(
        asset
            .spots
            .iter()
            .zip(&terminal)
            .map(|(&spot, &weight)| [spot, weight * spot, weight])
            .collect(),
        payoff.sign,
    );
    let mut price = |strike| {
        crate::cancellation::check()?;
        let payoff = Payoff { strike, ..payoff };
        if let Some(([boundary, boundary_scale], [mut value, terminal_scale])) = edge_index
            .and_then(|edges| edges.at(strike, discount))
            .zip(terminal_moments.at(strike))
        {
            // A strike splits at most one interior control volume.
            let coordinate = asset.coordinate(strike);
            let right = asset
                .nodes
                .partition_point(|&node| node < coordinate)
                .min(nx - 1);
            for (i, &weight) in terminal
                .iter()
                .enumerate()
                .take(right + 1)
                .skip(right.saturating_sub(1))
            {
                let payment = project_payoff(asset, &[strike], i, |spot| payoff.value(spot));
                value += weight * (payment - payoff.value(asset.asset(i)));
            }
            let price = value + boundary;
            if price.is_finite()
                && (boundary_scale + terminal_scale == 0.0
                    || price.abs() > 1e-6 * (boundary_scale + terminal_scale))
            {
                return checked_with_scale(scale * price, scale);
            }
        }
        // Use the original compensated evaluation for extreme scales or cancellation.
        *quote_work = quote_work.saturating_add((times.len() + nx) as u64);
        validate_work(*quote_work, 1)?;
        let mut boundary =
            ExerciseBoundaries::new(payoff, bounds, end, [risk, dividend], times.len())?;
        let mut value = crate::numerics::Sum::default();
        for (i, (&time, edges)) in times.iter().zip(edge_weights).enumerate() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let [left, right] = boundary.at(time)?;
            value.add(edges[0] * left + edges[1] * right);
        }
        for (i, &weight) in terminal.iter().enumerate() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let payment = project_payoff(asset, &[payoff.strike], i, |spot| payoff.value(spot));
            value.add(weight * payment);
        }
        checked_with_scale(scale * value.total(), scale)
    };
    for &strike in strikes {
        prices.push(price(strike)?);
    }
    Ok(())
}

#[derive(PartialEq)]
enum HestonState {
    Expiry(Payoff),
    Exercise(AmericanExercise),
    Direct(Payoff),
    Grid {
        scale: f64,
        asset: SpatialGrid,
        payoff: Payoff,
        spline: Spline,
    },
}
impl HestonState {
    fn price(&self, option: &impl OptionContract, context: &PricingContext) -> Result<f64> {
        let spot = context.spot_price();
        match self {
            Self::Expiry(payoff) => Ok(payoff.value(spot)),
            Self::Exercise(exercise) => exercise.price(context),
            Self::Direct(payoff) => deterministic::vanilla(option, *payoff, context),
            Self::Grid {
                scale,
                asset,
                payoff,
                spline,
            } => grid_price(asset, spline, *payoff, *scale, spot),
        }
    }
}
fn grid_price(
    asset: &SpatialGrid,
    spline: &Spline,
    payoff: Payoff,
    scale: f64,
    spot: f64,
) -> Result<f64> {
    let spot = spot / scale;
    if spot == 0.0 {
        return Err(unstable());
    }
    let mut price = asset.sample(spline, spot)[0];
    if payoff.is_american() {
        let exercise = payoff.value(spot);
        price = if asset.exercise_cell(spline, spot, |s| payoff.value(s)) {
            exercise
        } else {
            price.max(exercise)
        };
    }
    checked_with_scale(scale * price, scale)
}

impl<I: OptionContract> super::solution::Backend<I> for HestonState {
    fn query_work(&self, _: &I, _: &PricingContext) -> usize {
        if let Self::Exercise(exercise) = self {
            exercise.interval_count()
        } else {
            0
        }
    }

    fn evaluate(
        &self,
        option: &I,
        context: &PricingContext,
        _: GreeksRequest,
        _: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        PricingResult::new(self.price(option, context)?)
    }
}
macro_rules! heston_solution {
    ($option:ty, $american:literal) => {
        impl super::FiniteDifferenceEngine<$option> for FiniteDifferenceHestonVanillaEngine {
            fn solve(
                &self,
                option: &$option,
                context: &PricingContext,
                range: std::ops::RangeInclusive<f64>,
            ) -> Result<super::FiniteDifferenceSolution> {
                context.model_parameters().as_heston()?;
                super::solution::validate_range(&range)?;
                crate::cancellation::check()?;
                let anchor = super::solution::at_spot(context, *range.end())?;
                let state = prepare(
                    option,
                    &anchor,
                    Payoff::vanilla(option.option_type(), option.strike(), $american),
                    self.settings,
                    *range.start(),
                )?;
                let low = super::solution::at_spot(context, *range.start())?;
                state.price(option, &low)?;
                crate::cancellation::check()?;
                Ok(super::FiniteDifferenceSolution::new(
                    option.clone(),
                    context,
                    range,
                    state,
                ))
            }
        }
    };
}
heston_solution!(EuropeanOption, false);
heston_solution!(AmericanOption, true);

// Generator-oriented upwinding preserves nonnegative directional off-diagonals.
fn directional(nodes: &[f64], i: usize, diffusion: f64, drift: f64, discount: f64) -> [f64; 3] {
    if i == 0 {
        let c = drift / (nodes[1] - nodes[0]);
        return [0.0, -c - discount, c];
    }
    let left = nodes[i] - nodes[i - 1];
    let right = nodes[i + 1] - nodes[i];
    let span = left + right;
    let mut a = (2.0 * diffusion - drift * right) / (left * span);
    let mut c = (2.0 * diffusion + drift * left) / (right * span);
    if a < 0.0 || c < 0.0 {
        a = 2.0 * diffusion / (left * span) + (-drift).max(0.0) / left;
        c = 2.0 * diffusion / (right * span) + drift.max(0.0) / right;
    }
    [a, -a - c - discount, c]
}

fn derivative(nodes: &[f64], i: usize) -> [f64; 3] {
    let left = nodes[i] - nodes[i - 1];
    let right = nodes[i + 1] - nodes[i];
    [
        -right / (left * (left + right)),
        (right - left) / (left * right),
        left / (right * (left + right)),
    ]
}

static HESTON_POOL_PROCESS: AtomicU32 = AtomicU32::new(0);
static HESTON_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();

struct AssetWorkers {
    count: usize,
    tokens: Vec<crate::CancellationToken>,
    rows_per_task: usize,
}

fn heston_pool() -> Option<(&'static rayon::ThreadPool, usize)> {
    let available = std::thread::available_parallelism().ok()?.get().min(4);
    if available < 2 {
        return None;
    }
    let pid = std::process::id();
    if HESTON_POOL_PROCESS
        .compare_exchange(0, pid, Ordering::Relaxed, Ordering::Relaxed)
        .is_err_and(|owner| owner != pid)
    {
        // A forked child can price serially, including while the parent was
        // initializing its pool.
        return None;
    }
    let pool = HESTON_POOL
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(available)
                .thread_name(|index| format!("kiyosi-heston-{index}"))
                .build()
                .ok()
        })
        .as_ref()?;
    Some((pool, available.min(pool.current_num_threads())))
}

impl AssetWorkers {
    fn apply<T: Send>(
        &self,
        work: impl ParallelIterator<Item = T>,
        op: impl Fn(T) -> Result<()> + Sync,
    ) -> Result<()> {
        work.try_for_each(|item| crate::cancellation::with_tokens(self.tokens.clone(), || op(item)))
    }
}

// Asset lines in a tile are independent, so the recurrence advances their
// four variance lanes together while the rest of the grid stays row-major.
const ASSET_LANES: usize = 4;
type AssetFactor = [[f64; ASSET_LANES]; 3];

struct Operator {
    nx: usize,
    rows: usize,
    variance_base: Vec<[f64; 3]>,
    volatilities: Vec<f64>,
    asset_stencils: Option<Vec<super::grid::PreparedLogStencil>>,
    mixed: Vec<f64>,
    dx: Vec<[f64; 3]>,
    dv: Vec<[f64; 3]>,
    matrices: DirectionalMatrices,
    cached: Vec<DirectionalMatrices>,
    cache_limit: usize,
}

struct DirectionalMatrices {
    asset: Vec<[f64; 3]>,
    variance: Vec<[f64; 3]>,
    asset_factors: Vec<AssetFactor>,
    variance_factors: Vec<[f64; 3]>,
    coefficients_key: Option<[u64; 3]>,
    weight: f64,
}

impl DirectionalMatrices {
    fn new(nx: usize, rows: usize, dynamic_rows: usize) -> Self {
        Self {
            asset: vec![[0.0; 3]; nx * rows],
            variance: vec![[0.0; 3]; rows],
            asset_factors: vec![[[0.0; ASSET_LANES]; 3]; nx * dynamic_rows.div_ceil(ASSET_LANES)],
            variance_factors: vec![[0.0; 3]; rows],
            coefficients_key: None,
            weight: f64::NAN,
        }
    }

    fn matches(&self, key: [u64; 3], weight: f64) -> bool {
        self.coefficients_key == Some(key) && self.weight.to_bits() == weight.to_bits()
    }
}

impl Operator {
    fn new(
        asset: &SpatialGrid,
        variance: Option<&[f64]>,
        model: &HestonParameters,
    ) -> Result<Self> {
        let nx = asset.nodes.len();
        let rows = variance.map_or(1, <[f64]>::len);
        let dynamic_rows = variance.map_or(1, |nodes| nodes.len() - 1);
        let derivatives = |nodes: &[f64]| {
            let mut coefficients = vec![[0.0; 3]; nodes.len()];
            for (index, row) in coefficients
                .iter_mut()
                .enumerate()
                .take(nodes.len() - 1)
                .skip(1)
            {
                *row = derivative(nodes, index);
            }
            coefficients
        };
        let mut variance_base = vec![[0.0; 3]; dynamic_rows];
        let mut volatilities = Vec::new();
        let mut mixed = vec![0.0; rows];
        if let Some(nodes) = variance {
            volatilities.reserve_exact(dynamic_rows);
            for (j, &value) in nodes.iter().take(dynamic_rows).enumerate() {
                if j % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                variance_base[j] = directional(
                    nodes,
                    j,
                    0.5 * model.vol_of_vol().powi(2) * value,
                    model.mean_reversion_speed() * (model.long_run_variance() - value),
                    0.0,
                );
                mixed[j] = model.correlation() * model.vol_of_vol() * value;
                if variance_base[j].iter().any(|x| !x.is_finite()) || !mixed[j].is_finite() {
                    return Err(unstable());
                }
                volatilities.push(value.sqrt());
            }
        }
        Ok(Self {
            nx,
            rows,
            variance_base,
            volatilities,
            asset_stencils: asset.prepared_stencils()?,
            mixed,
            dx: derivatives(&asset.nodes),
            dv: variance.map_or_else(Vec::new, derivatives),
            matrices: DirectionalMatrices::new(nx, rows, dynamic_rows),
            cached: Vec::new(),
            cache_limit: 0,
        })
    }

    fn dynamic_rows(&self) -> usize {
        if self.rows == 1 { 1 } else { self.rows - 1 }
    }

    fn prepare(
        &mut self,
        asset_grid: &SpatialGrid,
        [deterministic, rate, dividend, weight]: [f64; 4],
        workers: Option<&AssetWorkers>,
    ) -> Result<()> {
        let key = [deterministic, rate, dividend].map(f64::to_bits);
        if self.matrices.matches(key, weight) {
            return Ok(());
        }
        let nx = self.nx;
        let rows = self.dynamic_rows();
        if let Some(index) = self
            .cached
            .iter()
            .position(|entry| entry.matches(key, weight))
        {
            let entry = self.cached.remove(index);
            self.cached
                .push(std::mem::replace(&mut self.matrices, entry));
            return Ok(());
        }
        if self.cache_limit > 0 && self.matrices.coefficients_key.is_some() {
            let entry = if self.cached.len() == self.cache_limit {
                self.cached.remove(0)
            } else {
                DirectionalMatrices::new(nx, self.rows, rows)
            };
            self.cached
                .push(std::mem::replace(&mut self.matrices, entry));
        }
        let rebuild = self.matrices.coefficients_key != Some(key);
        if rebuild {
            for (index, (row, &base)) in self.matrices.variance[..rows]
                .iter_mut()
                .zip(&self.variance_base)
                .enumerate()
            {
                if index % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                *row = base;
                row[1] -= rate / 2.0;
                if !row[1].is_finite() {
                    return Err(unstable());
                }
            }
        }
        let fallback_volatility = deterministic.sqrt();
        let volatilities = &self.volatilities;
        let asset_stencils = &self.asset_stencils;
        let carry = rate - dividend;
        let tile_width = nx * ASSET_LANES;
        let coefficients_for = |first_row, assets: &mut [[f64; 3]]| {
            for (lane, coefficients) in assets.chunks_exact_mut(nx).enumerate() {
                crate::cancellation::check()?;
                let volatility = volatilities
                    .get(first_row + lane)
                    .copied()
                    .unwrap_or(fallback_volatility);
                let interior = &mut coefficients[1..nx - 1];
                let variance = volatility * volatility;
                if let Some(stencils) = asset_stencils
                    .as_ref()
                    .filter(|_| super::grid::PreparedLogStencil::bounded([variance, rate, carry]))
                {
                    for (row, stencil) in interior.iter_mut().zip(stencils) {
                        *row = stencil.coefficients(rate, carry, variance);
                        row[1] += rate / 2.0;
                    }
                    continue;
                }
                for (i, row) in interior.iter_mut().enumerate() {
                    *row = asset_grid.coefficients(i + 1, rate, dividend, volatility);
                    row[1] += rate / 2.0;
                    if row.iter().any(|x| !x.is_finite()) {
                        return Err(unstable());
                    }
                }
            }
            Ok(())
        };
        let factor_assets = |assets: &[[f64; 3]], factors: &mut [AssetFactor]| {
            for (assets, factors) in assets.chunks(tile_width).zip(factors.chunks_exact_mut(nx)) {
                crate::cancellation::check()?;
                let coefficients = assets.chunks_exact(nx);
                let mut previous = [[0.0; ASSET_LANES]; 3];
                for (i, slot) in factors.iter_mut().enumerate() {
                    let mut current = [[0.0; ASSET_LANES], [1.0; ASSET_LANES], [0.0; ASSET_LANES]];
                    for (lane, row) in coefficients.clone().enumerate() {
                        let [mut multiplier, mut diagonal, upper] = implicit_row(row[i], weight);
                        if i > 0 {
                            multiplier /= previous[1][lane];
                            diagonal -= multiplier * previous[2][lane];
                        }
                        if !diagonal.is_finite() || diagonal == 0.0 {
                            return Err(unstable());
                        }
                        current[0][lane] = multiplier;
                        current[1][lane] = diagonal;
                        current[2][lane] = upper;
                    }
                    *slot = current;
                    previous = current;
                }
            }
            Ok(())
        };
        let assets = &mut self.matrices.asset[..rows * nx];
        // Weight-only updates reuse the explicit operator and have less work per row.
        if let Some(workers) = workers.filter(|_| rebuild || rows * nx >= 128_000) {
            let tiles = workers.rows_per_task.div_ceil(ASSET_LANES);
            workers.apply(
                assets
                    .par_chunks_mut(tiles * tile_width)
                    .zip(self.matrices.asset_factors.par_chunks_mut(tiles * nx))
                    .enumerate(),
                |(chunk, (assets, factors))| {
                    for (tile, (assets, factors)) in assets
                        .chunks_mut(tile_width)
                        .zip(factors.chunks_exact_mut(nx))
                        .enumerate()
                    {
                        if rebuild {
                            coefficients_for((chunk * tiles + tile) * ASSET_LANES, assets)?;
                        }
                        // Factor the independent rows while the tile is in cache.
                        factor_assets(assets, factors)?;
                    }
                    Ok(())
                },
            )?;
        } else {
            if rebuild {
                coefficients_for(0, assets)?;
            }
            factor_assets(assets, &mut self.matrices.asset_factors)?;
        }
        for (factor, &row) in self
            .matrices
            .variance_factors
            .iter_mut()
            .zip(&self.matrices.variance)
            .take(rows)
        {
            *factor = implicit_row(row, weight);
        }
        if self.rows > 1 {
            self.matrices.variance_factors[rows] = [-1.0, 1.0, 0.0];
        }
        factor(&mut self.matrices.variance_factors)?;
        self.matrices.coefficients_key = Some(key);
        self.matrices.weight = weight;
        Ok(())
    }

    fn boundaries(&self, values: &mut [f64], edges: [f64; 2]) {
        for row in values.chunks_exact_mut(self.nx) {
            row[0] = edges[0];
            row[self.nx - 1] = edges[1];
        }
        self.variance_boundary(values);
    }

    fn variance_boundary(&self, values: &mut [f64]) {
        if self.rows > 1 {
            let top = (self.rows - 1) * self.nx;
            let (below, last) = values.split_at_mut(top);
            last.copy_from_slice(&below[top - self.nx..top]);
        }
    }

    #[inline]
    fn for_each_component(&self, values: &[f64], j: usize, mut write: impl FnMut(usize, [f64; 3])) {
        let nx = self.nx;
        let start = j * nx;
        let current = &values[start..start + nx];
        let previous = &values[start.saturating_sub(nx)..][..nx];
        let next = &values[if self.rows > 1 { start + nx } else { start }..][..nx];
        let [lower, diagonal, upper] = self.matrices.variance[j];
        let [left_weight, _, right_weight] = self.dv.get(j).copied().unwrap_or([0.0; 3]);
        let has_mixed = j > 0 && self.rows > 1;
        // Reuse the two neighboring variance differences as the asset stencil moves.
        let difference = |i: usize| {
            if has_mixed {
                left_weight * (previous[i] - current[i]) + right_weight * (next[i] - current[i])
            } else {
                0.0
            }
        };
        let mut left = difference(0);
        let mut center = difference(1);
        for i in 1..nx - 1 {
            let [a, b, c] = self.matrices.asset[start + i];
            let asset = a * current[i - 1] + b * current[i] + c * current[i + 1];
            let mut variance = diagonal * current[i];
            if j > 0 {
                variance += lower * previous[i];
            }
            if self.rows > 1 {
                variance += upper * next[i];
            }
            let right = difference(i + 1);
            let mixed = if has_mixed {
                self.mixed[j] * (self.dx[i][0] * (left - center) + self.dx[i][2] * (right - center))
            } else {
                0.0
            };
            write(i, [mixed, asset, variance]);
            left = center;
            center = right;
        }
    }

    fn predict(
        &self,
        values: &[f64],
        multiplier: &[f64],
        [predictor, stage]: [&mut [f64]; 2],
        dt: f64,
        workers: Option<&AssetWorkers>,
    ) -> Result<()> {
        let nx = self.nx;
        let calculate = |start: usize, predictor: &mut [f64], stage: &mut [f64]| {
            for (row, (predictor, stage)) in predictor
                .chunks_exact_mut(nx)
                .zip(stage.chunks_exact_mut(nx))
                .enumerate()
            {
                crate::cancellation::check()?;
                let j = start + row;
                self.for_each_component(values, j, |i, [mixed, along_asset, along_variance]| {
                    let k = j * nx + i;
                    let explicit = mixed + along_asset + along_variance;
                    let correction = multiplier.get(k).copied().unwrap_or(0.0);
                    let value = dt * (explicit + correction);
                    predictor[i] = value;
                    stage[i] = value;
                });
            }
            Ok(())
        };
        let active = self.dynamic_rows() * nx;
        if let Some(workers) = workers {
            let chunk = workers.rows_per_task * nx;
            workers.apply(
                predictor[..active]
                    .par_chunks_mut(chunk)
                    .zip(stage[..active].par_chunks_mut(chunk))
                    .enumerate(),
                |(index, (predictor, stage))| {
                    calculate(index * workers.rows_per_task, predictor, stage)
                },
            )
        } else {
            calculate(0, &mut predictor[..active], &mut stage[..active])
        }
    }

    fn correct(
        &self,
        stage: &[f64],
        predictor: &mut [f64],
        dt: f64,
        workers: Option<&AssetWorkers>,
    ) -> Result<()> {
        let nx = self.nx;
        let calculate = |start: usize, predictor: &mut [f64]| {
            for (row, predictor) in predictor.chunks_exact_mut(nx).enumerate() {
                crate::cancellation::check()?;
                let j = start + row;
                self.for_each_component(stage, j, |i, [cross, along_asset, along_variance]| {
                    predictor[i] += dt * (0.5 * cross + (along_asset + along_variance) / 6.0);
                });
            }
            Ok(())
        };
        let active = self.dynamic_rows() * nx;
        if let Some(workers) = workers {
            let chunk = workers.rows_per_task * nx;
            workers.apply(
                predictor[..active].par_chunks_mut(chunk).enumerate(),
                |(index, predictor)| calculate(index * workers.rows_per_task, predictor),
            )
        } else {
            calculate(0, &mut predictor[..active])
        }
    }

    fn take_boundary_weights(&self, values: &mut [f64]) -> [f64; 2] {
        let nx = self.nx;
        if self.rows > 1 {
            let top = (self.rows - 1) * nx;
            let (below, last) = values.split_at_mut(top);
            for (value, other) in below[top - nx..].iter_mut().zip(last) {
                *value += std::mem::take(other);
            }
        }
        let mut edges = [0.0; 2];
        for row in values.chunks_exact_mut(nx) {
            edges[0] += std::mem::take(&mut row[0]);
            edges[1] += std::mem::take(&mut row[nx - 1]);
        }
        edges
    }

    fn transpose_solve(
        &self,
        values: &mut [f64],
        contributions: &mut [[f64; 2]],
        workers: Option<&AssetWorkers>,
    ) -> Result<[f64; 2]> {
        let mut edges = self.take_boundary_weights(values);
        self.variance_strips(values, workers, |strip| {
            transpose_variance_strip(strip, &self.matrices.variance_factors)
        })?;
        let nx = self.nx;
        let active = self.dynamic_rows() * nx;
        values[active..].fill(0.0);
        let width = ASSET_LANES * nx;
        let solve = |values: &mut [f64], factors: &[AssetFactor], edges: &mut [[f64; 2]]| {
            for ((tile, factors), edges) in values
                .chunks_mut(width)
                .zip(factors.chunks_exact(nx))
                .zip(edges)
            {
                crate::cancellation::check()?;
                *edges = transpose_asset_tile(tile, factors);
            }
            Ok(())
        };
        if let Some(workers) = workers {
            let tiles = workers.rows_per_task.div_ceil(ASSET_LANES);
            workers.apply(
                values[..active]
                    .par_chunks_mut(tiles * width)
                    .zip(self.matrices.asset_factors.par_chunks(tiles * nx))
                    .zip(contributions.par_chunks_mut(tiles)),
                |((values, factors), edges)| solve(values, factors, edges),
            )?;
        } else {
            solve(
                &mut values[..active],
                &self.matrices.asset_factors,
                contributions,
            )?;
        }
        for contribution in contributions {
            edges[0] += contribution[0];
            edges[1] += contribution[1];
        }
        Ok(edges)
    }

    fn transpose_explicit(
        &self,
        input: &[f64],
        output: &mut [f64],
        mixed: &mut [f64],
        [directional_weight, mixed_weight]: [f64; 2],
        workers: Option<&AssetWorkers>,
    ) -> Result<()> {
        let nx = self.nx;
        let active = self.dynamic_rows();
        // The mixed stencil is separable. Gather along the asset axis once
        // per source row, then gather along variance into owned output rows.
        let transpose_dx = |first: usize, rows: &mut [f64]| {
            for (j, dst) in (first..).zip(rows.chunks_exact_mut(nx)) {
                crate::cancellation::check()?;
                if j == 0 || j >= active {
                    dst.fill(0.0);
                    continue;
                }
                let src = &input[j * nx..][..nx];
                let scale = self.mixed[j] * mixed_weight;
                dst[0] = (src[1] * scale) * self.dx[1][0];
                dst[nx - 1] = (src[nx - 2] * scale) * self.dx[nx - 2][2];
                for ((dst, src), dx) in dst[1..nx - 1]
                    .iter_mut()
                    .zip(src.windows(3))
                    .zip(self.dx.windows(3))
                {
                    *dst = (src[0] * scale) * dx[0][2]
                        + (src[1] * scale) * dx[1][1]
                        + (src[2] * scale) * dx[2][0];
                }
            }
            Ok(())
        };
        if let Some(workers) = workers {
            let chunk = workers.rows_per_task * nx;
            workers.apply(mixed.par_chunks_mut(chunk).enumerate(), |(i, rows)| {
                transpose_dx(i * workers.rows_per_task, rows)
            })?;
        } else {
            transpose_dx(0, mixed)?;
        }
        let gather = |first: usize, rows: &mut [f64]| {
            for (j, dst) in (first..).zip(rows.chunks_exact_mut(nx)) {
                crate::cancellation::check()?;
                let low = j.saturating_sub(1);
                let high = (j + 1).min(self.rows - 1);
                let offset = j * nx;
                let here = &input[offset..][..nx];
                let below = &input[low * nx..][..nx];
                let above = &input[high * nx..][..nx];
                let asset = &self.matrices.asset[offset..][..nx];
                let middle = self.matrices.variance[j][1];
                let lower = if j > 0 {
                    self.matrices.variance[low][2]
                } else {
                    0.0
                };
                let upper = if j + 1 < active {
                    self.matrices.variance[high][0]
                } else {
                    0.0
                };
                dst[0] += directional_weight * here[1] * asset[1][0];
                dst[nx - 1] += directional_weight * here[nx - 2] * asset[nx - 2][2];
                for ((((dst, here), below), above), asset) in dst[1..nx - 1]
                    .iter_mut()
                    .zip(here.windows(3))
                    .zip(&below[1..nx - 1])
                    .zip(&above[1..nx - 1])
                    .zip(asset.windows(3))
                {
                    let center = here[1] * directional_weight;
                    *dst += (here[0] * directional_weight) * asset[0][2]
                        + center * asset[1][1]
                        + (here[2] * directional_weight) * asset[2][0]
                        + (below * directional_weight) * lower
                        + center * middle
                        + (above * directional_weight) * upper;
                }
                if self.rows > 1 {
                    let previous = &mixed[low * nx..][..nx];
                    let current = &mixed[offset..][..nx];
                    let next = &mixed[high * nx..][..nx];
                    let a = if j > 0 { self.dv[low][2] } else { 0.0 };
                    let b = self.dv[j][1];
                    let c = if j + 1 < active {
                        self.dv[high][0]
                    } else {
                        0.0
                    };
                    for (((dst, previous), current), next) in
                        dst.iter_mut().zip(previous).zip(current).zip(next)
                    {
                        *dst += a * previous + b * current + c * next;
                    }
                }
            }
            Ok(())
        };
        if let Some(workers) = workers {
            let chunk = workers.rows_per_task * nx;
            workers.apply(output.par_chunks_mut(chunk).enumerate(), |(i, rows)| {
                gather(i * workers.rows_per_task, rows)
            })
        } else {
            gather(0, output)
        }
    }

    fn solve(
        &self,
        values: &mut [f64],
        edges: [f64; 2],
        workers: Option<&AssetWorkers>,
    ) -> Result<()> {
        let nx = self.nx;
        let rows = self.dynamic_rows();
        let active = rows * nx;
        let tile_width = ASSET_LANES * nx;
        let factor_count = rows.div_ceil(ASSET_LANES) * nx;
        let factor_rows = &self.matrices.asset_factors[..factor_count];
        let solve_rows = |values: &mut [f64], factors: &[AssetFactor]| {
            for (tile, factors) in values.chunks_mut(tile_width).zip(factors.chunks_exact(nx)) {
                crate::cancellation::check()?;
                solve_asset_tile(tile, factors, edges)?;
            }
            Ok(())
        };
        if let Some(workers) = workers {
            let tiles = workers.rows_per_task.div_ceil(ASSET_LANES);
            workers.apply(
                values[..active]
                    .par_chunks_mut(tiles * tile_width)
                    .zip(factor_rows.par_chunks(tiles * nx)),
                |(values, factors)| solve_rows(values, factors),
            )?;
        } else {
            solve_rows(&mut values[..active], factor_rows)?;
        }

        self.variance_strips(values, workers, |strip| {
            solve_variance_strip(strip, &self.matrices.variance_factors, rows)
        })?;
        self.boundaries(values, edges);
        Ok(())
    }

    fn variance_strips(
        &self,
        values: &mut [f64],
        workers: Option<&AssetWorkers>,
        solve: impl Fn(&mut [&mut [f64]]) -> Result<()> + Sync,
    ) -> Result<()> {
        let nx = self.nx;
        // Each strip owns a cache-sized group of complete variance lines.
        let interior = nx - 2;
        let columns = workers.map_or(interior, |workers| {
            interior.div_ceil(workers.count).next_multiple_of(8)
        });
        let mut strips: Vec<_> = (0..interior.div_ceil(columns))
            .map(|_| Vec::with_capacity(self.rows))
            .collect();
        for values in values.chunks_exact_mut(nx) {
            for (strip, values) in strips.iter_mut().zip(values[1..nx - 1].chunks_mut(columns)) {
                strip.push(values);
            }
        }
        if let Some(workers) = workers {
            workers.apply(strips.par_iter_mut(), |strip| solve(strip))
        } else {
            strips.iter_mut().try_for_each(|strip| solve(strip))
        }
    }
}

fn transpose_asset_tile(tile: &mut [f64], factors: &[AssetFactor]) -> [f64; 2] {
    let nx = factors.len();
    let mut rows = tile.chunks_exact_mut(nx);
    let mut rows: [&mut [f64]; ASSET_LANES] =
        std::array::from_fn(|_| rows.next().unwrap_or(&mut []));
    let lanes = rows.iter().take_while(|row| !row.is_empty()).count();
    let mut previous = [0.0; ASSET_LANES];
    for i in 1..nx - 1 {
        let mut rhs = [0.0; ASSET_LANES];
        for (value, row) in rhs.iter_mut().zip(&rows[..lanes]) {
            *value = row[i];
        }
        previous = std::array::from_fn(|lane| {
            (rhs[lane] - factors[i - 1][2][lane] * previous[lane]) / factors[i][1][lane]
        });
        for (row, value) in rows[..lanes].iter_mut().zip(previous) {
            row[i] = value;
        }
    }
    let mut next = previous;
    for i in (1..nx - 2).rev() {
        let mut rhs = [0.0; ASSET_LANES];
        for (value, row) in rhs.iter_mut().zip(&rows[..lanes]) {
            *value = row[i];
        }
        next = std::array::from_fn(|lane| rhs[lane] - factors[i + 1][0][lane] * next[lane]);
        for (row, value) in rows[..lanes].iter_mut().zip(next) {
            row[i] = value;
        }
    }
    let mut edges = [0.0; 2];
    for (lane, row) in rows[..lanes].iter().enumerate() {
        edges[0] -= factors[1][0][lane] * row[1];
        edges[1] -= factors[nx - 2][2][lane] * row[nx - 2];
    }
    edges
}

fn transpose_variance_strip(strip: &mut [&mut [f64]], factors: &[[f64; 3]]) -> Result<()> {
    for value in &mut *strip[0] {
        *value /= factors[0][1];
    }
    for j in 1..strip.len() {
        crate::cancellation::check()?;
        let (before, after) = strip.split_at_mut(j);
        let upper = factors[j - 1][2];
        let diagonal = factors[j][1];
        for (value, previous) in after[0].iter_mut().zip(before[j - 1].iter()) {
            *value = (*value - upper * previous) / diagonal;
        }
    }
    for j in (1..strip.len()).rev() {
        let (before, after) = strip.split_at_mut(j);
        let multiplier = factors[j][0];
        for (value, next) in before[j - 1].iter_mut().zip(after[0].iter()) {
            *value -= multiplier * next;
        }
    }
    Ok(())
}

fn solve_asset_tile(tile: &mut [f64], factors: &[AssetFactor], edges: [f64; 2]) -> Result<()> {
    let nx = factors.len();
    let mut rows = tile.chunks_exact_mut(nx);
    let mut rows: [&mut [f64]; ASSET_LANES] =
        std::array::from_fn(|_| rows.next().unwrap_or(&mut []));
    let lanes = rows.iter().take_while(|row| !row.is_empty()).count();
    let mut previous = [edges[0]; ASSET_LANES];
    for (i, [multipliers, _, _]) in factors.iter().enumerate().take(nx - 1).skip(1) {
        let mut rhs = [0.0; ASSET_LANES];
        for (value, row) in rhs.iter_mut().zip(&rows[..lanes]) {
            *value = row[i];
        }
        let next: [f64; ASSET_LANES] =
            std::array::from_fn(|lane| rhs[lane] - multipliers[lane] * previous[lane]);
        for (row, value) in rows[..lanes].iter_mut().zip(next) {
            row[i] = value;
        }
        previous = next;
    }
    let mut next = [edges[1]; ASSET_LANES];
    for (i, [_, diagonals, uppers]) in factors.iter().enumerate().take(nx - 1).skip(1).rev() {
        let mut rhs = [0.0; ASSET_LANES];
        for (value, row) in rhs.iter_mut().zip(&rows[..lanes]) {
            *value = row[i];
        }
        let values: [f64; ASSET_LANES] =
            std::array::from_fn(|lane| (rhs[lane] - uppers[lane] * next[lane]) / diagonals[lane]);
        for (row, value) in rows[..lanes].iter_mut().zip(values) {
            row[i] = value;
        }
        next = values;
    }
    for row in &mut rows[..lanes] {
        row[0] = edges[0];
        row[nx - 1] = edges[1];
        if row.iter().any(|value| !value.is_finite()) {
            return Err(unstable());
        }
    }
    Ok(())
}

fn solve_variance_strip(
    strip: &mut [&mut [f64]],
    factors: &[[f64; 3]],
    dynamic_rows: usize,
) -> Result<()> {
    for (j, &[multiplier, _, _]) in factors.iter().enumerate().skip(1) {
        crate::cancellation::check()?;
        let (previous, current) = strip.split_at_mut(j);
        let prior = &previous[j - 1];
        let row = &mut current[0];
        if j < dynamic_rows {
            for (value, prior) in row.iter_mut().zip(prior.iter()) {
                *value -= multiplier * prior;
            }
        } else {
            for (value, prior) in row.iter_mut().zip(prior.iter()) {
                *value = -multiplier * prior;
            }
        }
    }
    for (j, &[_, diagonal, upper]) in factors.iter().enumerate().rev() {
        let (current, next) = strip.split_at_mut(j + 1);
        let row = &mut current[j];
        if let Some(next) = next.first() {
            for (value, next) in row.iter_mut().zip(next.iter()) {
                *value = (*value - upper * next) / diagonal;
            }
        } else {
            for value in row.iter_mut() {
                *value /= diagonal;
            }
        }
        if row.iter().any(|value| !value.is_finite()) {
            return Err(unstable());
        }
    }
    Ok(())
}

fn implicit_row([lower, diagonal, upper]: [f64; 3], weight: f64) -> [f64; 3] {
    [-weight * lower, 1.0 - weight * diagonal, -weight * upper]
}

fn factor(rows: &mut [[f64; 3]]) -> Result<()> {
    let mut previous = None;
    for [multiplier, diagonal, upper] in rows {
        if let Some((pivot, previous_upper)) = previous {
            *multiplier /= pivot;
            *diagonal -= *multiplier * previous_upper;
        }
        if !diagonal.is_finite() || *diagonal == 0.0 {
            return Err(unstable());
        }
        previous = Some((*diagonal, *upper));
    }
    Ok(())
}

#[cfg(test)]
mod adjoint_tests {
    use super::*;

    #[test]
    fn heston_operator_tiles_match_serial_across_rate_and_step_changes() -> Result<()> {
        let mut asset = SpatialGrid::new(33, [0.25, 4.0], 1.0)?;
        concentrate(&mut asset, vec![1.0], 0.06)?;
        let variance = [0.0, 0.01, 0.02, 0.035, 0.06, 0.1, 0.15, 0.22, 0.3, 0.4];
        let model = HestonParameters::new(crate::HestonParameterInputs {
            risk_free_rate: 0.04.into(),
            dividend_yield: 0.01.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.2,
            vol_of_vol: 0.3,
            correlation: -0.7,
        })?;
        let mut serial = Operator::new(&asset, Some(&variance), &model)?;
        let mut parallel = Operator::new(&asset, Some(&variance), &model)?;
        parallel.cache_limit = 2;
        let workers = AssetWorkers {
            count: 2,
            tokens: Vec::new(),
            rows_per_task: 5,
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .map_err(std::io::Error::other)?;
        let recurring = [0.0, 0.04, 0.01, 0.02];
        for key in [
            recurring,
            [0.0, 0.04, 0.01, 0.01],
            [0.0, -0.02, 0.03, 0.01],
            [0.0, -0.02, 0.03, 0.01],
            recurring,
        ] {
            let cached = parallel
                .cached
                .iter()
                .find(|entry| entry.matches([key[0], key[1], key[2]].map(f64::to_bits), key[3]))
                .map(|entry| entry.asset_factors.as_ptr());
            serial.prepare(&asset, key, None)?;
            pool.install(|| parallel.prepare(&asset, key, Some(&workers)))?;
            assert_eq!(serial.matrices.asset, parallel.matrices.asset);
            assert_eq!(serial.matrices.variance, parallel.matrices.variance);
            assert_eq!(
                serial.matrices.asset_factors,
                parallel.matrices.asset_factors
            );
            assert_eq!(
                serial.matrices.variance_factors,
                parallel.matrices.variance_factors
            );
            if key.map(f64::to_bits) == recurring.map(f64::to_bits) && parallel.cached.len() == 2 {
                assert_eq!(cached, Some(parallel.matrices.asset_factors.as_ptr()));
            }
        }
        Ok(())
    }

    #[test]
    fn heston_transpose_matches_primal_directional_solve_and_boundaries() -> Result<()> {
        let asset = SpatialGrid::new(33, [0.25, 4.0], 1.0)?;
        let variance = [0.0, 0.01, 0.02, 0.035, 0.06, 0.1, 0.15, 0.22, 0.3, 0.4];
        let model = HestonParameters::new(crate::HestonParameterInputs {
            risk_free_rate: 0.04.into(),
            dividend_yield: 0.01.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.2,
            vol_of_vol: 0.3,
            correlation: -0.7,
        })?;
        let mut op = Operator::new(&asset, Some(&variance), &model)?;
        op.prepare(&asset, [0.0, 0.04, 0.01, 0.02], None)?;
        let nx = asset.nodes.len();
        let length = nx * variance.len();
        let input: Vec<_> = [-2.0, -1.0, 0.0, 1.0, 2.0]
            .into_iter()
            .cycle()
            .take(length)
            .collect();
        let weights: Vec<_> = [-0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75]
            .into_iter()
            .cycle()
            .take(length)
            .collect();
        let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
        let edges = [0.8, -0.25];
        let mut values = input.clone();
        op.solve(&mut values, edges, None)?;
        let solve_expected = dot(&values, &weights);

        let mut values = vec![0.0; input.len()];
        for (j, row) in values
            .chunks_exact_mut(nx)
            .take(op.dynamic_rows())
            .enumerate()
        {
            op.for_each_component(&input, j, |i, [mixed, along_asset, along_variance]| {
                row[i] = 0.5 * mixed + (along_asset + along_variance) / 6.0;
            });
        }
        let explicit_expected = dot(&values, &weights);
        let transpose = |workers| {
            let mut weights_at_input = weights.clone();
            let mut contributions = vec![[0.0; 2]; op.dynamic_rows().div_ceil(ASSET_LANES)];
            let edge_weights =
                op.transpose_solve(&mut weights_at_input, &mut contributions, workers)?;
            let solve_actual = dot(&input, &weights_at_input) + dot(&edges, &edge_weights);
            assert!((solve_actual - solve_expected).abs() < 1e-11);
            let mut explicit = vec![0.0; length];
            op.transpose_explicit(
                &weights,
                &mut explicit,
                &mut vec![0.0; length],
                [1.0 / 6.0, 0.5],
                workers,
            )?;
            assert!((explicit_expected - dot(&input, &explicit)).abs() < 1e-11);
            Ok::<_, crate::Error>((weights_at_input, edge_weights, explicit))
        };
        let serial = transpose(None)?;
        let workers = AssetWorkers {
            count: 2,
            tokens: Vec::new(),
            rows_per_task: 5,
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .map_err(std::io::Error::other)?;
        assert_eq!(serial, pool.install(|| transpose(Some(&workers)))?);
        Ok(())
    }
}
