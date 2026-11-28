use crate::{
    AutocallableNote, Error, PhoenixOption, PricingContext, PricingEngine, Result,
    SnowballCouponTerms,
};

/// Selects how a snowball maturity coupon follows its quoted observation coupon.
///
/// See [`implied_snowball_coupon`] for an example.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CouponQuoteConvention {
    /// Shifts maturity coupon by the same amount as every observation coupon.
    ShiftMaturityCoupon,
    /// Keeps maturity coupon fixed while shifting observation coupons.
    PreserveMaturityCoupon,
}

/// Owns an annual snowball coupon quote and its maturity convention.
///
/// Apply both fields with [`AutocallableNote::with_coupon`] to preserve
/// the solved cash flows. See [`implied_snowball_coupon`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnowballCouponQuote {
    /// Records the first observation's annual decimal coupon; later spreads are preserved.
    pub first_observation_coupon: f64,
    /// Records how the maturity coupon follows the quoted observation coupon.
    pub quote_convention: CouponQuoteConvention,
}

/// Sets finite signed coupon bounds and convergence controls in the contract's units.
///
/// Success requires the price tolerance; a collapsed bracket without a price match
/// returns [`Error::NonConvergence`]. Endpoint and interpolation evaluations do
/// not count toward the iteration limit. See [`implied_phoenix_coupon`] for an example.
#[derive(Clone, Copy, Debug)]
pub struct ImpliedCouponSettings {
    /// Sets the finite lower coupon bound (default 0).
    pub lower_bound: f64,
    /// Sets the finite upper coupon bound, above the lower bound (default 2).
    pub upper_bound: f64,
    /// Sets positive finite absolute price tolerance (default 1e-8).
    pub price_tolerance: f64,
    /// Sets positive finite absolute coupon interval tolerance (default 1e-8).
    pub parameter_tolerance: f64,
    /// Sets the positive bisection iteration limit (default 100).
    pub max_iterations: i32,
}
impl Default for ImpliedCouponSettings {
    fn default() -> Self {
        Self {
            lower_bound: 0.0,
            upper_bound: 2.0,
            price_tolerance: 1e-8,
            parameter_tolerance: 1e-8,
            max_iterations: 100,
        }
    }
}

/// Solves a snowball family's price curve in its first observation coupon.
///
/// Shifts the whole coupon schedule, with maturity linkage selected explicitly.
/// Intersects the supplied bracket with the ternary minimum-coupon domain when
/// maturity shifts. The minimum coupon is a fixed lower bound.
/// Tries affine interpolation, verifies its residual, then bisects if needed.
/// The solver borrows its inputs as read-only. Custom engines define their own
/// concurrency contract.
/// Model support follows the engine, including local volatility with finite differences.
/// The returned [`SnowballCouponQuote`] retains the supplied maturity convention.
///
/// Monte Carlo engines require an explicit seed; retain the engine returned by
/// `with_resolved_seed` for solving and repricing the quote.
///
/// # Errors
///
/// Checks settings and finite observed price ([`Error::InvalidInput`])
/// before replacing and pricing both bounds. Replacement and engine errors propagate.
/// Equal endpoint prices return [`Error::UnsupportedOperation`] before
/// accepting endpoint matches; same-sign residuals return [`Error::RootNotBracketed`].
/// A bracket with no nondegenerate interval in the valid coupon domain also
/// returns [`Error::RootNotBracketed`].
/// Non-finite scalar prices or residuals return [`Error::ComputationFailure`];
/// exhausted bisection or bracket convergence without a price match returns [`Error::NonConvergence`].
///
/// # Examples
///
/// ```rust
/// use kiyosi::*;
/// # fn main() -> Result<()> {
/// let start = Date::new(2025, 1, 1)?.start_of_day();
/// let end = Date::new(2025, 4, 1)?.start_of_day();
/// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.06, start, end,
///     SnowballObservation::from_columns(&[end], &[110.0], &[0.08])?, kiyosi::TradingCalendar::weekdays()))?;
/// let market = PricingContext::new(
///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
/// let engine = QuadratureAutocallableEngine::default();
/// let convention = CouponQuoteConvention::PreserveMaturityCoupon;
/// let target = note.with_coupon(0.12, convention)?;
/// let price = engine.price(&target, &market)?;
/// let quote = implied_snowball_coupon(&engine, &note, &market, price,
///     convention, ImpliedCouponSettings::default())?;
/// assert!((quote.first_observation_coupon - 0.12).abs() < 1e-7);
/// let repriced = note.with_coupon(quote.first_observation_coupon, quote.quote_convention)?;
/// assert!((engine.price(&repriced, &market)? - price).abs() <= 1e-8);
/// # Ok(()) }
/// ```
pub fn implied_snowball_coupon<
    T: SnowballCouponTerms,
    E: PricingEngine<AutocallableNote<T>> + ?Sized,
>(
    engine: &E,
    option: &AutocallableNote<T>,
    context: &PricingContext,
    observed_price: f64,
    convention: CouponQuoteConvention,
    settings: ImpliedCouponSettings,
) -> Result<SnowballCouponQuote> {
    let first_observation_coupon = solve(
        engine,
        option,
        context,
        observed_price,
        settings,
        option.minimum_quoted_coupon(convention),
        &|note, coupon| note.with_coupon(coupon, convention),
    )?;
    Ok(SnowballCouponQuote {
        first_observation_coupon,
        quote_convention: convention,
    })
}

/// Solves a Phoenix price curve in its conditional periodic coupon rate.
///
/// Uses the validation, endpoint, convergence and failure-state contract of
/// [`implied_snowball_coupon`], without a quote convention.
/// Bounds, parameter tolerance and the result use decimal periodic coupon rates.
/// Apply the result with [`PhoenixOption::with_coupon`].
///
/// Monte Carlo engines require an explicit seed; retain the engine returned by
/// `with_resolved_seed` for solving and repricing the quote.
///
/// # Errors
///
/// Returns the validation, pricing and solver errors of [`implied_snowball_coupon`].
///
/// # Examples
///
/// ```rust
/// use kiyosi::*;
/// # fn main() -> Result<()> {
/// let start = Date::new(2025, 1, 1)?.start_of_day();
/// let end = Date::new(2025, 4, 1)?.start_of_day();
/// let knock_in_observation = KnockInObservation::EveryTradingDay {
///     observation_clock_time: TimeOfDay::new(0, 0, 0, 0)?,
/// };
/// let note = PhoenixOption::new(PhoenixTerms::new(0.02, 100.0, 80.0, 100.0, 60.0,
///     start, end, vec![KnockOutObservation::new(end, 110.0)?], vec![CouponObservation::new(end, 90.0)?], knock_in_observation, kiyosi::TradingCalendar::weekdays()))?;
/// let market = PricingContext::new(
///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
/// let engine = QuadratureAutocallableEngine::default();
/// let quoted = note.with_coupon(0.03)?;
/// let quote = engine.price(&quoted, &market)?;
/// let coupon = implied_phoenix_coupon(&engine, &note, &market, quote, ImpliedCouponSettings::default())?;
/// assert!((coupon - 0.03).abs() < 1e-7);
/// let repriced = note.replace(PhoenixOptionChanges { coupon_per_period: Some(coupon), ..Default::default() })?;
/// assert!((engine.price(&repriced, &market)? - quote).abs() < 1e-8);
/// # Ok(()) }
/// ```
pub fn implied_phoenix_coupon<E: PricingEngine<PhoenixOption> + ?Sized>(
    engine: &E,
    option: &PhoenixOption,
    context: &PricingContext,
    observed_price: f64,
    settings: ImpliedCouponSettings,
) -> Result<f64> {
    solve(
        engine,
        option,
        context,
        observed_price,
        settings,
        None,
        &PhoenixOption::with_coupon,
    )
}

#[expect(
    clippy::float_cmp,
    reason = "Exactly equal coupon endpoints identify a collapsed bracket."
)]
fn solve<I, E: PricingEngine<I> + ?Sized>(
    engine: &E,
    option: &I,
    context: &PricingContext,
    observed: f64,
    mut s: ImpliedCouponSettings,
    minimum: Option<f64>,
    replace: &impl Fn(&I, f64) -> Result<I>,
) -> Result<f64> {
    let _sampling = crate::engines::monte_carlo::RequestCache::new();
    crate::error::finite(observed, false, "observed_price")?;
    crate::error::finite(s.lower_bound, false, "lower_bound")?;
    crate::error::finite(s.upper_bound, false, "upper_bound")?;
    crate::error::finite(s.price_tolerance, true, "price_tolerance")?;
    crate::error::finite(s.parameter_tolerance, true, "parameter_tolerance")?;
    super::validate_solver_bounds(s.lower_bound, s.upper_bound, s.max_iterations)?;
    if let Some(minimum) = minimum {
        s.lower_bound = s.lower_bound.max(minimum);
        if s.lower_bound >= s.upper_bound {
            return Err(Error::RootNotBracketed(
                "coupon bounds exclude the contract's valid domain".into(),
            ));
        }
    }
    engine.validate_solver_seed()?;
    let prepared = engine.prepare_request()?;
    if let Some(prepared) = &prepared {
        prepared.validate_solver_seed()?;
    }
    let checked = |price: f64| {
        if !price.is_finite() {
            return Err(Error::ComputationFailure("non-finite coupon price".into()));
        }
        Ok(price)
    };
    let checked_price = |note: &I| {
        crate::cancellation::check()?;
        let price = match &prepared {
            Some(prepared) => prepared.price(note, context)?,
            None => engine.price(note, context)?,
        };
        checked(price)
    };
    let residual = |value: f64| {
        let residual = value - observed;
        if !residual.is_finite() {
            return Err(Error::ComputationFailure(
                "non-finite coupon residual".into(),
            ));
        }
        Ok(residual)
    };
    let (mut lo, mut hi) = (s.lower_bound, s.upper_bound);
    let lower_note = replace(option, lo)?;
    let upper_note = replace(option, hi)?;
    crate::cancellation::check()?;
    let (original_lo, original_hi) = (lo, hi);
    // Tight tolerances require independent pricing of each trial to account
    // for rounding in the rolled-back cash flows.
    let mut curve = None;
    if s.price_tolerance >= 1e-10 * observed.abs().max(1.0) {
        let shared = match &prepared {
            Some(prepared) => prepared.prepare_coupon_pricer(&lower_note, &upper_note, context),
            None => engine.prepare_coupon_pricer(&lower_note, &upper_note, context),
        };
        match shared {
            Ok(result) => curve = result,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted => {
                return Err(error.into());
            }
            Err(_) => {}
        }
    }
    let mut bounds = Ok(None);
    if let Some(pricer) = &curve {
        bounds = pricer(&lower_note, 0.0).and_then(|lower| {
            Ok(lower
                .zip(pricer(&upper_note, 1.0)?)
                .map(|(lo, hi)| [lo, hi])
                .filter(|values| {
                    values.iter().all(|value| {
                        value.is_finite() && s.price_tolerance >= 1e-10 * value.abs().max(1.0)
                    })
                }))
        });
    }
    if matches!(&bounds, Ok(None)) {
        curve = None;
        bounds = match &prepared {
            Some(prepared) => prepared.price_coupon_bounds(&lower_note, &upper_note, context),
            None => engine.price_coupon_bounds(&lower_note, &upper_note, context),
        };
    } else if bounds.is_err() {
        curve = None;
    }
    let (lower, upper) = match bounds {
        Ok(Some([lower, upper])) => (checked(lower)?, checked(upper)?),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted => {
            return Err(error.into());
        }
        Ok(None) | Err(_) => (checked_price(&lower_note)?, checked_price(&upper_note)?),
    };
    if lower == upper {
        return Err(Error::UnsupportedOperation(
            "coupon does not affect remaining cashflows".into(),
        ));
    }
    let (mut flo, fhi) = (residual(lower)?, residual(upper)?);
    if flo.abs() <= s.price_tolerance {
        return Ok(lo);
    }
    if fhi.abs() <= s.price_tolerance {
        return Ok(hi);
    }
    if (flo < 0.0) == (fhi < 0.0) {
        return Err(Error::RootNotBracketed(
            "price is not bracketed by coupon bounds".into(),
        ));
    }
    let price = |coupon: f64| {
        let note = replace(option, coupon)?;
        crate::cancellation::check()?;
        if let Some(pricer) = &curve {
            let span = original_hi - original_lo;
            let fraction = if span.is_finite() {
                (coupon - original_lo) / span
            } else {
                (0.5 * coupon - 0.5 * original_lo) / (0.5 * original_hi - 0.5 * original_lo)
            };
            match pricer(&note, fraction) {
                Ok(Some(value)) => return checked(value),
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted => {
                    return Err(error.into());
                }
                Ok(None) | Err(_) => {}
            }
        }
        checked_price(&note)
    };
    let span = upper - lower;
    let fraction = if span.is_finite() {
        (observed - lower) / span
    } else {
        (0.5 * observed - 0.5 * lower) / (0.5 * upper - 0.5 * lower)
    };
    let interpolate = |fraction| {
        if (lo < 0.0) == (hi < 0.0) {
            lo + (hi - lo) * fraction
        } else {
            lo * (1.0 - fraction) + hi * fraction
        }
    };
    let coupon = interpolate(fraction);
    if lo < coupon && coupon < hi && residual(price(coupon)?)?.abs() <= s.price_tolerance {
        return Ok(coupon);
    }
    for _ in 0..s.max_iterations {
        crate::cancellation::check()?;
        let mid = lo.midpoint(hi);
        if mid == lo || mid == hi {
            return Err(Error::NonConvergence(
                "coupon bracket has no representable midpoint".into(),
            ));
        }
        let value = residual(price(mid)?)?;
        if value.abs() <= s.price_tolerance {
            return Ok(mid);
        }
        if hi - lo <= s.parameter_tolerance {
            return Err(Error::NonConvergence(
                "coupon bracket converged without matching the observed price".into(),
            ));
        }
        if (flo < 0.0) == (value < 0.0) {
            lo = mid;
            flo = value;
        } else {
            hi = mid;
        }
    }
    Err(Error::NonConvergence(
        "implied-coupon solver did not converge".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_coupon_bounds_price_only_the_endpoints() -> Result<()> {
        struct EndpointEngine(std::cell::Cell<usize>);
        impl PricingEngine<f64> for EndpointEngine {
            fn price(&self, coupon: &f64, _: &PricingContext) -> Result<f64> {
                self.0.set(self.0.get() + 1);
                Ok(if *coupon <= 0.125 { 1.0 } else { 3.0 })
            }
        }
        let context = PricingContext::new(
            crate::BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
            100.0,
            crate::Date::new(2025, 1, 1)?.start_of_day(),
        )?;
        let engine = EndpointEngine(std::cell::Cell::new(0));
        let result = solve(
            &engine,
            &0.125,
            &context,
            2.0,
            ImpliedCouponSettings {
                lower_bound: 0.125,
                upper_bound: libm::nextafter(0.125, f64::INFINITY),
                parameter_tolerance: 1e-30,
                ..Default::default()
            },
            None,
            &|_, coupon| Ok(coupon),
        );
        assert!(matches!(result, Err(Error::NonConvergence(_))));
        assert_eq!(engine.0.get(), 2);
        Ok(())
    }

    struct PreparedEngine(bool);

    impl PricingEngine<f64> for PreparedEngine {
        fn prepare_request(&self) -> Result<Option<Box<dyn PricingEngine<f64> + '_>>> {
            if self.0 {
                return Err(Error::UnsupportedOperation(
                    "request is already prepared".into(),
                ));
            }
            Ok(Some(Box::new(Self(true))))
        }

        fn price(&self, coupon: &f64, _context: &PricingContext) -> Result<f64> {
            if !self.0 {
                return Err(Error::UnsupportedOperation(
                    "request is not prepared".into(),
                ));
            }
            Ok(coupon * coupon)
        }
    }

    #[test]
    fn coupon_solver_retains_prepared_price_curve_across_iterations() -> Result<()> {
        let context = PricingContext::new(
            crate::BlackScholesMertonParameters::new(0.0, 0.0, 0.2)?,
            100.0,
            crate::Date::new(2025, 1, 1)?.start_of_day(),
        )?;
        let engine = PreparedEngine(false);
        let coupon = solve(
            &engine,
            &0.2,
            &context,
            0.09,
            ImpliedCouponSettings {
                lower_bound: 0.1,
                upper_bound: 0.5,
                ..Default::default()
            },
            None,
            &|_, coupon| Ok(coupon),
        )?;
        assert!((coupon - 0.3).abs() < 1e-7);
        Ok(())
    }
}
