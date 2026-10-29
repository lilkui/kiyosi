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
    fn new(
        direction: OptionType,
        strike: f64,
        start: Date,
        averaging: Date,
        end: Date,
        realized: f64,
    ) -> Result<Self> {
        crate::error::finite(strike, true)?;
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
        /// Owns validated continuous-average terms; a zero-length window fixes once at expiry.
        ///
        /// See [`Self::new`] for construction and [`Self::replace`] for history updates.
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub(crate) AverageTerms);
        impl $name {
            /// Creates an average-price contract with explicit history.
            ///
            /// Use [`DEFAULT_REALIZED_AVERAGE`] before averaging starts. Positive history
            /// is required strictly after the averaging start, checked during pricing.
            ///
            /// # Errors
            ///
            /// Returns [`Error::InvalidInput`] for invalid strike, negative or non-finite
            /// history, reversed life dates, or averaging outside contract life, checked in
            /// that order.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi_core::{Date, OptionType, GeometricAveragePriceOption,
            ///     ArithmeticAveragePriceOption, DEFAULT_REALIZED_AVERAGE};
            /// # fn main() -> kiyosi_core::Result<()> {
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
            /// use kiyosi_core::{Date, OptionType, GeometricAveragePriceOption};
            /// # fn main() -> kiyosi_core::Result<()> {
            /// let start = Date::new(2025, 1, 1)?;
            /// let original = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, Date::new(2026, 1, 1)?, 0.0)?;
            /// let seasoned = original.replace(105.0)?;
            /// assert_eq!(seasoned.realized_average(), 105.0);
            /// assert_eq!(original.realized_average(), 0.0);
            /// # Ok(()) }
            /// ```
            pub fn replace(&self, realized_average: f64) -> Result<Self> {
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
            /// Returns the caller-supplied history; see [`Self::replace`].
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
    "Defines a geometric-average price option."
);
average_option!(
    ArithmeticAveragePriceOption,
    "Defines an arithmetic-average price option."
);
