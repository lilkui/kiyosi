use crate::{Date, ErrorCategory, Result, Timestamp, error::finite};

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

/// Owning, validated continuous flat rates and strictly positive annual volatility.
#[derive(Clone, Debug, PartialEq)]
pub struct BlackScholesMertonParameters {
    risk_free_rate: f64,
    dividend_yield: f64,
    volatility: f64,
}

impl BlackScholesMertonParameters {
    pub fn new(risk_free_rate: f64, dividend_yield: f64, volatility: f64) -> Result<Self> {
        finite(risk_free_rate, ErrorCategory::InvalidRiskFreeRate, false)?;
        finite(dividend_yield, ErrorCategory::InvalidDividendYield, false)?;
        finite(volatility, ErrorCategory::InvalidVolatility, true)?;
        Ok(Self {
            risk_free_rate,
            dividend_yield,
            volatility,
        })
    }

    pub fn risk_free_rate(&self) -> f64 {
        self.risk_free_rate
    }
    pub fn dividend_yield(&self) -> f64 {
        self.dividend_yield
    }
    pub fn volatility(&self) -> f64 {
        self.volatility
    }
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
        Ok(Self {
            model_parameters,
            spot_price,
            valuation_time,
            calendar,
        })
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
