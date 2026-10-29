use crate::DoubleBarrierOption;
use crate::engines::analytic::barrier::{checked, log_ratio, scaled_cash, scaled_cdf};
use crate::engines::quadrature::gaussian::{Sum, log_interval_mass};
use crate::{
    AnalyticEuropeanEngine, Error, GreeksRequest, NumericalShiftSettings, ObservationMode,
    OptionType, PricingContext, PricingEngine, PricingResult, RebateTiming, Result,
};
use std::f64::consts::PI;

/// Stateless continuous double-barrier engine with flat rates; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticDoubleBarrierEngine;
impl AnalyticDoubleBarrierEngine {
    /// Prices inclusive boundary touches, prior history and remaining cashflows.
    /// Uses image/spectral absorbing-density series and first-exit rebate integration.
    /// Resolved contracts settle before the continuous-monitoring restriction.
    ///
    /// # Errors
    /// Returns [`Error::UnsupportedOperation`] for node curves or unresolved schedules,
    /// [`Error::InvalidInput`] outside life, for non-trading fixings, or for
    /// missing or impossible history, checked in that order. Unrepresentable
    /// calculations return [`Error::ComputationFailure`]; series or integration
    /// limits reached without convergence return [`Error::NonConvergence`].
    /// Inputs remain unchanged.
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
            return Err(Error::InvalidInput(
                "valuation outside contract life".into(),
            ));
        }
        if option
            .observation_dates()
            .iter()
            .any(|&d| !context.calendar().is_trading_day(d))
        {
            return Err(Error::InvalidInput(
                "observation is not a trading day".into(),
            ));
        }
        if let Some(value) = resolved_price(option, context)? {
            return Ok(value);
        }
        if option.observation_mode() == ObservationMode::Scheduled {
            return Err(Error::UnsupportedOperation(
                "analytic double-barrier pricing requires continuous monitoring".into(),
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
        return AnalyticEuropeanEngine
            .price(&option.vanilla()?, context)
            .map(Some);
    }
    Ok(None)
}

fn scaled(value: f64, exponent: f64) -> f64 {
    scaled_cash(value.abs(), exponent).copysign(value)
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
            return Err(Error::ComputationFailure(
                "double-barrier transition is not representable".into(),
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
        if !value.is_finite() || value < -1e-12 * scale {
            return Err(Error::ComputationFailure(
                "double-barrier density series produced an invalid moment".into(),
            ));
        }
        if !converged {
            return Err(Error::NonConvergence(
                "double-barrier density series did not converge".into(),
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
            return Err(Error::ComputationFailure(
                "double-barrier first-exit value is not representable".into(),
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
    if !refined.is_finite() {
        return Err(Error::ComputationFailure(
            "double-barrier first-exit integral is not representable".into(),
        ));
    }
    if (refined - estimate).abs() <= 15.0 * tolerance || center == left || center == right {
        return Ok(refined + (refined - estimate) / 15.0);
    }
    if depth == 0 {
        return Err(Error::NonConvergence(
            "double-barrier first-exit integral did not converge".into(),
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
        value = AnalyticEuropeanEngine.price(&option.vanilla()?, context)? - value;
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
        return Err(Error::ComputationFailure(
            "analytic double-barrier price is not representable".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_exit_integral_exhaustion_reports_non_convergence() {
        let evaluate = |x: f64| Ok(x.powi(4));
        let error = integrate_exit(
            &evaluate,
            [0.0, 1.0],
            [0.0, 0.0625, 1.0],
            1.25 / 6.0,
            1e-12,
            0,
        )
        .unwrap_err();
        assert!(matches!(error, Error::NonConvergence(_)));
    }

    #[test]
    fn first_exit_integral_preserves_evaluation_failure() {
        let evaluate = |_: f64| checked(f64::INFINITY);
        let error = integrate_exit(&evaluate, [0.0, 1.0], [0.0; 3], 0.0, 1e-12, 0).unwrap_err();
        assert!(matches!(error, Error::ComputationFailure(_)));
    }

    #[test]
    fn non_finite_first_exit_integral_is_computation_failure() {
        let evaluate = |_: f64| Ok(f64::MAX);
        let error =
            integrate_exit(&evaluate, [0.0, 1.0], [f64::MAX; 3], 0.0, 1e-12, 0).unwrap_err();
        assert!(matches!(error, Error::ComputationFailure(_)));
    }

    #[test]
    fn unrepresentable_double_barrier_transition_is_computation_failure() {
        let distribution = AbsorbingDistribution {
            initial: 0.5,
            band: 1.0,
            log_lower: 0.0,
            volatility: f64::MAX,
            drift: 0.0,
            time: 4.0,
        };
        assert!(matches!(
            distribution.moment(0, 0.0, 1.0, 0.0, false),
            Err(Error::ComputationFailure(_))
        ));
    }
}
