use std::fmt;

/// Stable machine-readable domain rejection categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ErrorCategory {
    InvalidStrike = 2,
    InvalidVolatility = 3,
    InvalidRiskFreeRate = 4,
    InvalidDividendYield = 5,
    InvalidSpotPrice = 6,
    InvalidDate = 7,
    InvalidTimeRange = 8,
    InvalidResult = 9,
}

impl ErrorCategory {
    /// Categories implemented by this migration slice.
    pub const ALL: [Self; 8] = [
        Self::InvalidStrike,
        Self::InvalidVolatility,
        Self::InvalidRiskFreeRate,
        Self::InvalidDividendYield,
        Self::InvalidSpotPrice,
        Self::InvalidDate,
        Self::InvalidTimeRange,
        Self::InvalidResult,
    ];

    /// Stable category identifier.
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidStrike => "INVALID_STRIKE",
            Self::InvalidVolatility => "INVALID_VOLATILITY",
            Self::InvalidRiskFreeRate => "INVALID_RISK_FREE_RATE",
            Self::InvalidDividendYield => "INVALID_DIVIDEND_YIELD",
            Self::InvalidSpotPrice => "INVALID_SPOT_PRICE",
            Self::InvalidDate => "INVALID_DATE",
            Self::InvalidTimeRange => "INVALID_TIME_RANGE",
            Self::InvalidResult => "INVALID_RESULT",
        }
    }
}

/// A domain rejection. Diagnostics are for humans; inspect `category` in code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub category: ErrorCategory,
    pub message: &'static str,
}

impl Error {
    pub(crate) const fn new(category: ErrorCategory, message: &'static str) -> Self {
        Self { category, message }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl std::error::Error for Error {}

/// Domain result; not a guarantee against allocator failure or internal panics.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn finite(value: f64, category: ErrorCategory, positive: bool) -> Result<()> {
    if !value.is_finite() || (positive && value <= 0.0) {
        return Err(Error::new(
            category,
            "value must be finite and in its valid range",
        ));
    }
    Ok(())
}
