use super::barrier::Contract;
use super::{greeks, unstable};
use crate::{
    DoubleBarrierOption, GreeksRequest, MonteCarloBarrierEngine, MonteCarloEstimate,
    MonteCarloSettings, PricingContext, PricingEngine, PricingResult, Result,
};
use std::f64::consts::PI;

/// Prices double barriers with conditional-survival CPU Monte Carlo.
///
/// Continuous monitoring uses one terminal draw and an image/spectral
/// Brownian-bridge survival series. Scheduled monitoring uses original fixing
/// dates and an unobserved expiry leg when needed.
/// Near-strike payoffs retain movements even when terminal spot rounds to strike.
/// Sampling, exact path counts and thread reproducibility follow
/// [`MonteCarloBarrierEngine`]. Shared and distinct instances permit concurrent
/// calls with request-owned samples and shared workers. See [`Self::price_with_greeks`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonteCarloDoubleBarrierEngine {
    engine: MonteCarloBarrierEngine,
}

impl MonteCarloDoubleBarrierEngine {
    /// Returns a replayable engine, drawing a seed only when none is configured.
    ///
    /// All other controls, including any integration or exercise grid, are preserved.
    /// The original engine remains unchanged. Save this engine before pricing to
    /// replay the calculation within the same build; see
    /// [`crate::MonteCarloVanillaEngine::with_resolved_seed`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Io`] if system entropy is unavailable.
    pub fn with_resolved_seed(&self) -> crate::Result<Self> {
        Ok(Self {
            engine: self.engine.with_resolved_seed()?,
        })
    }

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
    pub fn sampling_settings(&self) -> MonteCarloSettings {
        self.engine.sampling_settings()
    }

    /// Prices after validating flat rates, inclusive life and prior history.
    ///
    /// Current touches and resolved cash flows precede sampling. Resolved vanilla
    /// payoffs and continuous at-hit rebates use analytic formulas.
    /// Cancellation follows [`crate::CancellationToken`].
    /// See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`crate::Error::UnsupportedOperation`] for nonconstant rate curves,
    /// [`crate::Error::InvalidInput`] outside life,
    /// [`crate::Error::InvalidInput`] for Sobol
    /// dimensions above 21,201, [`crate::Error::InvalidInput`] for
    /// impossible history, or [`crate::Error::ComputationFailure`] for
    /// unrepresentable paths, nonconvergent series or results. Operational failures
    /// retain native I/O details through [`crate::Error::Io`].
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work has finished.
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior (normally process abort).
    pub fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        context.bsm_parameters()?;
        Ok(self
            .engine
            .native(
                Contract::Double(option),
                context,
                GreeksRequest::new([]),
                Default::default(),
                false,
            )?
            .0
            .price())
    }

    /// Returns a price with sampling standard error and an asymptotic 95% interval.
    ///
    /// Uncertainty follows [`MonteCarloEstimate`]. Resolved analytic branches have
    /// zero pseudorandom uncertainty; analytic at-hit rebates add no sampling
    /// variance. See [`Self::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors and returns [`crate::Error::ComputationFailure`]
    /// for non-finite uncertainty or confidence bounds.
    ///
    /// # Panics
    ///
    /// Propagates worker panics as in [`Self::price`].
    ///
    /// # Aborts
    ///
    /// Allocation exhaustion follows the allocator's behavior, as in [`Self::price`].
    pub fn price_with_statistics(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        let (result, error) = self.engine.native(
            Contract::Double(option),
            context,
            GreeksRequest::new([]),
            Default::default(),
            true,
        )?;
        MonteCarloEstimate::from_sampling(
            result.price(),
            error,
            self.sampling_settings(),
            result.seed(),
        )
    }

    /// Returns requested delta/gamma using the price's normal samples.
    ///
    /// Other Greeks are absent. Expiry spot Greeks follow terminal payoffs; stencils crossing either
    /// current monitored boundary suppress delta/gamma. The optional [`crate::NumericalShiftSettings::spot_shift`]
    /// controls that availability and numerical derivatives of continuous at-hit rebates.
    /// Configure it with [`PricingEngine::price_with_greeks_with_settings`].
    /// Custom scalar differences use [`crate::numerical_greeks`].
    /// Unavailable rebate stencils leave the contract's delta/gamma absent; see
    /// [`crate::numerical_greeks`] for stencil requirements.
    /// Near-expiry likelihood estimates can have substantial sampling error.
    /// Scalar numerical analysis is available through [`crate::numerical_greeks`].
    ///
    /// # Errors
    ///
    /// Checks flat rates and request support before estimating with default controls,
    /// then propagates [`Self::price`] errors. Request handling follows
    /// [`GreeksRequest`]; non-finite requested Greek samples
    /// return [`crate::Error::ComputationFailure`].
    ///
    /// # Panics
    ///
    /// Propagates worker panics after the call's parallel work, as in [`Self::price`].
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
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, start, Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0, DoubleBarrierType::KnockOut))?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let engine = MonteCarloDoubleBarrierEngine::new(MonteCarloSettings {
    ///     path_count: 32768, seed: Some(42), sampling: MonteCarloSampling::Sobol,
    ///     ..Default::default()
    /// })?;
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([crate::Greek::Delta, crate::Greek::Gamma]))?;
    /// assert!((result.price() - 1.0838814592583166).abs() < 0.003);
    /// assert!(result.has(crate::Greek::Delta));
    /// assert_eq!(engine.sampling_settings().path_count, 32768);
    /// let estimate = engine.price_with_statistics(&option, &context)?;
    /// assert_eq!(estimate.standard_error(), None); // Sobol omits sampling uncertainty.
    /// let tuned = engine.price_with_greeks_with_settings(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), NumericalShiftSettings {
    ///         spot_shift: Some(0.005), ..Default::default()
    ///     })?;
    /// assert_eq!(tuned.price(), result.price());
    /// # Ok(()) }
    /// ```
    pub fn price_with_greeks(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        self.greeks_with_settings(option, context, request, Default::default())
    }

    fn greeks_with_settings(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        greeks(self, option, context, request, shifts, || {
            self.engine
                .native(Contract::Double(option), context, request, shifts, false)
                .map(|result| result.0)
        })
    }
}

impl PricingEngine<DoubleBarrierOption> for MonteCarloDoubleBarrierEngine {
    fn price_with_seed(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> Result<(f64, Option<u64>)> {
        context.bsm_parameters()?;
        let (result, _) = self.engine.native(
            Contract::Double(option),
            context,
            GreeksRequest::new([]),
            Default::default(),
            false,
        )?;
        Ok((result.price(), result.seed()))
    }
    fn greek_method(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<DoubleBarrierOption>>::validate_applicability(self, context)?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            crate::GreekMethod::NativeWithSpotControl,
        ))
    }

    fn price_with_statistics(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> Result<MonteCarloEstimate> {
        self.price_with_statistics(option, context)
    }
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.bsm_parameters()?;
        self.engine.validate_applicability(context)
    }
    fn validate_solver_seed(&self) -> Result<()> {
        self.sampling_settings().validate_solver_seed()
    }

    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<DoubleBarrierOption> + '_>>> {
        if self.sampling_settings().seed.is_some() {
            return Ok(None);
        }
        Ok(Some(Box::new(self.with_resolved_seed()?)))
    }
    fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.greeks_with_settings(option, context, request, settings)
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
