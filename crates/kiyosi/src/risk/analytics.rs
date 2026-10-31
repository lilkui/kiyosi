use crate::{Error, EuropeanOption, PricingContext, Result};

/// Selects a price sensitivity and its reporting units.
///
/// Spot derivatives are per spot unit, volatility/rate derivatives per percentage
/// point, and time derivatives per calendar day of forward valuation time.
/// See [`GreeksRequest::all`] for a request example.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Greek {
    /// Measures the first spot derivative per spot unit.
    Delta,
    /// Measures the second spot derivative per squared spot unit.
    Gamma,
    /// Measures the third spot derivative per cubed spot unit.
    Speed,
    /// Measures the price change per calendar day of forward valuation time.
    Theta,
    /// Measures the delta change per calendar day of forward valuation time.
    Charm,
    /// Measures the gamma change per calendar day of forward valuation time.
    Color,
    /// Measures the price change per volatility percentage point.
    Vega,
    /// Measures the delta change per volatility percentage point.
    Vanna,
    /// Measures the gamma change per volatility percentage point.
    Zomma,
    /// Measures the price change per risk-free-rate percentage point.
    Rho,
}

impl Greek {
    /// Lists every supported Greek in result-storage order.
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
    /// Returns the lowercase Python choice string for this Greek.
    ///
    /// For example, `Greek::Vanna.name()` is `"vanna"`; see [`Self::ALL`].
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

/// Selects price sensitivities, ignoring duplicates.
///
/// Empty requests fail at valuation; see [`Self::all`] for usage.
#[derive(Clone, Copy, Debug, Default)]
pub struct GreeksRequest([bool; 10]);
impl GreeksRequest {
    /// Creates a request, ignoring duplicates; see [`Self::all`].
    pub fn new(greeks: impl IntoIterator<Item = Greek>) -> Self {
        let mut result = Self::default();
        for greek in greeks {
            result.0[greek as usize] = true;
        }
        result
    }
    /// Requests all supported Greeks.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Greek, GreeksRequest};
    /// let request = GreeksRequest::all();
    /// assert!(request.has(Greek::Delta));
    /// ```
    pub const fn all() -> Self {
        Self([true; 10])
    }
    /// Reports whether this Greek was requested; see [`Self::all`].
    pub fn has(self, greek: Greek) -> bool {
        self.0[greek as usize]
    }
    pub(crate) fn validate(self) -> Result<()> {
        if !self.0.contains(&true) {
            return Err(Error::InvalidInput("empty Greeks request".into()));
        }
        Ok(())
    }
}

/// Owns a finite price and optional finite Greeks.
///
/// Missing measures use `None`, never zero; see [`crate::AnalyticEuropeanEngine`].
#[derive(Clone, Debug, PartialEq)]
pub struct PricingResult {
    price: f64,
    values: [Option<f64>; 10],
}
impl PricingResult {
    /// Returns the finite price; see [`crate::AnalyticEuropeanEngine`].
    pub fn price(&self) -> f64 {
        self.price
    }
    /// Returns a Greek, or `None` if unavailable; see [`Self::require`].
    pub fn get(&self, greek: Greek) -> Option<f64> {
        self.values[greek as usize]
    }
    /// Reports whether a finite value is available; see [`Self::require`].
    pub fn has(&self, greek: Greek) -> bool {
        self.get(greek).is_some()
    }
    /// Returns a measure or rejects one unavailable in this result.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] when the measure was not requested
    /// or cannot be provided at this valuation point. [`Self::get`] returns `None`
    /// for these cases. See [`crate::AnalyticEuropeanEngine::price_with_greeks`]
    /// for an example of requesting measures.
    pub fn require(&self, greek: Greek) -> Result<f64> {
        self.get(greek).ok_or(Error::UnsupportedOperation(
            "requested Greek is unavailable".into(),
        ))
    }
    /// Borrows immutable Greek storage in [`Greek::ALL`] order.
    pub fn values(&self) -> &[Option<f64>; 10] {
        &self.values
    }
    pub(crate) fn new(price: f64) -> Result<Self> {
        if !price.is_finite() {
            return Err(Error::ComputationFailure("non-finite price".into()));
        }
        Ok(Self {
            price,
            values: [None; 10],
        })
    }
    pub(crate) fn set(&mut self, request: GreeksRequest, greek: Greek, value: f64) -> Result<()> {
        if request.has(greek) {
            if !value.is_finite() {
                return Err(Error::ComputationFailure("non-finite Greek".into()));
            }
            self.values[greek as usize] = Some(value);
        }
        Ok(())
    }
}

/// Configures absolute finite-difference bumps.
///
/// Numerical pricing requests validate all bump settings before calculation.
/// See [`crate::numerical_greeks`] for usage.
#[derive(Clone, Copy, Debug)]
pub struct NumericalShiftSettings {
    /// Sets the positive absolute spot bump (default 0.01 spot units).
    pub spot_shift: f64,
    /// Sets the positive absolute volatility bump (default 0.0001 decimal units).
    pub volatility_shift: f64,
    /// Sets the positive parallel rate bump (default 0.0001 decimal units).
    pub rate_shift: f64,
    /// Sets the positive time bump in calendar days (default 1).
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
            return Err(Error::InvalidInput("numerical shifts are invalid".into()));
        }
        Ok(())
    }
}

/// Provides scalar pricing for numerical analysis and solvers, independent of exercise style.
///
/// Implementations must return domain failures through [`Result<T>`](crate::Result),
/// without mutating inputs.
/// The instrument parameter selects supported combinations; it defaults to
/// [`EuropeanOption`]. See [`crate::AnalyticDigitalEngine::price`] for digital usage.
pub trait PricingEngine<I = EuropeanOption> {
    /// Checks engine applicability before numerical requests or implied-volatility validation.
    ///
    /// Coupon solves validate their settings and endpoint replacements before pricing.
    /// The default accepts the context. See
    /// [`crate::MonteCarloVanillaEngine::price_with_greeks`] for flat-rate restrictions.
    ///
    /// # Errors
    ///
    /// Returns engine-specific errors for unsupported market representations.
    fn validate_applicability(&self, _context: &PricingContext) -> Result<()> {
        Ok(())
    }

    /// Prepares request-local state for numerical Greeks and implied-parameter solves.
    ///
    /// Returns `None` when this engine already evaluates one consistent price
    /// function, as deterministic engines do by default. A returned engine must
    /// itself return `None` here. See [`crate::MonteCarloVanillaEngine::price_with_greeks`].
    /// Prepared handles support sequential use within the calling thread; they
    /// need not implement [`Send`] or [`Sync`]. Independent requests prepare their
    /// own handles, leaving the original engine's concurrency contract unchanged.
    ///
    /// # Errors
    ///
    /// Propagates operational failures while creating request-local state.
    fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<I> + '_>>> {
        Ok(None)
    }

    /// Prices a supported contract without changing the inputs.
    ///
    /// # Errors
    ///
    /// Returns the engine's structured domain errors for invalid valuation horizons
    /// or numerical failures. Recoverable operational errors retain the implementation's
    /// contract. See [`crate::AnalyticDigitalEngine::price`] for an example.
    fn price(&self, option: &I, context: &PricingContext) -> Result<f64>;

    /// Returns requested sensitivities, using scalar differences unless overridden.
    ///
    /// Settings configure numerical differences; native-only overrides ignore them.
    ///
    /// See [`crate::CoxRossRubinsteinVanillaEngine::price_with_greeks`] for an example.
    ///
    /// # Errors
    ///
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

    /// Reports whether volatility affects a validated contract's remaining payoff.
    ///
    /// Defaults to `true`. The solver checks this after endpoint pricing so that
    /// life, history and applicability errors take precedence. See
    /// [`crate::TurnbullWakemanArithmeticAveragePriceEngine::price`].
    fn volatility_affects_price(&self, _option: &I, _context: &PricingContext) -> bool {
        true
    }

    /// Restricts a validated solver bracket to the engine's supported domain.
    ///
    /// The default retains the caller's settings. See [`crate::implied_volatility`].
    ///
    /// # Errors
    ///
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
