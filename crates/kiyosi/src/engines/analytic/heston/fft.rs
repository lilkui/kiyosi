use std::f64::consts::TAU;

use rustfft::FftPlanner;

use super::{
    Complex64, EuropeanOption, HestonIntegrationSettings, HestonParameters, OptionType, PI,
    PricingContext, Result, characteristic_difference, checked_with_scale, integrated_variance,
    lognormal_price, scaled_cash,
};

struct Quote {
    moneyness: f64,
    weight: f64,
    base: f64,
    scale: f64,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "The bounded transform and strike counts fit exactly in f64."
)]
pub(super) fn price_strikes(
    option: &EuropeanOption,
    context: &PricingContext,
    model: &HestonParameters,
    strikes: &[f64],
    settings: HestonIntegrationSettings,
) -> Result<Option<Vec<f64>>> {
    if !(256..=16_384).contains(&strikes.len())
        || model.vol_of_vol() == 0.0
        || model.initial_variance() == 0.0
        || settings.absolute_tolerance == 0.0
    {
        return Ok(None);
    }
    let start = context.valuation_time();
    let end = option.expiry_time();
    let time = start.years_until(end);
    if time == 0.0 {
        return Ok(None);
    }
    let variance = integrated_variance(model, time);
    if !variance.is_normal() {
        return Ok(None);
    }
    let risk = model.risk_free_curve().log_factor(start, end)?;
    let dividend = model.dividend_curve().log_factor(start, end)?;
    let spot = context.spot_price();
    let moneyness = |strike| context.log_forward_moneyness(end, strike, dividend - risk);
    let first = moneyness(strikes[0]);
    let last = moneyness(strikes[strikes.len() - 1]);
    let step = (last - first) / (strikes.len() - 1) as f64;
    if !step.is_normal() {
        return Ok(None);
    }
    let sign = if option.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let max_moneyness = first.abs().max(last.abs());
    let mut phase_error: f64 = 0.0;
    let mut max_weight: f64 = 0.0;
    let mut quotes = Vec::with_capacity(strikes.len());
    for (index, &strike) in strikes.iter().enumerate() {
        crate::cancellation::check()?;
        let moneyness = moneyness(strike);
        let error = ((index as f64).mul_add(step, first) - moneyness).abs();
        if error > 32.0 * f64::EPSILON * max_moneyness.max(1.0) {
            return Ok(None);
        }
        phase_error = phase_error.max(error);
        let weight = (spot.ln() + dividend).midpoint(strike.ln() + risk).exp() / PI;
        let scale = scaled_cash(spot, dividend).max(scaled_cash(strike, risk));
        if !weight.is_normal() || !scale.is_normal() {
            return Ok(None);
        }
        max_weight = max_weight.max(weight);
        let base = lognormal_price(
            spot,
            strike,
            moneyness,
            (dividend, risk),
            variance.sqrt(),
            1.0,
            sign,
        )?;
        quotes.push(Quote {
            moneyness,
            weight,
            base,
            scale,
        });
    }
    let target = settings.absolute_tolerance / (16.0 * max_weight);
    if !(1e-15..1.0).contains(&target) {
        return Ok(None);
    }
    // Sampling frequencies with spacing 2π/L repeats the inverse transform at
    // multiples of L. Each inverse half-moment term is bounded by π exp(-|m|/2),
    // since the centered log return X satisfies E[exp(X)] <= 1.
    let period = max_moneyness + 2.0 * (8.0 * PI / target).ln();
    let alias_error = 4.0 * PI * (0.5 * (max_moneyness - period)).exp() / -(-0.5 * period).exp_m1();
    let frequency_step = TAU / period;
    let mut count = 128;
    let mut tail_error;
    loop {
        let limit = frequency_step * (count - 1) as f64;
        tail_error = tail_bound(model, time, variance, limit);
        if tail_error <= target {
            break;
        }
        count *= 2;
        if count > 16_384 || count * 2 > settings.max_evaluations {
            return Ok(None);
        }
    }
    if count * 2 > settings.max_evaluations {
        return Ok(None);
    }

    // A chirp convolution evaluates the Fourier sum at exactly the requested
    // log-strike spacing, without tying that spacing to the frequency step.
    let len = (count + strikes.len() - 1).next_power_of_two();
    let mut coefficients = vec![Complex64::default(); len];
    let mut kernel = vec![Complex64::default(); len];
    let phase = 0.5 * frequency_step * step;
    let chirp = |index: usize, direction| {
        let index = index as f64;
        Complex64::from_polar(1.0, direction * phase * index * index)
    };
    let mut absolute = 0.0;
    for (index, coefficient) in coefficients.iter_mut().take(count).enumerate() {
        if index % 256 == 0 {
            crate::cancellation::check()?;
        }
        let u = frequency_step * index as f64;
        let weight = if index == 0 { 0.5 } else { 1.0 } * frequency_step;
        let sample =
            characteristic_difference(model, time, variance, u) * (weight / (u * u + 0.25));
        absolute += sample.norm();
        *coefficient = sample * Complex64::from_polar(1.0, u * first) * chirp(index, 1.0);
    }
    for (index, value) in kernel.iter_mut().take(strikes.len()).enumerate() {
        *value = chirp(index, -1.0);
    }
    for index in 1..count {
        kernel[len - index] = chirp(index, -1.0);
    }
    let mut planner = FftPlanner::new();
    let forward = planner.plan_fft_forward(len);
    let inverse = planner.plan_fft_inverse(len);
    let mut scratch = vec![
        Complex64::default();
        forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len())
    ];
    forward.process_with_scratch(&mut coefficients, &mut scratch);
    forward.process_with_scratch(&mut kernel, &mut scratch);
    for (coefficient, kernel) in coefficients.iter_mut().zip(kernel) {
        *coefficient *= kernel;
    }
    inverse.process_with_scratch(&mut coefficients, &mut scratch);
    crate::cancellation::check()?;
    let max_u = frequency_step * (count - 1) as f64;
    let max_chirp = phase.abs() * (count.max(strikes.len()) as f64).powi(2);
    let roundoff = (phase_error * max_u
        + 32.0 * f64::EPSILON * (f64::from(len.ilog2()) + max_chirp + max_u * max_moneyness))
        * absolute;
    let error = alias_error + tail_error + roundoff;
    let mut prices = Vec::with_capacity(strikes.len());
    for (index, (quote, value)) in quotes.into_iter().zip(coefficients).enumerate() {
        crate::cancellation::check()?;
        let correction = (value * chirp(index, 1.0)).re / len as f64;
        let price = quote.base + quote.weight * correction;
        let tolerance = settings.absolute_tolerance + settings.relative_tolerance * price.abs();
        if !error.is_finite()
            || !price.is_finite()
            || (error * quote.weight) > 0.25 * tolerance
            || quote.moneyness.abs() > 16.0
            || price < 0.0
        {
            return Ok(None);
        }
        prices.push(checked_with_scale(price, quote.scale)?);
    }
    Ok(Some(prices))
}

fn tail_bound(model: &HestonParameters, time: f64, variance: f64, limit: f64) -> f64 {
    let xi = model.vol_of_vol();
    let rho = model.correlation();
    // Integrating out the independent spot Brownian motion bounds the
    // half-moment transform by the Laplace transform of integrated variance.
    // The half-moment change of measure shifts the CIR mean reversion by ρ ξ/2.
    let a = 0.125 + 0.5 * (1.0 - rho * rho) * limit * limit;
    let kappa = model.mean_reversion_speed() - 0.5 * rho * xi;
    let d = (kappa * kappa + 2.0 * xi * xi * a).sqrt();
    let rise = -(-d * time).exp_m1();
    let b = 2.0 * a * rise / (2.0 * d - (d - kappa) * rise);
    let heston = (-model.initial_variance() * b).exp();
    let black = (-0.5 * variance * (limit * limit + 0.25)).exp();
    // Both envelopes decrease with frequency, and 1/(u²+1/4) <= 1/u².
    (heston + black) / limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalyticHestonEuropeanEngine, Date, HestonParameterInputs};

    #[test]
    fn dense_log_strikes_use_fft_and_meet_scalar_tolerance() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(365)?;
        let model = HestonParameters::new(HestonParameterInputs {
            risk_free_rate: 0.05.into(),
            dividend_yield: 0.02.into(),
            initial_variance: 0.04,
            long_run_variance: 0.04,
            mean_reversion_speed: 1.5,
            vol_of_vol: 0.3,
            correlation: -0.7,
        })?;
        let market = PricingContext::new(model.clone(), 100.0, start)?;
        let engine = AnalyticHestonEuropeanEngine::default();
        let settings = engine.settings();
        let strikes: Vec<_> = (0..512)
            .map(|index| 60.0 * ((160.0_f64 / 60.0).ln() * f64::from(index) / 511.0).exp())
            .collect();
        for direction in [OptionType::Call, OptionType::Put] {
            let option = EuropeanOption::new(direction, 100.0, start, end)?;
            let actual = price_strikes(&option, &market, &model, &strikes, settings)?
                .expect("regular strikes and ordinary Heston inputs use the transform");
            for (value, &strike) in actual.into_iter().zip(&strikes) {
                let scalar = engine.price(
                    &EuropeanOption::new(direction, strike, start, end)?,
                    &market,
                )?;
                let tolerance =
                    settings.absolute_tolerance + settings.relative_tolerance * scalar.abs();
                assert!(
                    (value - scalar).abs() <= 0.5 * tolerance,
                    "{direction:?} {strike}: {value} != {scalar}"
                );
            }
        }
        Ok(())
    }
}
