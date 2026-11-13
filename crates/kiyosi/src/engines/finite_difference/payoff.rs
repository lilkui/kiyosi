use super::{
    Diffusion, Error, Payoff, Result, black_scholes_arguments, scaled, unstable, weighted_cdf,
};

pub(super) fn upper_boundary<D: Diffusion>(
    payoff: Payoff,
    upper: f64,
    [risk_log, dividend_log]: [f64; 2],
    diffusion: D,
    tau: f64,
) -> f64 {
    let continuation = if D::LOCAL {
        if payoff.is_digital() {
            if payoff.sign > 0.0 {
                scaled(
                    payoff.payout().unwrap_or(upper),
                    if payoff.payout().is_some() {
                        risk_log
                    } else {
                        dividend_log
                    },
                )
            } else {
                0.0
            }
        } else {
            (payoff.sign * (scaled(upper, dividend_log) - scaled(payoff.strike, risk_log))).max(0.0)
        }
    } else {
        let (d1, d2) = black_scholes_arguments(
            upper,
            payoff.strike,
            dividend_log - risk_log,
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
    relative_carry_error: f64,
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
    let carry_error = bound * relative_carry_error;
    if value < -tolerance
        || value - bound
            > tolerance
                + if carry_error.is_finite() {
                    carry_error
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
