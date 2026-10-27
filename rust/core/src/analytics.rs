use crate::{Error, ErrorCategory, EuropeanOption, PricingContext, Result};

/// Spot derivatives are per spot unit; volatility/rate derivatives per percentage
/// point; time derivatives per calendar day of forward valuation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Greek {
    Delta,
    Gamma,
    Speed,
    Theta,
    Charm,
    Color,
    Vega,
    Vanna,
    Zomma,
    Rho,
}

impl Greek {
    pub const ALL: [Self; 10] = [
        Self::Delta,
        Self::Gamma,
        Self::Speed,
        Self::Theta,
        Self::Charm,
        Self::Color,
        Self::Vega,
        Self::Vanna,
        Self::Zomma,
        Self::Rho,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Delta => "delta",
            Self::Gamma => "gamma",
            Self::Speed => "speed",
            Self::Theta => "theta",
            Self::Charm => "charm",
            Self::Color => "color",
            Self::Vega => "vega",
            Self::Vanna => "vanna",
            Self::Zomma => "zomma",
            Self::Rho => "rho",
        }
    }
}

/// Selected measures; duplicates are ignored. Empty requests fail at valuation.
#[derive(Clone, Copy, Debug, Default)]
pub struct GreeksRequest([bool; 10]);
impl GreeksRequest {
    pub fn new(greeks: impl IntoIterator<Item = Greek>) -> Self {
        let mut result = Self::default();
        for greek in greeks {
            result.0[greek as usize] = true;
        }
        result
    }
    pub const fn all() -> Self {
        Self([true; 10])
    }
    pub fn has(self, greek: Greek) -> bool {
        self.0[greek as usize]
    }
    pub(crate) fn validate(self) -> Result<()> {
        if !self.0.contains(&true) {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "empty Greeks request",
            ));
        }
        Ok(())
    }
}

/// Finite price and optional finite Greeks. Missing measures are never zero sentinels.
#[derive(Clone, Debug, PartialEq)]
pub struct PricingResult {
    price: f64,
    values: [Option<f64>; 10],
}
impl PricingResult {
    pub fn price(&self) -> f64 {
        self.price
    }
    pub fn get(&self, greek: Greek) -> Option<f64> {
        self.values[greek as usize]
    }
    pub fn has(&self, greek: Greek) -> bool {
        self.get(greek).is_some()
    }
    pub fn require(&self, greek: Greek) -> Result<f64> {
        self.get(greek).ok_or(Error::new(
            ErrorCategory::InvalidResult,
            "requested Greek is unavailable",
        ))
    }
    /// Borrowed immutable storage, valid for this result's lifetime.
    pub fn values(&self) -> &[Option<f64>; 10] {
        &self.values
    }
    pub(crate) fn new(price: f64) -> Result<Self> {
        if !price.is_finite() {
            return Err(Error::new(ErrorCategory::InvalidResult, "non-finite price"));
        }
        Ok(Self {
            price,
            values: [None; 10],
        })
    }
    pub(crate) fn set(&mut self, request: GreeksRequest, greek: Greek, value: f64) -> Result<()> {
        if request.has(greek) {
            if !value.is_finite() {
                return Err(Error::new(ErrorCategory::InvalidResult, "non-finite Greek"));
            }
            self.values[greek as usize] = Some(value);
        }
        Ok(())
    }
}

/// Absolute finite-difference bumps. Positive values are validated even for analytic requests.
#[derive(Clone, Copy, Debug)]
pub struct NumericalShiftSettings {
    pub spot_shift: f64,
    pub volatility_shift: f64,
    pub rate_shift: f64,
    pub time_shift_days: i32,
}
impl Default for NumericalShiftSettings {
    fn default() -> Self {
        Self {
            spot_shift: 1e-2,
            volatility_shift: 1e-4,
            rate_shift: 1e-4,
            time_shift_days: 1,
        }
    }
}
impl NumericalShiftSettings {
    pub(crate) fn validate(self) -> Result<()> {
        if [self.spot_shift, self.volatility_shift, self.rate_shift]
            .iter()
            .any(|x| !x.is_finite() || *x <= 0.0)
            || self.time_shift_days <= 0
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "numerical shifts are invalid",
            ));
        }
        Ok(())
    }
}

/// Provides scalar pricing for numerical analysis and solvers, independent of exercise style.
/// Implementations must return domain failures through Result, without mutating inputs.
/// The instrument parameter selects supported combinations; it defaults to
/// [`EuropeanOption`]. See [`crate::AnalyticDigitalEngine::price`] for digital usage.
pub trait PricingEngine<I = EuropeanOption> {
    /// Checks engine applicability before numerical request or solver validation.
    /// The default accepts the context. See
    /// [`crate::MonteCarloVanillaEngine::price_with_greeks`] for flat-rate restrictions.
    ///
    /// # Errors
    /// Returns engine-specific errors for unsupported market representations.
    fn validate_applicability(&self, _context: &PricingContext) -> Result<()> {
        Ok(())
    }

    /// Prepares request-local state for numerical Greeks and implied volatility.
    /// Returns `None` when this engine already evaluates one consistent price
    /// function, as deterministic engines do by default. A returned engine must
    /// itself return `None` here. See [`crate::MonteCarloVanillaEngine::price_with_greeks`].
    ///
    /// # Errors
    /// Propagates operational failures while creating request-local state.
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<I> + Send + Sync + '_>>> {
        Ok(None)
    }

    /// Prices a supported contract without changing the inputs.
    ///
    /// # Errors
    /// Returns the engine's structured domain errors for invalid valuation horizons
    /// or numerical failures. Recoverable operational errors retain the implementation's
    /// contract. See [`crate::AnalyticDigitalEngine::price`] for an example.
    fn price(&self, option: &I, context: &PricingContext) -> Result<f64>;

    /// Returns requested sensitivities, using scalar differences unless overridden.
    /// See [`crate::CoxRossRubinsteinVanillaEngine::price_with_greeks`] for an example.
    ///
    /// # Errors
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing errors.
    fn price_with_greeks(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        I: crate::OptionContract,
    {
        crate::numerical_greeks(self, option, context, request, settings)
    }

    /// Restricts a validated solver bracket to the engine's supported domain.
    /// The default retains the caller's settings. See [`crate::implied_volatility`].
    ///
    /// # Errors
    /// Engine implementations may reject unsupported contracts, horizons or brackets.
    fn prepare_implied_volatility(
        &self,
        _option: &I,
        _context: &PricingContext,
        settings: crate::ImpliedVolatilitySettings,
    ) -> Result<crate::ImpliedVolatilitySettings> {
        Ok(settings)
    }
}
