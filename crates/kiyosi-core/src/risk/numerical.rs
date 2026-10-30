use crate::{
    BlackScholesMertonParameterChanges, Greek, GreeksRequest, NumericalShiftSettings,
    OptionContract, PricingContext, PricingContextChanges, PricingEngine, PricingResult, Result,
};

pub(crate) fn symmetric_shift(value: f64, requested: f64) -> f64 {
    // Reconstruct the bump so both stencil legs use the same representable distance.
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

/// Computes numerical price sensitivities for scalar engines.
///
/// Uses rounded symmetric absolute bumps; collapsed, asymmetric or nonpositive
/// stencils are unavailable.
/// Time rolls stay within permitted valuation dates, curve coverage and neighboring knots, with
/// right-hand differences at knots. Feasible bumped pricing errors fail the request.
/// Vega/vanna/zomma/rho are per percentage point, time Greeks per calendar day.
/// [`crate::EuropeanOption`], [`crate::DigitalOption`] and [`crate::AmericanOption`] expose their inclusive
/// life through [`OptionContract`]; engine/instrument support remains type checked.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`](crate::Error::InvalidInput) for empty requests
/// or invalid bumps, and [`Error::ComputationFailure`](crate::Error::ComputationFailure)
/// for non-finite prices or derivatives. Propagates engine and bumped-context errors.
/// Inputs remain unchanged.
///
/// # Examples
///
/// ```rust
/// use kiyosi_core::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
///     DigitalOption, OptionType, PricingContext, Greek, GreeksRequest, numerical_greeks};
/// # fn main() -> kiyosi_core::Result<()> {
/// let start = Date::new(2025, 1, 1)?;
/// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
///     start, Date::new(2026, 1, 1)?)?;
/// let context = PricingContext::new(
///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
/// let result = numerical_greeks(&AnalyticDigitalEngine, &option, &context,
///     GreeksRequest::new([Greek::Delta]), Default::default())?;
/// assert!(result.require(Greek::Delta)? > 0.0);
/// # Ok(()) }
/// ```
pub fn numerical_greeks<I: OptionContract, E: PricingEngine<I> + ?Sized>(
    engine: &E,
    option: &I,
    context: &PricingContext,
    request: GreeksRequest,
    settings: NumericalShiftSettings,
) -> Result<PricingResult> {
    engine.validate_applicability(context)?;
    request.validate()?;
    settings.validate()?;
    if let Some(prepared) = engine.prepare_request()? {
        return numerical_greeks(prepared.as_ref(), option, context, request, settings);
    }
    complete_greeks(option, context, request, settings, false, |context, _| {
        PricingResult::new(engine.price(option, context)?)
    })
}

pub(crate) fn complete_greeks(
    option: &impl OptionContract,
    context: &PricingContext,
    request: GreeksRequest,
    settings: NumericalShiftSettings,
    native_spot_greeks: bool,
    native: impl Fn(&PricingContext, GreeksRequest) -> Result<PricingResult>,
) -> Result<PricingResult> {
    request.validate()?;
    settings.validate()?;
    let mut result = native(context, request)?;
    let p0 = result.price();
    let available = GreeksRequest::new(Greek::ALL.into_iter().filter(|&g| result.has(g)));
    let time = context.valuation_time();
    if time == option.expiry_date().start_of_day() || option.all_greeks_unavailable(context) {
        return Ok(result);
    }
    let model = context.model_parameters();
    let spot = context.spot_price();
    let vol = model.volatility();
    let rate = model.risk_free_curve().flat_rate().unwrap_or(0.0);
    let h = symmetric_shift(spot, settings.spot_shift);
    let v = symmetric_shift(vol, settings.volatility_shift);
    let r = symmetric_shift(rate, settings.rate_shift);
    let spot_ok = h > 0.0 && spot > h && !option.at_spot_discontinuity(context, h);
    let two_h = 2.0 * h;
    let scalar_speed_ok = two_h.is_finite()
        && spot > two_h
        && symmetric_shift(spot, two_h) == two_h
        && !option.at_spot_discontinuity(context, two_h);
    let speed_ok = spot_ok && (native_spot_greeks || scalar_speed_ok);
    let want = |g| request.has(g) && !available.has(g);
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
    let price = |s, vol, r, t| -> Result<f64> {
        Ok(native(&shifted(s, vol, r, t)?, GreeksRequest::default())?.price())
    };
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
    let paired_derivatives = |first_vol,
                              first_time,
                              last_vol,
                              last_time,
                              delta: bool,
                              gamma: bool|
     -> Result<((f64, f64), (f64, f64))> {
        if !native_spot_greeks {
            return Ok((
                derivatives(spot_samples(first_vol, first_time, gamma)?),
                derivatives(spot_samples(last_vol, last_time, gamma)?),
            ));
        }
        let measures = GreeksRequest::new(
            [Greek::Delta, Greek::Gamma]
                .into_iter()
                .filter(|g| if *g == Greek::Delta { delta } else { gamma }),
        );
        let first = native(&shifted(spot, first_vol, 0.0, first_time)?, measures)?;
        let last = native(&shifted(spot, last_vol, 0.0, last_time)?, measures)?;
        let native_delta = first.get(Greek::Delta).zip(last.get(Greek::Delta));
        let native_gamma = first.get(Greek::Gamma).zip(last.get(Greek::Gamma));
        let missing_delta = delta && native_delta.is_none();
        let missing_gamma = gamma && native_gamma.is_none();
        // Both legs must use scalar stencils when either lacks the native derivative.
        let (first, last) = if missing_delta || missing_gamma {
            (
                derivatives(spot_samples(first_vol, first_time, missing_gamma)?),
                derivatives(spot_samples(last_vol, last_time, missing_gamma)?),
            )
        } else {
            ((0.0, 0.0), (0.0, 0.0))
        };
        let deltas = native_delta.unwrap_or((first.0, last.0));
        let gammas = native_gamma.unwrap_or((first.1, last.1));
        Ok(((deltas.0, gammas.0), (deltas.1, gammas.1)))
    };
    if spot_ok && (want(Greek::Delta) || want(Greek::Gamma) || (want(Greek::Speed) && speed_ok)) {
        let up = price(spot + h, vol, 0.0, time)?;
        let down = price(spot - h, vol, 0.0, time)?;
        let (delta, gamma) = derivatives((up, p0, down));
        if want(Greek::Delta) {
            result.set(request, Greek::Delta, delta)?;
        }
        if want(Greek::Gamma) {
            result.set(request, Greek::Gamma, gamma)?;
        }
        if want(Greek::Speed) && speed_ok {
            let gamma_request = GreeksRequest::new([Greek::Gamma]);
            let high = if native_spot_greeks {
                native(&shifted(spot + h, vol, 0.0, time)?, gamma_request)?
            } else {
                PricingResult::new(up)?
            };
            let low = if native_spot_greeks {
                native(&shifted(spot - h, vol, 0.0, time)?, gamma_request)?
            } else {
                PricingResult::new(down)?
            };
            let speed =
                if let (Some(high), Some(low)) = (high.get(Greek::Gamma), low.get(Greek::Gamma)) {
                    Some((high - low) / h / 2.0)
                } else if scalar_speed_ok {
                    let up2 = price(spot + two_h, vol, 0.0, time)?;
                    let down2 = price(spot - two_h, vol, 0.0, time)?;
                    Some(((up2 - up) - (up - down) + (down - down2)) / h / h / two_h)
                } else {
                    None
                };
            if let Some(speed) = speed {
                result.set(request, Greek::Speed, speed)?;
            }
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
            let (high, low) = paired_derivatives(
                vol + v,
                time,
                vol - v,
                time,
                want(Greek::Vanna),
                want(Greek::Zomma),
            )?;
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
        let mut lower = option
            .valuation_start()
            .unwrap_or(crate::Date::new(-32767, 1, 1)?)
            .start_of_day();
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
        if before != after && option.time_stencil_available(context, before, after) {
            let days = before.micros_until(after) as f64 / 86_400_000_000.0;
            if want(Greek::Theta) {
                result.set(
                    request,
                    Greek::Theta,
                    (price(spot, vol, 0.0, after)? - price(spot, vol, 0.0, before)?) / days,
                )?;
            }
            if spot_ok && (want(Greek::Charm) || want(Greek::Color)) {
                let (first, last) = paired_derivatives(
                    vol,
                    before,
                    vol,
                    after,
                    want(Greek::Charm),
                    want(Greek::Color),
                )?;
                result.set(request, Greek::Charm, (last.0 - first.0) / days)?;
                result.set(request, Greek::Color, (last.1 - first.1) / days)?;
            }
        }
    }
    Ok(result)
}
