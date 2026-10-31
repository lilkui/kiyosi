use crate::{
    BarrierTouchState, BarrierType, Date, Error, ObservationMode, ObservationSchedule,
    OptionContract, OptionType, PricingContext, Result, Timestamp, TradingCalendar,
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
    effective_date: Date,
    expiry_date: Date,
    barrier_level: f64,
    barrier_type: BarrierType,
    observation_mode: ObservationMode,
    schedule: ObservationSchedule,
    touch_state: Option<BarrierTouchState>,
}

impl Monitoring {
    pub(crate) fn new(
        effective_date: Date,
        expiry_date: Date,
        barrier_level: f64,
        barrier_type: BarrierType,
        observation_mode: ObservationMode,
        observation_dates: Vec<Date>,
        touch_state: Option<BarrierTouchState>,
    ) -> Result<Self> {
        if !barrier_level.is_finite() || barrier_level <= 0.0 {
            return Err(Error::InvalidInput(
                "barrier level must be finite and positive".into(),
            ));
        }
        if (observation_mode == ObservationMode::Continuous) != observation_dates.is_empty() {
            return Err(Error::InvalidInput(
                "monitoring mode and dates disagree".into(),
            ));
        }
        let schedule = ObservationSchedule::new(
            observation_dates,
            effective_date,
            expiry_date,
            &TradingCalendar::all_days(),
        )?;
        Ok(Self {
            effective_date,
            expiry_date,
            barrier_level,
            barrier_type,
            observation_mode,
            schedule,
            touch_state,
        })
    }

    pub(crate) fn replace(&self, touch_state: Option<BarrierTouchState>) -> Self {
        Self {
            touch_state,
            ..self.clone()
        }
    }
    pub(crate) fn effective_date(&self) -> Date {
        self.effective_date
    }
    pub(crate) fn expiry_date(&self) -> Date {
        self.expiry_date
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
    pub(crate) fn observation_dates(&self) -> &[Date] {
        self.schedule.dates()
    }
    pub(crate) fn touch_state(&self) -> Option<BarrierTouchState> {
        self.touch_state
    }
    pub(crate) fn mean_observation_year_fraction(&self) -> f64 {
        self.observation_dates().last().map_or(0.0, |&last| {
            self.effective_date().days_until(last) as f64 / 365.0 / self.schedule.len() as f64
        })
    }
    pub(crate) fn continuous(&self) -> bool {
        self.observation_mode() == ObservationMode::Continuous
    }
    pub(crate) fn monitored(&self, time: Timestamp) -> bool {
        time >= self.effective_date().start_of_day()
            && time <= self.expiry_date().start_of_day()
            && (self.continuous()
                || self.observation_dates().binary_search(&time.date()).is_ok()
                    && time == time.date().start_of_day())
    }
    pub(crate) fn prior(&self, time: Timestamp) -> Result<bool> {
        let had = if self.continuous() {
            time > self.effective_date().start_of_day()
        } else {
            self.observation_dates()[0].start_of_day() < time
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
        let hit = (if self.barrier_type().is_up() {
            context.spot_price() >= self.barrier_level()
        } else {
            context.spot_price() <= self.barrier_level()
        }) || upper.is_some_and(|upper| context.spot_price() >= upper);
        Ok((
            prior,
            prior || self.monitored(time) && hit,
            time == self.expiry_date().start_of_day()
                || !self.continuous()
                    && self
                        .observation_dates()
                        .last()
                        .is_some_and(|d| d.start_of_day() <= time),
        ))
    }
    pub(crate) fn payoff_requires_touch(&self, option_type: OptionType, strike: f64) -> bool {
        (self.continuous() || self.observation_dates().last() == Some(&self.expiry_date()))
            && if self.barrier_type().is_up() {
                option_type == OptionType::Call && strike >= self.barrier_level()
            } else {
                option_type == OptionType::Put && strike <= self.barrier_level()
            }
    }
    pub(crate) fn spot_discontinuity(
        &self,
        context: &PricingContext,
        radius: f64,
        upper: Option<f64>,
    ) -> bool {
        self.touch_state() != Some(BarrierTouchState::Touched)
            && self.monitored(context.valuation_time())
            && (context.spot_price() - radius <= self.barrier_level()
                && self.barrier_level() <= context.spot_price() + radius
                || upper.is_some_and(|upper| {
                    context.spot_price() - radius <= upper && upper <= context.spot_price() + radius
                }))
    }
}
impl OptionContract for Monitoring {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
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
                    .observation_dates()
                    .iter()
                    .any(|d| before <= d.start_of_day() && d.start_of_day() <= after))
    }
}

pub(crate) fn validate_life(
    option: &impl crate::OptionContract,
    context: &PricingContext,
) -> Result<()> {
    let time = context.valuation_time();
    if time < option.effective_date().start_of_day() || time > option.expiry_date().start_of_day() {
        return Err(Error::InvalidInput(
            "valuation outside contract life".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_schedule(
    schedule: &ObservationSchedule,
    context: &PricingContext,
) -> Result<()> {
    if schedule
        .dates()
        .iter()
        .any(|&d| !context.calendar().is_trading_day(d))
    {
        return Err(Error::InvalidInput(
            "observation is not a trading day".into(),
        ));
    }
    Ok(())
}
