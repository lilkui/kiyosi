use crate::{Date, Error, ErrorCategory, Result, error::finite};

/// Direction of a vanilla option payoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionType {
    Call,
    Put,
}

/// Owning, validated European vanilla contract. Expiry occurs at midnight UTC.
#[derive(Clone, Debug, PartialEq)]
pub struct EuropeanOption {
    option_type: OptionType,
    strike: f64,
    effective_date: Date,
    expiry_date: Date,
}

impl EuropeanOption {
    /// Creates a positive-strike contract with inclusive, ordered life dates.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Result<Self> {
        finite(strike, ErrorCategory::InvalidStrike, true)?;
        if expiry_date < effective_date {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "expiry precedes effective date",
            ));
        }
        Ok(Self {
            option_type,
            strike,
            effective_date,
            expiry_date,
        })
    }

    pub fn option_type(&self) -> OptionType {
        self.option_type
    }
    pub fn strike(&self) -> f64 {
        self.strike
    }
    pub fn effective_date(&self) -> Date {
        self.effective_date
    }
    pub fn expiry_date(&self) -> Date {
        self.expiry_date
    }
}
