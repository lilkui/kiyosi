use crate::{
    BlackScholesMertonParameterChanges, Greek, GreeksRequest, NumericalShiftSettings,
    OptionContract, PricingContext, PricingContextChanges, PricingEngine, PricingResult, Result,
};

#[expect(
    clippy::float_cmp,
    reason = "Exact equality verifies a representably symmetric finite-difference stencil."
)]
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

#[expect(
    clippy::float_cmp,
    reason = "Exact equality checks that the wider terminal spot stencil is symmetric."
)]
pub(crate) fn expiry_greeks(
    option: &impl OptionContract,
    context: &PricingContext,
    settings: NumericalShiftSettings,
    mut result: PricingResult,
    price: impl Fn(&PricingContext) -> Result<f64>,
) -> Result<PricingResult> {
    if context.valuation_time() != option.expiry_time() {
        return Ok(if option.at_spot_discontinuity(context, 0.0) {
            result
        } else {
            result.with_unavailable_stencils(GreeksRequest::new([Greek::Delta, Greek::Gamma]))
        });
    }
    settings.validate()?;
    let request = GreeksRequest::new(
        Greek::ALL
            .into_iter()
            .filter(|&greek| result.requested().has(greek) && result.supported().has(greek)),
    );
    // Terminal payments no longer depend on volatility or discount rates.
    for greek in [Greek::Vega, Greek::Volga, Greek::Rho, Greek::DividendRho] {
        result.set(request, greek, 0.0)?;
    }
    if !option.at_spot_discontinuity(context, 0.0) {
        result = result.with_unavailable_stencils(GreeksRequest::new([
            Greek::Delta,
            Greek::Gamma,
            Greek::Speed,
            Greek::Vanna,
            Greek::Zomma,
        ]));
    }
    let spot = context.spot_price();
    let h = symmetric_shift(spot, settings.spot_shift_at(spot));
    if h <= 0.0 || spot <= h || option.at_spot_discontinuity(context, h) {
        return Ok(result);
    }
    for greek in [Greek::Vanna, Greek::Zomma] {
        result.set(request, greek, 0.0)?;
    }
    if [Greek::Delta, Greek::Gamma, Greek::Speed]
        .into_iter()
        .any(|g| request.has(g))
    {
        let shifted_price = |spot| {
            price(&context.replace(PricingContextChanges {
                spot_price: Some(spot),
                ..Default::default()
            })?)
        };
        let up = shifted_price(spot + h)?;
        let down = shifted_price(spot - h)?;
        let center = result.price();
        result.set(request, Greek::Delta, (up - down) / h / 2.0)?;
        result.set(
            request,
            Greek::Gamma,
            ((up - center) - (center - down)) / h / h,
        )?;
        let two_h = 2.0 * h;
        if request.has(Greek::Speed)
            && two_h.is_finite()
            && spot > two_h
            && symmetric_shift(spot, two_h) == two_h
            && !option.at_spot_discontinuity(context, two_h)
        {
            let up2 = shifted_price(spot + two_h)?;
            let down2 = shifted_price(spot - two_h)?;
            result.set(
                request,
                Greek::Speed,
                ((up2 - up) - (up - down) + (down - down2)) / h / h / two_h,
            )?;
        }
    }
    Ok(result)
}

/// Computes BSM numerical price sensitivities for scalar engines.
///
/// Heston and local volatility return [`crate::Error::UnsupportedOperation`]. For local volatility, use
/// [`crate::FiniteDifferenceVanillaEngine::price_with_greeks`] for delta and gamma
/// computed directly with the local-volatility surface held fixed.
///
/// At expiry, supported spot Greeks use terminal-payoff stencils. Vega, volga,
/// rho and dividend rho are zero; vanna and zomma are zero where spot derivatives
/// exist. Theta, charm and color are absent. Stencils crossing terminal kinks
/// or product events are unavailable. Automatic terminal spot bumps use the smaller
/// of 0.01 spot units and 1% of spot for every engine, including American Monte Carlo.
///
/// Uses rounded symmetric absolute bumps; collapsed, asymmetric or nonpositive
/// stencils have [`crate::GreekStatus::StencilUnavailable`]. A derivative undefined
/// at the valuation, such as a terminal payoff kink or zero-volatility boundary,
/// retains [`crate::GreekStatus::Undefined`].
/// Time rolls stay within permitted valuation dates, curve coverage and neighboring knots, with
/// right-hand differences at knots. Automatic time bumps start at one day and are halved
/// until their stencil avoids observations, down to one microsecond. Explicit time bumps
/// are not shortened for observations. Feasible bumped pricing errors fail the request.
/// Returns unscaled partial derivatives in the units documented by [`Greek`].
/// Volatility and rates use decimals; valuation time uses Actual/365 Fixed years.
/// Dividend rho shifts the dividend curve in parallel using `settings.rate_shift`.
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
/// use kiyosi::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
///     DigitalOption, OptionType, PricingContext, Greek, GreeksRequest, numerical_greeks};
/// # fn main() -> kiyosi::Result<()> {
/// let start = Date::new(2025, 1, 1)?.start_of_day();
/// let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
///     start, Date::new(2026, 1, 1)?.start_of_day())?;
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
    context.bsm_parameters()?;
    engine.validate_applicability(context)?;
    request.validate()?;
    settings.validate()?;
    let settings = NumericalShiftSettings {
        spot_shift: Some(settings.spot_shift.unwrap_or_else(|| {
            if context.valuation_time() == option.expiry_time() {
                settings.spot_shift_at(context.spot_price())
            } else {
                engine.default_spot_shift(option, context)
            }
        })),
        ..settings
    };
    let prepared = engine.prepare_request()?;
    if let Some(prepared) = &prepared {
        prepared.validate_applicability(context)?;
    }
    let seed = std::cell::Cell::new(None);
    let native = |context: &PricingContext, _| {
        let (price, used_seed) = match &prepared {
            Some(prepared) => prepared.price_with_seed(option, context)?,
            None => engine.price_with_seed(option, context)?,
        };
        if used_seed.is_some() {
            seed.set(used_seed);
        }
        PricingResult::new(price)
    };
    complete_greeks(
        option,
        context,
        request,
        settings,
        false,
        native(context, request)?,
        native,
    )
    .map(|result| result.with_seed(seed.get()))
}

#[expect(
    clippy::float_cmp,
    reason = "Stencils require exactly representable shifts."
)]
pub(crate) fn complete_greeks(
    option: &impl OptionContract,
    context: &PricingContext,
    request: GreeksRequest,
    settings: NumericalShiftSettings,
    native_spot_greeks: bool,
    mut result: PricingResult,
    native: impl Fn(&PricingContext, GreeksRequest) -> Result<PricingResult>,
) -> Result<PricingResult> {
    result = result.with_request(request, GreeksRequest::all());
    // Entry points validate requests and shifts before any fallible preparation.
    let p0 = result.price();
    let available = GreeksRequest::new(Greek::ALL.into_iter().filter(|&g| result.has(g)));
    let time = context.valuation_time();
    if time == option.expiry_time() {
        return expiry_greeks(option, context, settings, result, |context| {
            Ok(native(context, GreeksRequest::new([]))?.price())
        });
    }
    if option.all_greeks_unavailable(context) {
        return Ok(result);
    }
    let want = |g| request.has(g) && !available.has(g);
    if !Greek::ALL.into_iter().any(want) {
        return Ok(result);
    }
    let model = context.bsm_parameters()?;
    let spot = context.spot_price();
    let vol = model.volatility();
    let rate = model.risk_free_curve().exact_flat_rate().unwrap_or(0.0);
    let h = symmetric_shift(spot, settings.spot_shift_at(spot));
    let v = symmetric_shift(vol, settings.volatility_shift);
    let r = symmetric_shift(rate, settings.rate_shift);
    let spot_ok = [
        Greek::Delta,
        Greek::Gamma,
        Greek::Speed,
        Greek::Vanna,
        Greek::Zomma,
        Greek::Charm,
        Greek::Color,
    ]
    .into_iter()
    .any(want)
        && h > 0.0
        && spot > h
        && !option.at_spot_discontinuity(context, h);
    let two_h = 2.0 * h;
    let scalar_speed_ok = want(Greek::Speed)
        && spot_ok
        && two_h.is_finite()
        && spot > two_h
        && symmetric_shift(spot, two_h) == two_h
        && !option.at_spot_discontinuity(context, two_h);
    let speed_ok = spot_ok && (native_spot_greeks || scalar_speed_ok);
    let shifted = |spot, volatility, rate_shift, valuation_time| -> Result<PricingContext> {
        let parameters = model.replace(BlackScholesMertonParameterChanges {
            volatility: Some(volatility),
            risk_free_rate: if rate_shift == 0.0 {
                None
            } else {
                Some(model.risk_free_curve().shifted(rate_shift)?.into())
            },
            ..Default::default()
        })?;
        let mut shifted = context.replace(PricingContextChanges {
            model_parameters: Some(parameters.into()),
            spot_price: Some(spot),
            valuation_time: Some(valuation_time),
        })?;
        // Time sensitivities hold observed history fixed while moving hypothetical valuation.
        shifted.history_anchor = Some(context.history_anchor.unwrap_or(context.valuation_time()));
        Ok(shifted)
    };
    let price = |s, vol, r, t| -> Result<f64> {
        Ok(native(&shifted(s, vol, r, t)?, GreeksRequest::new([]))?.price())
    };
    let spot_samples = |vol, t, center| -> Result<(f64, f64, f64)> {
        Ok((
            price(spot + h, vol, 0.0, t)?,
            center,
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
                              gamma: bool,
                              prices: Option<(f64, f64)>|
     -> Result<((f64, f64), (f64, f64))> {
        if !native_spot_greeks {
            let (first, last) = if gamma {
                match prices {
                    Some(prices) => prices,
                    None => (
                        price(spot, first_vol, 0.0, first_time)?,
                        price(spot, last_vol, 0.0, last_time)?,
                    ),
                }
            } else {
                (p0, p0)
            };
            return Ok((
                derivatives(spot_samples(first_vol, first_time, first)?),
                derivatives(spot_samples(last_vol, last_time, last)?),
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
                derivatives(spot_samples(first_vol, first_time, first.price())?),
                derivatives(spot_samples(last_vol, last_time, last.price())?),
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
        && (2.0 * v).is_finite()
        && (want(Greek::Vega)
            || want(Greek::Volga)
            || (spot_ok && (want(Greek::Vanna) || want(Greek::Zomma))))
    {
        let prices = if want(Greek::Vega) || want(Greek::Volga) {
            Some((
                price(spot, vol + v, 0.0, time)?,
                price(spot, vol - v, 0.0, time)?,
            ))
        } else {
            None
        };
        if let Some((up, down)) = prices {
            result.set(request, Greek::Vega, (up - down) / (2.0 * v))?;
            result.set(request, Greek::Volga, ((up - p0) - (p0 - down)) / v / v)?;
        }
        if spot_ok && (want(Greek::Vanna) || want(Greek::Zomma)) {
            let (high, low) = paired_derivatives(
                vol + v,
                time,
                vol - v,
                time,
                want(Greek::Vanna),
                want(Greek::Zomma),
                prices,
            )?;
            result.set(request, Greek::Vanna, (high.0 - low.0) / (2.0 * v))?;
            result.set(request, Greek::Zomma, (high.1 - low.1) / (2.0 * v))?;
        }
    }
    if want(Greek::Rho) && r > 0.0 && (2.0 * r).is_finite() {
        result.set(
            request,
            Greek::Rho,
            (price(spot, vol, r, time)? - price(spot, vol, -r, time)?) / (2.0 * r),
        )?;
    }
    if want(Greek::DividendRho) {
        let dividend = model.dividend_curve().exact_flat_rate().unwrap_or(0.0);
        let q = symmetric_shift(dividend, settings.rate_shift);
        if q > 0.0 && (2.0 * q).is_finite() {
            let evaluate = |shift| -> Result<f64> {
                let model = model.replace(BlackScholesMertonParameterChanges {
                    dividend_yield: Some(model.dividend_curve().shifted(shift)?.into()),
                    ..Default::default()
                })?;
                let shifted = context.replace(PricingContextChanges {
                    model_parameters: Some(model.into()),
                    ..Default::default()
                })?;
                Ok(native(&shifted, GreeksRequest::new([]))?.price())
            };
            result.set(
                request,
                Greek::DividendRho,
                (evaluate(q)? - evaluate(-q)?) / (2.0 * q),
            )?;
        }
    }
    if want(Greek::Theta) || want(Greek::Charm) || want(Greek::Color) {
        let mut lower = option
            .valuation_start()
            .unwrap_or(crate::Date::new(-32767, 1, 1)?.start_of_day());
        let mut upper = option.expiry_time();
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
        let mut shift = settings
            .time_shift_micros()?
            .min(lower.micros_until(upper).max(0));
        if before_width > 0 && after_width > 0 {
            shift = shift.min(before_width).min(after_width);
        }
        let mut before = time.add_micros(-shift.min(before_width))?;
        let mut after = time.add_micros(shift.min(after_width))?;
        while settings.time_shift.is_none()
            && shift > 1
            && !option.time_stencil_available(context, before, after)
        {
            shift /= 2;
            before = time.add_micros(-shift.min(before_width))?;
            after = time.add_micros(shift.min(after_width))?;
        }
        if before != after && option.time_stencil_available(context, before, after) {
            let years = before.years_until(after);
            let prices = if want(Greek::Theta) {
                Some((
                    price(spot, vol, 0.0, after)?,
                    price(spot, vol, 0.0, before)?,
                ))
            } else {
                None
            };
            if let Some((after, before)) = prices {
                result.set(request, Greek::Theta, (after - before) / years)?;
            }
            if spot_ok && (want(Greek::Charm) || want(Greek::Color)) {
                let (first, last) = paired_derivatives(
                    vol,
                    before,
                    vol,
                    after,
                    want(Greek::Charm),
                    want(Greek::Color),
                    prices.map(|(after, before)| (before, after)),
                )?;
                result.set(request, Greek::Charm, (last.0 - first.0) / years)?;
                result.set(request, Greek::Color, (last.1 - first.1) / years)?;
            }
        }
    }
    let spot_undefined = option.at_spot_discontinuity(context, 0.0);
    let unavailable = GreeksRequest::new(Greek::ALL.into_iter().filter(|&greek| {
        let spot_derivative = matches!(
            greek,
            Greek::Delta
                | Greek::Gamma
                | Greek::Speed
                | Greek::Vanna
                | Greek::Zomma
                | Greek::Charm
                | Greek::Color
        );
        let volatility_derivative = matches!(
            greek,
            Greek::Vega | Greek::Volga | Greek::Vanna | Greek::Zomma
        );
        !(spot_derivative && spot_undefined || volatility_derivative && model.volatility() == 0.0)
    }));
    Ok(result.with_unavailable_stencils(unavailable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlackScholesMertonParameters, Date, EuropeanOption, OptionType};

    fn inputs() -> (EuropeanOption, PricingContext) {
        let start = Date::new(2025, 1, 1).unwrap().start_of_day();
        let end = Date::new(2026, 1, 1).unwrap().start_of_day();
        (
            EuropeanOption::new(OptionType::Call, 2.0, start, end).unwrap(),
            PricingContext::new(
                BlackScholesMertonParameters::new(0.05, 0.02, 0.2).unwrap(),
                2.0,
                start,
            )
            .unwrap(),
        )
    }

    #[test]
    fn native_greeks_return_without_repricing() {
        let (option, context) = inputs();
        let request = GreeksRequest::new([Greek::Delta]);
        let mut central = PricingResult::new(4.0).unwrap();
        central.set(request, Greek::Delta, 1.0).unwrap();
        let result = complete_greeks(
            &option,
            &context,
            request,
            Default::default(),
            true,
            central,
            |_, _| panic!("native result already satisfies the request"),
        )
        .unwrap();
        assert_eq!(result.price(), 4.0);
        assert_eq!(result.require(Greek::Delta).unwrap(), 1.0);
    }

    #[test]
    fn vega_completion_preserves_native_delta() {
        let (option, context) = inputs();
        let request = GreeksRequest::new([Greek::Delta, Greek::Vega]);
        let mut central = PricingResult::new(4.0).unwrap();
        central.set(request, Greek::Delta, 1.0).unwrap();
        let result = complete_greeks(
            &option,
            &context,
            request,
            Default::default(),
            true,
            central,
            |context, _| {
                PricingResult::new(
                    context.spot_price() + 10.0 * context.bsm_parameters()?.volatility(),
                )
            },
        )
        .unwrap();
        assert_eq!(result.price(), 4.0);
        assert_eq!(result.require(Greek::Delta).unwrap(), 1.0);
        assert!((result.require(Greek::Vega).unwrap() - 10.0).abs() < 1e-10);
        assert_eq!(result.get(Greek::Gamma), None);
    }

    #[test]
    fn scalar_speed_matches_cubic_derivative() {
        let (option, context) = inputs();
        let request = GreeksRequest::new([Greek::Speed]);
        let shifts = NumericalShiftSettings {
            spot_shift: Some(0.25),
            ..Default::default()
        };
        let result = complete_greeks(
            &option,
            &context,
            request,
            shifts,
            false,
            PricingResult::new(8.0).unwrap(),
            |context, _| PricingResult::new(context.spot_price().powi(3)),
        )
        .unwrap();
        assert_eq!(result.price(), 8.0);
        assert_eq!(result.require(Greek::Speed).unwrap(), 6.0);
        assert_eq!(result.get(Greek::Delta), None);
        assert_eq!(result.get(Greek::Gamma), None);
    }

    #[test]
    #[expect(
        clippy::cast_precision_loss,
        reason = "test times and counts are within exact f64 integer precision."
    )]
    fn selected_scalar_greeks_match_polynomial_derivatives() {
        let (option, context) = inputs();
        let context = context
            .replace(PricingContextChanges {
                model_parameters: Some(
                    BlackScholesMertonParameters::new(0.125, 0.0, 0.5)
                        .unwrap()
                        .into(),
                ),
                valuation_time: Some(Date::new(2025, 1, 3).unwrap().start_of_day()),
                ..Default::default()
            })
            .unwrap();
        for request in [
            GreeksRequest::new([Greek::Volga]),
            GreeksRequest::new([Greek::DividendRho]),
            GreeksRequest::new([Greek::Vega, Greek::Volga]),
            GreeksRequest::new([Greek::Vega, Greek::Volga, Greek::Vanna, Greek::Zomma]),
            GreeksRequest::new([Greek::Theta, Greek::Charm, Greek::Color]),
            GreeksRequest::all(),
        ] {
            let evaluate = |context: &PricingContext, _| {
                let days = option
                    .effective_time()
                    .micros_until(context.valuation_time()) as f64
                    / 86_400_000_000.0;
                let spot = context.spot_price();
                let model = context.bsm_parameters()?;
                PricingResult::new(
                    spot.powi(3) * (1.0 + model.volatility().powi(2))
                        + spot.powi(2) * days
                        + model.flat_risk_free_rate()?
                        + 3.0 * model.flat_dividend_yield()?,
                )
            };
            let result = complete_greeks(
                &option,
                &context,
                request,
                NumericalShiftSettings {
                    spot_shift: Some(0.25),
                    volatility_shift: 0.125,
                    rate_shift: 0.125,
                    time_shift: Some(std::time::Duration::from_secs(86_400)),
                },
                false,
                evaluate(&context, request).unwrap(),
                evaluate,
            )
            .unwrap();
            assert_eq!(result.price(), 18.125);
            for (greek, expected) in [
                (Greek::Delta, 23.078_125),
                (Greek::Gamma, 19.0),
                (Greek::Speed, 7.5),
                (Greek::Vega, 8.0),
                (Greek::Volga, 16.0),
                (Greek::Vanna, 12.0625),
                (Greek::Zomma, 12.0),
                (Greek::Theta, 1.46e+3),
                (Greek::Charm, 1.46e+3),
                (Greek::Color, 7.3e+2),
                (Greek::Rho, 1.0),
                (Greek::DividendRho, 3.0),
            ] {
                if request.has(greek) {
                    assert!((result.require(greek).unwrap() - expected).abs() < 1e-12);
                } else {
                    assert_eq!(result.get(greek), None);
                }
            }
        }
    }

    #[test]
    fn missing_native_gamma_falls_back_to_scalar_derivatives() {
        let (option, context) = inputs();
        let request = GreeksRequest::new([Greek::Zomma]);
        let evaluate = |context: &PricingContext, _| {
            PricingResult::new(
                context.spot_price().powi(2) * context.bsm_parameters()?.volatility().powi(2),
            )
        };
        let result = complete_greeks(
            &option,
            &context,
            request,
            NumericalShiftSettings {
                spot_shift: Some(0.25),
                volatility_shift: 0.125,
                ..Default::default()
            },
            true,
            evaluate(&context, request).unwrap(),
            evaluate,
        )
        .unwrap();
        assert!((result.require(Greek::Zomma).unwrap() - 0.8).abs() < 1e-12);
        assert_eq!(result.get(Greek::Gamma), None);
    }
}
