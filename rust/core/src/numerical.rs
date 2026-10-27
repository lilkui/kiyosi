use crate::{
    BlackScholesMertonParameterChanges, Error, ErrorCategory, EuropeanOption,
    EuropeanPricingEngine, Greek, GreeksRequest, NumericalShiftSettings, PricingContext,
    PricingContextChanges, PricingResult, Result,
};

fn symmetric_shift(value: f64, requested: f64) -> f64 {
    let high = value + requested;
    let shift = high - value;
    let low = value - shift;
    if high.is_finite()
        && shift.is_finite()
        && shift > 0.0
        && low.is_finite()
        && value - low == shift
    {
        shift
    } else {
        0.0
    }
}

/// Numerical price sensitivities reusable by scalar European engines. Uses rounded
/// symmetric absolute bumps; collapsed/asymmetric or nonpositive stencils are unavailable.
/// Time rolls stay within contract life, curve coverage and neighboring knots, with
/// right-hand differences at knots. Feasible bumped pricing errors fail the request.
/// Vega/vanna/zomma/rho are per percentage point, time Greeks per calendar day.
pub fn numerical_greeks<E: EuropeanPricingEngine + ?Sized>(
    engine: &E,
    option: &EuropeanOption,
    context: &PricingContext,
    request: GreeksRequest,
    settings: NumericalShiftSettings,
) -> Result<PricingResult> {
    request.validate()?;
    settings.validate()?;
    let checked = |context: &PricingContext| -> Result<f64> {
        let value = engine.price(option, context)?;
        if !value.is_finite() {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "non-finite bumped price",
            ));
        }
        Ok(value)
    };
    let p0 = checked(context)?;
    let mut result = PricingResult::new(p0)?;
    let time = context.valuation_time();
    if time == option.expiry_date().start_of_day() {
        return Ok(result);
    }
    let model = context.model_parameters();
    let spot = context.spot_price();
    let vol = model.volatility();
    let rate = model.risk_free_curve().flat_rate().unwrap_or(0.0);
    let h = symmetric_shift(spot, settings.spot_shift);
    let v = symmetric_shift(vol, settings.volatility_shift);
    let r = symmetric_shift(rate, settings.rate_shift);
    let spot_ok = h > 0.0 && spot > h;
    let two_h = 2.0 * h;
    let speed_ok =
        spot_ok && two_h.is_finite() && spot > two_h && symmetric_shift(spot, two_h) == two_h;
    let want = |g| request.has(g);
    let shifted = |spot, volatility, rate_shift, valuation_time| -> Result<PricingContext> {
        let parameters = model.replace(BlackScholesMertonParameterChanges {
            volatility: Some(volatility),
            risk_free_rate: Some(model.risk_free_curve().shifted(rate_shift)?.into()),
            ..Default::default()
        })?;
        context.replace(PricingContextChanges {
            model_parameters: Some(parameters),
            spot_price: Some(spot),
            valuation_time: Some(valuation_time),
            ..Default::default()
        })
    };
    let price = |s, vol, r, t| checked(&shifted(s, vol, r, t)?);
    let spot_samples = |vol, t, center| -> Result<(f64, f64, f64)> {
        Ok((
            price(spot + h, vol, 0.0, t)?,
            if center {
                price(spot, vol, 0.0, t)?
            } else {
                p0
            },
            price(spot - h, vol, 0.0, t)?,
        ))
    };
    let derivatives = |(up, center, down): (f64, f64, f64)| {
        (
            (up - down) / h / 2.0,
            ((up - center) - (center - down)) / h / h,
        )
    };
    if spot_ok && (want(Greek::Delta) || want(Greek::Gamma) || (want(Greek::Speed) && speed_ok)) {
        let up = price(spot + h, vol, 0.0, time)?;
        let down = price(spot - h, vol, 0.0, time)?;
        let (delta, gamma) = derivatives((up, p0, down));
        result.set(request, Greek::Delta, delta)?;
        result.set(request, Greek::Gamma, gamma)?;
        if want(Greek::Speed) && speed_ok {
            let up2 = price(spot + two_h, vol, 0.0, time)?;
            let down2 = price(spot - two_h, vol, 0.0, time)?;
            result.set(
                request,
                Greek::Speed,
                ((up2 - up) - (up - down) + (down - down2)) / h / h / two_h,
            )?;
        }
    }
    if v > 0.0
        && vol > v
        && (100.0 * v).is_finite()
        && (want(Greek::Vega) || (spot_ok && (want(Greek::Vanna) || want(Greek::Zomma))))
    {
        if want(Greek::Vega) {
            let up = price(spot, vol + v, 0.0, time)?;
            let down = price(spot, vol - v, 0.0, time)?;
            result.set(request, Greek::Vega, (up - down) / (200.0 * v))?;
        }
        if spot_ok && (want(Greek::Vanna) || want(Greek::Zomma)) {
            let high = derivatives(spot_samples(vol + v, time, want(Greek::Zomma))?);
            let low = derivatives(spot_samples(vol - v, time, want(Greek::Zomma))?);
            result.set(request, Greek::Vanna, (high.0 - low.0) / (200.0 * v))?;
            result.set(request, Greek::Zomma, (high.1 - low.1) / (200.0 * v))?;
        }
    }
    if want(Greek::Rho) && r > 0.0 && (200.0 * r).is_finite() {
        result.set(
            request,
            Greek::Rho,
            (price(spot, vol, r, time)? - price(spot, vol, -r, time)?) / (200.0 * r),
        )?;
    }
    if want(Greek::Theta) || want(Greek::Charm) || want(Greek::Color) {
        let mut lower = option.effective_date().start_of_day();
        let mut upper = option.expiry_date().start_of_day();
        for curve in [model.risk_free_curve(), model.dividend_curve()] {
            if let Some(reference) = curve.reference_time() {
                lower = lower.max(reference);
            }
            if let Some(last) = curve.max_time() {
                upper = upper.min(last);
            }
            for &knot in curve.nodes() {
                if knot <= time {
                    lower = lower.max(knot);
                } else {
                    upper = upper.min(knot);
                }
            }
        }
        let before_width = lower.micros_until(time);
        let after_width = time.micros_until(upper);
        let mut shift = i64::from(settings.time_shift_days)
            .saturating_mul(86_400_000_000)
            .min(lower.micros_until(upper).max(0));
        if before_width > 0 && after_width > 0 {
            shift = shift.min(before_width).min(after_width);
        }
        let before = time.add_micros(-shift.min(before_width))?;
        let after = time.add_micros(shift.min(after_width))?;
        if before != after {
            let days = before.micros_until(after) as f64 / 86_400_000_000.0;
            if want(Greek::Theta) {
                result.set(
                    request,
                    Greek::Theta,
                    (price(spot, vol, 0.0, after)? - price(spot, vol, 0.0, before)?) / days,
                )?;
            }
            if spot_ok && (want(Greek::Charm) || want(Greek::Color)) {
                let first = derivatives(spot_samples(vol, before, want(Greek::Color))?);
                let last = derivatives(spot_samples(vol, after, want(Greek::Color))?);
                result.set(request, Greek::Charm, (last.0 - first.0) / days)?;
                result.set(request, Greek::Color, (last.1 - first.1) / days)?;
            }
        }
    }
    Ok(result)
}
