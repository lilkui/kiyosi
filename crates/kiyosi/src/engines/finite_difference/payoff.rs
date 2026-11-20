use super::{
    Diffusion, Error, Payoff, Result, black_scholes_arguments, scaled, unstable, weighted_cdf,
};

pub(super) fn boundary_value<D: Diffusion>(
    payoff: Payoff,
    upper: f64,
    [risk_log, dividend_log]: [f64; 2],
    diffusion: D,
    tau: f64,
) -> f64 {
    let continuation = if D::LOCAL {
        let asset_value = scaled(upper, dividend_log);
        let strike_value = scaled(payoff.strike, risk_log);
        if payoff.is_digital() {
            if payoff.sign * (asset_value - strike_value) > 0.0 {
                payoff
                    .payout()
                    .map_or(asset_value, |amount| scaled(amount, risk_log))
            } else {
                0.0
            }
        } else {
            (payoff.sign * (asset_value - strike_value)).max(0.0)
        }
    } else {
        let (d1, d2) = black_scholes_arguments(
            crate::numerics::log_ratio(upper, payoff.strike) + (dividend_log - risk_log),
            diffusion.sigma(0.0, 0.0),
            tau.sqrt(),
        );
        if payoff.is_digital() {
            if let Some(amount) = payoff.payout() {
                weighted_cdf(amount, risk_log, payoff.sign * d2)
            } else {
                weighted_cdf(upper, dividend_log, payoff.sign * d1)
            }
        } else {
            payoff.sign
                * (weighted_cdf(upper, dividend_log, payoff.sign * d1)
                    - weighted_cdf(payoff.strike, risk_log, payoff.sign * d2))
        }
    };
    if payoff.is_american() {
        continuation.max(payoff.value(upper))
    } else {
        continuation
    }
}

pub(super) fn checked_price(
    payoff: Payoff,
    spot: f64,
    value: f64,
    [risk_log, dividend_log]: [f64; 2],
    relative_upper_error: f64,
) -> Result<f64> {
    if !value.is_finite() {
        return Err(unstable());
    }
    if !payoff.is_digital() {
        let scale = scaled(spot, dividend_log).max(scaled(payoff.strike, risk_log));
        let value = crate::numerics::checked_with_scale(value, scale)?;
        return Ok(value.max(if payoff.is_american() {
            payoff.value(spot)
        } else {
            0.0
        }));
    }
    let bound = scaled(
        payoff.payout().unwrap_or(spot),
        if payoff.payout().is_some() {
            risk_log
        } else {
            dividend_log
        },
    );
    let tolerance = 64.0 * f64::EPSILON * bound;
    let upper_error = bound * relative_upper_error;
    if value < -tolerance
        || value - bound
            > tolerance
                + if upper_error.is_finite() {
                    upper_error
                } else {
                    0.0
                }
    {
        return Err(Error::ComputationFailure(
            "finite-difference digital price violates payoff bounds".into(),
        ));
    }
    Ok(value.clamp(0.0, bound))
}
