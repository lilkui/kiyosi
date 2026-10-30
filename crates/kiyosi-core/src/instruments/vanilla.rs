use crate::{Date, Error, Result, error::finite};

/// Selects the call or put payoff direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionType {
    /// Pays the positive excess of spot over strike.
    Call,
    /// Pays the positive excess of strike over spot.
    Put,
}

/// Owns a validated European vanilla contract expiring at midnight UTC.
///
/// See [`Self::new`] for construction and [`crate::AnalyticEuropeanEngine`] for pricing.
#[derive(Clone, Debug, PartialEq)]
pub struct EuropeanOption {
    option_type: OptionType,
    strike: f64,
    effective_date: Date,
    expiry_date: Date,
}

impl EuropeanOption {
    /// Creates a positive-strike contract with inclusive, ordered life dates.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite or nonpositive strike,
    /// then for reversed life dates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{Date, EuropeanOption, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let option = EuropeanOption::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?)?;
    /// assert_eq!(option.strike(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Result<Self> {
        finite(strike, true)?;
        if expiry_date < effective_date {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        Ok(Self {
            option_type,
            strike,
            effective_date,
            expiry_date,
        })
    }

    /// Returns the payoff direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.option_type
    }
    /// Returns the positive strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.strike
    }
    /// Returns the inclusive first date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.effective_date
    }
    /// Returns expiry at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.expiry_date
    }
}

/// Exposes contract dates and permitted valuation bounds for numerical analysis and solving.
///
/// Implementations retain their own payoff; engines select supported contract types.
/// See [`DigitalOption::cash_or_nothing`] for an example contract.
pub trait OptionContract {
    /// Returns the inclusive first date. See [`DigitalOption::effective_date`].
    fn effective_date(&self) -> Date;
    /// Returns expiry at midnight UTC. See [`DigitalOption::expiry_date`].
    fn expiry_date(&self) -> Date;

    /// Returns the earliest valuation date, or `None` when pre-effective valuation is valid.
    ///
    /// Defaults to the effective date; see [`crate::AnalyticRangeAccrualEngine::price`].
    fn valuation_start(&self) -> Option<Date> {
        Some(self.effective_date())
    }

    /// Reports whether every Greek is undefined at the current spot discontinuity.
    ///
    /// Defaults to [`Self::at_spot_discontinuity`] with zero radius. Range coupons retain
    /// vega and rho; see [`crate::AnalyticRangeAccrualEngine::price_with_greeks`].
    fn all_greeks_unavailable(&self, context: &crate::PricingContext) -> bool {
        self.at_spot_discontinuity(context, 0.0)
    }

    /// Reports whether a spot interval crosses a current payoff discontinuity.
    ///
    /// Defaults to `false`. Barrier contracts suppress undefined sensitivities at
    /// a current touch and stencils reaching it; see [`crate::AnalyticBarrierEngine::price_with_greeks`].
    fn at_spot_discontinuity(&self, _context: &crate::PricingContext, _radius: f64) -> bool {
        false
    }

    /// Reports whether a time stencil preserves the supplied history regime.
    ///
    /// Defaults to `true`; Asian contracts prevent crossing their averaging start.
    /// See [`crate::AnalyticGeometricAveragePriceEngine::price_with_greeks`].
    fn time_stencil_available(
        &self,
        _context: &crate::PricingContext,
        _before: crate::Timestamp,
        _after: crate::Timestamp,
    ) -> bool {
        true
    }
}

impl OptionContract for EuropeanOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
}

/// Owns a validated European cash-or-nothing or asset-or-nothing contract.
///
/// Both pay only strictly in the money; equality at expiry pays zero.
/// See [`Self::cash_or_nothing`] and [`Self::asset_or_nothing`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct DigitalOption {
    terms: EuropeanOption,
    payout: Option<f64>,
}

/// Selects digital contract replacements; absent fields retain their values.
///
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
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite payout,
    /// invalid strike, or reversed dates, checked in that order.
    ///
    /// # Examples
    ///
    /// ```rust
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
        finite(payout, true)?;
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_date, expiry_date)?,
            payout: Some(payout),
        })
    }

    /// Creates a European contract paying the terminal asset strictly in the money.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite strike,
    /// then for reversed dates.
    ///
    /// # Examples
    ///
    /// ```rust
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
    ///
    /// Reuses constructor validation. Setting cash payout on an asset contract returns
    /// [`Error::UnsupportedOperation`]. Failure leaves the original unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
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
            Err(Error::UnsupportedOperation(
                "asset settlement has no cash payout".into(),
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

impl OptionContract for DigitalOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
}

/// Owns vanilla terms exercisable at any time during the inclusive contract life.
///
/// Expiry occurs at midnight UTC. See [`Self::new`] for an example.
#[derive(Clone, Debug, PartialEq)]
pub struct AmericanOption {
    terms: EuropeanOption,
}

/// Selects American contract replacements; absent fields retain their values.
///
/// See [`AmericanOption::replace`] for an example.
#[derive(Clone, Copy, Debug, Default)]
pub struct AmericanOptionChanges {
    /// Replaces the call-or-put direction.
    pub option_type: Option<OptionType>,
    /// Replaces the positive strike.
    pub strike: Option<f64>,
    /// Replaces the inclusive first exercise date.
    pub effective_date: Option<Date>,
    /// Replaces the inclusive last exercise date, at midnight UTC.
    pub expiry_date: Option<Date>,
}

impl AmericanOption {
    pub(crate) fn vanilla(&self) -> &EuropeanOption {
        &self.terms
    }

    /// Creates an American vanilla contract with ordered, inclusive exercise dates.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite strike,
    /// then for reversed dates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{AmericanOption, Date, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let option = AmericanOption::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?)?;
    /// assert_eq!(option.option_type(), OptionType::Put);
    /// assert_eq!(option.strike(), 100.0);
    /// assert_eq!(option.effective_date(), Date::new(2025, 1, 1)?);
    /// assert_eq!(option.expiry_date(), Date::new(2026, 1, 1)?);
    /// # Ok(()) }
    /// ```
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Result<Self> {
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_date, expiry_date)?,
        })
    }

    /// Returns a validated owning replacement, leaving the original unchanged.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation. Failure leaves the original unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{AmericanOption, AmericanOptionChanges, Date, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let original = AmericanOption::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?)?;
    /// let updated = original.replace(AmericanOptionChanges {
    ///     strike: Some(105.0), ..Default::default()
    /// })?;
    /// assert_eq!(updated.strike(), 105.0);
    /// assert_eq!(original.strike(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, changes: AmericanOptionChanges) -> Result<Self> {
        Self::new(
            changes.option_type.unwrap_or(self.option_type()),
            changes.strike.unwrap_or(self.strike()),
            changes.effective_date.unwrap_or(self.effective_date()),
            changes.expiry_date.unwrap_or(self.expiry_date()),
        )
    }
    /// Returns the direction. See [`Self::new`] for an example.
    pub fn option_type(&self) -> OptionType {
        self.terms.option_type()
    }
    /// Returns the strike. See [`Self::new`] for an example.
    pub fn strike(&self) -> f64 {
        self.terms.strike()
    }
    /// Returns the first exercise date. See [`Self::new`] for an example.
    pub fn effective_date(&self) -> Date {
        self.terms.effective_date()
    }
    /// Returns the last exercise date. See [`Self::new`] for an example.
    pub fn expiry_date(&self) -> Date {
        self.terms.expiry_date()
    }
}

impl OptionContract for AmericanOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
}
