use crate::{
    Date, Error, ObservationSchedule, OptionContract, OptionType, PricingContext, Result,
    Timestamp, TradingCalendar,
};

/// Supplies accumulator terms; see [`Accumulator::new`] for validation and usage.
#[derive(Clone, Debug, PartialEq)]
pub struct AccumulatorTerms {
    /// Accumulation direction: call purchases, put sells; defaults to call.
    pub option_type: OptionType,
    /// Positive finite purchase or sale strike.
    pub strike: f64,
    /// Positive finite level terminating accumulation, inclusive at equality.
    pub knock_out_level: f64,
    /// Nonnegative finite quantity accumulated on each trading date.
    pub daily_quantity: f64,
    /// Nonnegative finite multiplier strictly below strike for calls, above for puts.
    pub acceleration_factor: f64,
    /// Caller-asserted quantity strictly before valuation; `None` means unknown.
    pub accumulated_quantity: Option<f64>,
    /// First contract date, included in trading-date observations.
    pub effective_midnight_utc: Date,
    /// Final observation and settlement date, at midnight UTC.
    pub expiry_midnight_utc: Date,
    /// Strictly ordered knock-out dates within life; `None` observes every trading date.
    pub knock_out_observation_dates: Option<Vec<Date>>,
}
impl AccumulatorTerms {
    /// Supplies the accumulator direction; see [`Self::new`].
    pub const DEFAULT_OPTION_TYPE: OptionType = OptionType::Call;

    /// Supplies unknown accrued quantity; see [`Self::new`].
    pub const DEFAULT_ACCUMULATED_QUANTITY: Option<f64> = None;

    /// Creates call terms with unknown history and daily knock-out; see [`Accumulator::new`].
    pub fn new(
        strike: f64,
        knock_out_level: f64,
        daily_quantity: f64,
        acceleration_factor: f64,
        effective_midnight_utc: Date,
        expiry_midnight_utc: Date,
    ) -> Self {
        Self {
            option_type: Self::DEFAULT_OPTION_TYPE,
            strike,
            knock_out_level,
            daily_quantity,
            acceleration_factor,
            accumulated_quantity: Self::DEFAULT_ACCUMULATED_QUANTITY,
            effective_midnight_utc,
            expiry_midnight_utc,
            knock_out_observation_dates: None,
        }
    }
}

/// Owns daily accumulations settled together at knock-out or expiry.
///
/// Calls purchase units and settle as `quantity * (spot - strike)`; puts sell units
/// and settle as `quantity * (strike - spot)`. Acceleration applies strictly below
/// strike for calls, strictly above for puts, and not at equality. Both accumulate
/// every trading date at midnight UTC, including the life endpoints. Knock-out
/// observes every trading date by default, or only the explicit dates. An empty
/// schedule disables early knock-out. Calls knock out at or above the barrier,
/// puts at or below it; knock-out adds no units that day. A non-knock-out expiry
/// accumulates normally before settlement. Prices exclude previously settled cash flows;
/// callers must not value already terminated contracts. History is caller asserted,
/// and is required after any strictly prior trading-date observation. `None` means unknown;
/// an explicit zero records no outstanding units. No quantity is inferred from elapsed dates.
/// Shared reads and owning replacements support concurrent use; see [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct Accumulator {
    terms: AccumulatorTerms,
}
impl Accumulator {
    /// Creates a validated contract, allowing equal life dates and zero quantities.
    ///
    /// Validates explicit knock-out date order and life bounds here; trading-day
    /// compatibility is checked against the pricing context's calendar at pricing.
    /// Neither endpoint must appear in the explicit schedule. Strike and barrier
    /// have no ordering constraint.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid strike, invalid level or
    /// quantities, reversed life dates, or unordered/out-of-life knock-out dates,
    /// checked in that order. Inputs are unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Accumulator, AccumulatorTerms, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2025, 2, 3)?))?;
    /// let seasoned = option.replace_history(3.0)?;
    /// assert_eq!(seasoned.accumulated_quantity(), Some(3.0));
    /// assert_eq!(option.accumulated_quantity(), None);
    /// let end = Date::new(2025, 2, 3)?;
    /// let mut terms = AccumulatorTerms::new(100.0, 90.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?, end);
    /// terms.option_type = OptionType::Put;
    /// terms.knock_out_observation_dates = Some(vec![end]);
    /// let sales = Accumulator::new(terms)?;
    /// assert_eq!(sales.option_type(), OptionType::Put);
    /// assert_eq!(sales.knock_out_observation_dates(), Some([end].as_slice()));
    /// # Ok(()) }
    /// ```
    pub fn new(terms: AccumulatorTerms) -> Result<Self> {
        crate::error::finite(terms.strike, true)?;
        crate::error::finite(terms.knock_out_level, true)?;
        if [
            terms.daily_quantity,
            terms.acceleration_factor,
            terms.accumulated_quantity.unwrap_or(0.0),
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::InvalidInput(
                "quantities must be finite and nonnegative".into(),
            ));
        }
        if terms.expiry_midnight_utc < terms.effective_midnight_utc {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        if let Some(dates) = &terms.knock_out_observation_dates {
            ObservationSchedule::new(
                dates.clone(),
                terms.effective_midnight_utc,
                terms.expiry_midnight_utc,
                &TradingCalendar::all_days(),
            )?;
        }
        Ok(Self { terms })
    }
    /// Returns an owning replacement of accrued quantity; `None` restores unknown history.
    ///
    /// See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation, preserving the original on success and failure.
    pub fn replace_history(&self, accumulated_quantity: impl Into<Option<f64>>) -> Result<Self> {
        Self::new(AccumulatorTerms {
            accumulated_quantity: accumulated_quantity.into(),
            ..self.terms.clone()
        })
    }
    /// Returns the purchase or sale direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.terms.option_type
    }
    /// Borrows explicit knock-out dates, or returns `None` for daily observation.
    ///
    /// The immutable slice remains valid for the borrow's lifetime; owning
    /// replacements do not mutate this contract. See [`Self::new`] for an example.
    pub fn knock_out_observation_dates(&self) -> Option<&[Date]> {
        self.terms.knock_out_observation_dates.as_deref()
    }
    /// Returns the purchase or sale strike; see [`Self::new`].
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
    /// Returns the direction-dependent quantity multiplier; see [`Self::new`].
    pub fn acceleration_factor(&self) -> f64 {
        self.terms.acceleration_factor
    }
    /// Returns caller-asserted prior quantity, or `None` for unknown history; see [`Self::new`].
    pub fn accumulated_quantity(&self) -> Option<f64> {
        self.terms.accumulated_quantity
    }
    /// Returns the inclusive first observation date; see [`Self::new`].
    pub fn effective_midnight_utc(&self) -> Date {
        self.terms.effective_midnight_utc
    }
    /// Returns the final observation and settlement date; see [`Self::new`].
    pub fn expiry_midnight_utc(&self) -> Date {
        self.terms.expiry_midnight_utc
    }

    pub(crate) fn validate_pricing(&self, context: &PricingContext) -> Result<()> {
        let valuation = context.valuation_time();
        if valuation < self.effective_midnight_utc().start_of_day()
            || valuation > self.expiry_midnight_utc().start_of_day()
        {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        if !context
            .calendar()
            .is_trading_day(self.expiry_midnight_utc())
        {
            return Err(Error::InvalidInput("expiry must be a trading day".into()));
        }
        if self.knock_out_observation_dates().is_some_and(|dates| {
            dates
                .iter()
                .any(|date| !context.calendar().is_trading_day(*date))
        }) {
            return Err(Error::InvalidInput(
                "knock-out observation date is not a trading day".into(),
            ));
        }
        let calendar = context.calendar();
        let has_prior = calendar
            .trading_days_between(self.effective_midnight_utc(), valuation.date())?
            > 0
            || valuation.micros_since_midnight() > 0 && calendar.is_trading_day(valuation.date());
        if has_prior && self.accumulated_quantity().is_none() {
            return Err(Error::InvalidInput(
                "prior accumulated quantity is required".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn future_observation_dates(&self, context: &PricingContext) -> Result<Vec<Date>> {
        let mut dates = Vec::new();
        let mut date = context.valuation_date();
        loop {
            if date.start_of_day() > context.valuation_time()
                && context.calendar().is_trading_day(date)
            {
                dates.push(date);
            }
            if date == self.expiry_midnight_utc() {
                break;
            }
            date = date.add_days(1)?;
        }
        Ok(dates)
    }

    pub(crate) fn accrues(&self, context: &PricingContext) -> bool {
        if self.daily_quantity() == 0.0
            || context.valuation_time() >= self.expiry_midnight_utc().start_of_day()
        {
            return false;
        }
        if self.acceleration_factor() != 0.0 || !self.is_knock_out(self.strike()) {
            return true;
        }
        if self.knock_out_observation_dates().is_none() {
            return false;
        }
        let mut date = context.valuation_date();
        while date < self.expiry_midnight_utc() {
            let Ok(next) = date.add_days(1) else {
                break;
            };
            date = next;
            if context.calendar().is_trading_day(date) && !self.observes_knock_out(date) {
                return true;
            }
        }
        false
    }
    pub(crate) fn purchase(&self, spot: f64) -> f64 {
        let accelerated = match self.option_type() {
            OptionType::Call => spot < self.strike(),
            OptionType::Put => spot > self.strike(),
        };
        if accelerated {
            self.daily_quantity() * self.acceleration_factor()
        } else {
            self.daily_quantity()
        }
    }
    pub(crate) fn unit_payoff(&self, spot: f64) -> f64 {
        match self.option_type() {
            OptionType::Call => spot - self.strike(),
            OptionType::Put => self.strike() - spot,
        }
    }
    pub(crate) fn is_knock_out(&self, spot: f64) -> bool {
        match self.option_type() {
            OptionType::Call => spot >= self.knock_out_level(),
            OptionType::Put => spot <= self.knock_out_level(),
        }
    }
    pub(crate) fn observes_knock_out(&self, date: Date) -> bool {
        self.knock_out_observation_dates()
            .is_none_or(|dates| dates.binary_search(&date).is_ok())
    }
    pub(crate) fn current_observation(context: &PricingContext) -> bool {
        context.valuation_time().micros_since_midnight() == 0
            && context.calendar().is_trading_day(context.valuation_date())
    }
    pub(crate) fn initial(&self, context: &PricingContext) -> (f64, Option<f64>) {
        let mut quantity = self.accumulated_quantity().unwrap_or(0.0);
        let spot = context.spot_price();
        if Self::current_observation(context) {
            if self.observes_knock_out(context.valuation_date()) && self.is_knock_out(spot) {
                return (quantity, Some(quantity * self.unit_payoff(spot)));
            }
            quantity += self.purchase(spot);
        }
        let accrues = self.accrues(context);
        if quantity == 0.0 && !accrues {
            return (quantity, Some(0.0));
        }
        let model = context.model_parameters();
        if context.valuation_time() == self.expiry_midnight_utc().start_of_day()
            || (!accrues
                && model.risk_free_curve().flat_rate() == Some(0.0)
                && model.dividend_curve().flat_rate() == Some(0.0))
        {
            return (quantity, Some(quantity * self.unit_payoff(spot)));
        }
        (quantity, None)
    }

    #[expect(
        clippy::float_cmp,
        reason = "An acceleration factor of exactly one removes payoff dependence on the strike."
    )]
    pub(crate) fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        if Self::current_observation(context)
            && self.observes_knock_out(context.valuation_date())
            && self.is_knock_out(context.spot_price())
        {
            return false;
        }
        let future_accumulation = self.accrues(context);
        let mut future_knock_out = false;
        let mut early_knock_out = false;
        let mut date = context.valuation_date();
        while date <= self.expiry_midnight_utc() {
            if date.start_of_day() > context.valuation_time()
                && context.calendar().is_trading_day(date)
            {
                let observe = self.observes_knock_out(date);
                future_knock_out |= observe;
                early_knock_out |= observe && date < self.expiry_midnight_utc();
            }
            if date == self.expiry_midnight_utc() {
                break;
            }
            let Ok(next) = date.add_days(1) else {
                break;
            };
            date = next;
        }
        if future_accumulation {
            return future_knock_out || self.acceleration_factor() != 1.0;
        }
        let quantity = self.accumulated_quantity().unwrap_or(0.0)
            + if Self::current_observation(context) {
                self.purchase(context.spot_price())
            } else {
                0.0
            };
        let model = context.model_parameters();
        early_knock_out
            && quantity != 0.0
            && (model.risk_free_curve().flat_rate() != Some(0.0)
                || model.dividend_curve().flat_rate() != Some(0.0))
    }
}
impl OptionContract for Accumulator {
    fn effective_midnight_utc(&self) -> Date {
        self.effective_midnight_utc()
    }
    fn expiry_midnight_utc(&self) -> Date {
        self.expiry_midnight_utc()
    }
    fn all_greeks_unavailable(&self, _context: &PricingContext) -> bool {
        false
    }
    #[expect(
        clippy::float_cmp,
        reason = "An acceleration factor of exactly one removes the strike discontinuity."
    )]
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        let observes_knock_out = self.observes_knock_out(context.valuation_date());
        Self::current_observation(context)
            && ((observes_knock_out && crosses(self.knock_out_level()))
                || ((!observes_knock_out || !self.is_knock_out(spot))
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

/// Selects replacements for [`Accumulator::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct AccumulatorChanges {
    /// Replaces [`option_type`](AccumulatorTerms::option_type).
    pub option_type: Option<OptionType>,
    /// Replaces [`strike`](AccumulatorTerms::strike).
    pub strike: Option<f64>,
    /// Replaces [`knock_out_level`](AccumulatorTerms::knock_out_level).
    pub knock_out_level: Option<f64>,
    /// Replaces [`daily_quantity`](AccumulatorTerms::daily_quantity).
    pub daily_quantity: Option<f64>,
    /// Replaces [`acceleration_factor`](AccumulatorTerms::acceleration_factor).
    pub acceleration_factor: Option<f64>,
    /// Replaces [`accumulated_quantity`](AccumulatorTerms::accumulated_quantity).
    pub accumulated_quantity: Option<Option<f64>>,
    /// Replaces [`effective_midnight_utc`](AccumulatorTerms::effective_midnight_utc).
    pub effective_midnight_utc: Option<Date>,
    /// Replaces [`expiry_midnight_utc`](AccumulatorTerms::expiry_midnight_utc).
    pub expiry_midnight_utc: Option<Date>,
    /// Replaces [`knock_out_observation_dates`](AccumulatorTerms::knock_out_observation_dates).
    pub knock_out_observation_dates: Option<Option<Vec<Date>>>,
}

impl Accumulator {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar and valuation-relative history checks remain at pricing time.
    /// See [`Accumulator::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: AccumulatorChanges) -> Result<Self> {
        Self::new(AccumulatorTerms {
            option_type: changes.option_type.unwrap_or(self.option_type()),
            strike: changes.strike.unwrap_or(self.strike()),
            knock_out_level: changes.knock_out_level.unwrap_or(self.knock_out_level()),
            daily_quantity: changes.daily_quantity.unwrap_or(self.daily_quantity()),
            acceleration_factor: changes
                .acceleration_factor
                .unwrap_or(self.acceleration_factor()),
            accumulated_quantity: changes
                .accumulated_quantity
                .unwrap_or(self.accumulated_quantity()),
            effective_midnight_utc: changes
                .effective_midnight_utc
                .unwrap_or(self.effective_midnight_utc()),
            expiry_midnight_utc: changes
                .expiry_midnight_utc
                .unwrap_or(self.expiry_midnight_utc()),
            knock_out_observation_dates: changes
                .knock_out_observation_dates
                .unwrap_or_else(|| self.knock_out_observation_dates().map(<[Date]>::to_vec)),
        })
    }
}
