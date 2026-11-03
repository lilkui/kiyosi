use crate::numerics::checked;
use crate::{
    Date, Error, EuropeanOption, OptionContract, OptionType, PricingContext, Result, Timestamp,
};

/// Denotes no realized average before averaging starts; see [`GeometricAveragePriceOption::new`].
pub const DEFAULT_REALIZED_AVERAGE: f64 = 0.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AverageTerms {
    pub(crate) terms: EuropeanOption,
    pub(crate) averaging_start: Date,
    pub(crate) realized: f64,
}

impl AverageTerms {
    pub(crate) fn coverage(&self, context: &PricingContext) -> Result<()> {
        let expiry = self.terms.expiry_date().start_of_day();
        context
            .bsm_parameters()?
            .risk_free_curve()
            .validate_horizon(context.valuation_time(), expiry)?;
        context
            .bsm_parameters()?
            .dividend_curve()
            .validate_horizon(context.valuation_time(), expiry)
    }
    pub(crate) fn settlement(&self, context: &PricingContext) -> Result<f64> {
        checked(
            (self.sign()
                * (if self.realized > 0.0 {
                    self.realized
                } else {
                    context.spot_price()
                } - self.terms.strike()))
            .max(0.0),
        )
    }
    fn new(
        direction: OptionType,
        strike: f64,
        start: Date,
        averaging: Date,
        end: Date,
        realized: f64,
    ) -> Result<Self> {
        if !realized.is_finite() || realized < 0.0 {
            return Err(Error::InvalidInput(
                "realized average must be finite and non-negative".into(),
            ));
        }
        let terms = EuropeanOption::new(direction, strike, start, end)?;
        if averaging < start || averaging > end {
            return Err(Error::InvalidInput(
                "averaging start must be within contract life".into(),
            ));
        }
        Ok(Self {
            terms,
            averaging_start: averaging,
            realized,
        })
    }
    pub(crate) fn validate(&self, context: &PricingContext) -> Result<f64> {
        let valuation = context.valuation_time();
        let expiry = self.terms.expiry_date().start_of_day();
        if valuation < self.terms.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        if (valuation > self.averaging_start.start_of_day()) == (self.realized == 0.0) {
            return Err(Error::InvalidInput(
                "realized average must match the elapsed averaging period".into(),
            ));
        }
        Ok(valuation.years_until(expiry))
    }
    pub(crate) fn period(&self) -> f64 {
        self.averaging_start
            .start_of_day()
            .years_until(self.terms.expiry_date().start_of_day())
    }
    pub(crate) fn sign(&self) -> f64 {
        if self.terms.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        }
    }
    pub(crate) fn adjusted_strike(&self, tau: f64) -> f64 {
        let elapsed = self.period() - tau;
        if elapsed > 0.0 {
            self.terms.strike() + elapsed / tau * (self.terms.strike() - self.realized)
        } else {
            self.terms.strike()
        }
    }
}

macro_rules! average_option {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        ///
        /// Owns validated continuously monitored averaging terms; a zero-length window fixes once at expiry.
        ///
        /// See [`Self::new`] for construction and [`Self::replace_history`] for history updates.
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub(crate) AverageTerms);
        impl $name {
            /// Returns a validated replacement, preserving omitted terms and the original.
            ///
            /// Valuation-relative history checks remain at pricing time.
            /// See [`GeometricAveragePriceOption::new`] for construction examples.
            ///
            /// # Errors
            ///
            /// Reuses constructor validation for the resulting contract. Failure leaves
            /// this value unchanged.
            pub fn replace(&self, changes: AveragePriceOptionChanges) -> Result<Self> {
                Self::new(
                    changes.option_type.unwrap_or(self.option_type()),
                    changes.strike.unwrap_or(self.strike()),
                    changes.effective_date.unwrap_or(self.effective_date()),
                    changes
                        .averaging_start_date
                        .unwrap_or(self.averaging_start_date()),
                    changes.expiry_date.unwrap_or(self.expiry_date()),
                    changes.realized_average.unwrap_or(self.realized_average()),
                )
            }
            /// Creates an average-price contract with explicit history.
            ///
            /// Use [`DEFAULT_REALIZED_AVERAGE`] before averaging starts. Positive history
            /// is required strictly after the averaging start, checked during pricing.
            ///
            /// # Errors
            ///
            /// Returns [`Error::InvalidInput`] for invalid strike, negative or non-finite
            /// history, reversed life dates, or averaging outside contract life.
            /// Diagnostic order is unspecified when several inputs are invalid.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi::{Date, OptionType, GeometricAveragePriceOption,
            ///     ArithmeticAveragePriceOption, DEFAULT_REALIZED_AVERAGE};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 1)?;
            /// let end = Date::new(2026, 1, 1)?;
            /// let geometric = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, end, DEFAULT_REALIZED_AVERAGE)?;
            /// let arithmetic = ArithmeticAveragePriceOption::new(OptionType::Put,
            ///     100.0, start, start, end, DEFAULT_REALIZED_AVERAGE)?;
            /// assert_eq!(geometric.realized_average(), 0.0);
            /// assert_eq!(arithmetic.averaging_start_date(), start);
            /// # Ok(()) }
            /// ```
            pub fn new(
                option_type: OptionType,
                strike: f64,
                effective_date: Date,
                averaging_start_date: Date,
                expiry_date: Date,
                realized_average: f64,
            ) -> Result<Self> {
                Ok(Self(AverageTerms::new(
                    option_type,
                    strike,
                    effective_date,
                    averaging_start_date,
                    expiry_date,
                    realized_average,
                )?))
            }
            /// Returns an owning copy with replaced history, preserving all other terms.
            ///
            /// # Errors
            ///
            /// Reuses [`Self::new`] validation; failures leave this contract unchanged.
            /// Zero denotes no elapsed history, with time consistency checked at pricing.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi::{Date, OptionType, GeometricAveragePriceOption};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 1)?;
            /// let original = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, Date::new(2026, 1, 1)?, 0.0)?;
            /// let seasoned = original.replace_history(105.0)?;
            /// assert_eq!(seasoned.realized_average(), 105.0);
            /// assert_eq!(original.realized_average(), 0.0);
            /// # Ok(()) }
            /// ```
            pub fn replace_history(&self, realized_average: f64) -> Result<Self> {
                Self::new(
                    self.option_type(),
                    self.strike(),
                    self.effective_date(),
                    self.averaging_start_date(),
                    self.expiry_date(),
                    realized_average,
                )
            }
            /// Returns the direction; see [`Self::new`].
            pub fn option_type(&self) -> OptionType {
                self.0.terms.option_type()
            }
            /// Returns the positive strike; see [`Self::new`].
            pub fn strike(&self) -> f64 {
                self.0.terms.strike()
            }
            /// Returns the inclusive first date; see [`Self::new`].
            pub fn effective_date(&self) -> Date {
                self.0.terms.effective_date()
            }
            /// Returns expiry at midnight UTC; see [`Self::new`].
            pub fn expiry_date(&self) -> Date {
                self.0.terms.expiry_date()
            }
            /// Returns the averaging start; see [`Self::new`].
            pub fn averaging_start_date(&self) -> Date {
                self.0.averaging_start
            }
            /// Returns the caller-supplied history; see [`Self::replace_history`].
            pub fn realized_average(&self) -> f64 {
                self.0.realized
            }
        }
        impl OptionContract for $name {
            fn effective_date(&self) -> Date {
                self.effective_date()
            }
            fn expiry_date(&self) -> Date {
                self.expiry_date()
            }
            fn time_stencil_available(
                &self,
                _context: &PricingContext,
                before: Timestamp,
                after: Timestamp,
            ) -> bool {
                let start = self.averaging_start_date().start_of_day();
                if self.realized_average() == 0.0 {
                    after <= start
                } else {
                    before > start
                }
            }
        }
    };
}
average_option!(
    GeometricAveragePriceOption,
    "Defines a geometric average-price Asian option."
);
average_option!(
    ArithmeticAveragePriceOption,
    "Defines an arithmetic average-price Asian option."
);

/// Selects replacements for [`GeometricAveragePriceOption::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Copy, Debug, Default)]
pub struct AveragePriceOptionChanges {
    /// Replaces [`option_type`](GeometricAveragePriceOption::option_type).
    pub option_type: Option<OptionType>,
    /// Replaces [`strike`](GeometricAveragePriceOption::strike).
    pub strike: Option<f64>,
    /// Replaces [`effective_date`](GeometricAveragePriceOption::effective_date).
    pub effective_date: Option<Date>,
    /// Replaces [`averaging_start_date`](GeometricAveragePriceOption::averaging_start_date).
    pub averaging_start_date: Option<Date>,
    /// Replaces [`expiry_date`](GeometricAveragePriceOption::expiry_date).
    pub expiry_date: Option<Date>,
    /// Replaces [`realized_average`](GeometricAveragePriceOption::realized_average).
    pub realized_average: Option<f64>,
}
