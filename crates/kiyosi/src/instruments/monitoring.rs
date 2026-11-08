use crate::{
    BarrierTouchState, BarrierType, Error, ObservationMode, ObservationSchedule, OptionContract,
    OptionType, PricingContext, Result, Timestamp, TradingCalendar,
};

pub(crate) fn validate_upper(lower: f64, upper: f64) -> Result<()> {
    if !upper.is_finite() || upper <= lower {
        return Err(Error::InvalidInput(
            "double barriers require finite 0 < lower < upper".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Monitoring {
    history_time: Option<Timestamp>,
    effective_time: Timestamp,
    expiry_time: Timestamp,
    barrier_level: f64,
    barrier_type: BarrierType,
    observation_mode: ObservationMode,
    schedule: ObservationSchedule,
    calendar: TradingCalendar,
    touch_state: Option<BarrierTouchState>,
}

impl Monitoring {
    #[expect(
        clippy::too_many_arguments,
        reason = "complete monitoring terms include their contractual calendar"
    )]
    pub(crate) fn new(
        effective_time: Timestamp,
        expiry_time: Timestamp,
        barrier_level: f64,
        barrier_type: BarrierType,
        observation_mode: ObservationMode,
        observation_times: Vec<Timestamp>,
        touch_state: Option<BarrierTouchState>,
        calendar: TradingCalendar,
    ) -> Result<Self> {
        if !barrier_level.is_finite() || barrier_level <= 0.0 {
            return Err(Error::InvalidInput(
                "barrier level must be finite and positive".into(),
            ));
        }
        if (observation_mode == ObservationMode::Continuous) != observation_times.is_empty() {
            return Err(Error::InvalidInput(
                "monitoring mode and dates disagree".into(),
            ));
        }
        let schedule =
            ObservationSchedule::new(observation_times, effective_time, expiry_time, &calendar)?;
        Ok(Self {
            history_time: None,
            effective_time,
            expiry_time,
            barrier_level,
            barrier_type,
            observation_mode,
            schedule,
            calendar,
            touch_state,
        })
    }

    pub(crate) fn replacement_times(
        &self,
        mode: Option<ObservationMode>,
        times: Option<Vec<Timestamp>>,
    ) -> Vec<Timestamp> {
        times.unwrap_or_else(|| {
            if mode == Some(ObservationMode::Continuous) {
                Vec::new()
            } else {
                self.observation_times().to_vec()
            }
        })
    }
    pub(crate) fn validate_replacement_history(
        &self,
        updated: &Self,
        supplied: bool,
        upper_changed: bool,
    ) -> Result<()> {
        super::validate_history_replacement(
            self.touch_state.is_some(),
            supplied,
            upper_changed
                || self.history_time.is_none() && self.expiry_time != updated.expiry_time
                || (
                    self.effective_time,
                    self.barrier_level,
                    self.barrier_type.is_up(),
                    self.observation_mode,
                    super::historical_times(self.schedule.times(), self.history_time),
                ) != (
                    updated.effective_time,
                    updated.barrier_level,
                    updated.barrier_type.is_up(),
                    updated.observation_mode,
                    super::historical_times(updated.schedule.times(), self.history_time),
                ),
            "touch_state",
        )
    }
    pub(crate) fn replace_calendar(&self, calendar: TradingCalendar) -> Result<Self> {
        validate_schedule(&self.schedule, &calendar)?;
        let mut updated = self.clone();
        updated.calendar = calendar;
        Ok(updated)
    }
    pub(crate) fn calendar(&self) -> &TradingCalendar {
        &self.calendar
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
        self.observation_mode
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
        self.observation_mode() == ObservationMode::Continuous
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
            return Err(Error::InvalidInput(
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
        super::validate_history_time(
            self,
            self.touch_state.is_some(),
            context,
            self.continuous() && !prior,
        )?;
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
    pub(crate) fn payoff_terminated(&self, context: &PricingContext, upper: Option<f64>) -> bool {
        self.state(context, upper)
            .is_ok_and(|(_, touched, finished)| {
                if self.barrier_type().is_knock_in() {
                    !touched && finished
                } else {
                    touched
                }
            })
    }
    fn deterministic_discontinuity(
        &self,
        context: &PricingContext,
        radius: f64,
        upper: Option<f64>,
    ) -> bool {
        let Ok(model) = context.model_parameters().as_bsm() else {
            return false;
        };
        if model.volatility() != 0.0 {
            return false;
        }
        let mut times = self.observation_times().to_vec();
        if self.continuous() {
            times.extend([context.valuation_time(), self.expiry_time()]);
            for curve in [model.risk_free_curve(), model.dividend_curve()] {
                times.extend(
                    curve
                        .nodes()
                        .iter()
                        .copied()
                        .filter(|&time| time <= self.expiry_time()),
                );
            }
        }
        times
            .into_iter()
            .filter(|&time| time >= context.valuation_time())
            .any(|time| {
                context.deterministic_crosses(time, self.barrier_level(), radius)
                    || upper.is_some_and(|level| context.deterministic_crosses(time, level, radius))
            })
    }

    pub(crate) fn all_greeks_unavailable(
        &self,
        context: &PricingContext,
        upper: Option<f64>,
    ) -> bool {
        self.state(context, upper)
            .is_ok_and(|(_, touched, _)| !touched)
            && self.deterministic_discontinuity(context, 0.0, upper)
    }

    pub(crate) fn spot_discontinuity(
        &self,
        context: &PricingContext,
        radius: f64,
        upper: Option<f64>,
    ) -> bool {
        let spot = context.spot_price();
        let crosses = |level| spot - radius <= level && level <= spot + radius;
        self.touch_state() != Some(BarrierTouchState::Touched)
            && (self.deterministic_discontinuity(context, radius, upper)
                || self.monitored(context.valuation_time())
                    && (crosses(self.barrier_level()) || upper.is_some_and(crosses)))
    }
}
impl OptionContract for Monitoring {
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
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.spot_discontinuity(context, radius, None)
    }
    fn time_stencil_available(
        &self,
        _context: &PricingContext,
        before: Timestamp,
        after: Timestamp,
    ) -> bool {
        self.prior(before).is_ok()
            && self.prior(after).is_ok()
            && (self.continuous()
                || self.touch_state() == Some(BarrierTouchState::Touched)
                || !self
                    .observation_times()
                    .iter()
                    .any(|d| before <= *d && *d <= after))
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

pub(crate) fn validate_schedule(
    schedule: &ObservationSchedule,
    calendar: &TradingCalendar,
) -> Result<()> {
    if schedule
        .times()
        .iter()
        .any(|&d| !calendar.is_trading_day(d.date()))
    {
        return Err(Error::InvalidInput(
            "observation is not a trading day".into(),
        ));
    }
    Ok(())
}
