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
/// Empty requests select price only; every sensitivity is not requested.
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
    /// Records a derivative undefined at this valuation; see [`PricingResult::status`].
    Undefined,
    /// Records an unavailable numerical stencil; changing controls may recover the measure.
    ///
    /// See [`PricingResult::with_unavailable_stencils`].
    StencilUnavailable,
}

impl GreekStatus {
    /// Returns the Python status choice string; see [`PricingResult::status`].
    pub const fn name(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotRequested => "not_requested",
            Self::Unsupported => "unsupported",
            Self::Undefined => "undefined",
            Self::StencilUnavailable => "stencil_unavailable",
        }
    }
}

/// Owns a finite price and optional finite Greeks.
///
/// Missing measures use `None`, never zero; [`Self::status`] distinguishes
/// unrequested, unsupported, undefined and numerically unavailable sensitivities.
#[derive(Clone, Debug, PartialEq)]
pub struct PricingResult {
    price: f64,
    seed: Option<u64>,
    values: [Option<f64>; Greek::ALL.len()],
    requested: GreeksRequest,
    supported: GreeksRequest,
    unavailable_stencils: GreeksRequest,
}
impl PricingResult {
    /// Returns the simulation seed used by this request, or `None` without sampling.
    ///
    /// Replay with the same inputs, engine controls and build. See
    /// [`crate::MonteCarloVanillaEngine::with_resolved_seed`] for seed configuration.
    pub fn seed(&self) -> Option<u64> {
        self.seed
    }
    /// Attaches the seed used by a sampling request; see [`Self::seed`].
    #[must_use]
    pub fn with_seed(mut self, seed: Option<u64>) -> Self {
        self.seed = seed;
        self
    }
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
        } else if self.unavailable_stencils.has(greek) {
            GreekStatus::StencilUnavailable
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
                self.unavailable_stencils.0[greek as usize] = false;
            }
        }
        self
    }
    /// Marks absent, requested, supported measures as lacking a numerical stencil.
    ///
    /// Does not change finite values, the request or engine support. Adding a value
    /// with [`Self::with_greek`] clears its stencil status.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Greek, GreeksRequest, GreekStatus, PricingResult};
    /// # fn main() -> kiyosi::Result<()> {
    /// let request = GreeksRequest::new([Greek::Vega]);
    /// let result = PricingResult::new(10.0)?
    ///     .with_request(request, GreeksRequest::all())
    ///     .with_unavailable_stencils(request);
    /// assert_eq!(result.status(Greek::Vega), GreekStatus::StencilUnavailable);
    /// assert_eq!(result.with_greek(Greek::Vega, 2.0)?.status(Greek::Vega), GreekStatus::Available);
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn with_unavailable_stencils(mut self, measures: GreeksRequest) -> Self {
        for greek in Greek::ALL {
            if measures.has(greek)
                && self.requested.has(greek)
                && self.supported.has(greek)
                && !self.has(greek)
            {
                self.unavailable_stencils.0[greek as usize] = true;
            }
        }
        self
    }
    /// Returns a measure or rejects one unavailable in this result.
    ///
    /// # Errors
    ///
    /// Returns [`Error::GreekUnavailable`] with the measure and its [`GreekStatus`]
    /// when no value is available. [`Self::get`] returns `None` for these cases.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Error, Greek, GreekStatus, PricingResult};
    /// # fn main() -> kiyosi::Result<()> {
    /// let result = PricingResult::new(10.0)?;
    /// assert!(matches!(result.require(Greek::Vega),
    ///     Err(Error::GreekUnavailable { greek: Greek::Vega, status: GreekStatus::NotRequested })));
    /// # Ok(()) }
    /// ```
    pub fn require(&self, greek: Greek) -> Result<f64> {
        self.get(greek).ok_or(Error::GreekUnavailable {
            greek,
            status: self.status(greek),
        })
    }
    /// Borrows Greek storage in [`Greek::ALL`] order.
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
            seed: None,
            values: [None; Greek::ALL.len()],
            requested: GreeksRequest::new([]),
            supported: GreeksRequest::all(),
            unavailable_stencils: GreeksRequest::new([]),
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
            self.unavailable_stencils.0[greek as usize] = false;
        }
        Ok(())
    }
}

#[expect(
    clippy::float_cmp,
    reason = "Exact strike equality locates the terminal payoff kink."
)]
pub(crate) fn vanilla_expiry_greeks(
    option_type: crate::OptionType,
    strike: f64,
    context: &PricingContext,
    mut result: PricingResult,
) -> Result<PricingResult> {
    let request = GreeksRequest::new(
        Greek::ALL
            .into_iter()
            .filter(|&greek| result.requested().has(greek) && result.supported().has(greek)),
    );
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        result.set(request, greek, 0.0)?;
    }
    let spot = context.spot_price();
    if spot != strike {
        let delta = match option_type {
            crate::OptionType::Call => f64::from(spot > strike),
            crate::OptionType::Put => -f64::from(spot < strike),
        };
        result.set(request, Greek::Delta, delta)?;
        for greek in [Greek::Gamma, Greek::Speed, Greek::Vanna, Greek::Zomma] {
            result.set(request, greek, 0.0)?;
        }
    }
    Ok(result)
}

/// Configures absolute finite-difference bumps.
///
/// Scalar numerical requests validate all bump settings before calculation.
/// Finite-difference engines use their configured shifts for `price_with_greeks`;
/// [`PricingEngine::price_with_greeks_with_settings`] accepts explicit shifts while
/// retaining native estimators. [`PricingEngine::greek_settings`] supplies engine defaults.
/// See [`crate::numerical_greeks`] for usage.
#[derive(Clone, Copy, Debug)]
pub struct NumericalShiftSettings {
    /// Sets a positive absolute spot bump, or `None` (default) for the engine default.
    ///
    /// Automatic bumps use the smaller of 0.01 spot units and 1% of spot,
    /// except American Monte Carlo before expiry uses 8% of spot.
    pub spot_shift: Option<f64>,
    /// Sets the positive absolute volatility bump (default 0.0001 decimal units).
    pub volatility_shift: f64,
    /// Sets the positive risk-free and dividend rate bump (default 0.0001 decimal units).
    pub rate_shift: f64,
    /// Sets a positive time bump in whole microseconds, or `None` for automatic selection.
    ///
    /// Automatic selection starts at one day and shortens the bump to avoid observations.
    /// Explicit bumps retain their size within life and curve bounds.
    /// See [`crate::numerical_greeks`] for convergence checks.
    pub time_shift: Option<std::time::Duration>,
}
impl Default for NumericalShiftSettings {
    fn default() -> Self {
        Self {
            spot_shift: None,
            volatility_shift: 1e-4,
            rate_shift: 1e-4,
            time_shift: None,
        }
    }
}
impl NumericalShiftSettings {
    /// Returns the explicit bump or the general fallback of 0.01 spot units.
    ///
    /// This does not apply the spot-relative cap or an engine-specific automatic bump. Preserve `None`
    /// when forwarding settings to an engine. American Monte Carlo resolves
    /// automatic bumps before expiry; see
    /// [`crate::MonteCarloVanillaEngine::default_american_spot_shift`].
    pub fn fallback_spot_shift(self) -> f64 {
        self.spot_shift.unwrap_or(0.01)
    }

    pub(crate) fn spot_shift_at(self, spot: f64) -> f64 {
        self.spot_shift
            .unwrap_or_else(|| self.fallback_spot_shift().min(0.01 * spot))
    }
    /// Validates every bump control without pricing a contract.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite explicit spot,
    /// volatility or rate bumps, or a zero or sub-microsecond time bump.
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
        self.time_shift_micros()?;
        Ok(())
    }

    pub(crate) fn time_shift_micros(self) -> Result<i64> {
        let shift = self
            .time_shift
            .unwrap_or(std::time::Duration::from_secs(86_400));
        if shift.is_zero() || shift.subsec_nanos() % 1000 != 0 {
            return Err(Error::InvalidInput(
                "time_shift must be positive with whole-microsecond precision".into(),
            ));
        }
        Ok(i64::try_from(shift.as_micros()).unwrap_or(i64::MAX))
    }
}

/// Describes how an engine estimates Greeks and which shift controls it may use.
///
/// This describes the calculation path, independently of the requested measures.
/// A listed control may be unused for a particular request or resolved payoff.
/// Finite-difference settings expose all shifts; Monte Carlo barrier calls expose spot control.
/// Other engines use internal defaults. Customize scalar differences with
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
    /// Names correspond to [`NumericalShiftSettings`] fields. Every built-in engine accepts
    /// request-level controls through [`PricingEngine::price_with_greeks_with_settings`].
    /// See [`PricingEngine::greek_method`] for an example.
    pub const fn active_controls(self) -> &'static [&'static str] {
        match self {
            Self::Native | Self::Unsupported => &[],
            Self::NativeWithSpotControl | Self::Terminal => &["spot_shift"],
            Self::Numerical | Self::Mixed => {
                &["spot_shift", "volatility_shift", "rate_shift", "time_shift"]
            }
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
    /// Returns default Greek controls for this engine; see [`NumericalShiftSettings`].
    ///
    /// Finite-difference engines return their configured shifts. Other built-in
    /// engines use the shared defaults, including automatic product-specific spot bumps.
    fn greek_settings(&self, _option: &I) -> NumericalShiftSettings {
        NumericalShiftSettings::default()
    }

    /// Returns native sensitivities with explicit request-level numerical controls.
    ///
    /// Built-in overrides retain native estimators and apply shifts to numerical
    /// fallbacks, event checks and terminal stencils. Purely analytic measures ignore
    /// the validated controls. The default uses scalar differences, like
    /// [`Self::price_with_greeks`]. Inputs and engine configuration remain unchanged.
    ///
    /// # Errors
    ///
    /// Checks engine applicability before shift validation, then propagates
    /// pricing failures. See [`crate::numerical_greeks`] for scalar difference errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 2)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, start.add_days(365)?)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start)?;
    /// let shifts = NumericalShiftSettings { spot_shift: Some(0.1), ..Default::default() };
    /// let result = AnalyticEuropeanEngine.price_with_greeks_with_settings(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]), shifts)?;
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    fn price_with_greeks_with_settings(
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
    /// let sampler = MonteCarloVanillaEngine::default();
    /// assert_eq!(sampler.default_spot_shift(&option, &context), 0.01);
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
    /// Defaults to the smaller of 0.01 spot units and 1% of spot;
    /// American Monte Carlo uses 8% of spot.
    /// Terminal numerical Greeks use the shared capped default.
    /// The instrument selects its family and permits type inference for multi-product engines.
    /// See [`Self::greek_method`] for an example and [`NumericalShiftSettings`] for explicit control.
    fn default_spot_shift(&self, _option: &I, context: &PricingContext) -> f64 {
        NumericalShiftSettings::default().spot_shift_at(context.spot_price())
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

    /// Checks that a solver's random draws can be replayed by the caller.
    ///
    /// Deterministic engines accept the default. Monte Carlo engines require a
    /// configured seed; call their `with_resolved_seed` method before solving and
    /// retain the returned engine for repricing. Numerical Greeks may still resolve
    /// a request-local seed. See [`crate::implied_volatility`] for solver usage.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for an unseeded built-in Monte Carlo engine.
    /// Custom engines retain their own replayability requirements.
    fn validate_solver_seed(&self) -> Result<()> {
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

    /// Prices a supported contract.
    ///
    /// # Errors
    ///
    /// Returns the engine's structured domain errors for invalid valuation horizons
    /// or numerical failures. Recoverable operational errors retain the implementation's
    /// contract. See [`crate::AnalyticDigitalEngine::price`] for an example.
    fn price(&self, option: &I, context: &PricingContext) -> Result<f64>;

    /// Returns a scalar price and the seed used, or `None` without sampling.
    ///
    /// Numerical requests use this hook to retain replay metadata without computing
    /// sampling uncertainty. The default delegates to [`Self::price`] without a seed.
    /// See [`crate::MonteCarloVanillaEngine::with_resolved_seed`] for replay examples.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors.
    fn price_with_seed(&self, option: &I, context: &PricingContext) -> Result<(f64, Option<u64>)> {
        Ok((self.price(option, context)?, None))
    }

    /// Returns a price with sampling uncertainty when the engine provides an estimator.
    ///
    /// See [`crate::MonteCarloVanillaEngine::price_with_statistics`] for an example
    /// and [`crate::MonteCarloEstimate`] for uncertainty conventions. The default
    /// implementation reports an unsupported operation.
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
    /// Uses configured finite-difference shifts or engine-default controls. Configure scalar differences explicitly with
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
