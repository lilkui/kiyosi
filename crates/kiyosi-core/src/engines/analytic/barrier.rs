use crate::{
    AnalyticVanillaEngine, Error, ErrorCategory, GreeksRequest, NumericalShiftSettings,
    ObservationSchedule, OptionType, PricingContext, PricingEngine, PricingResult, Result,
    TradingCalendar,
};
use crate::{BarrierOption, RebateTiming};

/// Stateless single-barrier analytic engine with flat rates; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticBarrierEngine;
impl AnalyticBarrierEngine {
    /// Prices after validating flat rates, inclusive life, calendar and strictly prior history.
    /// Current boundary hits settle before future monitoring. Prior at-hit rebates are paid.
    /// Scheduled unresolved monitoring uses the BGK approximation and must end at expiry.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::UnsupportedOperation`] for node curves or unsupported schedules,
    /// [`ErrorCategory::InvalidTimeRange`] outside life, [`ErrorCategory::InvalidDate`] for
    /// non-trading fixings, [`ErrorCategory::InvalidParameter`] for missing history,
    /// [`ErrorCategory::InvalidOption`] for impossible history, or [`ErrorCategory::InvalidResult`]
    /// for non-finite prices. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = BarrierOption::new(BarrierOptionTerms::new(OptionType::Call, 100.0,
    ///     start, Date::new(2026, 1, 1)?, 120.0, BarrierType::UpAndOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
    ///     120.0, start.into())?;
    /// assert_eq!(AnalyticBarrierEngine.price(&option, &context)?, 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        price(option, context, &std::sync::OnceLock::new())
    }
    /// Computes requested scalar numerical Greeks with shared bump defaults.
    /// At expiry or a current barrier equality all Greeks are absent. Stencils crossing
    /// observations or spot barriers leave affected measures absent; see [`Self::price`].
    ///
    /// # Errors
    /// Reuses [`crate::numerical_greeks`] validation and propagates [`Self::price`] errors.
    pub fn price_with_greeks(
        &self,
        option: &BarrierOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::numerical_greeks(self, option, context, request, settings)
    }
}
pub(crate) fn checked(value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value.max(0.0))
    } else {
        Err(Error::new(
            ErrorCategory::InvalidResult,
            "non-finite barrier price",
        ))
    }
}
fn price(
    option: &BarrierOption,
    context: &PricingContext,
    schedule: &std::sync::OnceLock<ScheduleValidation>,
) -> Result<f64> {
    context.model_parameters().risk_free_rate()?;
    context.model_parameters().dividend_yield()?;
    validate_life(option, context)?;
    let validate = || validate_schedule(option, context);
    let cached = schedule.get_or_init(|| ScheduleValidation {
        schedule: option.observation_schedule().clone(),
        calendar: context.calendar().clone(),
        result: validate(),
    });
    if &cached.schedule == option.observation_schedule() && cached.calendar == *context.calendar() {
        cached.result.clone()?;
    } else {
        validate()?;
    }
    match resolved_price(option, context)? {
        Some(value) => Ok(value),
        None => analytic_price(option, context),
    }
}

pub(crate) fn validate_life(option: &BarrierOption, context: &PricingContext) -> Result<()> {
    let time = context.valuation_time();
    if time < option.effective_date().start_of_day() || time > option.expiry_date().start_of_day() {
        return Err(Error::new(
            ErrorCategory::InvalidTimeRange,
            "valuation outside contract life",
        ));
    }
    Ok(())
}

pub(crate) fn validate_schedule(option: &BarrierOption, context: &PricingContext) -> Result<()> {
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
    Ok(())
}

pub(crate) fn resolved_price(
    option: &BarrierOption,
    context: &PricingContext,
) -> Result<Option<f64>> {
    let time = context.valuation_time();
    let (prior, touched, finished) = option.state(context)?;
    let knock_in = option.barrier_type().is_knock_in();
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
            .price(option.vanilla(), context)
            .map(Some);
    }
    Ok(None)
}
impl PricingEngine<BarrierOption> for AnalyticBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().risk_free_rate()?;
        context.model_parameters().dividend_yield()?;
        Ok(())
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
    fn prepare_request(
        &self,
    ) -> Result<Option<Box<dyn PricingEngine<BarrierOption> + Send + Sync + '_>>> {
        Ok(Some(Box::new(BarrierPricer(std::sync::OnceLock::new()))))
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        volatility_affects_price(option, context)
    }
}
struct ScheduleValidation {
    schedule: ObservationSchedule,
    calendar: TradingCalendar,
    result: Result<()>,
}
struct BarrierPricer(std::sync::OnceLock<ScheduleValidation>);
impl PricingEngine<BarrierOption> for BarrierPricer {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        AnalyticBarrierEngine.validate_applicability(context)
    }
    fn price(&self, option: &BarrierOption, context: &PricingContext) -> Result<f64> {
        price(option, context, &self.0)
    }
    fn volatility_affects_price(&self, option: &BarrierOption, context: &PricingContext) -> bool {
        volatility_affects_price(option, context)
    }
}
pub(crate) fn volatility_affects_price(option: &BarrierOption, context: &PricingContext) -> bool {
    let Ok((_, touched, finished)) = option.state(context) else {
        return false;
    };
    let knock_in = option.barrier_type().is_knock_in();
    !(touched && !knock_in
        || !touched && finished && knock_in
        || !knock_in && option.payoff_requires_touch() && option.rebate() == 0.0)
}

pub(crate) fn log_ratio(numerator: f64, denominator: f64) -> f64 {
    let relative = (numerator - denominator) / denominator;
    if relative.abs() < 0.5 {
        relative.ln_1p()
    } else {
        let ratio = numerator / denominator;
        if ratio.is_normal() {
            ratio.ln()
        } else {
            numerator.ln() - denominator.ln()
        }
    }
}
pub(crate) fn standardize(value: f64, volatility: f64, root: f64) -> f64 {
    let width = volatility * root;
    if width == 0.0 {
        (value / volatility) / root
    } else {
        value / width
    }
}
pub(crate) fn scaled_cdf(amount: f64, exponent: f64, argument: f64) -> f64 {
    if amount == 0.0 {
        0.0
    } else {
        (amount.ln() + exponent + crate::engines::analytic::vanilla::log_normal_cdf(argument)).exp()
    }
}
pub(crate) fn scaled_cash(amount: f64, exponent: f64) -> f64 {
    if amount == 0.0 {
        return 0.0;
    }
    let discount = exponent.exp();
    let value = amount * discount;
    if discount.is_normal() && value.is_finite() {
        value
    } else {
        (amount.ln() + exponent).exp()
    }
}

fn analytic_price(option: &BarrierOption, context: &PricingContext) -> Result<f64> {
    let t = context
        .valuation_time()
        .years_until(option.expiry_date().start_of_day());
    let spot = context.spot_price();
    let model = context.model_parameters();
    let rate = model.risk_free_rate()?;
    let dividend = model.dividend_yield()?;
    let sigma = model.volatility();
    let upper = option.barrier_type().is_up();
    let knock_in = option.barrier_type().is_knock_in();
    let shift = analytic_log_shift(option, spot, sigma)?;
    let strike_above = log_ratio(option.strike(), option.barrier_level()) > shift;
    let requires_touch = option.payoff_requires_touch();
    let needs_vanilla = if requires_touch {
        knock_in
    } else if knock_in {
        upper == strike_above
    } else {
        upper != strike_above
    };
    let a = if needs_vanilla {
        AnalyticVanillaEngine.price(option.vanilla(), context)?
    } else {
        0.0
    };
    let ratio = log_ratio(option.barrier_level(), spot) + shift;
    let hit_value = if option.rebate() != 0.0 && option.rebate_timing() == RebateTiming::AtHit {
        hit_value(
            option.rebate(),
            ratio.abs(),
            upper,
            rate - dividend - 0.5 * sigma * sigma,
            sigma,
            t,
            rate,
        )?
    } else {
        0.0
    };
    let root = t.sqrt();
    let width = sigma * root;
    let x = option.strike();
    let carry = rate - dividend;
    let mu = carry / sigma / sigma - 0.5;
    let moneyness = log_ratio(spot, x);
    let x2 = if mu.is_finite() {
        -ratio / width + (1.0 + mu) * width
    } else {
        standardize(-ratio + carry * t, sigma, root) + 0.5 * width
    };
    let y1 = (2.0 * ratio + moneyness) / width + (1.0 + mu) * width;
    let y2 = ratio / width + (1.0 + mu) * width;
    let call = option.option_type() == OptionType::Call;
    let eta = if upper { -1.0 } else { 1.0 };
    let phi = if call { 1.0 } else { -1.0 };
    let reflected = |amount: f64, discount: f64, argument: f64, at_strike: bool, asset: bool| {
        let exponent = 2.0 * (if asset { mu + 1.0 } else { mu }) * ratio;
        if exponent.is_finite() {
            scaled_cdf(amount, discount + exponent, argument)
        } else {
            reflection_tail(
                amount,
                discount,
                ratio,
                if at_strike {
                    log_ratio(option.barrier_level(), x) + shift
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
    let b = || {
        phi * scaled_cdf(spot, -dividend * t, phi * x2)
            - phi * scaled_cdf(x, -rate * t, phi * x2 - phi * width)
    };
    let c = || {
        phi * reflected(spot, -dividend * t, eta * y1, true, true)
            - phi * reflected(x, -rate * t, eta * y1 - eta * width, true, false)
    };
    let d = || {
        phi * reflected(spot, -dividend * t, eta * y2, false, true)
            - phi * reflected(x, -rate * t, eta * y2 - eta * width, false, false)
    };
    let e = || {
        scaled_cdf(option.rebate(), -rate * t, eta * x2 - eta * width)
            - reflected(
                option.rebate(),
                -rate * t,
                eta * y2 - eta * width,
                false,
                false,
            )
    };
    let rebate = || {
        if option.rebate_timing() == RebateTiming::AtHit {
            hit_value
        } else {
            scaled_cdf(option.rebate(), -rate * t, -eta * x2 + eta * width)
                + reflected(
                    option.rebate(),
                    -rate * t,
                    eta * y2 - eta * width,
                    false,
                    false,
                )
        }
    };
    let value = if requires_touch {
        if knock_in { a + e() } else { rebate() }
    } else {
        match (call, knock_in, upper, strike_above) {
            (true, true, true, true) | (false, true, false, false) => a + e(),
            (true, true, true, false) | (false, true, false, true) => b() - c() + d() + e(),
            (true, true, false, true) | (false, true, true, false) => c() + e(),
            (true, true, false, false) | (false, true, true, true) => a - b() + d() + e(),
            (true, false, true, true) | (false, false, false, false) => rebate(),
            (true, false, true, false) | (false, false, false, true) => {
                a - b() + c() - d() + rebate()
            }
            (true, false, false, true) | (false, false, true, false) => a - c() + rebate(),
            (true, false, false, false) | (false, false, true, true) => b() - d() + rebate(),
        }
    };
    checked(value)
}

#[expect(
    clippy::too_many_arguments,
    reason = "matches the existing reflection-tail formula"
)]
pub(crate) fn reflection_tail(
    amount: f64,
    discount: f64,
    ratio: f64,
    barrier_strike: f64,
    carry: f64,
    sigma: f64,
    time: f64,
    drift_shift: f64,
) -> f64 {
    if amount == 0.0 || carry.is_sign_negative() != ratio.is_sign_negative() {
        return 0.0;
    }
    let root = time.sqrt();
    let width = sigma * root;
    let base = barrier_strike - ratio + carry * time;
    let standardized = standardize(base, sigma, root);
    let cross = if barrier_strike == 0.0 {
        0.0
    } else {
        2.0 * (ratio / width) * (barrier_strike / width)
    };
    let numerator = ratio + barrier_strike + carry * time;
    (amount.ln() + discount - 0.5 * standardized * standardized - cross - drift_shift * base
        + sigma.ln()
        + root.ln()
        - numerator.abs().ln()
        - 0.5 * (2.0 * std::f64::consts::PI).ln())
    .exp()
}
pub(crate) fn hit_value(
    amount: f64,
    distance: f64,
    upper: bool,
    drift: f64,
    sigma: f64,
    t: f64,
    rate: f64,
) -> Result<f64> {
    let variance = sigma * sigma;
    let drift = if upper { -drift } else { drift };
    if variance == 0.0 {
        return if drift >= 0.0 {
            Ok(0.0)
        } else {
            checked(scaled_cdf(
                amount,
                rate * (distance / drift),
                standardize(-drift * t - distance, sigma, t.sqrt()),
            ))
        };
    }
    let discriminant = drift * drift + 2.0 * rate * variance;
    let scale = 1.0_f64
        .max((drift * drift).abs())
        .max((2.0 * rate * variance).abs());
    if discriminant < 0.0 || rate < 0.0 && discriminant <= 16.0 * f64::EPSILON * scale {
        return integrated_hit_value(amount, distance, drift, sigma, t, rate);
    }
    let root = discriminant.sqrt();
    let width = sigma * t.sqrt();
    let first_exponent = if drift < 0.0 {
        -2.0 * rate / (root - drift)
    } else {
        (-drift - root) / variance
    };
    let second_exponent = if drift > 0.0 {
        2.0 * rate / (root + drift)
    } else {
        (-drift + root) / variance
    };
    let first = scaled_cdf(
        amount,
        first_exponent * distance,
        (root * t - distance) / width,
    );
    let second = if second_exponent.is_finite() {
        scaled_cdf(
            amount,
            second_exponent * distance,
            (-root * t - distance) / width,
        )
    } else {
        reflection_tail(
            amount,
            first_exponent * distance,
            distance,
            0.0,
            root,
            sigma,
            t,
            0.0,
        )
    };
    checked(first + second)
}

// Inverse square-root time removes the first-passage density singularity at zero.
fn integrated_hit_value(
    amount: f64,
    distance: f64,
    drift: f64,
    sigma: f64,
    time: f64,
    rate: f64,
) -> Result<f64> {
    if amount == 0.0 || distance == 0.0 {
        return Ok(amount);
    }
    let root = time.sqrt();
    let lower = distance / sigma / root;
    let drift_width = drift / sigma * root;
    if lower == 0.0 {
        return Ok(amount);
    }
    let endpoint = -0.5 * (lower + drift_width).powi(2) - rate * time;
    let coefficient = 0.5 * drift_width * drift_width + rate * time;
    let stationary = if coefficient > 0.0 {
        (lower * (2.0 * coefficient).sqrt()).sqrt()
    } else {
        0.0
    };
    let peak = if stationary > lower {
        -lower * drift_width - lower * (2.0 * coefficient).sqrt()
    } else {
        endpoint
    };
    if !lower.is_finite() || !peak.is_finite() {
        return Err(Error::new(
            ErrorCategory::InvalidResult,
            "barrier hit integral is not representable",
        ));
    }
    let upper = lower + if lower < 1.0 { 12.0 } else { 40.0 / lower };
    let density = |value: f64| {
        let ratio = lower / value;
        let standardized = value + drift_width * ratio;
        (-0.5 * standardized * standardized - rate * time * ratio * ratio - peak).exp()
    };
    let mut integral = 0.0;
    let mut left = lower;
    while left < upper {
        let right = upper.min(2.0 * left);
        let samples = [
            density(left),
            density(left + (right - left) / 2.0),
            density(right),
        ];
        let estimate = (right - left) * (samples[0] + 4.0 * samples[1] + samples[2]) / 6.0;
        integral += integrate(
            &density,
            [left, right],
            samples,
            estimate,
            1e-13 * (right - left) / (upper - lower).max(1.0),
            20,
        );
        left = right;
    }
    if integral == 0.0 {
        return Ok(0.0);
    }
    checked(scaled_cash(
        amount,
        peak + (2.0 / std::f64::consts::PI).sqrt().ln() + integral.ln(),
    ))
}
fn integrate(
    density: &impl Fn(f64) -> f64,
    [left, right]: [f64; 2],
    samples: [f64; 3],
    estimate: f64,
    tolerance: f64,
    depth: u32,
) -> f64 {
    let center = left + (right - left) / 2.0;
    let low = density(left + (center - left) / 2.0);
    let high = density(center + (right - center) / 2.0);
    let first = (center - left) * (samples[0] + 4.0 * low + samples[1]) / 6.0;
    let last = (right - center) * (samples[1] + 4.0 * high + samples[2]) / 6.0;
    let refined = first + last;
    if depth == 0 || (refined - estimate).abs() <= 15.0 * tolerance {
        return refined + (refined - estimate) / 15.0;
    }
    integrate(
        density,
        [left, center],
        [samples[0], low, samples[1]],
        first,
        tolerance / 2.0,
        depth - 1,
    ) + integrate(
        density,
        [center, right],
        [samples[1], high, samples[2]],
        last,
        tolerance / 2.0,
        depth - 1,
    )
}

pub(crate) fn analytic_log_shift(option: &BarrierOption, spot: f64, sigma: f64) -> Result<f64> {
    let upper = option.barrier_type().is_up();
    if option.continuous() {
        Ok(0.0)
    } else {
        // shortcut: BGK approximates scheduled monitoring; use a numerical engine for exact fixings.
        let shift = (if upper { 1.0 } else { -1.0 })
            * 0.5825971579390107
            * sigma
            * option.mean_observation_year_fraction().sqrt();
        if option.observation_dates().last() != Some(&option.expiry_date()) {
            return Err(Error::new(
                ErrorCategory::UnsupportedOperation,
                "analytic monitoring must end at expiry",
            ));
        }
        let ratio = log_ratio(option.barrier_level(), spot) + shift;
        let barrier = option.barrier_level() * shift.exp();
        let breached = if barrier.is_finite() && barrier > 0.0 {
            if upper {
                spot >= barrier
            } else {
                spot <= barrier
            }
        } else if upper {
            ratio <= 0.0
        } else {
            ratio >= 0.0
        };
        if breached {
            return Err(Error::new(
                ErrorCategory::UnsupportedOperation,
                "spot breaches shifted analytic barrier",
            ));
        }
        Ok(shift)
    }
}
