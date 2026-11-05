use crate::{
    Error, ObservationSchedule, OptionContract, OptionType, PricingContext, Result, Timestamp,
    TradingCalendar,
};

/// Supplies accumulator terms; see [`Accumulator::new`] for validation and usage.
#[derive(Clone, Debug, PartialEq)]
pub struct AccumulatorTerms {
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
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
    /// Records a settled knock-out strictly before valuation; no remaining cash flows are valued.
    pub terminated: bool,
    /// Inclusive effective time for daily observations at the expiry clock time.
    pub effective_time: Timestamp,
    /// Final observation and settlement time.
    pub expiry_time: Timestamp,
    /// Strictly ordered knock-out times within life; `None` observes every trading date.
    pub knock_out_observation_times: Option<Vec<Timestamp>>,
}
impl AccumulatorTerms {
    /// Supplies the accumulator direction; see [`Self::new`].
    pub const DEFAULT_OPTION_TYPE: OptionType = OptionType::Call;

    /// Supplies unknown accrued quantity; see [`Self::new`].
    pub const DEFAULT_ACCUMULATED_QUANTITY: Option<f64> = None;

    /// Supplies an active contract; see [`Self::new`].
    pub const DEFAULT_TERMINATED: bool = false;

    /// Creates call terms with unknown history and daily knock-out; see [`Accumulator::new`].
    pub fn new(
        strike: f64,
        knock_out_level: f64,
        daily_quantity: f64,
        acceleration_factor: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            option_type: Self::DEFAULT_OPTION_TYPE,
            strike,
            knock_out_level,
            daily_quantity,
            acceleration_factor,
            accumulated_quantity: Self::DEFAULT_ACCUMULATED_QUANTITY,
            terminated: Self::DEFAULT_TERMINATED,
            effective_time,
            expiry_time,
            knock_out_observation_times: None,
        }
    }
}

/// Owns daily accumulations settled together at knock-out or expiry.
///
/// Calls purchase units and settle as `quantity * (spot - strike)`; puts sell units
/// and settle as `quantity * (strike - spot)`. Acceleration applies strictly below
/// strike for calls, strictly above for puts, and not at equality. Both accumulate
/// at the expiry clock time on trading dates within the inclusive contract life. Knock-out
/// observes every trading date by default, or only the explicit times. An empty
/// schedule disables knock-out. Calls knock out at or above the barrier,
/// puts at or below it; knock-out adds no units at its own timestamp. A non-knock-out expiry
/// accumulates normally before settlement. Prices exclude previously settled cash flows;
/// `terminated` records an already settled knock-out and gives zero remaining value.
/// Pricing rejects this state unless a knock-out observation strictly precedes valuation.
/// Accrued quantity is ignored for terminated contracts. History is caller asserted,
/// and is required after any strictly prior trading-date observation. `None` means unknown;
/// an explicit zero records no outstanding units. No quantity is inferred from elapsed dates.
/// Shared reads and owning replacements support concurrent use; see [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct Accumulator {
    terms: AccumulatorTerms,
}
impl Accumulator {
    /// Creates a validated contract, allowing equal life times and zero quantities.
    ///
    /// Validates explicit knock-out time order and life bounds here; trading-day
    /// compatibility is checked against the contract's calendar on construction.
    /// Neither endpoint must appear in the explicit schedule. Strike and barrier
    /// have no ordering constraint.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid strike, invalid level or
    /// quantities, reversed life times, or unordered/out-of-life knock-out times,
    /// checked in that order. Inputs are unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Accumulator, AccumulatorTerms, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 2, 3)?.start_of_day()))?;
    /// let seasoned = option.replace_history(3.0)?;
    /// assert_eq!(seasoned.accumulated_quantity(), Some(3.0));
    /// assert_eq!(option.accumulated_quantity(), None);
    /// let settled = option.replace(kiyosi::AccumulatorChanges {
    ///     terminated: Some(true), ..Default::default()
    /// })?;
    /// assert!(settled.terminated());
    /// let end = Date::new(2025, 2, 3)?.start_of_day();
    /// let mut terms = AccumulatorTerms::new(100.0, 90.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), end);
    /// terms.option_type = OptionType::Put;
    /// terms.knock_out_observation_times = Some(vec![end]);
    /// terms.calendar = kiyosi::TradingCalendar::all_days();
    /// let sales = Accumulator::new(terms)?;
    /// assert_eq!(sales.option_type(), OptionType::Put);
    /// assert_eq!(sales.knock_out_observation_times(), Some([end].as_slice()));
    /// # Ok(()) }
    /// ```
    pub fn new(terms: AccumulatorTerms) -> Result<Self> {
        crate::error::finite(terms.strike, true, "strike")?;
        crate::error::finite(terms.knock_out_level, true, "knock_out_level")?;
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
        if terms.expiry_time < terms.effective_time {
            return Err(Error::InvalidInput("expiry precedes effective date".into()));
        }
        if let Some(dates) = &terms.knock_out_observation_times {
            ObservationSchedule::new(
                dates.clone(),
                terms.effective_time,
                terms.expiry_time,
                &TradingCalendar::all_days(),
            )?;
        }
        let option = Self { terms };
        option.validate_calendar()?;
        Ok(option)
    }

    /// Replaces the owned accumulation calendar; see [`Self::new`] for construction.
    ///
    /// Defaults to weekdays. Construction and replacement check observations and expiry against
    /// this calendar. Other terms are retained.
    ///
    /// Amends an already validated contract; this is not an initial-construction builder.
    /// Set [`AccumulatorTerms::calendar`] before calling [`Self::new`] when the
    /// terms require a custom calendar, such as a weekend expiry or observation.
    /// The original contract remains unchanged on success and failure.
    /// See [`crate::RangeAccrualOption::replace_calendar`] for an initial-calendar example.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] if observations or required expiry fall on closed dates.
    pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        let mut updated = self.clone();
        updated.terms.calendar = calendar;
        updated.validate_calendar()?;
        Ok(updated)
    }

    fn validate_calendar(&self) -> Result<()> {
        if !self.calendar().is_trading_day(self.expiry_time().date()) {
            return Err(Error::InvalidInput("expiry must be a trading day".into()));
        }
        if self.knock_out_observation_times().is_some_and(|times| {
            times
                .iter()
                .any(|time| !self.calendar().is_trading_day(time.date()))
        }) {
            return Err(Error::InvalidInput(
                "knock-out observation is not a trading day".into(),
            ));
        }
        Ok(())
    }

    /// Borrows the contract's accumulation calendar; see [`Self::replace_calendar`].
    pub fn calendar(&self) -> &TradingCalendar {
        &self.terms.calendar
    }
    /// Returns an owning replacement of accrued quantity; `None` restores unknown history.
    ///
    /// See [`Self::new`].
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation, preserving the original on success and failure.
    pub fn replace_history(&self, accumulated_quantity: impl Into<Option<f64>>) -> Result<Self> {
        self.replace(AccumulatorChanges {
            accumulated_quantity: Some(accumulated_quantity.into()),
            ..Default::default()
        })
    }
    /// Returns the purchase or sale direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.terms.option_type
    }
    /// Borrows explicit knock-out times, or returns `None` for daily observation.
    ///
    /// The immutable slice remains valid for the borrow's lifetime; owning
    /// replacements do not mutate this contract. See [`Self::new`] for an example.
    pub fn knock_out_observation_times(&self) -> Option<&[Timestamp]> {
        self.terms.knock_out_observation_times.as_deref()
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
    /// Returns whether a prior knock-out has settled; see [`Self::replace`].
    pub fn terminated(&self) -> bool {
        self.terms.terminated
    }
    /// Returns the inclusive effective time; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.terms.effective_time
    }
    /// Returns the final observation and settlement time; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.terms.expiry_time
    }

    pub(crate) fn validate_pricing(&self, context: &PricingContext) -> Result<()> {
        let valuation = context.valuation_time();
        if valuation < self.effective_time() || valuation > self.expiry_time() {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        let had_accumulation = ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            true,
        )?
        .times()
        .iter()
        .any(|&time| time < valuation);
        let had_knock_out = self
            .knock_out_observation_times()
            .map_or(had_accumulation, |times| {
                times.iter().any(|&time| time < valuation)
            });
        if self.terminated() && !had_knock_out {
            return Err(Error::InvalidInput(
                "termination predates knock-out observations".into(),
            ));
        }
        let has_prior = had_accumulation || had_knock_out;
        if has_prior && !self.terminated() && self.accumulated_quantity().is_none() {
            return Err(Error::InvalidInput(
                "prior accumulated quantity is required".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn future_observation_times(
        &self,
        context: &PricingContext,
    ) -> Result<Vec<Timestamp>> {
        let mut times = ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            true,
        )?
        .times()
        .to_vec();
        times.extend(self.knock_out_observation_times().unwrap_or(&[]));
        times.push(self.expiry_time());
        times.retain(|&time| time > context.valuation_time());
        times.sort_unstable();
        times.dedup();
        Ok(times)
    }

    pub(crate) fn accrues(&self, context: &PricingContext) -> bool {
        if self.terminated()
            || self.daily_quantity() == 0.0
            || context.valuation_time() >= self.expiry_time()
        {
            return false;
        }
        let Ok(schedule) = ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            true,
        ) else {
            return true;
        };
        schedule.times().iter().any(|&time| {
            time > context.valuation_time()
                && (self.acceleration_factor() != 0.0
                    || !self.is_knock_out(self.strike())
                    || !self.observes_knock_out(time))
        })
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
    pub(crate) fn observes_knock_out(&self, date: Timestamp) -> bool {
        self.knock_out_observation_times().map_or(
            date.micros_since_midnight() == self.expiry_time().micros_since_midnight(),
            |times| times.binary_search(&date).is_ok(),
        )
    }
    pub(crate) fn current_observation(&self, context: &PricingContext) -> bool {
        self.accumulates_at(context.valuation_time(), self.calendar())
    }
    pub(crate) fn accumulates_at(&self, time: Timestamp, calendar: &TradingCalendar) -> bool {
        self.effective_time() <= time
            && time <= self.expiry_time()
            && time.micros_since_midnight() == self.expiry_time().micros_since_midnight()
            && calendar.is_trading_day(time.date())
    }

    pub(crate) fn initial(&self, context: &PricingContext) -> (f64, Option<f64>) {
        if self.terminated() {
            return (0.0, Some(0.0));
        }
        let mut quantity = self.accumulated_quantity().unwrap_or(0.0);
        let spot = context.spot_price();
        if self.observes_knock_out(context.valuation_time())
            && self.is_knock_out(spot)
            && self.calendar().is_trading_day(context.valuation_date())
        {
            return (quantity, Some(quantity * self.unit_payoff(spot)));
        }
        if self.current_observation(context) {
            quantity += self.purchase(spot);
        }
        let accrues = self.accrues(context);
        if quantity == 0.0 && !accrues {
            return (quantity, Some(0.0));
        }
        let model = context.model_parameters();
        if context.valuation_time() == self.expiry_time()
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
        if self.terminated() {
            return false;
        }
        if self.observes_knock_out(context.valuation_time())
            && self.is_knock_out(context.spot_price())
            && self.calendar().is_trading_day(context.valuation_date())
        {
            return false;
        }
        let future_accumulation = self.accrues(context);
        let mut future_knock_out = false;
        let mut early_knock_out = false;
        let Ok(times) = self.future_observation_times(context) else {
            return true;
        };
        for time in times {
            let observe = self.observes_knock_out(time);
            future_knock_out |= observe;
            early_knock_out |= observe && time < self.expiry_time();
        }
        if future_accumulation {
            return future_knock_out || self.acceleration_factor() != 1.0;
        }
        let quantity = self.accumulated_quantity().unwrap_or(0.0)
            + if self.current_observation(context) {
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
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn all_greeks_unavailable(&self, context: &PricingContext) -> bool {
        context
            .model_parameters()
            .as_bsm()
            .is_ok_and(|model| model.volatility() == 0.0)
            && self.at_spot_discontinuity(context, 0.0)
    }
    #[expect(
        clippy::float_cmp,
        reason = "An acceleration factor of exactly one removes the strike discontinuity."
    )]
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        if self.terminated() {
            return false;
        }
        if context
            .model_parameters()
            .as_bsm()
            .is_ok_and(|model| model.volatility() == 0.0)
            && self.future_observation_times(context).is_ok_and(|times| {
                times.into_iter().any(|time| {
                    self.observes_knock_out(time)
                        && context.deterministic_crosses(time, self.knock_out_level(), radius)
                        || self.acceleration_factor() != 1.0
                            && self.accumulates_at(time, self.calendar())
                            && context.deterministic_crosses(time, self.strike(), radius)
                })
            })
        {
            return true;
        }
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        let observes = self.observes_knock_out(context.valuation_time())
            && self.calendar().is_trading_day(context.valuation_date());
        (observes && crosses(self.knock_out_level()))
            || (self.current_observation(context)
                && (!observes || !self.is_knock_out(spot))
                && self.acceleration_factor() != 1.0
                && crosses(self.strike()))
    }
    fn time_stencil_available(
        &self,
        _context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        if self.terminated() {
            return true;
        }
        let Ok(schedule) = ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            true,
        ) else {
            return false;
        };
        !schedule
            .times()
            .iter()
            .chain(self.knock_out_observation_times().unwrap_or(&[]))
            .any(|&time| before <= time && time <= after)
    }
}

/// Selects replacements for [`Accumulator::replace`]; absent fields retain their values.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Debug, Default)]
pub struct AccumulatorChanges {
    /// Replaces the owned accumulation calendar; see [`Accumulator::replace_calendar`].
    pub calendar: Option<TradingCalendar>,
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
    /// Replaces [`terminated`](AccumulatorTerms::terminated).
    pub terminated: Option<bool>,
    /// Replaces [`effective_time`](AccumulatorTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](AccumulatorTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`knock_out_observation_times`](AccumulatorTerms::knock_out_observation_times).
    pub knock_out_observation_times: Option<Option<Vec<Timestamp>>>,
}

impl Accumulator {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks remain at pricing time.
    /// Set `terminated: Some(true)` to record a settled knock-out without changing
    /// the stored accrued quantity. See [`Accumulator::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: AccumulatorChanges) -> Result<Self> {
        Self::new(AccumulatorTerms {
            calendar: changes.calendar.unwrap_or_else(|| self.calendar().clone()),
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
            terminated: changes.terminated.unwrap_or(self.terminated()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            knock_out_observation_times: changes.knock_out_observation_times.unwrap_or_else(|| {
                self.knock_out_observation_times()
                    .map(<[Timestamp]>::to_vec)
            }),
        })
    }
}
