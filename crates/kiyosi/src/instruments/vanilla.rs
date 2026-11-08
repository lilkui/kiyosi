use crate::{Error, Result, error::finite};
use crate::{PricingContext, Timestamp};

/// Selects the call or put payoff direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionType {
    /// Pays the positive excess of spot over strike.
    Call,
    /// Pays the positive excess of strike over spot.
    Put,
}

/// Owns a validated European vanilla contract.
///
/// Fixed strike and terminal settlement permit valuation before its effective
/// timestamp, subject to model coverage. See [`OptionContract::valuation_start`].
///
/// See [`Self::new`] for construction and [`crate::AnalyticEuropeanEngine`] for pricing.
#[derive(Clone, Debug, PartialEq)]
pub struct EuropeanOption {
    option_type: OptionType,
    strike: f64,
    effective_time: Timestamp,
    expiry_time: Timestamp,
}

/// Selects European contract replacements; absent fields retain their values.
///
/// See [`EuropeanOption::replace`] for an example.
#[derive(Clone, Copy, Debug, Default)]
pub struct EuropeanOptionChanges {
    /// Replaces the call-or-put direction.
    pub option_type: Option<OptionType>,
    /// Replaces the positive strike.
    pub strike: Option<f64>,
    /// Replaces the inclusive effective timestamp.
    pub effective_time: Option<Timestamp>,
    /// Replaces the inclusive expiry timestamp.
    pub expiry_time: Option<Timestamp>,
}

impl EuropeanOption {
    /// Creates a positive-strike contract between ordered timestamps.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite or nonpositive strike,
    /// then for reversed life times.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, EuropeanOption, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = EuropeanOption::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
    /// assert_eq!(option.strike(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
    ) -> Result<Self> {
        finite(strike, true, "strike")?;
        if expiry_time < effective_time {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        Ok(Self {
            option_type,
            strike,
            effective_time,
            expiry_time,
        })
    }

    /// Returns a validated replacement, leaving this contract unchanged.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation for the resulting contract.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, EuropeanOption, EuropeanOptionChanges, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = EuropeanOption::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
    /// let changed = option.replace(EuropeanOptionChanges {
    ///     strike: Some(105.0), ..Default::default()
    /// })?;
    /// assert_eq!(changed.strike(), 105.0);
    /// assert_eq!(option.strike(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, changes: EuropeanOptionChanges) -> Result<Self> {
        Self::new(
            changes.option_type.unwrap_or(self.option_type),
            changes.strike.unwrap_or(self.strike),
            changes.effective_time.unwrap_or(self.effective_time),
            changes.expiry_time.unwrap_or(self.expiry_time),
        )
    }
    /// Returns the payoff direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.option_type
    }
    /// Returns the positive strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.strike
    }
    /// Returns the inclusive effective time; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.effective_time
    }
    /// Returns the inclusive expiry timestamp; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.expiry_time
    }
}

/// Exposes contract times and permitted valuation bounds for numerical analysis and solving.
///
/// Implementations retain their own payoff; engines select supported contract types.
/// See [`DigitalOption::cash_or_nothing`] for an example contract.
pub trait OptionContract {
    /// Returns the inclusive effective time. See [`DigitalOption::effective_time`].
    fn effective_time(&self) -> Timestamp;
    /// Returns the inclusive expiry timestamp. See [`DigitalOption::expiry_time`].
    fn expiry_time(&self) -> Timestamp;

    /// Returns the timestamp through which supplied history is known, if dated.
    ///
    /// History describes events strictly before this timestamp. See [`Self::with_history_time`].
    fn history_time(&self) -> Option<Timestamp> {
        None
    }

    /// Dates supplied history at `time`, consuming and returning this contract.
    ///
    /// Built-in path-dependent contracts require dated history at pricing. Reuse across
    /// observations is rejected; continuously accumulated history requires the same instant.
    /// Dates after valuation are rejected at pricing. Contracts without history reject
    /// a supplied timestamp; clearing with `None` retains their value.
    /// See the [crate-level history example](crate).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for a supplied timestamp when history
    /// is unsupported, or [`Error::InvalidInput`] for a supported timestamp outside life.
    fn with_history_time(self, time: Option<Timestamp>) -> Result<Self>
    where
        Self: Sized,
    {
        if time.is_some() {
            return Err(Error::UnsupportedOperation(
                "this contract does not support history timestamps".into(),
            ));
        }
        Ok(self)
    }

    /// Returns the earliest valuation time, or `None` when pre-effective valuation is valid.
    ///
    /// European vanilla and digital contracts allow earlier valuation. Other built-in contracts
    /// use the default effective time. Custom contracts may
    /// allow earlier valuation by returning `None`; see [`Self::effective_time`].
    fn valuation_start(&self) -> Option<Timestamp> {
        Some(self.effective_time())
    }

    /// Reports whether every Greek is unavailable at a pre-expiry spot discontinuity.
    ///
    /// Defaults to deterministic BSM discontinuities from [`Self::at_spot_discontinuity`]
    /// with zero radius. Current barrier touches retain cash-flow parameter Greeks;
    /// see [`crate::AnalyticBarrierEngine::price_with_greeks`].
    /// Expiry Greeks use terminal-payoff rules in [`crate::numerical_greeks`].
    fn all_greeks_unavailable(&self, context: &crate::PricingContext) -> bool {
        context
            .model_parameters()
            .as_bsm()
            .is_ok_and(|model| model.volatility() == 0.0)
            && self.at_spot_discontinuity(context, 0.0)
    }

    /// Reports whether a spot interval crosses a payoff discontinuity or derivative kink.
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
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn valuation_start(&self) -> Option<Timestamp> {
        None
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        (context.valuation_time() == self.expiry_time()
            && context.spot_price() - radius <= self.strike()
            && self.strike() <= context.spot_price() + radius)
            || context.deterministic_crosses(self.expiry_time(), self.strike(), radius)
    }
}

/// Owns a validated European cash-or-nothing or asset-or-nothing contract.
///
/// Both pay only strictly in the money; equality at expiry pays zero.
/// Fixed terminal settlement permits valuation before the effective timestamp,
/// subject to market coverage; see [`OptionContract::valuation_start`].
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
    /// Replaces the inclusive effective time.
    pub effective_time: Option<Timestamp>,
    /// Replaces the inclusive expiry timestamp.
    pub expiry_time: Option<Timestamp>,
}

impl DigitalOption {
    /// Creates a European contract paying fixed cash strictly in the money.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite payout,
    /// invalid strike, or reversed times, checked in that order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, DigitalOption, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
    /// assert_eq!(option.payout(), Some(10.0));
    /// assert_eq!(option.option_type(), OptionType::Call);
    /// assert_eq!(option.strike(), 100.0);
    /// assert_eq!(option.effective_time(), Date::new(2025, 1, 1)?.start_of_day());
    /// assert_eq!(option.expiry_time(), Date::new(2026, 1, 1)?.start_of_day());
    /// # Ok(()) }
    /// ```
    pub fn cash_or_nothing(
        option_type: OptionType,
        strike: f64,
        payout: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
    ) -> Result<Self> {
        finite(payout, true, "payout")?;
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_time, expiry_time)?,
            payout: Some(payout),
        })
    }

    /// Creates a European contract paying the terminal asset strictly in the money.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite strike,
    /// then for reversed times.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, DigitalOption, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = DigitalOption::asset_or_nothing(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset_or_nothing(
        option_type: OptionType,
        strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
    ) -> Result<Self> {
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_time, expiry_time)?,
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
    /// use kiyosi::{Date, DigitalOption, DigitalOptionChanges, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let original = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
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
        let start = changes.effective_time.unwrap_or(self.effective_time());
        let end = changes.expiry_time.unwrap_or(self.expiry_time());
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
    /// Returns the effective time. See [`Self::cash_or_nothing`] for an example.
    pub fn effective_time(&self) -> Timestamp {
        self.terms.effective_time()
    }
    /// Returns the expiry time. See [`Self::cash_or_nothing`] for an example.
    pub fn expiry_time(&self) -> Timestamp {
        self.terms.expiry_time()
    }
    /// Returns fixed cash, or `None` for asset settlement. See [`Self::asset_or_nothing`].
    pub fn payout(&self) -> Option<f64> {
        self.payout
    }
}

impl OptionContract for DigitalOption {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn valuation_start(&self) -> Option<Timestamp> {
        None
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.terms.at_spot_discontinuity(context, radius)
    }
}

/// Owns vanilla terms exercisable at any time during the inclusive contract life.
///
/// Expiry occurs at the supplied timestamp. See [`Self::new`] for an example.
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
    /// Replaces the inclusive first exercise time.
    pub effective_time: Option<Timestamp>,
    /// Replaces the inclusive last exercise timestamp.
    pub expiry_time: Option<Timestamp>,
}

impl AmericanOption {
    pub(crate) fn vanilla(&self) -> &EuropeanOption {
        &self.terms
    }

    /// Creates an American vanilla contract with ordered, inclusive exercise times.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for nonpositive or non-finite strike,
    /// then for reversed times.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AmericanOption, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = AmericanOption::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
    /// assert_eq!(option.option_type(), OptionType::Put);
    /// assert_eq!(option.strike(), 100.0);
    /// assert_eq!(option.effective_time(), Date::new(2025, 1, 1)?.start_of_day());
    /// assert_eq!(option.expiry_time(), Date::new(2026, 1, 1)?.start_of_day());
    /// # Ok(()) }
    /// ```
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
    ) -> Result<Self> {
        Ok(Self {
            terms: EuropeanOption::new(option_type, strike, effective_time, expiry_time)?,
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
    /// use kiyosi::{AmericanOption, AmericanOptionChanges, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let original = AmericanOption::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2026, 1, 1)?.start_of_day())?;
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
            changes.effective_time.unwrap_or(self.effective_time()),
            changes.expiry_time.unwrap_or(self.expiry_time()),
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
    /// Returns the first exercise time. See [`Self::new`] for an example.
    pub fn effective_time(&self) -> Timestamp {
        self.terms.effective_time()
    }
    /// Returns the last exercise time. See [`Self::new`] for an example.
    pub fn expiry_time(&self) -> Timestamp {
        self.terms.expiry_time()
    }
}

impl OptionContract for AmericanOption {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.terms.at_spot_discontinuity(context, radius)
    }
}
