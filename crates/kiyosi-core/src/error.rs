use std::fmt;

/// Identifies stable domain rejection and reported operational failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    InvalidOption,
    InvalidStrike,
    InvalidVolatility,
    InvalidRiskFreeRate,
    InvalidDividendYield,
    InvalidSpotPrice,
    InvalidDate,
    InvalidTimeRange,
    InvalidResult,
    InvalidSchedule,
    InvalidCalendar,
    InvalidParameter,
    UnbracketedVolatility,
    SolverNonConvergence,
    SolverNonFinite,
    UnbracketedCoupon,
    /// Identifies an operational failure; inspect [`Error::operational_error`].
    BackendFailure,
    UnsupportedOperation,
}

impl ErrorCategory {
    /// Categories implemented by this migration slice.
    pub const ALL: [Self; 18] = [
        Self::InvalidOption,
        Self::InvalidStrike,
        Self::InvalidVolatility,
        Self::InvalidRiskFreeRate,
        Self::InvalidDividendYield,
        Self::InvalidSpotPrice,
        Self::InvalidDate,
        Self::InvalidTimeRange,
        Self::InvalidResult,
        Self::InvalidSchedule,
        Self::InvalidCalendar,
        Self::InvalidParameter,
        Self::UnbracketedVolatility,
        Self::SolverNonConvergence,
        Self::SolverNonFinite,
        Self::UnbracketedCoupon,
        Self::BackendFailure,
        Self::UnsupportedOperation,
    ];

    /// Stable category identifier.
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidOption => "INVALID_OPTION",
            Self::InvalidStrike => "INVALID_STRIKE",
            Self::InvalidVolatility => "INVALID_VOLATILITY",
            Self::InvalidRiskFreeRate => "INVALID_RISK_FREE_RATE",
            Self::InvalidDividendYield => "INVALID_DIVIDEND_YIELD",
            Self::InvalidSpotPrice => "INVALID_SPOT_PRICE",
            Self::InvalidDate => "INVALID_DATE",
            Self::InvalidTimeRange => "INVALID_TIME_RANGE",
            Self::InvalidResult => "INVALID_RESULT",
            Self::InvalidSchedule => "INVALID_SCHEDULE",
            Self::InvalidCalendar => "INVALID_CALENDAR",
            Self::InvalidParameter => "INVALID_PARAMETER",
            Self::UnbracketedVolatility => "UNBRACKETED_VOLATILITY",
            Self::SolverNonConvergence => "SOLVER_NON_CONVERGENCE",
            Self::SolverNonFinite => "SOLVER_NON_FINITE",
            Self::UnbracketedCoupon => "UNBRACKETED_COUPON",
            Self::BackendFailure => "BACKEND_FAILURE",
            Self::UnsupportedOperation => "UNSUPPORTED_OPERATION",
        }
    }
}

/// Describes a domain rejection or a recoverable operational failure.
/// Diagnostics are for humans; inspect `category` and [`Self::operational_error`] in code.
#[derive(Clone, Debug)]
pub struct Error {
    pub category: ErrorCategory,
    pub message: &'static str,
    operational: Option<std::sync::Arc<std::io::Error>>,
}

impl Error {
    pub(crate) const fn new(category: ErrorCategory, message: &'static str) -> Self {
        Self {
            category,
            message,
            operational: None,
        }
    }

    pub(crate) fn operational(error: std::io::Error) -> Self {
        Self {
            category: ErrorCategory::BackendFailure,
            message: "Monte Carlo resource failure",
            operational: Some(std::sync::Arc::new(error)),
        }
    }

    /// Returns the native entropy-source or thread-creation error, if present.
    /// Operational failures are separate from domain rejections; their kind and
    /// OS code remain available. See [`crate::MonteCarloVanillaEngine::price`].
    pub fn operational_error(&self) -> Option<&std::io::Error> {
        self.operational.as_deref()
    }
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        self.category == other.category
            && self.message == other.message
            && self
                .operational_error()
                .map(|e| (e.kind(), e.raw_os_error(), e.to_string()))
                == other
                    .operational_error()
                    .map(|e| (e.kind(), e.raw_os_error(), e.to_string()))
    }
}
impl Eq for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.operational_error() {
            Some(error) => error.fmt(f),
            None => f.write_str(self.message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.operational_error().map(|error| error as _)
    }
}

/// Returns a value, domain rejection or reported operational failure.
/// Native I/O details remain available through [`Error::operational_error`].
/// See [`crate::MonteCarloVanillaEngine::price`] for an example.
/// This type does not guarantee recovery from allocator failure or internal panics.
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
