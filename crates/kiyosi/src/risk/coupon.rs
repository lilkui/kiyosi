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

/// Sets finite signed coupon bounds and convergence controls.
///
/// Either tolerance stops bisection; endpoint and interpolation evaluations do
/// not count toward the iteration limit. See [`implied_coupon`] for an example.
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
/// Tries affine interpolation, verifies its residual, then bisects if needed.
/// Inputs remain unchanged; custom engines retain their own concurrency contract.
///
/// # Errors
///
/// Checks settings and finite observed price ([`Error::InvalidInput`])
/// before replacing and pricing both bounds. Replacement and engine errors propagate.
/// Equal endpoint prices return [`Error::UnsupportedOperation`] before
/// accepting endpoint matches; same-sign residuals return [`Error::RootNotBracketed`].
/// Non-finite scalar prices or residuals return [`Error::ComputationFailure`];
/// exhausted bisection returns [`Error::NonConvergence`].
///
/// # Examples
///
/// ```rust
/// use kiyosi::*;
/// # fn main() -> Result<()> {
/// let start = Date::new(2025, 1, 1)?;
/// let end = Date::new(2025, 4, 1)?;
/// let note = BinarySnowballOption::new(BinarySnowballTerms::new(0.06, start, end,
///     autocallable_observations(&[end], &[110.0], Some(&[0.08]), None)?))?;
/// let market = PricingContext::new(
///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
/// let engine = QuadratureAutocallableEngine::default();
/// let convention = CouponQuoteConvention::PreserveMaturityCoupon;
/// let target = note.with_coupon(0.12, convention)?;
/// let price = engine.price(&target, &market)?;
/// let coupon = implied_snowball_coupon(&engine, &note, &market, price,
///     convention, ImpliedCouponSettings::default())?;
/// assert!((coupon - 0.12).abs() < 1e-7);
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
) -> Result<f64> {
    solve(
        engine,
        option,
        context,
        observed_price,
        settings,
        &|note, coupon| note.with_coupon(coupon, convention),
    )
}

/// Solves a Phoenix price curve in its conditional annual coupon.
///
/// Uses the validation, endpoint, convergence and failure-state contract of
/// [`implied_snowball_coupon`], without a quote convention.
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
/// let start = Date::new(2025, 1, 1)?;
/// let end = Date::new(2025, 4, 1)?;
/// let note = PhoenixOption::new(PhoenixTerms::new(0.08, 100.0, 80.0, 100.0, 60.0,
///     start, end, autocallable_observations(&[end], &[110.0], None, Some(&[90.0]))?))?;
/// let market = PricingContext::new(
///     BlackScholesMertonParameters::new(0.04, 0.01, 0.3)?, 100.0, start.into())?;
/// let engine = QuadratureAutocallableEngine::default();
/// let quote = engine.price(&note.with_coupon(0.12)?, &market)?;
/// let coupon = implied_coupon(&engine, &note, &market, quote, ImpliedCouponSettings::default())?;
/// assert!((coupon - 0.12).abs() < 1e-7);
/// # Ok(()) }
/// ```
pub fn implied_coupon<E: PricingEngine<PhoenixOption> + ?Sized>(
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
        &PhoenixOption::with_coupon,
    )
}

fn solve<I, E: PricingEngine<I> + ?Sized>(
    engine: &E,
    option: &I,
    context: &PricingContext,
    observed: f64,
    s: ImpliedCouponSettings,
    replace: &impl Fn(&I, f64) -> Result<I>,
) -> Result<f64> {
    if !observed.is_finite()
        || !s.lower_bound.is_finite()
        || !s.upper_bound.is_finite()
        || s.lower_bound >= s.upper_bound
        || !s.price_tolerance.is_finite()
        || s.price_tolerance <= 0.0
        || !s.parameter_tolerance.is_finite()
        || s.parameter_tolerance <= 0.0
        || s.max_iterations <= 0
    {
        return Err(Error::InvalidInput(
            "implied-coupon settings or price are invalid".into(),
        ));
    }
    if let Some(prepared) = engine.prepare_request()? {
        return solve(prepared.as_ref(), option, context, observed, s, replace);
    }
    let checked_price = |note: &I| {
        let price = engine.price(note, context)?;
        if !price.is_finite() {
            return Err(Error::ComputationFailure("non-finite coupon price".into()));
        }
        Ok(price)
    };
    let price = |coupon| checked_price(&replace(option, coupon)?);
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
    let (lower, upper) = (checked_price(&lower_note)?, checked_price(&upper_note)?);
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
    let span = upper - lower;
    let fraction = if span.is_finite() {
        (observed - lower) / span
    } else {
        (0.5 * observed - 0.5 * lower) / (0.5 * upper - 0.5 * lower)
    };
    let interpolate = |fraction| {
        if (lo < 0.0) != (hi < 0.0) {
            lo * (1.0 - fraction) + hi * fraction
        } else {
            lo + (hi - lo) * fraction
        }
    };
    let coupon = interpolate(fraction);
    if residual(price(coupon)?)?.abs() <= s.price_tolerance {
        return Ok(coupon);
    }
    for _ in 0..s.max_iterations {
        let mid = if (lo < 0.0) != (hi < 0.0) {
            0.5 * lo + 0.5 * hi
        } else {
            lo + 0.5 * (hi - lo)
        };
        let value = residual(price(mid)?)?;
        if value.abs() <= s.price_tolerance || hi - lo <= s.parameter_tolerance {
            return Ok(mid);
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
