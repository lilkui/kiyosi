use super::unstable;
use crate::Result;
use std::f64::consts::PI;

// Drift cancels in the killed/free density ratio; derivatives hold the terminal log asset fixed.
pub(super) fn survival(
    initial: f64,
    terminal: f64,
    band: f64,
    deviation: f64,
    derivatives: bool,
) -> Result<[f64; 3]> {
    if terminal <= 0.0 || terminal >= band {
        return Ok([0.0; 3]);
    }
    let (mut x, mut y) = (initial / band, terminal / band);
    let ratio = deviation / band;
    let variance = ratio * ratio;
    if variance == 0.0 {
        return Ok([1.0, 0.0, 0.0]);
    }
    if !variance.is_finite() || variance <= 0.0 {
        return Err(unstable());
    }
    let reflected = x + y > 1.0;
    if reflected {
        x = 1.0 - x;
        y = 1.0 - y;
    }
    let mut sum = [0.0; 3];
    let mut converged = false;
    if variance < 0.25 {
        sum[0] = -(-2.0 * (x / ratio) * (y / ratio)).exp_m1();
        let mut add = |k: f64, direct: bool| {
            let exponent = if direct {
                -2.0 * (k / ratio) * ((y - x + k) / ratio)
            } else {
                -2.0 * ((x + k) / ratio) * ((y + k) / ratio)
            };
            let sign = if direct { 1.0 } else { -1.0 };
            let weight = exponent.exp();
            if k != 0.0 {
                sum[0] += sign * weight;
            }
            if derivatives {
                let numerator = if direct { 2.0 * k } else { -2.0 * (y + k) };
                if numerator != 0.0 {
                    let log_slope = numerator.abs().ln() - 2.0 * ratio.ln();
                    sum[1] += sign * (exponent + log_slope).exp().copysign(numerator);
                    sum[2] += sign * (exponent + 2.0 * log_slope).exp();
                }
            }
            weight
        };
        add(0.0, false);
        for n in 1..=64 {
            let k = f64::from(n);
            let tail = add(k, true) + add(k, false) + add(-k, true) + add(-k, false);
            if n >= 2 && tail < 1e-18 {
                converged = true;
                break;
            }
        }
    } else {
        let distance = y - x;
        let log_scale =
            (2.0 * (2.0 * PI).sqrt() * ratio).ln() + 0.5 * distance * distance / variance;
        for n in 1..=256 {
            let frequency = f64::from(n) * PI;
            let exponent = log_scale - 0.5 * frequency * frequency * variance;
            let weight = exponent.exp();
            let sy = (frequency * y).sin();
            let product = (frequency * x).sin() * sy;
            sum[0] += weight * product;
            if derivatives {
                let first = frequency * (frequency * x).cos() * sy;
                let score = -distance / variance;
                sum[1] += weight * (first + score * product);
                sum[2] += weight
                    * (2.0 * score * first
                        + (score * score + 1.0 / variance - frequency * frequency) * product);
            }
            if n >= 4 && exponent + 4.0 * frequency.ln() < -40.0 {
                converged = true;
                break;
            }
        }
    }
    if !converged || !(-1e-13..=1.0 + 1e-13).contains(&sum[0]) {
        return Err(unstable());
    }
    sum[0] = sum[0].clamp(0.0, 1.0);
    if derivatives {
        sum[1] = (if reflected { -sum[1] } else { sum[1] }) / band;
        sum[2] = (sum[2] / band) / band;
    }
    Ok(sum)
}
