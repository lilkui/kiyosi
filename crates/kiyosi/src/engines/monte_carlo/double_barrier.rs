use super::barrier::Contract;
use super::{random_seed, unstable};
use crate::{
    AnalyticDoubleBarrierEngine, DoubleBarrierOption, GreeksRequest, MonteCarloBarrierEngine,
    MonteCarloSettings, NumericalShiftSettings, PricingContext, PricingEngine, PricingResult,
    Result,
};
use std::f64::consts::PI;

/// Prices double barriers with conditional-survival CPU Monte Carlo.
///
/// Continuous monitoring uses one terminal draw and an image/spectral
/// Brownian-bridge survival series. Scheduled monitoring uses original fixing
/// dates and an unobserved expiry leg when needed. Ignores `step_count`.
/// Sampling, exact path counts and thread reproducibility follow
/// [`MonteCarloBarrierEngine`]. Shared and distinct instances permit concurrent
/// calls with request-owned workers and samples. See [`Self::price_with_greeks`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloDoubleBarrierEngine {
    engine: MonteCarloBarrierEngine,
}

impl MonteCarloDoubleBarrierEngine {
    /// Creates an engine with validated shared CPU sampling settings.
    ///
    /// See [`crate::MonteCarloVanillaEngine::new`] for configuration examples.
    ///
    /// # Errors
    ///
    /// Reuses [`MonteCarloBarrierEngine::new`] validation and error categories.
    pub fn new(settings: MonteCarloSettings) -> Result<Self> {
        Ok(Self {
            engine: MonteCarloBarrierEngine::new(settings)?,
        })
    }

    /// Returns owned sampling settings. See [`Self::new`].
    pub fn settings(&self) -> MonteCarloSettings {
        self.engine.settings()
    }

    /// Prices after validating flat rates, inclusive life, calendar and prior history.
    ///
    /// Current touches and resolved cashflows precede sampling. Resolved vanilla
    /// payoffs and continuous at-hit rebates use analytic formulas.
    /// Calls finish synchronously without cancellation polling.
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for node curves,
    /// [`crate::Error::InvalidInput`] outside life,
    /// [`crate::Error::InvalidInput`] for non-trading fixings,
    /// [`crate::Error::InvalidInput`] for missing history or Sobol
    /// dimensions above 21,201, [`crate::Error::InvalidInput`] for
    /// impossible history, or [`crate::Error::ComputationFailure`] for
    /// unrepresentable paths, nonconvergent series or results. Operational failures
    /// retain native I/O details through [`crate::Error::Io`].
    /// Inputs remain unchanged on every outcome.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        Ok(self
            .engine
            .native(
                Contract::Double(option),
                context,
                GreeksRequest::default(),
                Default::default(),
            )?
            .price())
    }

    /// Returns requested delta/gamma using the price's normal samples.
    ///
    /// Other Greeks remain absent. Expiry and spot stencils crossing either
    /// current monitored boundary suppress delta/gamma. Shifts affect that
    /// availability and numerical derivatives of continuous at-hit rebates.
    /// Near-expiry likelihood estimates can have substantial sampling error.
    /// Scalar numerical analysis is available through [`crate::numerical_greeks`].
    ///
    /// # Errors
    ///
    /// Checks flat rates before requests and shifts, then propagates [`Self::price`]
    /// errors. Empty requests or invalid shifts return
    /// [`crate::Error::InvalidInput`]; non-finite requested Greek samples
    /// return [`crate::Error::ComputationFailure`]. Inputs remain unchanged.
    ///
    /// # Panics
    ///
    /// Resumes worker panics after joining the call's workers, as in [`Self::price`].
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, start, Date::new(2026, 1, 1)?, 80.0, 120.0, DoubleBarrierType::KnockOut))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
    ///     path_count: 32768, seed: Some(42), sampling: MonteCarloSampling::Sobol,
    ///     ..Default::default()
    /// })?;
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta, Greek::Gamma]), Default::default())?;
    /// assert!((result.price() - 1.0838814592583166).abs() < 0.003);
    /// assert!(result.has(Greek::Delta));
    /// assert_eq!(engine.settings().path_count, 32768);
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.validate_applicability(context)?;
        request.validate()?;
        settings.validate()?;
        self.engine
            .native(Contract::Double(option), context, request, settings)
    }
}

impl PricingEngine<DoubleBarrierOption> for MonteCarloDoubleBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        self.engine.validate_applicability(context)
    }
    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<DoubleBarrierOption> + Send + Sync + '_>>> {
        let settings = self.settings();
        if settings.seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(Self::new(MonteCarloSettings {
            seed: Some(random_seed()?),
            ..settings
        })?)))
    }
    fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn price_with_greeks(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        Self::price_with_greeks(self, option, context, request, settings)
    }
    fn volatility_affects_price(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> bool {
        AnalyticDoubleBarrierEngine.volatility_affects_price(option, context)
    }
}

// Drift cancels in the killed/free density ratio; derivatives hold the terminal log asset fixed.
pub(super) fn survival(
    initial: f64,
    terminal: f64,
    band: f64,
    deviation: f64,
    derivatives: bool,
) -> Result<[f64; 3]> {
    if terminal <= 0.0 || terminal >= band {
        return Ok([0.0; 3]);
    }
    let (mut x, mut y) = (initial / band, terminal / band);
    let ratio = deviation / band;
    let variance = ratio * ratio;
    if variance == 0.0 {
        return Ok([1.0, 0.0, 0.0]);
    }
    if !variance.is_finite() || variance <= 0.0 {
        return Err(unstable());
    }
    let reflected = x + y > 1.0;
    if reflected {
        x = 1.0 - x;
        y = 1.0 - y;
    }
    let mut sum = [0.0; 3];
    let mut converged = false;
    if variance < 0.25 {
        sum[0] = -(-2.0 * (x / ratio) * (y / ratio)).exp_m1();
        let mut add = |k: f64, direct: bool| {
            let exponent = if direct {
                -2.0 * (k / ratio) * ((y - x + k) / ratio)
            } else {
                -2.0 * ((x + k) / ratio) * ((y + k) / ratio)
            };
            let sign = if direct { 1.0 } else { -1.0 };
            let weight = exponent.exp();
            if k != 0.0 {
                sum[0] += sign * weight;
            }
            if derivatives {
                let numerator = if direct { 2.0 * k } else { -2.0 * (y + k) };
                if numerator != 0.0 {
                    let log_slope = numerator.abs().ln() - 2.0 * ratio.ln();
                    sum[1] += sign * (exponent + log_slope).exp().copysign(numerator);
                    sum[2] += sign * (exponent + 2.0 * log_slope).exp();
                }
            }
            weight
        };
        add(0.0, false);
        for n in 1..=64 {
            let k = f64::from(n);
            let tail = add(k, true) + add(k, false) + add(-k, true) + add(-k, false);
            if n >= 2 && tail < 1e-18 {
                converged = true;
                break;
            }
        }
    } else {
        let distance = y - x;
        let log_scale =
            (2.0 * (2.0 * PI).sqrt() * ratio).ln() + 0.5 * distance * distance / variance;
        for n in 1..=256 {
            let frequency = f64::from(n) * PI;
            let exponent = log_scale - 0.5 * frequency * frequency * variance;
            let weight = exponent.exp();
            let sy = (frequency * y).sin();
            let product = (frequency * x).sin() * sy;
            sum[0] += weight * product;
            if derivatives {
                let first = frequency * (frequency * x).cos() * sy;
                let score = -distance / variance;
                sum[1] += weight * (first + score * product);
                sum[2] += weight
                    * (2.0 * score * first
                        + (score * score + 1.0 / variance - frequency * frequency) * product);
            }
            if n >= 4 && exponent + 4.0 * frequency.ln() < -40.0 {
                converged = true;
                break;
            }
        }
    }
    if !converged || !(-1e-13..=1.0 + 1e-13).contains(&sum[0]) {
        return Err(unstable());
    }
    sum[0] = sum[0].clamp(0.0, 1.0);
    if derivatives {
        sum[1] = (if reflected { -sum[1] } else { sum[1] }) / band;
        sum[2] = (sum[2] / band) / band;
    }
    Ok(sum)
}
