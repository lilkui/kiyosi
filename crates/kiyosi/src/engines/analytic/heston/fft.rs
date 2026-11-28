use std::{cell::Cell, f64::consts::TAU};

use rustfft::FftPlanner;

use super::{
    Complex64, EuropeanOption, HestonIntegrationSettings, HestonParameters, OptionType, PI,
    PricingContext, Result, characteristic_difference, checked_with_scale, integrated_variance,
    lognormal_price, scaled_cash,
};

pub(super) const MAX_STRIKES: usize = 16_384;

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
    batch_work: &Cell<usize>,
) -> Result<Option<Vec<Option<f64>>>> {
    if !(16..=MAX_STRIKES).contains(&strikes.len())
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
    let sign = if option.option_type() == OptionType::Call {
        1.0
    } else {
        -1.0
    };
    let mut max_moneyness: f64 = 0.0;
    let mut max_weight: f64 = 0.0;
    let mut eligible_strikes = 0;
    let mut quotes = Vec::with_capacity(strikes.len());
    for &strike in strikes {
        crate::cancellation::check()?;
        let moneyness = moneyness(strike);
        let weight = (spot.ln() + dividend).midpoint(strike.ln() + risk).exp() / PI;
        let scale = scaled_cash(spot, dividend).max(scaled_cash(strike, risk));
        if !moneyness.is_finite()
            || moneyness.abs() > 16.0
            || !weight.is_normal()
            || !scale.is_normal()
        {
            quotes.push(None);
            continue;
        }
        max_moneyness = max_moneyness.max(moneyness.abs());
        max_weight = max_weight.max(weight);
        eligible_strikes += 1;
        let base = lognormal_price(
            spot,
            strike,
            moneyness,
            (dividend, risk),
            variance.sqrt(),
            1.0,
            sign,
        )?;
        quotes.push(Some(Quote {
            moneyness,
            weight,
            base,
            scale,
        }));
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

    // Oversampling bounds the interpolation error through the highest retained
    // frequency. The inverse FFT has the same sign as the pricing transform.
    let len = 8 * count;
    // The direct integrator starts with five sampled panels per strike even
    // when their characteristic functions are cached. Weight the more expensive
    // characteristic samples and the cheaper FFT butterflies separately.
    let direct_work = eligible_strikes * 5 * super::PANEL_SAMPLES;
    let transform_work = count + len * len.ilog2() as usize / 64 + 16 * eligible_strikes;
    if transform_work >= direct_work {
        return Ok(None);
    }
    super::charge_batch_work(batch_work, transform_work)?;
    let mut coefficients = vec![Complex64::default(); len];
    let mut absolute = 0.0;
    let mut derivative = 0.0;
    let grid_scale = len as f64 / period;
    for (index, coefficient) in coefficients.iter_mut().take(count).enumerate() {
        if index % 256 == 0 {
            crate::cancellation::check()?;
        }
        let u = frequency_step * index as f64;
        let weight = if index == 0 { 0.5 } else { 1.0 } * frequency_step;
        let sample =
            characteristic_difference(model, time, variance, u) * (weight / (u * u + 0.25));
        let magnitude = sample.norm();
        absolute += magnitude;
        derivative += magnitude * (u / grid_scale).powi(16);
        *coefficient = sample;
    }
    FftPlanner::new()
        .plan_fft_inverse(len)
        .process(&mut coefficients);
    crate::cancellation::check()?;
    let max_u = frequency_step * (count - 1) as f64;
    let roundoff =
        32.0 * f64::EPSILON * (f64::from(len.ilog2()) + max_u * max_moneyness + 16.0) * absolute;
    let mut prices = Vec::with_capacity(strikes.len());
    for quote in quotes {
        crate::cancellation::check()?;
        let Some(quote) = quote else {
            prices.push(None);
            continue;
        };
        let (correction, remainder, amplification) =
            interpolate(&coefficients, quote.moneyness * grid_scale);
        let error = alias_error + tail_error + roundoff * amplification + derivative * remainder;
        let price = quote.base + quote.weight * correction;
        let tolerance = settings.absolute_tolerance + settings.relative_tolerance * price.abs();
        if !error.is_finite()
            || !price.is_finite()
            || (error * quote.weight) > 0.25 * tolerance
            || price < 0.0
        {
            prices.push(None);
        } else {
            prices.push(Some(checked_with_scale(price, quote.scale)?));
        }
    }
    Ok(Some(prices))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "The transform limits positions and indices to fewer than 2^18 grid points."
)]
fn interpolate(values: &[Complex64], position: f64) -> (f64, f64, f64) {
    // Alternating binomial coefficients give the barycentric weights for 16
    // equally spaced nodes. The remainder is the nodal product divided by 16!.
    const WEIGHTS: [f64; 16] = [
        1.0, -15.0, 105.0, -455.0, 1365.0, -3003.0, 5005.0, -6435.0, 6435.0, -5005.0, 3003.0,
        -1365.0, 455.0, -105.0, 15.0, -1.0,
    ];
    let center = position.floor();
    let fraction = position - center;
    let index = (center as isize).rem_euclid(values.len() as isize) as usize;
    if fraction == 0.0 {
        return (values[index].re, 0.0, 1.0);
    }
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    let mut absolute = 0.0;
    let mut product = 1.0;
    for (i, weight) in WEIGHTS.into_iter().enumerate() {
        let distance = fraction + (7.0 - i as f64);
        let barycentric = weight / distance;
        let sample = values[(index + values.len() - 7 + i) & (values.len() - 1)].re;
        numerator += barycentric * sample;
        denominator += barycentric;
        absolute += barycentric.abs();
        product *= distance;
    }
    (
        numerator / denominator,
        product.abs() / 20_922_789_888_000.0,
        absolute / denominator.abs(),
    )
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
    use crate::{AnalyticHestonEuropeanEngine, Date, HestonParameterInputs, PricingEngine};

    #[test]
    fn irregular_strikes_below_old_threshold_use_fft_and_isolate_outliers() -> Result<()> {
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
        let mut strikes: Vec<_> = (0..128)
            .map(|index| {
                let i = f64::from(index * 173 % 128);
                60.0 * ((160.0_f64 / 60.0).ln() * i / 127.0).exp() + 0.012 * i.sin()
            })
            .collect();
        strikes.insert(30, 1e12);
        for direction in [OptionType::Call, OptionType::Put] {
            let option = EuropeanOption::new(direction, 100.0, start, end)?;
            let work = Cell::new(super::super::BATCH_WORK_LIMIT);
            assert!(matches!(
                price_strikes(&option, &market, &model, &strikes, settings, &Cell::new(0)),
                Err(crate::Error::InvalidInput(_))
            ));
            let actual = price_strikes(&option, &market, &model, &strikes, settings, &work)?
                .expect("ordinary Heston inputs use the transform for this strip");
            assert!(actual[30].is_none());
            assert!(price_strikes(&option, &market, &model, &[100.0], settings, &work)?.is_none());
            assert_eq!(
                engine.price_strikes(&option, &market, &[100.0; 256])?,
                [engine.price(&option, &market)?; 256],
            );
            for (index, (value, &strike)) in actual.into_iter().zip(&strikes).enumerate() {
                if index == 30 {
                    continue;
                }
                let value = value.expect("ordinary quotes meet the transform error budget");
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
