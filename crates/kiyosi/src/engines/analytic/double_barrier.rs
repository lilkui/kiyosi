use crate::DoubleBarrierOption;
use crate::Timestamp;
use crate::engines::analytic::barrier::resolved_price_with_state;
use crate::instruments::monitoring::validate_life;
use crate::numerics::{
    Sum, adaptive_simpson, checked, log_difference, log_interval_mass, log_normal_tail, log_ratio,
    scaled_cash, scaled_signed_cash,
};
use crate::{
    AnalyticEuropeanEngine, Error, ObservationMode, OptionType, PricingContext, PricingEngine,
    RebateTiming, Result,
};
use std::f64::consts::PI;

/// Prices continuous double-barrier options analytically with flat rates.
///
/// See [`Self::price`] for usage and applicability limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticDoubleBarrierEngine;
impl AnalyticDoubleBarrierEngine {
    /// Prices inclusive boundary touches, prior history and remaining cash flows.
    ///
    /// Uses image/spectral absorbing-density series and first-exit rebate integration.
    /// Resolved contracts settle before the continuous-monitoring restriction.
    ///
    /// # Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves or unresolved schedules at positive volatility,
    /// [`Error::InvalidInput`] outside life, for impossible history,
    /// or outside required curve coverage. Unrepresentable
    /// calculations return [`Error::ComputationFailure`]; series or integration
    /// limits reached without convergence return [`Error::NonConvergence`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(OptionType::Call,
    ///     100.0, start, Date::new(2026, 1, 1)?.start_of_day(), 80.0, 120.0, DoubleBarrierType::KnockOut))?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?,
    ///     100.0, start.into())?;
    /// assert!((AnalyticDoubleBarrierEngine.price(&option, &context)? - 1.0838814592583166).abs() < 1e-9);
    /// # Ok(()) }
    /// ```
    ///
    /// # Greeks
    ///
    /// Computes all requested numerical Greeks using the shared core bump defaults.
    ///
    /// Expiry Greeks follow terminal payoffs. Current touches retain rate and volatility Greeks
    /// of the resolved payoff; spot derivatives remain absent at equality. Stencils reaching
    /// boundaries or changing observation/history regimes omit affected measures.
    /// See [`Self::price`] for a construction example.
    ///
    /// ## Errors
    ///
    /// Missing required history returns [`crate::Error::MissingHistory`];
    /// see the [caller-supplied history contract](crate).
    ///
    /// Reuses [`crate::numerical_greeks`] validation and propagates pricing failures.
    pub fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        context.natural_bsm_parameters()?;
        self.validate_applicability(context)?;
        validate_life(&option.lower, context)?;
        if crate::engines::deterministic::enabled(context) {
            return crate::engines::deterministic::double_barrier(option, context);
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
}
impl PricingEngine<DoubleBarrierOption> for AnalyticDoubleBarrierEngine {
    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        crate::engines::flat_rates(context).map(|_| ())
    }
    fn price(&self, option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
        self.price(option, context)
    }
}

pub(crate) fn resolved_price(
    option: &DoubleBarrierOption,
    context: &PricingContext,
) -> Result<Option<f64>> {
    resolved_price_with_state(
        &option.lower,
        context,
        option.state(context)?,
        option.payoff_requires_touch(),
    )
}

#[derive(Clone, Copy)]
pub(super) struct AbsorbingDistribution {
    initial: f64,
    upper_distance: f64,
    band: f64,
    log_lower: f64,
    volatility: f64,
    drift: f64,
    time: f64,
}
impl AbsorbingDistribution {
    pub(super) fn new(
        lower: f64,
        upper: f64,
        expiry: Timestamp,
        context: &PricingContext,
    ) -> Result<Self> {
        let model = context.natural_bsm_parameters()?;
        context.validate_curve_horizon(expiry)?;
        let sigma = model.volatility();
        Ok(Self {
            initial: log_ratio(context.spot_price(), lower),
            upper_distance: log_ratio(upper, context.spot_price()),
            band: log_ratio(upper, lower),
            log_lower: lower.ln(),
            volatility: sigma,
            drift: model.flat_risk_free_rate()?
                - model.flat_dividend_yield()?
                - 0.5 * sigma * sigma,
            time: context.valuation_time().years_until(expiry),
        })
    }
    // Integrate asset^order over terminal spots on paths that survived or touched.
    #[expect(
        clippy::float_cmp,
        reason = "Exact endpoints select the absorbing boundary formulas."
    )]
    fn moment(
        &self,
        order: i32,
        [payoff_lower, payoff_upper]: [f64; 2],
        discount: f64,
        complement: bool,
    ) -> Result<f64> {
        if payoff_lower >= payoff_upper {
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
            ..
        } = *self;
        let deviation = volatility * time.sqrt();
        let variance = deviation * deviation;
        let ratio = deviation / band;
        if deviation <= 0.0 || !deviation.is_finite() || !(drift * time).is_finite() {
            return Err(Error::ComputationFailure(
                "double-barrier transition is not representable".into(),
            ));
        }
        let lower = payoff_lower.max(0.0);
        let upper = payoff_upper.min(band);
        let base = order * (log_lower + initial + drift * time)
            + 0.5 * order * order * variance
            + discount;
        let mean = initial + drift * time + order * variance;
        let unrestricted =
            |a, b| (base + log_interval_mass((a - mean) / deviation, (b - mean) / deviation)).exp();
        if lower >= upper {
            return if complement {
                checked(unrestricted(payoff_lower, payoff_upper))
            } else {
                Ok(0.0)
            };
        }
        let mut sum = Sum::default();
        let mut scale: f64 = 0.0;
        let mut converged = false;
        if ratio < 0.5 {
            let image = |k: f64, reflected: bool| {
                let shift = k * band;
                let distance = if reflected { initial + shift } else { shift };
                let center = mean - 2.0 * distance;
                let exponent = if distance == 0.0 {
                    base
                } else {
                    base - 2.0 * distance * ((drift / volatility) / volatility + order)
                };
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
                    let reflected_boundary = if reflected {
                        boundary + shift
                    } else {
                        boundary - initial + shift
                    };
                    let reflection = if distance == 0.0 || reflected_boundary == 0.0 {
                        0.0
                    } else {
                        2.0 * (distance / deviation) * (reflected_boundary / deviation)
                    };
                    log_normal_tail(z, base - 0.5 * free_z * free_z - reflection)
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
                log_difference(large, small).exp()
            };
            if complement {
                // Terminal spots outside the band have touched. Inside, the
                // unrestricted density cancels the original image exactly.
                sum.add(unrestricted(payoff_lower, lower));
                sum.add(unrestricted(upper, payoff_upper));
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
                    scaled_signed_cash(coefficient, log_weight - decay)
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
                difference.add(unrestricted(payoff_lower, payoff_upper));
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
    pub(super) fn cash_value(&self, amount: f64, rate: f64, complement: bool) -> Result<f64> {
        self.moment(
            0,
            [f64::NEG_INFINITY, f64::INFINITY],
            amount.ln() - rate * self.time,
            complement,
        )
    }
    fn log_exit_density(&self, origin: f64, offset: f64, rate: f64) -> Result<f64> {
        let time = origin + offset;
        if time == 0.0 {
            return Ok(f64::NEG_INFINITY);
        }
        let deviation = self.volatility * time.sqrt();
        let ratio = deviation / self.band;
        let boundary = |distance: f64, opposite: f64, drift: f64| -> Result<f64> {
            let mut sum = Sum::default();
            let mut converged = false;
            let base;
            if ratio < 0.5 {
                // Center the time offset before standardizing a narrow drift-driven peak.
                let z = if drift > 0.0 {
                    -drift * ((origin - distance / drift) + offset) / deviation
                } else {
                    (distance - drift * time) / deviation
                };
                base = distance.ln()
                    - self.volatility.ln()
                    - 1.5 * time.ln()
                    - 0.5 * (2.0 * PI).ln()
                    - 0.5 * z * z
                    - rate * time;
                sum.add(1.0);
                for index in 1..=64 {
                    let shift = f64::from(index) * self.band;
                    let reflected = f64::from(index - 1) * self.band + opposite;
                    let plus = scaled_signed_cash(
                        1.0 + 2.0 * shift / distance,
                        -2.0 * (shift / deviation) * ((shift + distance) / deviation),
                    );
                    let minus = scaled_signed_cash(
                        -1.0 - 2.0 * reflected / distance,
                        -2.0 * (shift / deviation) * (reflected / deviation),
                    );
                    sum.add(plus);
                    sum.add(minus);
                    if index >= 2 && plus.abs() + minus.abs() <= 1e-16 {
                        converged = true;
                        break;
                    }
                }
            } else {
                let first_decay = 0.5 * (PI * ratio).powi(2);
                base = 2.0 * (self.volatility / self.band).ln() - first_decay
                    + (drift / self.volatility)
                        * ((distance - 0.5 * drift * time) / self.volatility)
                    - rate * time;
                for index in 1..=256 {
                    let frequency = f64::from(index) * PI;
                    let sine = (frequency * (distance.min(opposite) / self.band)).sin();
                    let sine = if distance > opposite && index % 2 == 0 {
                        -sine
                    } else {
                        sine
                    };
                    sum.add(
                        frequency * sine * (first_decay - 0.5 * (frequency * ratio).powi(2)).exp(),
                    );
                    let next = f64::from(index + 1) * PI;
                    if index >= 4 && next.ln() + first_decay - 0.5 * (next * ratio).powi(2) < -40.0
                    {
                        converged = true;
                        break;
                    }
                }
            }
            let value = sum.total();
            if !value.is_finite() || value < -1e-12 || base.is_nan() || base == f64::INFINITY {
                return Err(Error::ComputationFailure(
                    "double-barrier exit density is not representable".into(),
                ));
            }
            if !converged {
                return Err(Error::NonConvergence(
                    "double-barrier exit density did not converge".into(),
                ));
            }
            Ok(base + value.max(0.0).ln())
        };
        let lower = boundary(self.initial, self.upper_distance, -self.drift)?;
        let upper = boundary(self.upper_distance, self.initial, self.drift)?;
        let scale = lower.max(upper);
        Ok(if scale == f64::NEG_INFINITY {
            scale
        } else {
            scale + ((lower - scale).exp() + (upper - scale).exp()).ln()
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "The allocated integration panel count is well within exact f64 integer precision."
    )]
    pub(super) fn hit_value(&self, amount: f64, rate: f64) -> Result<f64> {
        if rate == 0.0 {
            return self.cash_value(amount, rate, true);
        }
        let width = self.volatility * self.time.sqrt();
        let log_hit_bound = |distance: f64, toward: f64| {
            let z = (distance - toward.max(0.0) * self.time).max(0.0) / width;
            let reflected = std::f64::consts::LN_2 - 0.5 * z * z;
            if toward < 0.0 {
                // Against the drift, even the probability of ever hitting may round to zero.
                let ever_hit = 2.0 * ((toward / self.volatility) * (distance / self.volatility));
                reflected.min(ever_hit)
            } else {
                reflected
            }
        };
        let largest = log_hit_bound(self.initial, -self.drift)
            .max(log_hit_bound(self.upper_distance, self.drift));
        let log_bound =
            amount.ln() + (-rate * self.time).max(0.0) + std::f64::consts::LN_2 + largest;
        if log_bound < f64::from_bits(1).ln() - std::f64::consts::LN_2 {
            return Ok(0.0);
        }
        // Integrate first-exit flux directly; subtracting cumulative values loses early payments.
        let mut breaks = vec![0.0, self.time];
        let mut append = |t: f64| {
            if t > 0.0 && t < self.time {
                breaks.push(t);
            }
        };
        let tilted_speed = (self.drift * self.drift
            + 2.0 * rate * self.volatility * self.volatility)
            .max(0.0)
            .sqrt();
        for distance in [self.initial, self.upper_distance] {
            for factor in [0.0625, 1.0, 16.0] {
                append(factor * (distance / self.volatility).powi(2));
            }
            for speed in [self.drift.abs(), tilted_speed] {
                if speed > 0.0 {
                    let hit = distance / speed;
                    let width = self.volatility * hit.sqrt() / speed;
                    append(hit);
                    for factor in [-12.0, -4.0, -1.0, 1.0, 4.0, 12.0] {
                        append(hit + factor * width);
                    }
                }
            }
        }
        for factor in [1.0, 4.0, 16.0, 64.0] {
            append(self.time - factor / rate.abs());
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        // Share the error budget so negligible tails do not require unattainable relative accuracy.
        let mut panels = Vec::with_capacity(breaks.len());
        let mut integral_scale = f64::NEG_INFINITY;
        for pair in breaks.windows(2) {
            let [mut left, end] = [pair[0], pair[1]];
            while left < end {
                crate::cancellation::check()?;
                // Bound the time ratio so early exit density cannot set the error scale for a long tail.
                let right = if left > 0.0 { end.min(4.0 * left) } else { end };
                let width = right - left;
                let logs = [
                    self.log_exit_density(left, 0.0, rate)?,
                    self.log_exit_density(left, width / 2.0, rate)?,
                    self.log_exit_density(left, width, rate)?,
                ];
                let scale = logs.into_iter().fold(f64::NEG_INFINITY, f64::max);
                if scale != f64::NEG_INFINITY {
                    integral_scale = integral_scale.max(width.ln() + scale);
                    panels.push((left, width, logs, scale));
                }
                left = right;
            }
        }
        let mut result = Sum::default();
        let panel_count = panels.len() as f64;
        for (left, width, logs, scale) in panels {
            let evaluate =
                |u: f64| Ok((self.log_exit_density(left, width * u, rate)? - scale).exp());
            let values = logs.map(|value| (value - scale).exp());
            let integral = adaptive_simpson(
                &evaluate,
                [0.0, 1.0],
                values,
                (values[0] + 4.0 * values[1] + values[2]) / 6.0,
                1e-13 * (integral_scale - width.ln() - scale).exp() / panel_count,
                32,
            )?;
            result.add(checked(scaled_cash(
                integral,
                amount.ln() + width.ln() + scale,
            ))?);
        }
        let value = checked(result.total())?;
        if rate > 0.0 && value > amount {
            if value - amount > 1e-12 * amount {
                return Err(Error::ComputationFailure(
                    "discounted first-exit value exceeds its payout".into(),
                ));
            }
            return Ok(amount);
        }
        Ok(value)
    }
}
fn analytic_price(option: &DoubleBarrierOption, context: &PricingContext) -> Result<f64> {
    let density = AbsorbingDistribution::new(
        option.lower_barrier(),
        option.upper_barrier(),
        option.expiry_time(),
        context,
    )?;
    let rate = context.natural_bsm_parameters()?.flat_risk_free_rate()?;
    let discount = -rate * density.time;
    let call = option.option_type() == OptionType::Call;
    let strike = log_ratio(option.strike(), option.lower_barrier());
    let knock_in = option.knock_in();
    let mut value = if knock_in && option.payoff_requires_touch() {
        AnalyticEuropeanEngine.price(option.vanilla(), context)?
    } else {
        let bounds = if call {
            [strike, f64::INFINITY]
        } else {
            [f64::NEG_INFINITY, strike]
        };
        let cash = density.moment(0, bounds, discount + option.strike().ln(), knock_in)?;
        let asset = density.moment(1, bounds, discount, knock_in)?;
        if call { asset - cash } else { cash - asset }
    };
    if option.rebate() != 0.0 {
        value += if option.rebate_timing() == RebateTiming::AtHit && !option.knock_in() {
            density.hit_value(option.rebate(), rate)?
        } else {
            density.cash_value(option.rebate(), rate, !option.knock_in())?
        };
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
    AbsorbingDistribution::new(
        option.lower_barrier(),
        option.upper_barrier(),
        option.expiry_time(),
        context,
    )?
    .hit_value(
        option.rebate(),
        context.natural_bsm_parameters()?.flat_risk_free_rate()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrepresentable_double_barrier_transition_is_computation_failure() {
        let distribution = AbsorbingDistribution {
            initial: 0.5,
            upper_distance: 0.5,
            band: 1.0,
            log_lower: 0.0,
            volatility: f64::MAX,
            drift: 0.0,
            time: 4.0,
        };
        assert!(matches!(
            distribution.moment(0, [0.0, 1.0], 0.0, false),
            Err(Error::ComputationFailure(_))
        ));
    }
}
