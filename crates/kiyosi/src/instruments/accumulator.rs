use crate::{
    Error, ObservationSchedule, OptionContract, OptionType, PricingContext, Result, TimeOfDay,
    Timestamp, TradingCalendar,
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
    /// Records prior knock-out history: `None` is unknown, `Some(false)` is active,
    /// and `Some(true)` is settled with no remaining cash flows. See [`Accumulator::replace_history`].
    pub knock_out_settled: Option<bool>,
    /// Inclusive effective time for daily observations at the specified observation clock time.
    pub effective_time: Timestamp,
    /// Maturity settlement time.
    pub expiry_time: Timestamp,
    /// Sets the explicit daily observation clock; see [`TimeOfDay::new`].
    pub observation_clock_time: TimeOfDay,
    /// Strictly ordered knock-out times within life; `None` observes every trading date.
    pub knock_out_observation_times: Option<Vec<Timestamp>>,
}
impl AccumulatorTerms {
    /// Supplies the accumulator direction; see [`Self::new`].
    pub const DEFAULT_OPTION_TYPE: OptionType = OptionType::Call;

    /// Supplies unknown accrued quantity; see [`Self::new`].
    pub const DEFAULT_ACCUMULATED_QUANTITY: Option<f64> = None;

    /// Supplies unknown knock-out settlement history; see [`Self::new`].
    pub const DEFAULT_KNOCK_OUT_SETTLED: Option<bool> = None;

    /// Creates call terms with unknown history and daily knock-out; see [`Accumulator::new`].
    pub fn new(
        strike: f64,
        knock_out_level: f64,
        daily_quantity: f64,
        acceleration_factor: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        observation_clock_time: TimeOfDay,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            option_type: Self::DEFAULT_OPTION_TYPE,
            strike,
            knock_out_level,
            daily_quantity,
            acceleration_factor,
            accumulated_quantity: Self::DEFAULT_ACCUMULATED_QUANTITY,
            knock_out_settled: Self::DEFAULT_KNOCK_OUT_SETTLED,
            effective_time,
            expiry_time,
            observation_clock_time,
            knock_out_observation_times: None,
        }
    }
}

/// Selects accumulator history replacements; absent fields retain their values.
///
/// See [`Accumulator::replace_history`] for an example. A quantity alone can be
/// passed as `f64` or `Option<f64>`; it preserves knock-out settlement state.
#[derive(Clone, Copy, Debug, Default)]
pub struct AccumulatorHistoryChanges {
    /// Replaces outstanding quantity; `Some(None)` restores unknown history.
    pub accumulated_quantity: Option<Option<f64>>,
    /// Replaces prior knock-out history; `Some(None)` restores unknown history.
    pub knock_out_settled: Option<Option<bool>>,
}

impl From<f64> for AccumulatorHistoryChanges {
    fn from(quantity: f64) -> Self {
        Self::from(Some(quantity))
    }
}

impl From<Option<f64>> for AccumulatorHistoryChanges {
    fn from(quantity: Option<f64>) -> Self {
        Self {
            accumulated_quantity: Some(quantity),
            knock_out_settled: None,
        }
    }
}

/// Owns daily accumulations settled together at knock-out or expiry.
///
/// Calls purchase units and settle as `quantity * (spot - strike)`; puts sell units
/// and settle as `quantity * (strike - spot)`. Acceleration applies strictly below
/// strike for calls, strictly above for puts, and not at equality. Both accumulate
/// at the specified observation clock time on trading dates within the inclusive contract life. Knock-out
/// observes every trading date by default, or only the explicit times. An empty
/// schedule disables knock-out. Calls knock out at or above the barrier,
/// puts at or below it; knock-out adds no units at its own timestamp. Expiry
/// accumulates before settlement only when it is a daily observation and does not knock out.
/// Settlement may fall on a closed date without adding an observation.
/// Prices exclude previously settled cash flows;
/// `knock_out_settled: Some(true)` records an already settled knock-out and gives zero remaining value.
/// Pricing rejects this state unless a knock-out observation strictly precedes valuation.
/// Accrued quantity is ignored for settled contracts. History is caller asserted,
/// and is required after any strictly prior accumulation or knock-out observation.
/// Termination history is required after a strictly prior knock-out observation:
/// `None` means unknown, `Some(false)` means active and `Some(true)` means settled.
/// For accrued quantity, `None` means unknown;
/// an explicit zero records no outstanding units. Active contracts reject positive quantity
/// before a strictly prior accumulation observation. No quantity is inferred from elapsed dates.
/// Shared reads and owning replacements support concurrent use; see [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct Accumulator {
    history_time: Option<Timestamp>,
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
    /// use kiyosi::{Accumulator, AccumulatorHistoryChanges, AccumulatorTerms, Date, OptionType};
    /// # fn main() -> kiyosi::Result<()> {
    /// let option = Accumulator::new(AccumulatorTerms::new(100.0, 110.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), Date::new(2025, 2, 3)?.start_of_day(), kiyosi::TimeOfDay::new(0, 0, 0, 0)?))?;
    /// let seasoned = option.replace_history(AccumulatorHistoryChanges {
    ///     accumulated_quantity: Some(Some(3.0)), knock_out_settled: Some(Some(false)),
    /// })?;
    /// assert_eq!(seasoned.accumulated_quantity(), Some(3.0));
    /// assert_eq!(option.accumulated_quantity(), None);
    /// let settled = option.replace(kiyosi::AccumulatorChanges {
    ///     knock_out_settled: Some(Some(true)), ..Default::default()
    /// })?;
    /// assert_eq!(settled.knock_out_settled(), Some(true));
    /// let end = Date::new(2025, 2, 3)?.start_of_day();
    /// let mut terms = AccumulatorTerms::new(100.0, 90.0, 1.0, 2.0,
    ///     Date::new(2025, 1, 1)?.start_of_day(), end, kiyosi::TimeOfDay::new(0, 0, 0, 0)?);
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
        for (field, value) in [
            ("daily_quantity", terms.daily_quantity),
            ("acceleration_factor", terms.acceleration_factor),
            (
                "accumulated_quantity",
                terms.accumulated_quantity.unwrap_or(0.0),
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(Error::InvalidInput(
                    format!("{field} must be finite and nonnegative").into(),
                ));
            }
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
        let option = Self {
            terms,
            history_time: None,
        };
        option.validate_calendar()?;
        Ok(option)
    }

    /// Replaces the owned accumulation calendar; see [`Self::new`] for construction.
    ///
    /// Defaults to weekdays. Construction and replacement check explicit observations against
    /// this calendar. Other terms are retained.
    ///
    /// Amends an already validated contract; this is not an initial-construction builder.
    /// Set [`AccumulatorTerms::calendar`] before calling [`Self::new`] when the
    /// terms require a custom calendar, such as weekend observations.
    /// The original contract remains unchanged on success and failure.
    /// See [`crate::RangeAccrualOption::replace_calendar`] for an initial-calendar example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Returns [`crate::Error::InvalidInput`] if explicit observations fall on closed dates.
    pub fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        self.replace(AccumulatorChanges {
            calendar: Some(calendar),
            ..Default::default()
        })
    }

    fn validate_calendar(&self) -> Result<()> {
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
    /// Returns a validated replacement of quantity, knock-out settlement or both history fields.
    ///
    /// Omitted fields retain their values. A quantity alone (`f64` or `Option<f64>`)
    /// replaces only quantity; `None` restores unknown quantity. Time consistency
    /// is checked at pricing. The original is unchanged on success and failure.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] numeric and contract validation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Accumulator, AccumulatorHistoryChanges, AccumulatorTerms, Date};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = Accumulator::new(AccumulatorTerms::new(90.0, 110.0, 1.0, 2.0, start, start.add_days(9)?, kiyosi::TimeOfDay::new(0, 0, 0, 0)?))?;
    /// let settled = option.replace_history(AccumulatorHistoryChanges {
    ///     knock_out_settled: Some(Some(true)), ..Default::default()
    /// })?;
    /// assert_eq!(settled.knock_out_settled(), Some(true));
    /// assert_eq!(option.knock_out_settled(), None);
    /// assert_eq!(settled.replace_history(3.0)?.knock_out_settled(), Some(true));
    /// # Ok(()) }
    /// ```
    pub fn replace_history(&self, changes: impl Into<AccumulatorHistoryChanges>) -> Result<Self> {
        let changes = changes.into();
        self.replace(AccumulatorChanges {
            accumulated_quantity: changes.accumulated_quantity,
            knock_out_settled: changes.knock_out_settled,
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
    /// Returns prior knock-out settlement history, or `None` if unknown; see [`Self::replace_history`].
    pub fn knock_out_settled(&self) -> Option<bool> {
        self.terms.knock_out_settled
    }
    /// Returns the inclusive effective time; see [`Self::new`].
    pub fn effective_time(&self) -> Timestamp {
        self.terms.effective_time
    }
    /// Returns the maturity settlement time; see [`Self::new`].
    pub fn expiry_time(&self) -> Timestamp {
        self.terms.expiry_time
    }
    /// Returns the daily observation clock; see [`TimeOfDay::new`].
    pub fn observation_clock_time(&self) -> TimeOfDay {
        self.terms.observation_clock_time
    }

    /// Returns an owned schedule of daily accumulation opportunities.
    ///
    /// Includes the effective instant when it matches the contractual clock and
    /// calendar. Expiry adds no observation. A coincident knock-out can prevent
    /// accumulation; history and spot do not change this contractual schedule.
    ///
    /// # Errors
    ///
    /// Propagates [`ObservationSchedule`] time-range errors.
    ///
    /// # Panics
    ///
    /// Propagates panics from custom calendar predicates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Accumulator, AccumulatorTerms, Date, TimeOfDay};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 6)?.start_of_day();
    /// let option = Accumulator::new(AccumulatorTerms::new(
    ///     90.0, 120.0, 1.0, 2.0, start, start.add_days(2)?, TimeOfDay::new(0, 0, 0, 0)?))?;
    /// assert_eq!(option.accumulation_schedule()?.times()[0], start);
    /// assert_eq!(option.knock_out_observation_schedule()?, option.accumulation_schedule()?);
    /// # Ok(()) }
    /// ```
    pub fn accumulation_schedule(&self) -> Result<ObservationSchedule> {
        ObservationSchedule::daily(
            self.effective_time(),
            self.expiry_time(),
            self.calendar(),
            true,
            self.observation_clock_time(),
        )
    }

    /// Returns an owned schedule resolving daily or explicit knock-out monitoring.
    ///
    /// An absent explicit list uses [`Self::accumulation_schedule`]; an empty
    /// list disables knock-out. See that method for an example. The schedule
    /// covers the whole contract life independently of supplied history.
    ///
    /// # Errors
    ///
    /// Propagates [`ObservationSchedule`] validation and time-range errors.
    ///
    /// # Panics
    ///
    /// Propagates panics from custom calendar predicates.
    pub fn knock_out_observation_schedule(&self) -> Result<ObservationSchedule> {
        match self.knock_out_observation_times() {
            None => self.accumulation_schedule(),
            Some(times) => ObservationSchedule::new(
                times.to_vec(),
                self.effective_time(),
                self.expiry_time(),
                self.calendar(),
            ),
        }
    }

    pub(crate) fn validate_pricing(&self, context: &PricingContext) -> Result<()> {
        let valuation = context.valuation_time();
        if valuation < self.effective_time() || valuation > self.expiry_time() {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        let schedule = self.accumulation_schedule()?;
        let had_accumulation = schedule.times().iter().any(|&time| time < valuation);
        let had_knock_out_at = |at| {
            self.knock_out_observation_times()
                .unwrap_or(schedule.times())
                .iter()
                .any(|&time| time < at)
        };
        let had_knock_out = had_knock_out_at(valuation);
        if self.knock_out_settled() == Some(true)
            && !had_knock_out_at(self.history_time.unwrap_or(valuation).min(valuation))
        {
            return Err(Error::InvalidInput(
                "knock-out settlement predates knock-out observations".into(),
            ));
        }
        if had_knock_out && self.knock_out_settled().is_none() {
            return Err(Error::InvalidInput(
                "prior knock-out settlement state is required".into(),
            ));
        }
        if self.knock_out_settled() != Some(true)
            && !had_accumulation
            && self
                .accumulated_quantity()
                .is_some_and(|quantity| quantity > 0.0)
        {
            return Err(Error::InvalidInput(
                "accumulated_quantity predates accumulation observations".into(),
            ));
        }
        let has_prior = had_accumulation || had_knock_out;
        if has_prior
            && self.knock_out_settled() != Some(true)
            && self.accumulated_quantity().is_none()
        {
            return Err(Error::InvalidInput(
                "prior accumulated quantity is required".into(),
            ));
        }
        super::validate_history_time(
            self,
            self.accumulated_quantity().is_some() || self.knock_out_settled().is_some(),
            context,
            false,
        )?;
        Ok(())
    }

    pub(crate) fn future_observation_times(
        &self,
        context: &PricingContext,
    ) -> Result<Vec<Timestamp>> {
        let mut times = self.accumulation_schedule()?.times().to_vec();
        times.extend(self.knock_out_observation_times().unwrap_or(&[]));
        times.push(self.expiry_time());
        times.retain(|&time| time > context.valuation_time());
        times.sort_unstable();
        times.dedup();
        Ok(times)
    }

    pub(crate) fn accrues(&self, context: &PricingContext) -> bool {
        if self.knock_out_settled() == Some(true)
            || self.daily_quantity() == 0.0
            || context.valuation_time() >= self.expiry_time()
        {
            return false;
        }
        let Ok(schedule) = self.accumulation_schedule() else {
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
            date.micros_since_midnight() == self.observation_clock_time().micros_since_midnight(),
            |times| times.binary_search(&date).is_ok(),
        )
    }
    pub(crate) fn current_observation(&self, context: &PricingContext) -> bool {
        self.accumulates_at(context.valuation_time(), self.calendar())
    }
    pub(crate) fn accumulates_at(&self, time: Timestamp, calendar: &TradingCalendar) -> bool {
        self.effective_time() <= time
            && time <= self.expiry_time()
            && time.micros_since_midnight() == self.observation_clock_time().micros_since_midnight()
            && calendar.is_trading_day(time.date())
    }

    pub(crate) fn initial(&self, context: &PricingContext) -> (f64, Option<f64>) {
        if self.knock_out_settled() == Some(true) {
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
                && model.risk_free_curve().exact_flat_rate() == Some(0.0)
                && model.dividend_curve().exact_flat_rate() == Some(0.0))
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
        if self.knock_out_settled() == Some(true) {
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
            && (model.risk_free_curve().exact_flat_rate() != Some(0.0)
                || model.dividend_curve().exact_flat_rate() != Some(0.0))
    }
}
impl OptionContract for Accumulator {
    fn history_time(&self) -> Option<Timestamp> {
        self.history_time
    }
    fn with_history_time(mut self, time: Option<Timestamp>) -> Result<Self> {
        crate::instruments::validate_history_time_bounds(&self, time)?;
        self.history_time = time;
        Ok(self)
    }

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
        if self.knock_out_settled() == Some(true) {
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
        if self.knock_out_settled() == Some(true) {
            return true;
        }
        let Ok(schedule) = self.accumulation_schedule() else {
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
    /// Replaces or clears the history timestamp; see [`OptionContract::with_history_time`].
    pub history_time: Option<Option<Timestamp>>,
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
    /// Replaces [`knock_out_settled`](AccumulatorTerms::knock_out_settled).
    pub knock_out_settled: Option<Option<bool>>,
    /// Replaces [`effective_time`](AccumulatorTerms::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](AccumulatorTerms::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces the daily observation clock; see [`TimeOfDay::new`].
    pub observation_clock_time: Option<TimeOfDay>,
    /// Replaces [`knock_out_observation_times`](AccumulatorTerms::knock_out_observation_times).
    pub knock_out_observation_times: Option<Option<Vec<Timestamp>>>,
}

impl Accumulator {
    /// Returns a validated replacement, preserving omitted terms and the original.
    ///
    /// Calendar compatibility is validated here; valuation-relative history checks occur at pricing time.
    /// Set `knock_out_settled: Some(Some(true))` to record a settled knock-out without changing
    /// the stored accrued quantity. See [`Accumulator::new`] for construction examples.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when historical observation terms change
    /// with known history unless the history is explicitly replaced or cleared.
    /// See the [crate-level history contract](crate).
    ///
    /// Reuses constructor validation for the resulting contract. Failure leaves
    /// this value unchanged.
    pub fn replace(&self, changes: AccumulatorChanges) -> Result<Self> {
        let history_supplied =
            changes.accumulated_quantity.is_some() && changes.knock_out_settled.is_some();
        let updated = Self::new(AccumulatorTerms {
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
            knock_out_settled: changes
                .knock_out_settled
                .unwrap_or(self.knock_out_settled()),
            effective_time: changes.effective_time.unwrap_or(self.effective_time()),
            expiry_time: changes.expiry_time.unwrap_or(self.expiry_time()),
            observation_clock_time: changes
                .observation_clock_time
                .unwrap_or(self.observation_clock_time()),
            knock_out_observation_times: changes.knock_out_observation_times.unwrap_or_else(|| {
                self.knock_out_observation_times()
                    .map(<[Timestamp]>::to_vec)
            }),
        })?;

        super::validate_history_replacement(
            self.accumulated_quantity().is_some() || self.knock_out_settled().is_some(),
            history_supplied,
            self.history_time.is_none() && self.expiry_time() != updated.expiry_time()
                || (
                    self.option_type(),
                    self.knock_out_level(),
                    self.effective_time(),
                    self.observation_clock_time(),
                ) != (
                    updated.option_type(),
                    updated.knock_out_level(),
                    updated.effective_time(),
                    updated.observation_clock_time(),
                )
                || !(self.knock_out_settled() == Some(true)
                    && updated.knock_out_settled() == Some(true))
                    && (
                        self.strike(),
                        self.daily_quantity(),
                        self.acceleration_factor(),
                    ) != (
                        updated.strike(),
                        updated.daily_quantity(),
                        updated.acceleration_factor(),
                    )
                || self.calendar() != updated.calendar()
                    && super::historical_times(
                        self.accumulation_schedule()?.times(),
                        self.history_time,
                    ) != super::historical_times(
                        updated.accumulation_schedule()?.times(),
                        self.history_time,
                    )
                || (self.knock_out_observation_times() != updated.knock_out_observation_times()
                    || self.calendar() != updated.calendar())
                    && super::historical_times(
                        self.knock_out_observation_schedule()?.times(),
                        self.history_time,
                    ) != super::historical_times(
                        updated.knock_out_observation_schedule()?.times(),
                        self.history_time,
                    ),
            "accumulated_quantity and knock_out_settled",
        )?;
        updated.with_history_time(changes.history_time.unwrap_or(self.history_time))
    }
}
