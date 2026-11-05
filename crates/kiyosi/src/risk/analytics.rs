use crate::{Error, EuropeanOption, PricingContext, Result};

/// Selects an unscaled partial derivative of the price.
///
/// Uses price `V`, spot `S`, decimal volatility `σ`, decimal continuously
/// compounded rates `r` and `q`, and forward valuation time `t` in years
/// (Actual/365 Fixed). Other inputs, including expiry, remain fixed.
/// Rho and dividend rho differentiate parallel shifts of the entire risk-free
/// and dividend-yield curves, respectively, rather than individual curve nodes.
/// Theta, charm and color advance valuation with the market curves held fixed;
/// they do not differentiate increasing time to expiry.
/// See [`GreeksRequest::all`] for a request example.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Greek {
    /// Measures `∂V/∂S`.
    Delta,
    /// Measures `∂²V/∂S²`.
    Gamma,
    /// Measures `∂³V/∂S³`.
    Speed,
    /// Measures `∂V/∂t` with valuation time in years.
    Theta,
    /// Measures `∂²V/(∂S∂t)` with valuation time in years.
    Charm,
    /// Measures `∂³V/(∂S²∂t)` with valuation time in years.
    Color,
    /// Measures `∂V/∂σ` with decimal volatility.
    Vega,
    /// Measures `∂²V/∂σ²` with decimal volatility.
    Volga,
    /// Measures `∂²V/(∂S∂σ)` with decimal volatility.
    Vanna,
    /// Measures `∂³V/(∂S²∂σ)` with decimal volatility.
    Zomma,
    /// Measures `∂V/∂r` for a decimal parallel risk-free-rate shift.
    Rho,
    /// Measures `∂V/∂q` for a decimal parallel dividend-yield shift.
    DividendRho,
}

impl Greek {
    /// Lists every known Greek in result-storage order.
    pub const ALL: [Self; 12] = [
        Self::Delta,
        Self::Gamma,
        Self::Speed,
        Self::Theta,
        Self::Charm,
        Self::Color,
        Self::Vega,
        Self::Volga,
        Self::Vanna,
        Self::Zomma,
        Self::Rho,
        Self::DividendRho,
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
            Self::Volga => "volga",
            Self::Vanna => "vanna",
            Self::Zomma => "zomma",
            Self::Rho => "rho",
            Self::DividendRho => "dividend_rho",
        }
    }
}

/// Selects price sensitivities, ignoring duplicates.
///
/// Select measures explicitly with [`Self::new`] or [`Self::all`].
/// Empty requests fail at valuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GreeksRequest([bool; Greek::ALL.len()]);
impl GreeksRequest {
    /// Creates a request, ignoring duplicates; see [`Self::all`].
    pub fn new(greeks: impl IntoIterator<Item = Greek>) -> Self {
        let mut result = Self([false; Greek::ALL.len()]);
        for greek in greeks {
            result.0[greek as usize] = true;
        }
        result
    }
    /// Requests every known Greek; engine/model support is reported in the result.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Greek, GreeksRequest};
    /// let request = GreeksRequest::all();
    /// assert!(request.has(Greek::Delta));
    /// ```
    pub const fn all() -> Self {
        Self([true; Greek::ALL.len()])
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

/// Describes whether a sensitivity was requested and could be calculated.
///
/// See [`PricingResult::status`] for a request-inspection example.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GreekStatus {
    /// Records a finite sensitivity value.
    Available,
    /// Records a measure omitted from the request.
    NotRequested,
    /// Records a requested measure outside the engine/model's supported set.
    Unsupported,
    /// Records a supported request unavailable at this valuation or numerical stencil.
    Undefined,
}

/// Owns a finite price and optional finite Greeks.
///
/// Missing measures use `None`, never zero; [`Self::status`] distinguishes
/// unrequested, unsupported and undefined sensitivities.
#[derive(Clone, Debug, PartialEq)]
pub struct PricingResult {
    price: f64,
    values: [Option<f64>; Greek::ALL.len()],
    requested: GreeksRequest,
    supported: GreeksRequest,
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
    /// Returns the request and availability status of a sensitivity.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Greek, GreeksRequest, GreekStatus, PricingResult};
    /// # fn main() -> kiyosi::Result<()> {
    /// let result = PricingResult::new(10.0)?.with_request(
    ///     GreeksRequest::new([Greek::Delta]), GreeksRequest::all());
    /// assert_eq!(result.status(Greek::Delta), GreekStatus::Undefined);
    /// assert_eq!(result.status(Greek::Vega), GreekStatus::NotRequested);
    /// # Ok(()) }
    /// ```
    pub fn status(&self, greek: Greek) -> GreekStatus {
        if !self.requested.has(greek) {
            GreekStatus::NotRequested
        } else if self.has(greek) {
            GreekStatus::Available
        } else if !self.supported.has(greek) {
            GreekStatus::Unsupported
        } else {
            GreekStatus::Undefined
        }
    }
    /// Returns the requested measures; see [`Self::status`].
    pub fn requested(&self) -> GreeksRequest {
        self.requested
    }
    /// Returns engine/model support, independent of valuation and stencil availability.
    ///
    /// See [`Self::status`] for the distinction between unsupported and undefined measures.
    pub fn supported(&self) -> GreeksRequest {
        self.supported
    }
    /// Attaches request metadata, discarding values outside the requested and supported sets.
    ///
    /// Custom engines can report partial support explicitly. Built-in engines attach
    /// this metadata before returning. See [`Self::status`] for an example.
    #[must_use]
    pub fn with_request(mut self, requested: GreeksRequest, supported: GreeksRequest) -> Self {
        self.requested = requested;
        self.supported = supported;
        for greek in Greek::ALL {
            if !requested.has(greek) || !supported.has(greek) {
                self.values[greek as usize] = None;
            }
        }
        self
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
    pub fn values(&self) -> &[Option<f64>; Greek::ALL.len()] {
        &self.values
    }
    /// Creates a finite price with all Greeks absent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ComputationFailure`] for a non-finite price.
    /// See [`Self::with_greek`] for custom-engine result construction.
    pub fn new(price: f64) -> Result<Self> {
        if !price.is_finite() {
            return Err(Error::ComputationFailure("non-finite price".into()));
        }
        Ok(Self {
            price,
            values: [None; Greek::ALL.len()],
            requested: GreeksRequest::new([]),
            supported: GreeksRequest::all(),
        })
    }

    /// Returns this result with a finite Greek added or replaced.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ComputationFailure`] for a non-finite value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Greek, PricingResult};
    /// # fn main() -> kiyosi::Result<()> {
    /// let result = PricingResult::new(10.0)?.with_greek(Greek::Delta, 0.5)?;
    /// assert_eq!(result.require(Greek::Delta)?, 0.5);
    /// assert_eq!(result.get(Greek::Vega), None);
    /// # Ok(()) }
    /// ```
    pub fn with_greek(mut self, greek: Greek, value: f64) -> Result<Self> {
        self.set(GreeksRequest::new([greek]), greek, value)?;
        Ok(self)
    }
    pub(crate) fn set(&mut self, request: GreeksRequest, greek: Greek, value: f64) -> Result<()> {
        if request.has(greek) {
            if !value.is_finite() {
                return Err(Error::ComputationFailure("non-finite Greek".into()));
            }
            self.requested.0[greek as usize] = true;
            self.supported.0[greek as usize] = true;
            self.values[greek as usize] = Some(value);
        }
        Ok(())
    }
}

/// Configures absolute finite-difference bumps.
///
/// Scalar numerical requests validate all bump settings before calculation.
/// Engine `price_with_greeks` methods use defaults and accept no settings.
/// See [`crate::numerical_greeks`] for usage.
#[derive(Clone, Copy, Debug)]
pub struct NumericalShiftSettings {
    /// Sets a positive absolute spot bump, or `None` (default) for the engine default.
    ///
    /// Automatic bumps use 0.01 spot units, except American Monte Carlo before expiry uses 8% of spot.
    pub spot_shift: Option<f64>,
    /// Sets the positive absolute volatility bump (default 0.0001 decimal units).
    pub volatility_shift: f64,
    /// Sets the positive risk-free and dividend rate bump (default 0.0001 decimal units).
    pub rate_shift: f64,
    /// Sets the positive time bump in calendar days (default 1).
    pub time_shift_days: i32,
}
impl Default for NumericalShiftSettings {
    fn default() -> Self {
        Self {
            spot_shift: None,
            volatility_shift: 1e-4,
            rate_shift: 1e-4,
            time_shift_days: 1,
        }
    }
}
impl NumericalShiftSettings {
    /// Returns the explicit bump or the general fallback of 0.01 spot units.
    ///
    /// This does not resolve an engine-specific automatic bump. Preserve `None`
    /// when forwarding settings to an engine. American Monte Carlo resolves
    /// automatic bumps before expiry; see
    /// [`crate::MonteCarloVanillaEngine::default_american_spot_shift`].
    pub fn fallback_spot_shift(self) -> f64 {
        self.spot_shift.unwrap_or(0.01)
    }
    /// Validates every bump control without pricing a contract.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite explicit spot,
    /// volatility or rate bumps, or a nonpositive calendar-day bump.
    /// Leaves the settings unchanged. Automatic spot bumps are resolved by the engine;
    /// representability of numerical stencils is checked during pricing.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::NumericalShiftSettings;
    /// # fn main() -> kiyosi::Result<()> {
    /// let settings = NumericalShiftSettings { spot_shift: Some(0.1), ..Default::default() };
    /// settings.validate()?;
    /// # Ok(()) }
    /// ```
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("spot_shift", self.fallback_spot_shift()),
            ("volatility_shift", self.volatility_shift),
            ("rate_shift", self.rate_shift),
        ] {
            crate::error::finite(value, true, name)?;
        }
        if self.time_shift_days <= 0 {
            return Err(Error::InvalidInput(
                "time_shift_days must be positive".into(),
            ));
        }
        Ok(())
    }
}

/// Describes how an engine estimates Greeks and which shift controls it may use.
///
/// This describes the calculation path, independently of the requested measures.
/// A listed control may be unused for a particular request or resolved payoff.
/// Listed controls describe internal defaults; customize scalar differences with
/// [`crate::numerical_greeks`]. See
/// [`PricingEngine::greek_method`] for an example.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GreekMethod {
    /// Uses native analytic, grid or sampling estimates without numerical shifts.
    Native,
    /// Uses scalar numerical differences for the supported measures.
    Numerical,
    /// Combines native estimates with numerical completion of other measures.
    Mixed,
    /// Uses native estimates with spot shifts for availability or rebate derivatives.
    NativeWithSpotControl,
    /// Uses terminal-payoff spot stencils at expiry.
    Terminal,
    /// Provides no Greeks for this model and engine.
    Unsupported,
}

impl GreekMethod {
    /// Returns the stable choice string; see [`PricingEngine::greek_method`].
    pub const fn name(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Numerical => "numerical",
            Self::Mixed => "mixed",
            Self::NativeWithSpotControl => "native_with_spot_control",
            Self::Terminal => "terminal",
            Self::Unsupported => "unsupported",
        }
    }

    /// Returns names of shift controls potentially used by this calculation path.
    ///
    /// Names correspond to [`NumericalShiftSettings`] fields and describe internal
    /// defaults. See [`PricingEngine::greek_method`] for an example.
    pub const fn active_controls(self) -> &'static [&'static str] {
        match self {
            Self::Native | Self::Unsupported => &[],
            Self::NativeWithSpotControl | Self::Terminal => &["spot_shift"],
            Self::Numerical | Self::Mixed => &[
                "spot_shift",
                "volatility_shift",
                "rate_shift",
                "time_shift_days",
            ],
        }
    }

    pub(crate) fn for_valuation(
        option: &impl crate::OptionContract,
        context: &PricingContext,
        before_expiry: Self,
    ) -> Self {
        if before_expiry != Self::Unsupported && context.valuation_time() == option.expiry_time() {
            Self::Terminal
        } else {
            before_expiry
        }
    }
}

/// Provides scalar pricing for numerical analysis and solvers, independent of exercise style.
///
/// Implementations must return domain failures through [`Result<T>`](crate::Result),
/// without mutating inputs.
/// The instrument parameter selects supported combinations; it defaults to
/// [`EuropeanOption`]. See [`crate::AnalyticDigitalEngine::price`] for digital usage.
pub trait PricingEngine<I = EuropeanOption> {
    /// Reports the native Greek calculation path without pricing or drawing samples.
    ///
    /// Controls are potentially active across supported requests; individual requests,
    /// current events and resolved payoffs may use fewer. Contract life and history
    /// are validated by pricing, rather than by this query. The standalone
    /// [`crate::numerical_greeks`] helper always selects numerical differences.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::validate_applicability`] failures for unsupported models or curves.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, start.add_days(365)?)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start)?;
    /// let method = AnalyticEuropeanEngine.greek_method(&option, &context)?;
    /// assert_eq!(method, GreekMethod::Native);
    /// assert!(method.active_controls().is_empty());
    /// # Ok(()) }
    /// ```
    fn greek_method(&self, option: &I, context: &PricingContext) -> Result<GreekMethod>
    where
        I: crate::OptionContract,
    {
        self.validate_applicability(context)?;
        Ok(GreekMethod::for_valuation(
            option,
            context,
            GreekMethod::Numerical,
        ))
    }

    /// Returns the automatic pre-expiry spot bump for this engine and contract family.
    ///
    /// Defaults to 0.01 spot units; American Monte Carlo uses 8% of spot.
    /// Terminal numerical Greeks use the shared 0.01 spot-unit default.
    /// See [`NumericalShiftSettings`] for explicit control.
    fn default_spot_shift(&self, _context: &PricingContext) -> f64 {
        NumericalShiftSettings::default().fallback_spot_shift()
    }

    /// Checks engine applicability before numerical requests or implied-volatility validation.
    ///
    /// Coupon solves validate their settings and endpoint replacements before pricing.
    /// The default requires BSM; engines supporting another model override it. See
    /// [`crate::MonteCarloVanillaEngine::price_with_greeks`] for flat-rate restrictions.
    ///
    /// # Errors
    ///
    /// Returns engine-specific errors for unsupported market representations.
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.bsm_parameters()?;
        Ok(())
    }

    /// Prepares request-local state for numerical Greeks and implied-parameter solvers.
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

    /// Returns a price with sampling uncertainty when the engine provides an estimator.
    ///
    /// See [`crate::MonteCarloVanillaEngine::price_with_statistics`] for an example
    /// and [`crate::MonteCarloEstimate`] for uncertainty conventions. The default
    /// implementation reports an unsupported operation. Inputs remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] when sampling statistics are not
    /// implemented. Implementations propagate their pricing and uncertainty errors.
    fn price_with_statistics(
        &self,
        _option: &I,
        _context: &PricingContext,
    ) -> Result<crate::MonteCarloEstimate> {
        Err(Error::UnsupportedOperation(
            "sampling statistics are unavailable for this engine and instrument".into(),
        ))
    }

    /// Returns requested sensitivities, using scalar differences unless overridden.
    ///
    /// Uses engine-default controls. Configure scalar differences explicitly with
    /// [`crate::numerical_greeks`] and [`NumericalShiftSettings`].
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
    ) -> Result<PricingResult>
    where
        I: crate::OptionContract,
    {
        let settings = crate::NumericalShiftSettings::default();
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
