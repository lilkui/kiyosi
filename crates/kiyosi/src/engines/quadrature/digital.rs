use super::{QuadratureSettings, flat_rates, simpson};
use crate::instruments::monitoring::validate_life;
use crate::numerics::{checked, log_ratio, standardize};
use crate::{
    DigitalOption, Error, GreeksRequest, OptionType, PricingContext, PricingEngine, PricingResult,
    Result,
};

/// Prices European cash-or-nothing and asset-or-nothing options by BSM quadrature.
///
/// Requires flat risk-free rates and dividend yields. Composite Simpson integration
/// covers the paying side of the strike, using its one-sided endpoint limit.
/// Asset payments shift the normal variable to center the weighted density.
/// The integration extent is measured around that density's center; omitted
/// tails are zero. Check step-count and extent convergence separately: fixed-grid
/// completion provides no error estimate. Numerical Greeks also require bump
/// convergence checks, especially higher derivatives and short maturities.
///
/// Owns immutable settings; distinct and shared instances support concurrent
/// pricing without shared working storage. See [`Self::new`] for usage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureDigitalEngine {
    settings: QuadratureSettings,
}

impl QuadratureDigitalEngine {
    /// Creates an engine after validating the integration controls.
    ///
    /// # Errors
    ///
    /// Propagates [`QuadratureSettings::validate`] errors without changing inputs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     start, Date::new(2026, 1, 1)?.start_of_day())?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let settings = QuadratureSettings::default();
    /// let engine = QuadratureDigitalEngine::new(settings)?;
    /// assert_eq!(engine.settings(), settings);
    /// assert!((engine.price(&option, &context)? - 4.945810910532236).abs() < 1e-8);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]))?;
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: QuadratureSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned integration settings; see [`Self::new`] for usage.
    pub fn settings(&self) -> QuadratureSettings {
        self.settings
    }

    /// Prices a digital through expiry, permitting pre-effective valuation.
    ///
    /// See [`Self::new`] for usage. Equality at expiry pays zero.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for other models or nonconstant rate curves,
    /// before valuation validation. Returns [`Error::InvalidInput`] after expiry
    /// or outside curve coverage and [`Error::ComputationFailure`] for unrepresentable transitions,
    /// weights or prices. Inputs and settings remain unchanged.
    pub fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        <Self as PricingEngine<DigitalOption>>::price(self, option, context)
    }

    /// Computes requested numerical Greeks with shared bump controls.
    ///
    /// Expiry Greeks follow terminal payoffs; time Greeks are absent. Infeasible stencils leave affected
    /// measures absent; feasible bumped failures propagate. See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Checks flat-rate BSM applicability before shift validation,
    /// then propagates [`crate::numerical_greeks`] and [`Self::price`] errors.
    /// Inputs and settings remain unchanged.
    pub fn price_with_greeks(
        &self,
        option: &DigitalOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let shifts = crate::NumericalShiftSettings::default();
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}

impl PricingEngine<DigitalOption> for QuadratureDigitalEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        flat_rates(context).map(|_| ())
    }

    fn price(&self, option: &DigitalOption, context: &PricingContext) -> Result<f64> {
        let (rate, dividend) = flat_rates(context)?;
        validate_life(option, context)?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::vanilla(
                option,
                crate::engines::Payoff::digital(option),
                context,
            );
        }
        context.validate_curve_horizon(option.expiry_time())?;
        let spot = context.spot_price();
        let call = option.option_type() == OptionType::Call;
        let time = context.valuation_time().years_until(option.expiry_time());
        if time == 0.0 {
            return Ok(
                if (call && spot > option.strike()) || (!call && spot < option.strike()) {
                    option.payout().unwrap_or(spot)
                } else {
                    0.0
                },
            );
        }
        let sigma = context.bsm_parameters()?.volatility();
        let root = time.sqrt();
        let width = sigma * root;
        let forward = log_ratio(spot, option.strike()) + (rate - dividend) * time;
        let center = standardize(forward, sigma, root);
        let (argument, log_weight) = if let Some(payout) = option.payout() {
            (center - 0.5 * width, payout.ln() - rate * time)
        } else {
            (center + 0.5 * width, spot.ln() - dividend * time)
        };
        if !width.is_finite()
            || !forward.is_finite()
            || !log_weight.is_finite()
            || argument.is_nan()
        {
            return Err(Error::ComputationFailure(
                "digital quadrature transition or weight is not representable".into(),
            ));
        }
        let extent = self.settings.integration_standard_deviations;
        let (lower, upper) = if call {
            ((-argument).max(-extent), extent)
        } else {
            (-extent, (-argument).min(extent))
        };
        if lower >= upper {
            return Ok(0.0);
        }
        // The paying-side endpoint limit avoids giving the strike jump a Simpson weight.
        // Integrate unit mass first so the panel sum cannot overflow a finite price.
        let mass = simpson(&[lower, upper], self.settings.asset_step_count, |z| {
            (-0.5 * z * z - 0.918_938_533_204_672_7).exp()
        })?;
        checked((log_weight + mass.ln()).exp())
    }
}
