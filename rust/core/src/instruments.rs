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

/// Exposes the inclusive life of a European contract for numerical analysis and solving.
/// Implementations retain their own payoff; engines select supported contract types.
/// See [`DigitalOption::cash_or_nothing`] for an example contract.
pub trait EuropeanContract {
    /// Returns the inclusive first date. See [`DigitalOption::effective_date`].
    fn effective_date(&self) -> Date;
    /// Returns expiry at midnight UTC. See [`DigitalOption::expiry_date`].
    fn expiry_date(&self) -> Date;
}

impl EuropeanContract for EuropeanOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
}

/// Owns a validated European cash-or-nothing or asset-or-nothing contract.
/// Both pay only strictly in the money; equality at expiry pays zero.
/// See [`Self::cash_or_nothing`] and [`Self::asset_or_nothing`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct DigitalOption {
    terms: EuropeanOption,
    payout: Option<f64>,
}

/// Selects digital contract replacements; absent fields retain their values.
/// See [`DigitalOption::replace`] for an example.
#[derive(Clone, Copy, Debug, Default)]
pub struct DigitalOptionChanges {
    /// Replaces the call-or-put direction.
    pub option_type: Option<OptionType>,
    /// Replaces the positive strike.
    pub strike: Option<f64>,
    /// Replaces the positive cash payout; unsupported for asset settlement.
    pub payout: Option<f64>,
    /// Replaces the inclusive first date.
    pub effective_date: Option<Date>,
    /// Replaces the inclusive expiry date, at midnight UTC.
    pub expiry_date: Option<Date>,
}

impl DigitalOption {
    /// Creates a European contract paying fixed cash strictly in the money.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for nonpositive or non-finite
    /// payout, then [`ErrorCategory::InvalidStrike`] for invalid strike, or
    /// [`ErrorCategory::InvalidTimeRange`] for reversed dates, in that order.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{Date, DigitalOption, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?)?;
    /// assert_eq!(option.payout(), Some(10.0));
    /// assert_eq!(option.option_type(), OptionType::Call);
    /// assert_eq!(option.strike(), 100.0);
    /// assert_eq!(option.effective_date(), Date::new(2025, 1, 1)?);
    /// assert_eq!(option.expiry_date(), Date::new(2026, 1, 1)?);
    /// # Ok(()) }
    /// ```
    pub fn cash_or_nothing(
        option_type: OptionType,
        strike: f64,
        payout: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Result<Self> {
        finite(payout, ErrorCategory::InvalidParameter, true)?;
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_date, expiry_date)?,
            payout: Some(payout),
        })
    }

    /// Creates a European contract paying the terminal asset strictly in the money.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidStrike`] for nonpositive or non-finite strike,
    /// or [`ErrorCategory::InvalidTimeRange`] for reversed dates.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{Date, DigitalOption, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let option = DigitalOption::asset_or_nothing(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?)?;
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset_or_nothing(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Result<Self> {
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_date, expiry_date)?,
            payout: None,
        })
    }

    /// Returns an owning replacement, preserving the settlement kind and the original.
    ///
    /// # Errors
    /// Reuses constructor validation. Setting cash payout on an asset contract returns
    /// [`ErrorCategory::UnsupportedOperation`]. Failure leaves the original unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{Date, DigitalOption, DigitalOptionChanges, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let original = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?)?;
    /// let updated = original.replace(DigitalOptionChanges {
    ///     payout: Some(7.0), ..Default::default()
    /// })?;
    /// assert_eq!(original.payout(), Some(10.0));
    /// assert_eq!(updated.payout(), Some(7.0));
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, changes: DigitalOptionChanges) -> Result<Self> {
        let direction = changes.option_type.unwrap_or(self.option_type());
        let strike = changes.strike.unwrap_or(self.strike());
        let start = changes.effective_date.unwrap_or(self.effective_date());
        let end = changes.expiry_date.unwrap_or(self.expiry_date());
        if let Some(payout) = self.payout {
            Self::cash_or_nothing(
                direction,
                strike,
                changes.payout.unwrap_or(payout),
                start,
                end,
            )
        } else if changes.payout.is_some() {
            Err(Error::new(
                ErrorCategory::UnsupportedOperation,
                "asset settlement has no cash payout",
            ))
        } else {
            Self::asset_or_nothing(direction, strike, start, end)
        }
    }

    /// Returns the direction. See [`Self::cash_or_nothing`] for an example.
    pub fn option_type(&self) -> OptionType {
        self.terms.option_type()
    }
    /// Returns the strike. See [`Self::cash_or_nothing`] for an example.
    pub fn strike(&self) -> f64 {
        self.terms.strike()
    }
    /// Returns the effective date. See [`Self::cash_or_nothing`] for an example.
    pub fn effective_date(&self) -> Date {
        self.terms.effective_date()
    }
    /// Returns the expiry date. See [`Self::cash_or_nothing`] for an example.
    pub fn expiry_date(&self) -> Date {
        self.terms.expiry_date()
    }
    /// Returns fixed cash, or `None` for asset settlement. See [`Self::asset_or_nothing`].
    pub fn payout(&self) -> Option<f64> {
        self.payout
    }
}

impl EuropeanContract for DigitalOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
}
