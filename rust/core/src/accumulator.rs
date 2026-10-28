use crate::{Date, Error, ErrorCategory, OptionContract, PricingContext, Result, Timestamp};

/// Supplies accumulator terms; see [`Accumulator::new`] for validation and usage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccumulatorTerms {
    /// Positive finite purchase strike.
    pub strike: f64,
    /// Positive finite level terminating purchases, inclusive at equality.
    pub knock_out_level: f64,
    /// Nonnegative finite quantity purchased on each trading date.
    pub daily_quantity: f64,
    /// Nonnegative finite quantity multiplier strictly below strike.
    pub acceleration_factor: f64,
    /// Caller-asserted quantity strictly before valuation; zero means none.
    pub accumulated_quantity: f64,
    /// First contract date, included in trading-date observations.
    pub effective_date: Date,
    /// Final observation and settlement date, at midnight UTC.
    pub expiry_date: Date,
}
impl AccumulatorTerms {
    /// Creates terms with zero accrued quantity; see [`Accumulator::new`].
    pub fn new(
        strike: f64,
        knock_out_level: f64,
        daily_quantity: f64,
        acceleration_factor: f64,
        effective_date: Date,
        expiry_date: Date,
    ) -> Self {
        Self {
            strike,
            knock_out_level,
            daily_quantity,
            acceleration_factor,
            accumulated_quantity: 0.0,
            effective_date,
            expiry_date,
        }
    }
}

/// Owns outstanding purchases settled at knock-out or expiry as `quantity * (spot - strike)`.
/// Observes trading dates at midnight UTC, including both life endpoints. A knock-out
/// observation purchases no units. Prices exclude previously settled cashflows;
/// callers must not value already terminated contracts. History is caller asserted,
/// with no unknown sentinel or inferred quantity/date consistency restrictions.
/// Shared reads and owning replacements support concurrent use; see [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct Accumulator {
    terms: AccumulatorTerms,
}
impl Accumulator {
    /// Creates a validated contract, allowing equal life dates and zero quantities.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidStrike`] for invalid strike, then
    /// [`ErrorCategory::InvalidParameter`] for invalid level or quantities, then
    /// [`ErrorCategory::InvalidTimeRange`] for reversed life dates. Inputs are unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{Accumulator, AccumulatorTerms, Date};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2025, 2, 3)?))?;
    /// let seasoned = option.replace(3.0)?;
    /// assert_eq!(seasoned.accumulated_quantity(), 3.0);
    /// assert_eq!(option.accumulated_quantity(), 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: AccumulatorTerms) -> Result<Self> {
        crate::error::finite(terms.strike, ErrorCategory::InvalidStrike, true)?;
        crate::error::finite(terms.knock_out_level, ErrorCategory::InvalidParameter, true)?;
        if [
            terms.daily_quantity,
            terms.acceleration_factor,
            terms.accumulated_quantity,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "quantities must be finite and nonnegative",
            ));
        }
        if terms.expiry_date < terms.effective_date {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "expiry precedes effective date",
            ));
        }
        Ok(Self { terms })
    }
    /// Returns an owning replacement of accrued quantity; see [`Self::new`].
    ///
    /// # Errors
    /// Reuses [`Self::new`] validation, preserving the original on success and failure.
    pub fn replace(&self, accumulated_quantity: f64) -> Result<Self> {
        Self::new(AccumulatorTerms {
            accumulated_quantity,
            ..self.terms
        })
    }
    /// Returns the purchase strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.terms.strike
    }
    /// Returns the inclusive knock-out level; see [`Self::new`].
    pub fn knock_out_level(&self) -> f64 {
        self.terms.knock_out_level
    }
    /// Returns the base daily quantity; see [`Self::new`].
    pub fn daily_quantity(&self) -> f64 {
        self.terms.daily_quantity
    }
    /// Returns the below-strike multiplier; see [`Self::new`].
    pub fn acceleration_factor(&self) -> f64 {
        self.terms.acceleration_factor
    }
    /// Returns caller-asserted prior quantity; see [`Self::new`].
    pub fn accumulated_quantity(&self) -> f64 {
        self.terms.accumulated_quantity
    }
    /// Returns the inclusive first observation date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.terms.effective_date
    }
    /// Returns the final observation and settlement date; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.terms.expiry_date
    }

    pub(crate) fn accrues(&self) -> bool {
        self.daily_quantity() != 0.0
            && (self.acceleration_factor() != 0.0 || self.knock_out_level() > self.strike())
    }
    pub(crate) fn purchase(&self, spot: f64) -> f64 {
        if spot < self.strike() {
            self.daily_quantity() * self.acceleration_factor()
        } else {
            self.daily_quantity()
        }
    }
    pub(crate) fn current_observation(&self, context: &PricingContext) -> bool {
        context.valuation_time().micros_since_midnight() == 0
            && context.calendar().is_trading_day(context.valuation_date())
    }
    pub(crate) fn initial(
        &self,
        context: &PricingContext,
        rate: f64,
        dividend: f64,
    ) -> (f64, Option<f64>) {
        let mut quantity = self.accumulated_quantity();
        let spot = context.spot_price();
        if quantity == 0.0 && !self.accrues() {
            return (quantity, Some(0.0));
        }
        if self.current_observation(context) {
            if spot >= self.knock_out_level() {
                return (quantity, Some(quantity * (spot - self.strike())));
            }
            quantity += self.purchase(spot);
        }
        if context.valuation_time() == self.expiry_date().start_of_day()
            || (!self.accrues() && rate == 0.0 && dividend == 0.0)
        {
            return (quantity, Some(quantity * (spot - self.strike())));
        }
        (quantity, None)
    }
}
impl OptionContract for Accumulator {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
    fn all_greeks_unavailable(&self, _context: &PricingContext) -> bool {
        false
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        self.current_observation(context)
            && (crosses(self.knock_out_level())
                || (spot < self.knock_out_level()
                    && self.acceleration_factor() != 1.0
                    && crosses(self.strike())))
    }
    fn time_stencil_available(
        &self,
        context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        let mut date = before.date();
        loop {
            if before <= date.start_of_day()
                && date.start_of_day() <= after
                && context.calendar().is_trading_day(date)
            {
                return false;
            }
            if date == after.date() {
                return true;
            }
            let Ok(next) = date.add_days(1) else {
                return false;
            };
            date = next;
        }
    }
}
