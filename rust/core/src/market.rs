use crate::{Date, Error, ErrorCategory, RateTermStructure, Result, Timestamp, error::finite};

/// Calendar identity used by the candidate context. Scheduling arrives in a later slice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TradingCalendar {
    #[default]
    Weekdays,
}

impl TradingCalendar {
    /// Existing weekdays-calendar annualization default.
    pub const fn trading_days_per_year(self) -> u32 {
        252
    }
}

/// A continuously compounded flat rate or an owning term structure.
/// Scalar inputs receive the model field's error category on validation.
#[derive(Clone, Debug, PartialEq)]
pub enum Rate {
    Flat(f64),
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
    fn into_curve(self, category: ErrorCategory) -> Result<RateTermStructure> {
        match self {
            Self::Flat(rate) => {
                finite(rate, category, false)?;
                RateTermStructure::flat(rate, None)
            }
            Self::Curve(curve) => Ok(curve),
        }
    }
}

/// Model replacements; absent fields retain their values.
#[derive(Clone, Debug, Default)]
pub struct BlackScholesMertonParameterChanges {
    pub risk_free_rate: Option<Rate>,
    pub dividend_yield: Option<Rate>,
    pub volatility: Option<f64>,
}

/// Owning, validated continuous rate curves and strictly positive annual volatility.
#[derive(Clone, Debug, PartialEq)]
pub struct BlackScholesMertonParameters {
    risk_free_curve: RateTermStructure,
    dividend_curve: RateTermStructure,
    volatility: f64,
}

impl BlackScholesMertonParameters {
    /// Accepts scalar rates, curves, or a mixture. Non-finite scalar rates return
    /// `InvalidRiskFreeRate` / `InvalidDividendYield`; invalid volatility returns `InvalidVolatility`.
    pub fn new(
        risk_free_rate: impl Into<Rate>,
        dividend_yield: impl Into<Rate>,
        volatility: f64,
    ) -> Result<Self> {
        let risk_free_curve = risk_free_rate
            .into()
            .into_curve(ErrorCategory::InvalidRiskFreeRate)?;
        let dividend_curve = dividend_yield
            .into()
            .into_curve(ErrorCategory::InvalidDividendYield)?;
        finite(volatility, ErrorCategory::InvalidVolatility, true)?;
        Ok(Self {
            risk_free_curve,
            dividend_curve,
            volatility,
        })
    }
    /// Returns a validated owning copy; failures leave this model unchanged.
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
    /// Explicit flat rate, or `UnsupportedOperation` for node curves.
    pub fn risk_free_rate(&self) -> Result<f64> {
        self.risk_free_curve.flat_rate().ok_or(Error::new(
            ErrorCategory::UnsupportedOperation,
            "risk-free curve is not explicitly flat",
        ))
    }
    /// Explicit flat yield, or `UnsupportedOperation` for node curves.
    pub fn dividend_yield(&self) -> Result<f64> {
        self.dividend_curve.flat_rate().ok_or(Error::new(
            ErrorCategory::UnsupportedOperation,
            "dividend curve is not explicitly flat",
        ))
    }
    /// Borrows the risk-free curve; storage remains valid for this model's lifetime.
    pub fn risk_free_curve(&self) -> &RateTermStructure {
        &self.risk_free_curve
    }
    /// Borrows the dividend curve; storage remains valid for this model's lifetime.
    pub fn dividend_curve(&self) -> &RateTermStructure {
        &self.dividend_curve
    }
    pub fn volatility(&self) -> f64 {
        self.volatility
    }
}

/// Context replacements; absent fields retain their values, including the calendar.
#[derive(Clone, Debug, Default)]
pub struct PricingContextChanges {
    pub model_parameters: Option<BlackScholesMertonParameters>,
    pub spot_price: Option<f64>,
    pub valuation_time: Option<Timestamp>,
    pub calendar: Option<TradingCalendar>,
}

/// Owning valuation snapshot. Contract-life validation occurs at pricing time.
#[derive(Clone, Debug, PartialEq)]
pub struct PricingContext {
    model_parameters: BlackScholesMertonParameters,
    spot_price: f64,
    valuation_time: Timestamp,
    calendar: TradingCalendar,
}

impl PricingContext {
    /// Creates a validated context using the core's default weekdays calendar.
    pub fn new(
        model_parameters: BlackScholesMertonParameters,
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

    pub fn with_calendar(
        model_parameters: BlackScholesMertonParameters,
        spot_price: f64,
        valuation_time: Timestamp,
        calendar: TradingCalendar,
    ) -> Result<Self> {
        finite(spot_price, ErrorCategory::InvalidSpotPrice, true)?;
        model_parameters
            .risk_free_curve()
            .validate_time(valuation_time)?;
        model_parameters
            .dividend_curve()
            .validate_time(valuation_time)?;
        Ok(Self {
            model_parameters,
            spot_price,
            valuation_time,
            calendar,
        })
    }

    /// Returns a validated owning copy. Valuation must be covered by both curves;
    /// contract-life and expiry coverage are checked at pricing time. Never mutates this context.
    pub fn replace(&self, changes: PricingContextChanges) -> Result<Self> {
        Self::with_calendar(
            changes
                .model_parameters
                .unwrap_or_else(|| self.model_parameters.clone()),
            changes.spot_price.unwrap_or(self.spot_price),
            changes.valuation_time.unwrap_or(self.valuation_time),
            changes.calendar.unwrap_or(self.calendar),
        )
    }

    pub fn model_parameters(&self) -> &BlackScholesMertonParameters {
        &self.model_parameters
    }
    pub fn spot_price(&self) -> f64 {
        self.spot_price
    }
    pub fn valuation_time(&self) -> Timestamp {
        self.valuation_time
    }
    pub fn valuation_date(&self) -> Date {
        self.valuation_time.date()
    }
    pub fn calendar(&self) -> TradingCalendar {
        self.calendar
    }
}
