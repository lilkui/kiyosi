use std::cmp::Ordering;

use crate::{
    BarrierTouchState, BarrierType, Error, ObservationMode, ObservationSchedule, OptionContract,
    OptionType, PricingContext, Result, SettlementTiming, Timestamp, TradingCalendar,
};

pub(crate) fn validate_upper(lower: f64, upper: f64) -> Result<()> {
    if !upper.is_finite() || upper <= lower {
        return Err(Error::InvalidInput(
            "double barriers require finite 0 < lower < upper".into(),
        ));
    }
    Ok(())
}

pub(crate) enum MonitoringInput {
    Schedule(Option<ObservationSchedule>),
    Times(Vec<Timestamp>),
}

impl MonitoringInput {
    pub(crate) fn new(
        schedule: Option<ObservationSchedule>,
        raw_times: Option<Vec<Timestamp>>,
    ) -> Self {
        match raw_times {
            Some(times) => Self::Times(times),
            None => Self::Schedule(schedule),
        }
    }

    fn resolve(
        self,
        start: Timestamp,
        end: Timestamp,
        calendar: Option<TradingCalendar>,
    ) -> Result<ObservationSchedule> {
        let scheduled_empty = match &self {
            Self::Schedule(schedule) => {
                schedule.as_ref().is_some_and(ObservationSchedule::is_empty)
            }
            Self::Times(times) => times.is_empty(),
        };
        if scheduled_empty {
            return Err(Error::InvalidInput(
                "scheduled monitoring requires at least one observation time".into(),
            ));
        }
        match self {
            Self::Schedule(Some(schedule)) => schedule.for_contract(start, end, calendar),
            Self::Schedule(None) => {
                ObservationSchedule::new(Vec::new(), start, end, &calendar.unwrap_or_default())
            }
            Self::Times(times) => {
                ObservationSchedule::new(times, start, end, &calendar.unwrap_or_default())
            }
        }
    }
}

#[expect(
    clippy::option_option,
    reason = "Replacement distinguishes retaining, clearing and supplying a schedule."
)]
pub(crate) fn replace_schedule(
    current: Option<&ObservationSchedule>,
    current_calendar: &TradingCalendar,
    replacement: Option<Option<ObservationSchedule>>,
    calendar: Option<TradingCalendar>,
) -> (Option<ObservationSchedule>, Option<TradingCalendar>) {
    match replacement {
        Some(Some(schedule)) => (Some(schedule), calendar),
        Some(None) => (None, calendar.or_else(|| Some(current_calendar.clone()))),
        None => {
            let calendar = calendar.or_else(|| current.is_none().then(|| current_calendar.clone()));
            (current.cloned(), calendar)
        }
    }
}

// Keep raw-iterable adapters out of the standalone contract types.
pub mod binding {
    use crate::{
        BarrierOption, BarrierOptionChanges, BarrierOptionTerms, BinaryBarrierOption,
        BinaryBarrierOptionChanges, BinaryBarrierTerms, BinaryPayoff, DoubleBarrierOption,
        DoubleBarrierOptionChanges, DoubleBarrierOptionTerms, DoubleTouchOption,
        DoubleTouchOptionChanges, DoubleTouchOptionTerms, ObservationSchedule, Result, Timestamp,
        TouchOption, TouchOptionChanges, TouchOptionTerms, TradingCalendar,
    };

    #[doc(hidden)]
    pub fn requires_calendar_scan(
        schedule: &ObservationSchedule,
        calendar: &TradingCalendar,
    ) -> bool {
        !schedule.calendar().shares_trading_days(calendar)
    }

    macro_rules! monitoring_adapter {
        ($new:ident, $replace:ident, $option:ident, $terms:ident, $changes:ident,
         |$input:ident, $times:ident $(, $payoff:ident : $payoff_type:ty)?| $construct:expr) => {
            #[doc(hidden)]
            pub fn $new(
                $input: $terms,
                $times: Option<Vec<Timestamp>>,
                $($payoff: $payoff_type,)?
            ) -> Result<$option> {
                $construct
            }

            #[doc(hidden)]
            pub fn $replace(
                option: &$option,
                changes: $changes,
                times: Option<Vec<Timestamp>>,
            ) -> Result<$option> {
                option.replace_with_observation_times(changes, times)
            }
        };
    }

    monitoring_adapter!(
        new_barrier,
        replace_barrier,
        BarrierOption,
        BarrierOptionTerms,
        BarrierOptionChanges,
        |terms, times| BarrierOption::new_with_observation_times(terms, times)
    );
    monitoring_adapter!(
        new_double_barrier,
        replace_double_barrier,
        DoubleBarrierOption,
        DoubleBarrierOptionTerms,
        DoubleBarrierOptionChanges,
        |terms, times| DoubleBarrierOption::new_with_observation_times(terms, times)
    );
    monitoring_adapter!(
        new_binary_barrier,
        replace_binary_barrier,
        BinaryBarrierOption,
        BinaryBarrierTerms,
        BinaryBarrierOptionChanges,
        |terms, times, payoff: BinaryPayoff| match payoff {
            BinaryPayoff::Cash(payout) =>
                BinaryBarrierOption::cash_with_observation_times(terms, payout, times),
            BinaryPayoff::Asset => BinaryBarrierOption::asset_with_observation_times(terms, times),
        }
    );
    monitoring_adapter!(
        new_touch,
        replace_touch,
        TouchOption,
        TouchOptionTerms,
        TouchOptionChanges,
        |terms, times, payoff: BinaryPayoff| match payoff {
            BinaryPayoff::Cash(payout) =>
                TouchOption::cash_with_observation_times(terms, payout, times),
            BinaryPayoff::Asset => TouchOption::asset_with_observation_times(terms, times),
        }
    );
    monitoring_adapter!(
        new_double_touch,
        replace_double_touch,
        DoubleTouchOption,
        DoubleTouchOptionTerms,
        DoubleTouchOptionChanges,
        |terms, times, payout: f64| {
            DoubleTouchOption::cash_with_observation_times(terms, payout, times)
        }
    );
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Monitoring {
    effective_time: Timestamp,
    expiry_time: Timestamp,
    barrier_level: f64,
    barrier_type: BarrierType,
    schedule: ObservationSchedule,
    touch_state: Option<BarrierTouchState>,
}

#[derive(PartialEq, Eq)]
enum DeterministicTouch {
    Boundary,
    Confirmed,
}

impl Monitoring {
    pub(crate) fn new(
        effective_time: Timestamp,
        expiry_time: Timestamp,
        barrier_level: f64,
        barrier_type: BarrierType,
        observation_schedule: MonitoringInput,
        touch_state: Option<BarrierTouchState>,
        calendar: Option<TradingCalendar>,
    ) -> Result<Self> {
        if !barrier_level.is_finite() || barrier_level <= 0.0 {
            return Err(Error::InvalidInput(
                "barrier level must be finite and positive".into(),
            ));
        }
        let schedule = observation_schedule.resolve(effective_time, expiry_time, calendar)?;
        Ok(Self {
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            schedule,
            touch_state,
        })
    }

    pub(crate) fn calendar(&self) -> &TradingCalendar {
        self.schedule.calendar()
    }
    pub(crate) fn effective_time(&self) -> Timestamp {
        self.effective_time
    }
    pub(crate) fn expiry_time(&self) -> Timestamp {
        self.expiry_time
    }
    pub(crate) fn barrier_level(&self) -> f64 {
        self.barrier_level
    }
    pub(crate) fn barrier_type(&self) -> BarrierType {
        self.barrier_type
    }
    pub(crate) fn observation_mode(&self) -> ObservationMode {
        if self.continuous() {
            ObservationMode::Continuous
        } else {
            ObservationMode::Scheduled
        }
    }
    pub(crate) fn observation_schedule(&self) -> &ObservationSchedule {
        &self.schedule
    }
    pub(crate) fn observation_times(&self) -> &[Timestamp] {
        self.schedule.times()
    }
    pub(crate) fn touch_state(&self) -> Option<BarrierTouchState> {
        self.touch_state
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "Day and observation counts are bounded by the supported calendar range and fit exactly in f64."
    )]
    pub(crate) fn mean_observation_year_fraction(&self) -> f64 {
        self.observation_times().last().map_or(0.0, |&last| {
            self.effective_time().years_until(last) / self.schedule.len() as f64
        })
    }
    pub(crate) fn continuous(&self) -> bool {
        self.schedule.is_empty()
    }
    pub(crate) fn monitored(&self, time: Timestamp) -> bool {
        time >= self.effective_time()
            && time <= self.expiry_time()
            && (self.continuous() || self.observation_times().binary_search(&time).is_ok())
    }
    pub(crate) fn prior(&self, time: Timestamp) -> Result<bool> {
        let had = if self.continuous() {
            time > self.effective_time()
        } else {
            self.observation_times()[0] < time
        };
        if !had && self.touch_state() == Some(BarrierTouchState::Touched) {
            return Err(Error::InvalidInput("touch predates monitoring".into()));
        }
        if had && self.touch_state().is_none() {
            return Err(Error::MissingHistory(
                "prior barrier touch state is required".into(),
            ));
        }
        Ok(self.touch_state() == Some(BarrierTouchState::Touched))
    }
    pub(crate) fn state(
        &self,
        context: &PricingContext,
        upper: Option<f64>,
    ) -> Result<(bool, bool, bool)> {
        let time = context.valuation_time();
        let prior = self.prior(time)?;

        let hit = (if self.barrier_type().is_up() {
            context.spot_price() >= self.barrier_level()
        } else {
            context.spot_price() <= self.barrier_level()
        }) || upper.is_some_and(|upper| context.spot_price() >= upper);
        Ok((
            prior,
            prior || self.monitored(time) && hit,
            time == self.expiry_time()
                || !self.continuous()
                    && self.observation_times().last().is_some_and(|d| *d <= time),
        ))
    }
    pub(crate) fn payoff_requires_touch(&self, option_type: OptionType, strike: f64) -> bool {
        (self.continuous() || self.observation_times().last() == Some(&self.expiry_time()))
            && if self.barrier_type().is_up() {
                option_type == OptionType::Call && strike >= self.barrier_level()
            } else {
                option_type == OptionType::Put && strike <= self.barrier_level()
            }
    }
    pub(crate) fn live_payoff_discontinuity(
        &self,
        context: &PricingContext,
        radius: f64,
        upper: Option<f64>,
        payoff_discontinuity: impl FnOnce() -> bool,
    ) -> bool {
        let Ok((_, touched, finished)) = self.state(context, upper) else {
            return payoff_discontinuity();
        };
        if self.barrier_type().is_knock_in() {
            (touched || !finished) && payoff_discontinuity()
        } else {
            !touched
                && payoff_discontinuity()
                && self.deterministic_touch(context, radius, upper, SettlementTiming::AtExpiry)
                    != Some(DeterministicTouch::Confirmed)
        }
    }
    fn deterministic_touch(
        &self,
        context: &PricingContext,
        radius: f64,
        upper: Option<f64>,
        timing: SettlementTiming,
    ) -> Option<DeterministicTouch> {
        let start = context.valuation_time();
        let end = context.deterministic_until(self.expiry_time()).ok()?;
        if end <= start {
            return None;
        }
        let lower_spot = (context.spot_price() - radius).max(0.0);
        let upper_spot = context.spot_price() + radius;
        let check_fixing = |time| {
            let carry = context.forward_carry(time).ok()?;
            let compare = |level, up| {
                let low = context.forward_spot_cmp(time, lower_spot, level, carry)?;
                let high = context.forward_spot_cmp(time, upper_spot, level, carry)?;
                if low.is_le() && high.is_ge() {
                    Some(DeterministicTouch::Boundary)
                } else if (up && low == Ordering::Greater) || (!up && high == Ordering::Less) {
                    // Every spot has touched. Later fixings cannot change that event.
                    Some(DeterministicTouch::Confirmed)
                } else {
                    None
                }
            };
            compare(self.barrier_level(), self.barrier_type().is_up())
                .or_else(|| upper.and_then(|level| compare(level, true)))
        };
        let mut boundary = None;
        let mut relevant_touch = |time| match check_fixing(time) {
            Some(DeterministicTouch::Boundary) if timing == SettlementTiming::AtExpiry => {
                // Later guaranteed touches can resolve whether the expiry payoff is activated.
                boundary = Some(DeterministicTouch::Boundary);
                None
            }
            touch => touch,
        };
        let running = |_: &Timestamp| crate::cancellation::check().is_ok();
        let touch = if self.continuous() {
            let model = context.model_parameters();
            let mut risk = model
                .risk_free_curve()
                .nodes_after(start)
                .iter()
                .copied()
                .peekable();
            let mut dividend = model
                .dividend_curve()
                .nodes_after(start)
                .iter()
                .copied()
                .peekable();
            let knots = std::iter::from_fn(|| match (risk.peek(), dividend.peek()) {
                (Some(r), Some(q)) if r <= q => risk.next(),
                (_, Some(_)) => dividend.next(),
                _ => risk.next(),
            });
            std::iter::once(start)
                .chain(knots.take_while(|&time| time < end))
                .chain([end])
                .take_while(running)
                .find_map(&mut relevant_touch)
        } else {
            let times = self.observation_times();
            times[times.partition_point(|&time| time < start)..]
                .iter()
                .copied()
                .take_while(|&time| time <= end)
                .take_while(running)
                .find_map(&mut relevant_touch)
        };
        touch.or(boundary)
    }

    pub(crate) fn all_greeks_unavailable(
        &self,
        context: &PricingContext,
        upper: Option<f64>,
        timing: SettlementTiming,
    ) -> bool {
        self.state(context, upper)
            .is_ok_and(|(_, touched, _)| !touched)
            && self.deterministic_touch(context, 0.0, upper, timing)
                == Some(DeterministicTouch::Boundary)
    }

    pub(crate) fn spot_discontinuity(
        &self,
        context: &PricingContext,
        radius: f64,
        upper: Option<f64>,
        timing: SettlementTiming,
    ) -> bool {
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        self.touch_state() != Some(BarrierTouchState::Touched)
            && (self.monitored(context.valuation_time())
                && (crosses(self.barrier_level()) || upper.is_some_and(crosses))
                || self.deterministic_touch(context, radius, upper, timing)
                    == Some(DeterministicTouch::Boundary))
    }
}
impl OptionContract for Monitoring {
    fn effective_time(&self) -> Timestamp {
        self.effective_time()
    }
    fn expiry_time(&self) -> Timestamp {
        self.expiry_time()
    }
    fn time_stencil_available(
        &self,
        _context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        let times = self.observation_times();
        self.prior(before).is_ok()
            && self.prior(after).is_ok()
            && (self.continuous()
                || self.touch_state() == Some(BarrierTouchState::Touched)
                || times
                    .get(times.partition_point(|&time| time < before))
                    .is_none_or(|&time| time > after))
    }
}

pub(crate) fn validate_life(
    option: &impl crate::OptionContract,
    context: &PricingContext,
) -> Result<()> {
    let time = context.valuation_time();
    if option.valuation_start().is_some_and(|start| time < start) || time > option.expiry_time() {
        return Err(Error::InvalidInput(
            "valuation outside contract life".into(),
        ));
    }
    Ok(())
}
