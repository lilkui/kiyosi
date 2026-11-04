use crate::{Date, Error, RateTermStructure, Result, Timestamp, TradingCalendar, error::finite};

/// A continuously compounded flat rate or an owning term structure.
///
/// Non-finite scalar inputs return [`Error::InvalidInput`] on validation.
/// See [`BlackScholesMertonParameters::new`] for mixing scalar rates and curves.
#[derive(Clone, Debug, PartialEq)]
pub enum Rate {
    /// Supplies a continuously compounded scalar rate, validated by the model.
    Flat(f64),
    /// Supplies an owned, validated rate curve.
    Curve(RateTermStructure),
}

impl From<f64> for Rate {
    fn from(rate: f64) -> Self {
        Self::Flat(rate)
    }
}
impl From<RateTermStructure> for Rate {
    fn from(curve: RateTermStructure) -> Self {
        Self::Curve(curve)
    }
}
impl Rate {
    pub(super) fn into_curve(self) -> Result<RateTermStructure> {
        match self {
            Self::Flat(rate) => RateTermStructure::flat(rate, None),
            Self::Curve(curve) => Ok(curve),
        }
    }
}

/// Selects model replacements; absent fields retain their values.
///
/// See [`BlackScholesMertonParameters::replace`].
#[derive(Clone, Debug, Default)]
pub struct BlackScholesMertonParameterChanges {
    /// Replaces the risk-free rate or curve.
    pub risk_free_rate: Option<Rate>,
    /// Replaces the dividend yield or curve.
    pub dividend_yield: Option<Rate>,
    /// Replaces the strictly positive annualized volatility in decimal units.
    pub volatility: Option<f64>,
}

/// Owns Black-Scholes-Merton (BSM) rate curves and annualized log-return volatility.
///
/// Volatility is strictly positive and expressed in decimal units.
/// Zero volatility is outside this model's input domain, including at expiry.
/// For exact deterministic vanilla pricing, use [`crate::HestonParameters`] with
/// zero initial variance, long-run variance and volatility of volatility, and a
/// Heston engine. See [`crate::AnalyticHestonEuropeanEngine::new`] for setup.
///
/// See [`Self::new`] for construction.
#[derive(Clone, Debug, PartialEq)]
pub struct BlackScholesMertonParameters {
    risk_free_curve: RateTermStructure,
    dividend_curve: RateTermStructure,
    volatility: f64,
}

impl BlackScholesMertonParameters {
    /// Creates a model from scalar rates, curves or a mixture.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite scalar rates or nonpositive
    /// or non-finite volatility. Risk-free rate validation precedes dividend
    /// yield and volatility validation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BlackScholesMertonParameters, Date, RateTermStructure};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let discounts = RateTermStructure::new(start, vec![start.add_days(180)?, end], vec![0.98, 0.95])?;
    /// let model = BlackScholesMertonParameters::new(discounts, 0.02, 0.2)?;
    /// assert!((model.risk_free_curve().factor(start, end)? - 0.95).abs() < 1e-14);
    /// assert_eq!(model.flat_dividend_yield()?, 0.02);
    /// assert!(model.flat_risk_free_rate().is_err()); // This curve has varying forwards.
    /// # Ok(()) }
    /// ```
    pub fn new(
        risk_free_rate: impl Into<Rate>,
        dividend_yield: impl Into<Rate>,
        volatility: f64,
    ) -> Result<Self> {
        let risk_free_curve = risk_free_rate.into().into_curve()?;
        let dividend_curve = dividend_yield.into().into_curve()?;
        finite(volatility, true)?;
        Ok(Self {
            risk_free_curve,
            dividend_curve,
            volatility,
        })
    }
    /// Returns a validated replacement, leaving this model unchanged.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation for the resulting model.
    /// See [`crate::implied_volatility`] for an example of changing volatility.
    pub fn replace(&self, changes: BlackScholesMertonParameterChanges) -> Result<Self> {
        Self::new(
            changes
                .risk_free_rate
                .unwrap_or_else(|| self.risk_free_curve.clone().into()),
            changes
                .dividend_yield
                .unwrap_or_else(|| self.dividend_curve.clone().into()),
            changes.volatility.unwrap_or(self.volatility),
        )
    }
    /// Returns the constant risk-free rate.
    ///
    /// This scalar omits coverage bounds; use [`Self::risk_free_curve`] when
    /// reconstructing a model.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for curves with nonconstant forwards. See [`Self::new`] for scalar-rate construction.
    pub fn flat_risk_free_rate(&self) -> Result<f64> {
        self.risk_free_curve
            .flat_rate()
            .ok_or(Error::UnsupportedOperation(
                "risk-free curve is not constant".into(),
            ))
    }
    /// Returns the constant dividend yield.
    ///
    /// This scalar omits coverage bounds; use [`Self::dividend_curve`] when
    /// reconstructing a model.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for curves with nonconstant forwards. See [`Self::new`] for scalar-rate construction.
    pub fn flat_dividend_yield(&self) -> Result<f64> {
        self.dividend_curve
            .flat_rate()
            .ok_or(Error::UnsupportedOperation(
                "dividend curve is not constant".into(),
            ))
    }
    /// Borrows the risk-free curve; storage remains valid for this model's lifetime.
    ///
    /// See [`Self::new`] for a curve-based model example.
    pub fn risk_free_curve(&self) -> &RateTermStructure {
        &self.risk_free_curve
    }
    /// Borrows the dividend curve; storage remains valid for this model's lifetime.
    ///
    /// Scalar yields become undated flat curves; see [`Self::new`].
    pub fn dividend_curve(&self) -> &RateTermStructure {
        &self.dividend_curve
    }
    /// Returns annualized volatility in decimal units; see [`Self::new`].
    pub fn volatility(&self) -> f64 {
        self.volatility
    }
}

/// Selects a concrete pricing model at the valuation boundary.
///
/// Numerical engines dispatch before entering their hot loops. See
/// [`crate::LocalVolatilityParameters::new`] for a local-volatility context example.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelParameters {
    /// Supplies BSM parameters; see [`BlackScholesMertonParameters::new`].
    BlackScholesMerton(BlackScholesMertonParameters),
    /// Supplies local volatility; see [`crate::LocalVolatilityParameters::new`].
    LocalVolatility(crate::LocalVolatilityParameters),
    /// Supplies stochastic variance; see [`crate::HestonParameters::new`].
    Heston(crate::HestonParameters),
}

impl From<BlackScholesMertonParameters> for ModelParameters {
    fn from(value: BlackScholesMertonParameters) -> Self {
        Self::BlackScholesMerton(value)
    }
}
impl From<crate::LocalVolatilityParameters> for ModelParameters {
    fn from(value: crate::LocalVolatilityParameters) -> Self {
        Self::LocalVolatility(value)
    }
}
impl From<crate::HestonParameters> for ModelParameters {
    fn from(value: crate::HestonParameters) -> Self {
        Self::Heston(value)
    }
}
impl ModelParameters {
    /// Borrows Heston parameters without changing the model; see [`crate::HestonParameters::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for other models.
    pub fn as_heston(&self) -> Result<&crate::HestonParameters> {
        match self {
            Self::Heston(model) => Ok(model),
            _ => Err(Error::UnsupportedOperation(
                "operation requires the Heston model".into(),
            )),
        }
    }
    /// Borrows BSM parameters without changing the model.
    ///
    /// See [`BlackScholesMertonParameters::new`] for a BSM example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for a non-BSM model.
    pub fn as_bsm(&self) -> Result<&BlackScholesMertonParameters> {
        match self {
            Self::BlackScholesMerton(model) => Ok(model),
            Self::LocalVolatility(_) | Self::Heston(_) => Err(Error::UnsupportedOperation(
                "operation requires the BSM model".into(),
            )),
        }
    }
    /// Borrows the risk-free curve; see [`PricingContext::new`] for an example.
    pub fn risk_free_curve(&self) -> &RateTermStructure {
        match self {
            Self::BlackScholesMerton(m) => m.risk_free_curve(),
            Self::LocalVolatility(m) => m.risk_free_curve(),
            Self::Heston(m) => m.risk_free_curve(),
        }
    }
    /// Borrows the dividend curve; see [`PricingContext::new`] for an example.
    pub fn dividend_curve(&self) -> &RateTermStructure {
        match self {
            Self::BlackScholesMerton(m) => m.dividend_curve(),
            Self::LocalVolatility(m) => m.dividend_curve(),
            Self::Heston(m) => m.dividend_curve(),
        }
    }
}

/// Selects context replacements; absent fields retain their values.
///
/// See [`PricingContext::replace`].
#[derive(Clone, Debug, Default)]
pub struct PricingContextChanges {
    /// Replaces the model parameters.
    pub model_parameters: Option<ModelParameters>,
    /// Replaces the positive spot price.
    pub spot_price: Option<f64>,
    /// Replaces the valuation instant.
    pub valuation_time: Option<Timestamp>,
    /// Replaces the trading calendar.
    pub calendar: Option<TradingCalendar>,
}

/// Owns a validated valuation snapshot.
///
/// Contract-life validation occurs at pricing time; see [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct PricingContext {
    model_parameters: ModelParameters,
    spot_price: f64,
    valuation_time: Timestamp,
    calendar: TradingCalendar,
}

impl PricingContext {
    /// Creates a context using the default weekday calendar with a 252-day basis.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::with_calendar`] spot and curve-coverage validation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{BlackScholesMertonParameters, Date, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    /// let context = PricingContext::new(model, 100.0, Date::new(2025, 1, 1)?.start_of_day().into())?;
    /// assert_eq!(context.spot_price(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn new(
        model_parameters: impl Into<ModelParameters>,
        spot_price: f64,
        valuation_time: Timestamp,
    ) -> Result<Self> {
        Self::with_calendar(
            model_parameters,
            spot_price,
            valuation_time,
            TradingCalendar::default(),
        )
    }

    /// Creates a validated context with an explicit calendar.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite or nonpositive spot,
    /// then for valuation outside either curve or the local-volatility surface coverage.
    /// See [`Self::new`] for a construction example.
    pub fn with_calendar(
        model_parameters: impl Into<ModelParameters>,
        spot_price: f64,
        valuation_time: Timestamp,
        calendar: TradingCalendar,
    ) -> Result<Self> {
        let model_parameters = model_parameters.into();
        finite(spot_price, true)?;
        model_parameters
            .risk_free_curve()
            .validate_time(valuation_time)?;
        model_parameters
            .dividend_curve()
            .validate_time(valuation_time)?;
        if let ModelParameters::LocalVolatility(model) = &model_parameters {
            model.surface().validate_time(valuation_time)?;
        }
        Ok(Self {
            model_parameters,
            spot_price,
            valuation_time,
            calendar,
        })
    }

    /// Returns a validated replacement, leaving this context unchanged.
    ///
    /// Contract-life and expiry coverage are checked at pricing time.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::with_calendar`] spot and valuation-coverage validation.
    /// See [`Self::new`] for context construction.
    pub fn replace(&self, changes: PricingContextChanges) -> Result<Self> {
        Self::with_calendar(
            changes
                .model_parameters
                .unwrap_or_else(|| self.model_parameters.clone()),
            changes.spot_price.unwrap_or(self.spot_price),
            changes.valuation_time.unwrap_or(self.valuation_time),
            changes.calendar.unwrap_or_else(|| self.calendar.clone()),
        )
    }

    /// Borrows the concrete model selection; see [`Self::new`].
    ///
    /// Match [`ModelParameters`] or use [`ModelParameters::as_bsm`] before reading
    /// BSM fields. Python returns an owned instance of the concrete model type.
    pub fn model_parameters(&self) -> &ModelParameters {
        &self.model_parameters
    }
    pub(crate) fn bsm_parameters(&self) -> Result<&BlackScholesMertonParameters> {
        self.model_parameters.as_bsm()
    }
    pub(crate) fn validate_curve_horizon(&self, expiry: Timestamp) -> Result<()> {
        self.model_parameters
            .risk_free_curve()
            .validate_horizon(self.valuation_time, expiry)?;
        self.model_parameters
            .dividend_curve()
            .validate_horizon(self.valuation_time, expiry)
    }
    /// Returns the positive spot price; see [`Self::new`].
    pub fn spot_price(&self) -> f64 {
        self.spot_price
    }
    /// Returns the valuation instant; see [`Self::new`].
    pub fn valuation_time(&self) -> Timestamp {
        self.valuation_time
    }
    /// Returns the date containing the valuation instant; see [`Self::new`].
    pub fn valuation_date(&self) -> Date {
        self.valuation_time.date()
    }
    /// Borrows the trading calendar; see [`Self::with_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        &self.calendar
    }
}
