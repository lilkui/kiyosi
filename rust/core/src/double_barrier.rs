use crate::barrier::{checked, log_ratio, scaled_cash, scaled_cdf};
use crate::{
    AnalyticVanillaEngine, BarrierOption, BarrierOptionTerms, BarrierTouchState, BarrierType, Date,
    Error, ErrorCategory, EuropeanOption, GreeksRequest, NumericalShiftSettings, ObservationMode,
    ObservationSchedule, OptionContract, OptionType, PricingContext, PricingEngine, PricingResult,
    RebateTiming, Result, Timestamp,
};
use std::f64::consts::PI;

/// Activation by either boundary; see [`DoubleBarrierOption::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoubleBarrierType {
    /// Activates the vanilla payoff on the first touch.
    KnockIn,
    /// Terminates the vanilla payoff on the first touch.
    KnockOut,
}

/// Input terms validated by [`DoubleBarrierOption::new`]; see its example.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleBarrierOptionTerms {
    /// Call-or-put payoff direction.
    pub option_type: OptionType,
    /// Positive finite strike.
    pub strike: f64,
    /// Inclusive first date.
    pub effective_date: Date,
    /// Inclusive expiry at midnight UTC.
    pub expiry_date: Date,
    /// Positive finite lower boundary; equality touches.
    pub lower_barrier: f64,
    /// Finite upper boundary, strictly above the lower boundary.
    pub upper_barrier: f64,
    /// Trigger direction and activation behavior.
    pub barrier_type: DoubleBarrierType,
    /// Nonnegative finite cash rebate, default zero.
    pub rebate: f64,
    /// Rebate payment timing, default expiry.
    pub rebate_timing: RebateTiming,
    /// Monitoring frequency, default continuous.
    pub observation_mode: ObservationMode,
    /// Strictly ordered dates within life, empty for continuous monitoring.
    pub observation_dates: Vec<Date>,
    /// Prior observations; `None` means unknown, not untouched.
    pub touch_state: Option<BarrierTouchState>,
}
impl DoubleBarrierOptionTerms {
    /// Supplies the shared single-barrier defaults without validation.
    /// See [`DoubleBarrierOption::new`] for validation and an example.
    pub fn new(
        option_type: OptionType,
        strike: f64,
        effective_date: Date,
        expiry_date: Date,
        lower_barrier: f64,
        upper_barrier: f64,
        barrier_type: DoubleBarrierType,
    ) -> Self {
        let defaults = BarrierOptionTerms::new(
            option_type,
            strike,
            effective_date,
            expiry_date,
            lower_barrier,
            BarrierType::DownAndOut,
        );
        Self {
            option_type,
            strike,
            effective_date,
            expiry_date,
            lower_barrier,
            upper_barrier,
            barrier_type,
            rebate: defaults.rebate,
            rebate_timing: defaults.rebate_timing,
            observation_mode: defaults.observation_mode,
            observation_dates: defaults.observation_dates,
            touch_state: defaults.touch_state,
        }
    }
}

/// Owns two ordered boundaries sharing monitoring and strictly prior history.
/// Immutable shared pricing and replacement are safe under the crate concurrency contract.
/// See [`Self::new`] and [`Self::replace`] for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleBarrierOption {
    pub(crate) lower: BarrierOption,
    upper: f64,
}
impl DoubleBarrierOption {
    /// Validates boundaries, life, schedule, strike and rebate before returning an owner.
    /// Calendar compatibility and history relative to valuation are checked during pricing.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidParameter`] unless finite `0 < lower < upper`,
    /// then reuses [`BarrierOption::new`] categories and validation order, including
    /// [`ErrorCategory::InvalidOption`] for at-hit knock-in rebates even if zero.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 80.0, 120.0,
    ///     DoubleBarrierType::KnockOut))?;
    /// assert_eq!(option.upper_barrier(), 120.0);
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn new(terms: DoubleBarrierOptionTerms) -> Result<Self> {
        if !terms.lower_barrier.is_finite()
            || !terms.upper_barrier.is_finite()
            || terms.lower_barrier <= 0.0
            || terms.upper_barrier <= terms.lower_barrier
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "double barriers require finite 0 < lower < upper",
            ));
        }
        let mut lower = BarrierOptionTerms::new(
            terms.option_type,
            terms.strike,
            terms.effective_date,
            terms.expiry_date,
            terms.lower_barrier,
            if terms.barrier_type == DoubleBarrierType::KnockIn {
                BarrierType::DownAndIn
            } else {
                BarrierType::DownAndOut
            },
        );
        lower.rebate = terms.rebate;
        lower.rebate_timing = terms.rebate_timing;
        lower.observation_mode = terms.observation_mode;
        lower.observation_dates = terms.observation_dates;
        lower.touch_state = terms.touch_state;
        Ok(Self {
            lower: BarrierOption::new(lower)?,
            upper: terms.upper_barrier,
        })
    }
    /// Returns an owning copy with replaced strictly prior history; `None` restores unknown.
    /// Time consistency is checked during pricing. The original is preserved on every outcome.
    ///
    /// # Errors
    /// Reuses [`BarrierOption::replace`] validation; typed history cannot invalidate construction.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, Date::new(2025, 1, 1)?, Date::new(2026, 1, 1)?, 80.0, 120.0,
    ///     DoubleBarrierType::KnockOut))?;
    /// let known = option.replace(Some(BarrierTouchState::Untouched))?;
    /// assert_eq!(known.touch_state(), Some(BarrierTouchState::Untouched));
    /// assert_eq!(option.touch_state(), None);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, touch_state: Option<BarrierTouchState>) -> Result<Self> {
        Ok(Self {
            lower: self.lower.replace(touch_state)?,
            upper: self.upper,
        })
    }
    /// Returns the payoff direction; see [`Self::new`].
    pub fn option_type(&self) -> OptionType {
        self.lower.option_type()
    }
    /// Returns the positive strike; see [`Self::new`].
    pub fn strike(&self) -> f64 {
        self.lower.strike()
    }
    /// Returns the inclusive first date; see [`Self::new`].
    pub fn effective_date(&self) -> Date {
        self.lower.effective_date()
    }
    /// Returns expiry at midnight UTC; see [`Self::new`].
    pub fn expiry_date(&self) -> Date {
        self.lower.expiry_date()
    }
    /// Returns the positive lower boundary; see [`Self::new`].
    pub fn lower_barrier(&self) -> f64 {
        self.lower.barrier_level()
    }
    /// Returns the upper boundary; see [`Self::new`].
    pub fn upper_barrier(&self) -> f64 {
        self.upper
    }
    /// Returns activation behavior; see [`Self::new`].
    pub fn barrier_type(&self) -> DoubleBarrierType {
        if self.knock_in() {
            DoubleBarrierType::KnockIn
        } else {
            DoubleBarrierType::KnockOut
        }
    }
    /// Returns the nonnegative rebate; see [`Self::new`].
    pub fn rebate(&self) -> f64 {
        self.lower.rebate()
    }
    /// Returns rebate payment timing; see [`Self::new`].
    pub fn rebate_timing(&self) -> RebateTiming {
        self.lower.rebate_timing()
    }
    /// Returns monitoring frequency; see [`Self::new`].
    pub fn observation_mode(&self) -> ObservationMode {
        self.lower.observation_mode()
    }
    /// Borrows immutable schedule storage until this owner is moved or dropped; see [`Self::new`].
    pub fn observation_schedule(&self) -> &ObservationSchedule {
        self.lower.observation_schedule()
    }
    /// Borrows immutable dates until this owner is moved or dropped; see [`Self::new`].
    pub fn observation_dates(&self) -> &[Date] {
        self.lower.observation_dates()
    }
    /// Returns strictly prior history; see [`Self::new`].
    pub fn touch_state(&self) -> Option<BarrierTouchState> {
        self.lower.touch_state()
    }

    fn knock_in(&self) -> bool {
        self.lower.barrier_type().is_knock_in()
    }
    fn vanilla(&self) -> Result<EuropeanOption> {
        EuropeanOption::new(
            self.option_type(),
            self.strike(),
            self.effective_date(),
            self.expiry_date(),
        )
    }
    pub(crate) fn state(&self, context: &PricingContext) -> Result<(bool, bool, bool)> {
        let (prior, touched, finished) = self.lower.state(context)?;
        Ok((
            prior,
            touched
                || self.lower.monitored(context.valuation_time())
                    && context.spot_price() >= self.upper,
            finished,
        ))
    }
    fn payoff_requires_touch(&self) -> bool {
        (self.observation_mode() == ObservationMode::Continuous
            || self.observation_dates().last() == Some(&self.expiry_date()))
            && match self.option_type() {
                OptionType::Call => self.strike() >= self.upper,
                OptionType::Put => self.strike() <= self.lower_barrier(),
            }
    }
}
impl OptionContract for DoubleBarrierOption {
    fn effective_date(&self) -> Date {
        self.effective_date()
    }
    fn expiry_date(&self) -> Date {
        self.expiry_date()
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.lower.at_spot_discontinuity(context, radius)
            || self.touch_state() != Some(BarrierTouchState::Touched)
                && self.lower.monitored(context.valuation_time())
                && context.spot_price() - radius <= self.upper
                && self.upper <= context.spot_price() + radius
    }
    fn time_stencil_available(&self, before: Timestamp, after: Timestamp) -> bool {
        self.lower.time_stencil_available(before, after)
    }
}

/// Stateless continuous double-barrier engine with flat rates; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticDoubleBarrierEngine;
impl AnalyticDoubleBarrierEngine {
    /// Prices inclusive boundary touches, prior history and remaining cashflows.
    /// Uses image/spectral absorbing-density series and first-exit rebate integration.
    /// Resolved contracts settle before the continuous-monitoring restriction.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::UnsupportedOperation`] for node curves or unresolved schedules,
    /// [`ErrorCategory::InvalidTimeRange`] outside life, [`ErrorCategory::InvalidDate`] for
    /// non-trading fixings, [`ErrorCategory::InvalidParameter`] for missing history,
    /// [`ErrorCategory::InvalidOption`] for impossible history, or [`ErrorCategory::InvalidResult`]
    /// for unrepresentable or nonconvergent calculations. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, start, Date::new(2026, 1, 1)?, 80.0, 120.0, DoubleBarrierType::KnockOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// assert!((AnalyticDoubleBarrierEngine.price(&option, &context)? - 1.0838814592583166).abs() < 1e-9);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        self.validate_applicability(context)?;
        let time = context.valuation_time();
        if time < option.effective_date().start_of_day()
            || time > option.expiry_date().start_of_day()
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
        if let Some(value) = resolved_price(option, context)? {
            return Ok(value);
        }
        if option.observation_mode() == ObservationMode::Scheduled {
            return Err(Error::new(
                ErrorCategory::UnsupportedOperation,
                "analytic double-barrier pricing requires continuous monitoring",
            ));
        }
        analytic_price(option, context)
    }
    /// Computes all requested numerical Greeks using the shared core bump defaults.
    /// Greeks are absent at expiry or current boundary equality; stencils reaching
    /// boundaries or changing observation/history regimes omit affected measures.
    /// See [`Self::price`] for a construction example.
    ///
    /// # Errors
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing failures.
    pub fn price_with_greeks(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}
impl PricingEngine<DoubleBarrierOption> for AnalyticDoubleBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn volatility_affects_price(
        &self,
        option: &DoubleBarrierOption,
        context: &PricingContext,
    ) -> bool {
        let Ok((_, touched, finished)) = option.state(context) else {
            return false;
        };
        !(touched && !option.knock_in()
            || !touched && finished && option.knock_in()
            || !option.knock_in() && option.payoff_requires_touch() && option.rebate() == 0.0)
    }
}

pub(crate) fn resolved_price(
    option: &DoubleBarrierOption,
    context: &PricingContext,
) -> Result<Option<f64>> {
    let time = context.valuation_time();
    let (prior, touched, finished) = option.state(context)?;
    let knock_in = option.knock_in();
    let cash = || {
        if option.rebate() == 0.0 {
            return Ok(0.0);
        }
        checked(scaled_cash(
            option.rebate(),
            context
                .model_parameters()
                .risk_free_curve()
                .log_factor(time, option.expiry_date().start_of_day())?,
        ))
    };
    if touched && !knock_in {
        return if option.rebate_timing() == RebateTiming::AtHit {
            Ok(Some(if prior { 0.0 } else { option.rebate() }))
        } else {
            cash().map(Some)
        };
    }
    if !touched && finished && knock_in {
        return cash().map(Some);
    }
    let fixed = option.rebate() == 0.0 && option.payoff_requires_touch();
    if fixed && !knock_in {
        return Ok(Some(0.0));
    }
    if touched || finished || fixed {
        return AnalyticVanillaEngine
            .price(&option.vanilla()?, context)
            .map(Some);
    }
    Ok(None)
}

#[derive(Default)]
struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    fn add(&mut self, term: f64) {
        let next = self.value + term;
        self.correction += if self.value.abs() >= term.abs() {
            (self.value - next) + term
        } else {
            (term - next) + self.value
        };
        self.value = next;
    }
    fn total(&self) -> f64 {
        self.value + self.correction
    }
}
fn scaled(value: f64, exponent: f64) -> f64 {
    scaled_cash(value.abs(), exponent).copysign(value)
}
fn log_interval_mass(lower: f64, upper: f64) -> f64 {
    use crate::pricing::log_normal_cdf;
    if lower >= upper {
        return f64::NEG_INFINITY;
    }
    if lower == f64::NEG_INFINITY {
        return log_normal_cdf(upper);
    }
    if upper == f64::INFINITY {
        return log_normal_cdf(-lower);
    }
    let center = lower + (upper - lower) / 2.0;
    let half = (upper - lower) / 2.0;
    if center.abs() * half + 0.5 * half * half < 0.25 {
        let (mut integral, mut previous, mut term) = (0.0, 0.0, 1.0);
        for order in 0..24 {
            if order % 2 == 0 {
                integral += 2.0 * term / f64::from(order + 1);
            }
            let next = (-center * half * term - half * half * previous) / f64::from(order + 1);
            previous = term;
            term = next;
        }
        return -0.5 * center * center - 0.5 * (2.0 * PI).ln() + half.ln() + integral.ln();
    }
    let (high, low) = if lower >= 0.0 {
        (log_normal_cdf(-lower), log_normal_cdf(-upper))
    } else {
        (log_normal_cdf(upper), log_normal_cdf(lower))
    };
    high + (-(low - high).exp_m1()).ln()
}

#[derive(Clone, Copy)]
struct AbsorbingDistribution {
    initial: f64,
    band: f64,
    log_lower: f64,
    volatility: f64,
    drift: f64,
    time: f64,
}
impl AbsorbingDistribution {
    fn new(option: &DoubleBarrierOption, context: &PricingContext) -> Result<Self> {
        let model = context.model_parameters();
        let sigma = model.volatility();
        Ok(Self {
            initial: log_ratio(context.spot_price(), option.lower_barrier()),
            band: log_ratio(option.upper_barrier(), option.lower_barrier()),
            log_lower: option.lower_barrier().ln(),
            volatility: sigma,
            drift: model.risk_free_rate()? - model.dividend_yield()? - 0.5 * sigma * sigma,
            time: context
                .valuation_time()
                .years_until(option.expiry_date().start_of_day()),
        })
    }
    // Integrate asset^order against the killed density; complement avoids 1-survival cancellation.
    fn moment(
        &self,
        order: i32,
        lower: f64,
        upper: f64,
        discount: f64,
        complement: bool,
    ) -> Result<f64> {
        if lower >= upper {
            return Ok(0.0);
        }
        let order = f64::from(order);
        let Self {
            initial,
            band,
            log_lower,
            volatility,
            drift,
            time,
        } = *self;
        let deviation = volatility * time.sqrt();
        let variance = deviation * deviation;
        let ratio = deviation / band;
        if deviation <= 0.0 || !deviation.is_finite() || !(drift * time).is_finite() {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "double-barrier transition is not representable",
            ));
        }
        let mut sum = Sum::default();
        let mut scale: f64 = 0.0;
        let mut converged = false;
        if ratio < 0.5 {
            let base = order * (log_lower + initial + drift * time)
                + 0.5 * order * order * variance
                + discount;
            let mean = initial + drift * time + order * variance;
            let image = |k: f64, reflected: bool| {
                let distance = if reflected {
                    initial + k * band
                } else {
                    k * band
                };
                let center = mean - 2.0 * distance;
                let exponent = base - 2.0 * distance * ((drift / volatility) / volatility + order);
                let low = (lower - center) / deviation;
                let high = (upper - center) / deviation;
                if low >= high {
                    return 0.0;
                }
                if low <= 10.0 && high >= -10.0 {
                    return (exponent + log_interval_mass(low, high)).exp();
                }
                // Combine reflection and Gaussian exponents before low-volatility cancellation.
                let tail = |boundary: f64, z: f64| {
                    let free_z = (boundary - mean) / deviation;
                    let gaussian = base
                        - 0.5 * free_z * free_z
                        - 2.0
                            * (distance / deviation)
                            * ((boundary - initial + distance) / deviation);
                    let inverse_square = (1.0 / z) / z;
                    let (mut term, mut series) = (1.0, 1.0);
                    for index in 1..=20 {
                        term *= -f64::from(2 * index - 1) * inverse_square;
                        series += term;
                    }
                    gaussian - (-z).ln() + (series / (2.0 * PI).sqrt()).ln()
                };
                let large = if high < 0.0 {
                    tail(upper, high)
                } else {
                    tail(lower, -low)
                };
                let small = if high < 0.0 {
                    tail(lower, low)
                } else {
                    tail(upper, -high)
                };
                (large + (-(small - large).exp_m1()).ln()).exp()
            };
            if complement {
                sum.add(scaled_cdf(1.0, discount, -mean / deviation));
                sum.add(scaled_cdf(1.0, discount, (mean - band) / deviation));
            } else {
                sum.add(image(0.0, false));
            }
            let orientation = if complement { -1.0 } else { 1.0 };
            sum.add(-orientation * image(0.0, true));
            scale = 1.0_f64.max(sum.total().abs());
            for index in 1..=64 {
                let k = f64::from(index);
                let terms = [
                    image(k, false),
                    image(k, true),
                    image(-k, false),
                    image(-k, true),
                ];
                for (term, sign) in terms.into_iter().zip([1.0, -1.0, 1.0, -1.0]) {
                    sum.add(orientation * sign * term);
                }
                if index >= 2 && terms.iter().sum::<f64>() <= 1e-16 * scale {
                    converged = true;
                    break;
                }
            }
        } else {
            let drift_ratio = (drift / volatility) / volatility;
            let slope = (drift_ratio + order) * band;
            let exponent = |y: f64| {
                discount + order * (log_lower + y) + drift_ratio * (y - initial)
                    - 0.5 * (drift / volatility).powi(2) * time
            };
            let first_exponent = exponent(lower);
            let last_exponent = exponent(upper);
            for index in 1..=256 {
                let frequency = f64::from(index) * PI;
                let decay = 0.5 * (frequency * ratio).powi(2);
                let denominator = slope.hypot(frequency);
                let endpoint = |y: f64, log_weight: f64| {
                    let sine = if y == 0.0 || y == band {
                        0.0
                    } else {
                        (frequency * (y / band)).sin()
                    };
                    let cosine = if y == 0.0 {
                        1.0
                    } else if y == band {
                        if index % 2 == 0 { 1.0 } else { -1.0 }
                    } else {
                        (frequency * (y / band)).cos()
                    };
                    let coefficient = ((slope / denominator) * sine
                        - (frequency / denominator) * cosine)
                        / denominator;
                    scaled(coefficient, log_weight - decay)
                };
                let term = 2.0
                    * (frequency * (initial / band)).sin()
                    * (endpoint(upper, last_exponent) - endpoint(lower, first_exponent));
                sum.add(term);
                scale = 1.0_f64.max(scale).max(term.abs());
                let next_frequency = f64::from(index + 1) * PI;
                let tail_exponent =
                    first_exponent.max(last_exponent) - 0.5 * (next_frequency * ratio).powi(2);
                if index >= 4 && tail_exponent < scale.ln() - 40.0 {
                    converged = true;
                    break;
                }
            }
            if complement {
                let mut difference = Sum::default();
                difference.add(discount.exp());
                difference.add(-sum.total());
                sum = difference;
            }
        }
        let value = sum.total();
        if !converged || !value.is_finite() || value < -1e-12 * scale {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "double-barrier density series did not converge to a representable moment",
            ));
        }
        Ok(value.max(0.0))
    }
    fn hit_value(&self, rate: f64) -> Result<f64> {
        let exit = self.moment(0, 0.0, self.band, 0.0, true)?;
        let mut value = scaled(exit, -rate * self.time);
        if rate != 0.0 {
            // Split at diffusion and drift exit scales to capture short boundary layers.
            let mut breaks = vec![0.0, self.time];
            let mut append = |t: f64| {
                if t > 0.0 && t < self.time {
                    breaks.push(t);
                }
            };
            for distance in [self.initial, self.band - self.initial] {
                for factor in [0.0625, 1.0, 16.0] {
                    append(factor * (distance / self.volatility).powi(2));
                }
            }
            if self.drift != 0.0 {
                let hit = if self.drift > 0.0 {
                    (self.band - self.initial) / self.drift
                } else {
                    -self.initial / self.drift
                };
                let width = self.volatility * hit.sqrt() / self.drift.abs();
                append(hit);
                for factor in [-12.0, -4.0, -1.0, 1.0, 4.0, 12.0] {
                    append(hit + factor * width);
                }
            }
            breaks.sort_by(f64::total_cmp);
            breaks.dedup();
            let evaluate = |t: f64| {
                if t == 0.0 {
                    return Ok(0.0);
                }
                Self { time: t, ..*self }.moment(0, 0.0, self.band, -rate * t, true)
            };
            let mut integral = Sum::default();
            for pair in breaks.windows(2) {
                let [left, right] = [pair[0], pair[1]];
                let values = [
                    evaluate(left)?,
                    evaluate(left + (right - left) / 2.0)?,
                    evaluate(right)?,
                ];
                integral.add(integrate_exit(
                    &evaluate,
                    [left, right],
                    values,
                    (right - left) * (values[0] + 4.0 * values[1] + values[2]) / 6.0,
                    1e-13 * (-rate * self.time).exp().max(1.0) / breaks.len() as f64,
                    32,
                )?);
            }
            value += rate * integral.total();
        }
        if !value.is_finite() || value < -1e-12 {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "double-barrier first-exit value is not representable",
            ));
        }
        Ok(value.max(0.0))
    }
}
fn integrate_exit(
    evaluate: &impl Fn(f64) -> Result<f64>,
    [left, right]: [f64; 2],
    values: [f64; 3],
    estimate: f64,
    tolerance: f64,
    depth: u32,
) -> Result<f64> {
    let center = left + (right - left) / 2.0;
    let low = evaluate(left + (center - left) / 2.0)?;
    let high = evaluate(center + (right - center) / 2.0)?;
    let first = (center - left) * (values[0] + 4.0 * low + values[1]) / 6.0;
    let second = (right - center) * (values[1] + 4.0 * high + values[2]) / 6.0;
    let refined = first + second;
    if (refined - estimate).abs() <= 15.0 * tolerance || center == left || center == right {
        return Ok(refined + (refined - estimate) / 15.0);
    }
    if depth == 0 {
        return Err(Error::new(
            ErrorCategory::InvalidResult,
            "double-barrier first-exit integral did not converge",
        ));
    }
    Ok(integrate_exit(
        evaluate,
        [left, center],
        [values[0], low, values[1]],
        first,
        tolerance / 2.0,
        depth - 1,
    )? + integrate_exit(
        evaluate,
        [center, right],
        [values[1], high, values[2]],
        second,
        tolerance / 2.0,
        depth - 1,
    )?)
}
fn analytic_price(option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
    let density = AbsorbingDistribution::new(option, context)?;
    let rate = context.model_parameters().risk_free_rate()?;
    let discount = -rate * density.time;
    let call = option.option_type() == OptionType::Call;
    let strike = log_ratio(option.strike(), option.lower_barrier());
    let lower = if call {
        strike.clamp(0.0, density.band)
    } else {
        0.0
    };
    let upper = if call {
        density.band
    } else {
        strike.clamp(0.0, density.band)
    };
    let cash = density.moment(0, lower, upper, discount, false)?;
    let asset = density.moment(1, lower, upper, discount, false)?;
    let mut value = (if call { 1.0 } else { -1.0 }) * (asset - option.strike() * cash);
    if option.knock_in() {
        value = AnalyticVanillaEngine.price(&option.vanilla()?, context)? - value;
    }
    if option.rebate() != 0.0 {
        let weight = if option.rebate_timing() == RebateTiming::AtHit && !option.knock_in() {
            density.hit_value(rate)?
        } else {
            density.moment(0, 0.0, density.band, discount, !option.knock_in())?
        };
        value += option.rebate() * weight;
    }
    if !value.is_finite()
        || value
            < -1e-11
                * 1.0_f64
                    .max(context.spot_price())
                    .max(option.strike())
                    .max(option.rebate())
    {
        return Err(Error::new(
            ErrorCategory::InvalidResult,
            "analytic double-barrier price is not representable",
        ));
    }
    Ok(value.max(0.0))
}

pub(crate) fn hit_value(option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
    checked(
        option.rebate()
            * AbsorbingDistribution::new(option, context)?
                .hit_value(context.model_parameters().risk_free_rate()?)?,
    )
}
