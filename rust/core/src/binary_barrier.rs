use crate::barrier::{
    analytic_log_shift, checked, hit_value, log_ratio, reflection_tail, scaled_cash, scaled_cdf,
    standardize,
};
use crate::{
    AnalyticDigitalEngine, BarrierOption, BarrierOptionTerms, BarrierTouchState, BarrierType, Date,
    DigitalOption, Error, ErrorCategory, GreeksRequest, NumericalShiftSettings, ObservationMode,
    ObservationSchedule, OptionContract, OptionType, PricingContext, PricingEngine, PricingResult,
    Result, Timestamp,
};

/// Payment timing for a touch contract; see [`TouchOption::cash`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettlementTiming {
    /// Pays on the first touch; available only for one-touch contracts.
    AtHit,
    /// Pays at expiry (the default).
    #[default]
    AtExpiry,
}

/// Binary strike and monitoring inputs; validated by [`BinaryBarrierOption::cash`] or
/// [`BinaryBarrierOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinaryBarrierTerms {
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike; terminal equality pays zero.
    pub strike: f64,
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite trigger; equality touches.
    pub barrier_level: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: BarrierType,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing dates, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl BinaryBarrierTerms {
    /// Supplies continuous monitoring and unknown history without validation.
    /// See [`BinaryBarrierOption::cash`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            option_type,
            strike,
            effective_date,
            expiry_date,
            barrier_level,
            barrier_type,
            observation_mode: ObservationMode::default(),
            observation_dates: Vec::new(),
            touch_state: None,
        }
    }
    fn monitoring(self) -> Result<BarrierOption> {
        let mut terms = BarrierOptionTerms::new(
            self.option_type,
            self.strike,
            self.effective_date,
            self.expiry_date,
            self.barrier_level,
            self.barrier_type,
        );
        terms.observation_mode = self.observation_mode;
        terms.observation_dates = self.observation_dates;
        terms.touch_state = self.touch_state;
        BarrierOption::new(terms)
    }
}

/// Owns a validated strike-based binary barrier paying cash or the asset at expiry.
/// Shared reads and pricing are safe; replacement returns an independent owner.
/// See [`Self::cash`] and [`Self::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct BinaryBarrierOption {
    monitoring: BarrierOption,
    digital: DigitalOption,
}
impl BinaryBarrierOption {
    /// Creates a positive-cash binary contract, paying strictly in the money if active.
    /// Validates payout, strike and life before barrier and schedule terms.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for invalid payout or barrier,
    /// [`ErrorCategory::InvalidStrike`] for invalid strike,
    /// [`ErrorCategory::InvalidTimeRange`] for reversed life, or
    /// [`ErrorCategory::InvalidSchedule`] for incompatible or invalid fixing dates.
    /// History relative to valuation and calendar compatibility are checked during pricing.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let terms = BinaryBarrierTerms::new(OptionType::Call, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut);
    /// let option = BinaryBarrierOption::cash(terms, 10.0)?;
    /// assert_eq!(option.payout(), Some(10.0));
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: BinaryBarrierTerms, payout: f64) -> Result<Self> {
        let digital = DigitalOption::cash_or_nothing(
            terms.option_type,
            terms.strike,
            payout,
            terms.effective_date,
            terms.expiry_date,
        )?;
        Ok(Self {
            monitoring: terms.monitoring()?,
            digital,
        })
    }
    /// Creates an asset binary with the same monitoring and strict payoff as [`Self::cash`].
    ///
    /// # Errors
    /// Reuses [`Self::cash`] validation without a cash payout.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = BinaryBarrierOption::asset(BinaryBarrierTerms::new(OptionType::Put, 100.0,
    ///     Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 80.0, BarrierType::DownAndIn))?;
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: BinaryBarrierTerms) -> Result<Self> {
        let digital = DigitalOption::asset_or_nothing(
            terms.option_type,
            terms.strike,
            terms.effective_date,
            terms.expiry_date,
        )?;
        Ok(Self {
            monitoring: terms.monitoring()?,
            digital,
        })
    }
    /// Returns the call-or-put direction; see [`Self::cash`].
    pub fn option_type(&self) -> OptionType {
        self.digital.option_type()
    }
    /// Returns the positive strike; see [`Self::cash`].
    pub fn strike(&self) -> f64 {
        self.digital.strike()
    }
    /// Returns cash payout, or `None` for asset settlement; see [`Self::cash`].
    pub fn payout(&self) -> Option<f64> {
        self.digital.payout()
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    /// See [`BarrierOption::replace`] for an example and time consistency rules.
    ///
    /// # Errors
    /// Reuses validated monitoring construction; typed history cannot invalidate construction.
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            monitoring: self.monitoring.replace(touch_state)?,
            digital: self.digital.clone(),
        })
    }
}

/// Strike-free touch inputs; validated by [`TouchOption::cash`] or [`TouchOption::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOptionTerms {
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite barrier; equality touches.
    pub barrier_level: f64,
    /// In means one-touch, out means no-touch; up/down selects direction.
    pub barrier_type: BarrierType,
    /// Payment timing, default expiry; at-hit requires one-touch.
    pub settlement_timing: SettlementTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Ordered fixing dates, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Observations strictly before valuation; `None` means unknown.
    pub touch_state: Option<BarrierTouchState>,
}
impl TouchOptionTerms {
    /// Supplies expiry payment, continuous monitoring and unknown history without validation.
    /// See [`TouchOption::cash`] for validation and an example.
    pub fn new(
        effective_date: Date,
        expiry_date: Date,
        barrier_level: f64,
        barrier_type: BarrierType,
    ) -> Self {
        Self {
            effective_date,
            expiry_date,
            barrier_level,
            barrier_type,
            settlement_timing: SettlementTiming::default(),
            observation_mode: ObservationMode::default(),
            observation_dates: Vec::new(),
            touch_state: None,
        }
    }
    fn monitoring(self) -> Result<BarrierOption> {
        if self.settlement_timing == SettlementTiming::AtHit && !self.barrier_type.is_knock_in() {
            return Err(Error::new(
                ErrorCategory::InvalidOption,
                "at-hit settlement requires a one-touch",
            ));
        }
        // Reuse the validated barrier's monitoring rules; no strike is exposed or used for touch payoffs.
        let mut terms = BarrierOptionTerms::new(
            OptionType::Call,
            self.barrier_level,
            self.effective_date,
            self.expiry_date,
            self.barrier_level,
            self.barrier_type,
        );
        terms.observation_mode = self.observation_mode;
        terms.observation_dates = self.observation_dates;
        terms.touch_state = self.touch_state;
        BarrierOption::new(terms)
    }
}

/// Owns a validated cash/asset one-touch or no-touch contract, with no strike.
/// Prior at-hit touches are already paid; shared reads and pricing are safe.
/// See [`Self::cash`] and [`Self::asset`].
#[derive(Clone, Debug, PartialEq)]
pub struct TouchOption {
    monitoring: BarrierOption,
    payout: Option<f64>,
    settlement_timing: SettlementTiming,
}
impl TouchOption {
    /// Creates a cash touch contract; one-touch pays on a hit, no-touch on its absence.
    /// Current at-hit cash pays the payout; prior at-hit settlement has zero remaining value.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] for invalid positive payout or barrier,
    /// [`ErrorCategory::InvalidTimeRange`] for reversed life,
    /// [`ErrorCategory::InvalidSchedule`] for invalid fixings, or
    /// [`ErrorCategory::InvalidOption`] for at-hit no-touch settlement.
    /// Valuation-relative history and calendar checks occur during pricing.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::cash(TouchOptionTerms::new(Date::new(2025, 1, 1)?,
    ///     Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndIn), 10.0)?;
    /// assert!(option.is_one_touch());
    /// assert_eq!(option.settlement_timing(), SettlementTiming::AtExpiry);
    /// # Ok(()) }
    /// ```
    pub fn cash(terms: TouchOptionTerms, payout: f64) -> Result<Self> {
        if !payout.is_finite() || payout <= 0.0 {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "payout must be finite and positive",
            ));
        }
        let settlement_timing = terms.settlement_timing;
        Ok(Self {
            monitoring: terms.monitoring()?,
            payout: Some(payout),
            settlement_timing,
        })
    }
    /// Creates an asset touch contract with [`Self::cash`]'s monitoring rules.
    /// A current at-hit touch pays spot; future continuous hits transfer the barrier amount.
    ///
    /// # Errors
    /// Reuses [`Self::cash`] validation without a cash payout.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = TouchOption::asset(TouchOptionTerms::new(Date::new(2025, 1, 1)?,
    ///     Date::new(2026, 1, 1)?, 80.0, BarrierType::DownAndOut))?;
    /// assert!(!option.is_one_touch());
    /// assert_eq!(option.payout(), None);
    /// # Ok(()) }
    /// ```
    pub fn asset(terms: TouchOptionTerms) -> Result<Self> {
        let settlement_timing = terms.settlement_timing;
        Ok(Self {
            monitoring: terms.monitoring()?,
            payout: None,
            settlement_timing,
        })
    }
    /// Returns cash payout, or `None` for asset settlement; see [`Self::cash`].
    pub fn payout(&self) -> Option<f64> {
        self.payout
    }
    /// Returns payment timing; see [`Self::cash`].
    pub fn settlement_timing(&self) -> SettlementTiming {
        self.settlement_timing
    }
    /// Reports whether a touch activates payment; see [`Self::cash`].
    pub fn is_one_touch(&self) -> bool {
        self.barrier_type().is_knock_in()
    }
    /// Reports whether the barrier triggers upward; see [`Self::cash`].
    pub fn is_up(&self) -> bool {
        self.barrier_type().is_up()
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    /// See [`BarrierOption::replace`] for examples and time consistency rules.
    ///
    /// # Errors
    /// Reuses validated monitoring construction; typed history cannot invalidate construction.
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            monitoring: self.monitoring.replace(touch_state)?,
            payout: self.payout,
            settlement_timing: self.settlement_timing,
        })
    }
}

macro_rules! monitoring_accessors {
    ($contract:ty) => {
        impl $contract {
            /// Returns the inclusive first date; see the cash constructor.
            pub fn effective_date(&self) -> Date {
                self.monitoring.effective_date()
            }
            /// Returns expiry at midnight UTC; see the cash constructor.
            pub fn expiry_date(&self) -> Date {
                self.monitoring.expiry_date()
            }
            /// Returns the positive barrier; see the cash constructor.
            pub fn barrier_level(&self) -> f64 {
                self.monitoring.barrier_level()
            }
            /// Returns direction and activation behavior; see the cash constructor.
            pub fn barrier_type(&self) -> BarrierType {
                self.monitoring.barrier_type()
            }
            /// Returns monitoring frequency; see the cash constructor.
            pub fn observation_mode(&self) -> ObservationMode {
                self.monitoring.observation_mode()
            }
            /// Borrows immutable schedule storage for this owner's lifetime; see the cash constructor.
            pub fn observation_schedule(&self) -> &ObservationSchedule {
                self.monitoring.observation_schedule()
            }
            /// Borrows ordered dates for this owner's lifetime; see the cash constructor.
            pub fn observation_dates(&self) -> &[Date] {
                self.monitoring.observation_dates()
            }
            /// Returns mean fixing spacing in Actual/365 Fixed years; see the cash constructor.
            pub fn mean_observation_year_fraction(&self) -> f64 {
                self.monitoring.mean_observation_year_fraction()
            }
            /// Returns strictly prior history; see the replace method.
            pub fn touch_state(&self) -> Option<BarrierTouchState> {
                self.monitoring.touch_state()
            }
        }
        impl OptionContract for $contract {
            fn effective_date(&self) -> Date {
                self.effective_date()
            }
            fn expiry_date(&self) -> Date {
                self.expiry_date()
            }
            fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
                self.monitoring.at_spot_discontinuity(context, radius)
            }
            fn time_stencil_available(&self, before: Timestamp, after: Timestamp) -> bool {
                self.monitoring.time_stencil_available(before, after)
            }
        }
    };
}
monitoring_accessors!(BinaryBarrierOption);
monitoring_accessors!(TouchOption);

/// Stateless flat-rate binary-barrier and single-touch analytics; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticBinaryBarrierEngine;
impl AnalyticBinaryBarrierEngine {
    /// Prices a binary barrier or touch contract with strictly prior history.
    /// Equality touches before future monitoring; scheduled unresolved monitoring uses BGK
    /// and must end at expiry. Terminal binary equality at strike pays zero.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::UnsupportedOperation`] for node curves or unsupported schedules,
    /// [`ErrorCategory::InvalidTimeRange`] outside life, [`ErrorCategory::InvalidDate`] for
    /// non-trading fixings, [`ErrorCategory::InvalidParameter`] for missing history,
    /// [`ErrorCategory::InvalidOption`] for impossible history, or
    /// [`ErrorCategory::InvalidResult`] for non-finite results. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let mut terms = TouchOptionTerms::new(start, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndIn);
    /// terms.settlement_timing = SettlementTiming::AtHit;
    /// let option = TouchOption::cash(terms, 10.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 120.0, start.into())?;
    /// assert_eq!(AnalyticBinaryBarrierEngine.price(&option, &context)?, 10.0);
    /// # Ok(()) }
    /// ```
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }
    /// Computes requested numerical Greeks with shared bump defaults; see [`Self::price`].
    /// Greeks at expiry or a current barrier equality are absent. Stencils crossing
    /// observations or barriers leave affected Greeks absent.
    ///
    /// # Errors
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing errors.
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}

pub(crate) struct Contract<'a> {
    pub(crate) monitoring: &'a BarrierOption,
    pub(crate) digital: Option<&'a DigitalOption>,
    pub(crate) payout: Option<f64>,
    pub(crate) timing: SettlementTiming,
}
impl Contract<'_> {
    fn vanilla(&self, context: &PricingContext, time: f64) -> Result<f64> {
        if let Some(digital) = self.digital {
            AnalyticDigitalEngine.price(digital, context)
        } else {
            let model = context.model_parameters();
            checked(scaled_cash(
                self.payout.unwrap_or(context.spot_price()),
                -if self.payout.is_some() {
                    model.risk_free_rate()?
                } else {
                    model.dividend_yield()?
                } * time,
            ))
        }
    }
    pub(crate) fn volatility_affects_price(&self, context: &PricingContext) -> bool {
        let Ok((_, touched, finished)) = self.monitoring.state(context) else {
            return false;
        };
        let knock_in = self.monitoring.barrier_type().is_knock_in();
        if touched || finished {
            return self.digital.is_some() && knock_in == touched;
        }
        !(!knock_in && self.digital.is_some() && self.monitoring.payoff_requires_touch())
    }
    fn price(&self, context: &PricingContext) -> Result<f64> {
        let option = self.monitoring;
        let model = context.model_parameters();
        let rate = model.risk_free_rate()?;
        let dividend = model.dividend_yield()?;
        let valuation = context.valuation_time();
        if valuation < option.effective_date().start_of_day()
            || valuation > option.expiry_date().start_of_day()
        {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "valuation outside contract life",
            ));
        }
        if option
            .observation_dates()
            .iter()
            .any(|&d| !context.calendar().is_trading_day(d))
        {
            return Err(Error::new(
                ErrorCategory::InvalidDate,
                "observation is not a trading day",
            ));
        }
        let (prior, touched, finished) = option.state(context)?;
        let t = valuation.years_until(option.expiry_date().start_of_day());
        let knock_in = option.barrier_type().is_knock_in();
        let at_hit = self.timing == SettlementTiming::AtHit;
        if prior && at_hit {
            return Ok(0.0);
        }
        if t == 0.0 || touched || finished {
            if knock_in != touched {
                return Ok(0.0);
            }
            if at_hit {
                return Ok(self.payout.unwrap_or(context.spot_price()));
            }
            return self.vanilla(context, t);
        }
        if self.digital.is_some() && option.payoff_requires_touch() {
            return if knock_in {
                self.vanilla(context, t)
            } else {
                Ok(0.0)
            };
        }
        let spot = context.spot_price();
        let sigma = model.volatility();
        let upper = option.barrier_type().is_up();
        let shift = analytic_log_shift(option, spot, sigma)?;
        let ratio = log_ratio(option.barrier_level(), spot) + shift;
        if at_hit {
            return hit_value(
                self.payout.unwrap_or(option.barrier_level()),
                ratio.abs(),
                upper,
                rate - dividend - 0.5 * sigma * sigma,
                sigma,
                t,
                rate,
            );
        }
        if self.digital.is_none() && sigma * sigma == 0.0 {
            let probability = hit_value(1.0, ratio.abs(), upper, rate - dividend, sigma, t, 0.0)?;
            let weight = if knock_in {
                probability
            } else {
                1.0 - probability
            };
            return if weight == 0.0 {
                Ok(0.0)
            } else {
                checked(scaled_cash(
                    self.payout.unwrap_or(spot),
                    -if self.payout.is_some() {
                        rate
                    } else {
                        dividend
                    } * t
                        + weight.ln(),
                ))
            };
        }
        let strike = self.digital.map_or(option.barrier_level(), |d| d.strike());
        let strike_above = log_ratio(strike, option.barrier_level()) > shift;
        let direction = self.digital.map(|d| d.option_type());
        let call = direction == Some(OptionType::Call);
        let carry = rate - dividend;
        let width = sigma * t.sqrt();
        let mu = carry / sigma / sigma - 0.5;
        let moneyness = log_ratio(spot, strike);
        let x1 = if mu.is_finite() {
            moneyness / width + (1.0 + mu) * width
        } else {
            standardize(moneyness + carry * t, sigma, t.sqrt()) + 0.5 * width
        };
        let x2 = if mu.is_finite() {
            -ratio / width + (1.0 + mu) * width
        } else {
            standardize(-ratio + carry * t, sigma, t.sqrt()) + 0.5 * width
        };
        let y1 = (2.0 * ratio + moneyness) / width + (1.0 + mu) * width;
        let y2 = ratio / width + (1.0 + mu) * width;
        let phi = if direction.is_some() {
            if call { 1.0 } else { -1.0 }
        } else if knock_in == upper {
            1.0
        } else {
            -1.0
        };
        let eta = if upper { -1.0 } else { 1.0 };
        let asset = self.payout.is_none();
        let amount = self.payout.unwrap_or(spot);
        let discount = -if asset { dividend } else { rate } * t;
        let reflection = 2.0 * (if asset { mu + 1.0 } else { mu }) * ratio;
        let payoff_shift = if asset { 0.0 } else { width };
        let f1 = || scaled_cdf(amount, discount, phi * x1 - phi * payoff_shift);
        let f2 = || scaled_cdf(amount, discount, phi * x2 - phi * payoff_shift);
        let reflected = |argument: f64, at_strike: bool| {
            if reflection.is_finite() {
                scaled_cdf(amount, discount + reflection, argument)
            } else {
                reflection_tail(
                    amount,
                    discount,
                    ratio,
                    if at_strike {
                        log_ratio(option.barrier_level(), strike) + shift
                    } else {
                        0.0
                    },
                    carry,
                    sigma,
                    t,
                    if asset { 0.5 } else { -0.5 },
                )
            }
        };
        let f3 = || reflected(eta * y1 - eta * payoff_shift, true);
        let f4 = || reflected(eta * y2 - eta * payoff_shift, false);
        let value = if direction.is_none() {
            if knock_in { f2() + f4() } else { f2() - f4() }
        } else {
            match (call, knock_in, upper, strike_above) {
                (true, true, true, true) | (false, true, false, false) => f1(),
                (true, true, true, false) | (false, true, false, true) => f2() - f3() + f4(),
                (true, true, false, true) | (false, true, true, false) => f3(),
                (true, true, false, false) | (false, true, true, true) => f1() - f2() + f4(),
                (true, false, true, true) | (false, false, false, false) => 0.0,
                (true, false, true, false) | (false, false, false, true) => {
                    f1() - f2() + f3() - f4()
                }
                (true, false, false, true) | (false, false, true, false) => f1() - f3(),
                (true, false, false, false) | (false, false, true, true) => f2() - f4(),
            }
        };
        checked(value)
    }
}

impl BinaryBarrierOption {
    pub(crate) fn contract(&self) -> Contract<'_> {
        Contract {
            monitoring: &self.monitoring,
            digital: Some(&self.digital),
            payout: self.payout(),
            timing: SettlementTiming::AtExpiry,
        }
    }
}
impl TouchOption {
    pub(crate) fn contract(&self) -> Contract<'_> {
        Contract {
            monitoring: &self.monitoring,
            digital: None,
            payout: self.payout,
            timing: self.settlement_timing,
        }
    }
}
macro_rules! pricing_engine {
    ($contract:ty) => {
        impl PricingEngine<$contract> for AnalyticBinaryBarrierEngine {
            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                context.model_parameters().risk_free_rate()?;
                context.model_parameters().dividend_yield()?;
                Ok(())
            }
            fn price(&self, option: &$contract, context: &PricingContext) -> Result<f64> {
                option.contract().price(context)
            }
            fn volatility_affects_price(
                &self,
                option: &$contract,
                context: &PricingContext,
            ) -> bool {
                option.contract().volatility_affects_price(context)
            }
        }
    };
}
pricing_engine!(BinaryBarrierOption);
pricing_engine!(TouchOption);
