use crate::{
    BlackScholesMertonParameterChanges, Error, OptionContract, PricingContext,
    PricingContextChanges, PricingEngine, Result,
};

/// Configures a nonnegative volatility bracket and convergence tolerances.
///
/// Success requires the price tolerance; a collapsed bracket without a price match
/// returns [`Error::NonConvergence`]. See [`implied_volatility`] for usage.
#[derive(Clone, Copy, Debug)]
pub struct ImpliedVolatilitySettings {
    /// Sets the caller-selected nonnegative lower volatility bound.
    pub lower_bound: f64,
    /// Sets the caller-selected upper volatility bound, above the lower bound.
    pub upper_bound: f64,
    /// Sets the positive absolute price tolerance (default 1e-8).
    pub price_tolerance: f64,
    /// Sets the positive absolute volatility tolerance (default 1e-8).
    pub parameter_tolerance: f64,
    /// Sets the positive iteration limit (default 100).
    pub max_iterations: i32,
}
impl ImpliedVolatilitySettings {
    /// Sets the default absolute price residual tolerance; see [`Self::new`].
    pub const DEFAULT_PRICE_TOLERANCE: f64 = 1e-8;
    /// Sets the default absolute volatility tolerance; see [`Self::new`].
    pub const DEFAULT_PARAMETER_TOLERANCE: f64 = 1e-8;
    /// Sets the default iteration budget; see [`Self::new`].
    pub const DEFAULT_MAX_ITERATIONS: i32 = 100;

    /// Creates controls for an explicit volatility bracket with default tolerances.
    ///
    /// Bounds and tolerances are validated by [`implied_volatility`]. Digital and
    /// barrier prices may have several roots; choose bounds around the desired root.
    /// See [`implied_volatility`] for an example.
    pub const fn new(lower_bound: f64, upper_bound: f64) -> Self {
        Self {
            lower_bound,
            upper_bound,
            price_tolerance: Self::DEFAULT_PRICE_TOLERANCE,
            parameter_tolerance: Self::DEFAULT_PARAMETER_TOLERANCE,
            max_iterations: Self::DEFAULT_MAX_ITERATIONS,
        }
    }
}

/// Solves implied volatility with safeguarded Brent iteration, retaining the caller's bracket.
///
/// Endpoint pricing validates the contract before expiry identifiability checks.
/// Pricing errors propagate; non-finite scalar prices or residuals return [`Error::ComputationFailure`].
/// Digital and barrier prices need not be monotone in volatility; the required bracket may select
/// one of several roots or fail to bracket a quote attainable elsewhere.
/// A zero lower bound checks the exact deterministic price. CRR engines then
/// intersect positive-volatility trials with the tree's admissible range.
/// Monte Carlo engines require an explicit seed. Retain the engine returned by
/// `with_resolved_seed` to replay the solved price curve; solver tolerances do not
/// measure sampling error.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] for invalid settings, non-finite quotes or an unseeded Monte Carlo engine,
/// [`Error::UnsupportedOperation`] for volatility-independent payoffs, [`Error::RootNotBracketed`]
/// for same-sign endpoint residuals or a bracket outside the tree domain,
/// [`Error::NonConvergence`] on exhaustion or bracket convergence without a price match,
/// or [`Error::ComputationFailure`] for non-finite evaluations. Engine errors propagate.
///
/// # Examples
///
/// ```rust
/// use kiyosi::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
///     DigitalOption, OptionType, PricingContext, ImpliedVolatilitySettings, implied_volatility};
/// # fn main() -> kiyosi::Result<()> {
/// let start = Date::new(2025, 1, 1)?.start_of_day();
/// let option = DigitalOption::asset_or_nothing(OptionType::Call, 100.0,
///     start, Date::new(2026, 1, 1)?.start_of_day())?;
/// let context = PricingContext::new(
///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
/// let quote = AnalyticDigitalEngine.price(&option, &context)?;
/// let sigma = implied_volatility(&AnalyticDigitalEngine, &option, &context, quote,
///     ImpliedVolatilitySettings::new(0.1, 0.22))?;
/// assert!((sigma - 0.2).abs() < 1e-7);
/// # Ok(()) }
/// ```
#[expect(
    clippy::float_cmp,
    reason = "Exactly distinct residuals are required for nonzero inverse-quadratic denominators."
)]
pub fn implied_volatility<I: OptionContract, E: PricingEngine<I> + ?Sized>(
    engine: &E,
    option: &I,
    context: &PricingContext,
    observed_price: f64,
    settings: ImpliedVolatilitySettings,
) -> Result<f64> {
    let _sampling = crate::engines::monte_carlo::RequestCache::new();
    context.bsm_parameters()?;
    engine.validate_applicability(context)?;
    let s = settings;
    crate::error::finite(observed_price, false, "observed_price")?;
    crate::error::finite(s.lower_bound, false, "lower_bound")?;
    crate::error::finite(s.upper_bound, false, "upper_bound")?;
    crate::error::finite(s.price_tolerance, true, "price_tolerance")?;
    crate::error::finite(s.parameter_tolerance, true, "parameter_tolerance")?;
    if s.lower_bound < 0.0 {
        return Err(Error::InvalidInput(
            "lower_bound must be nonnegative".into(),
        ));
    }
    super::validate_solver_bounds(s.lower_bound, s.upper_bound, s.max_iterations)?;
    engine.validate_solver_seed()?;
    if let Some(prepared) = engine.prepare_request()? {
        return implied_volatility(prepared.as_ref(), option, context, observed_price, settings);
    }
    let evaluate = |volatility| -> Result<f64> {
        crate::cancellation::check()?;
        let model = context
            .bsm_parameters()?
            .replace(BlackScholesMertonParameterChanges {
                volatility: Some(volatility),
                ..Default::default()
            })?;
        let shifted = context.replace(PricingContextChanges {
            model_parameters: Some(model.into()),
            ..Default::default()
        })?;
        let residual = engine.price(option, &shifted)? - observed_price;
        if !residual.is_finite() {
            return Err(Error::ComputationFailure(
                "non-finite solver evaluation".into(),
            ));
        }
        Ok(residual)
    };
    let zero_residual = if s.lower_bound == 0.0 {
        Some(evaluate(0.0)?)
    } else {
        None
    };
    if zero_residual.is_some_and(|value| value.abs() <= s.price_tolerance) {
        if context.valuation_time() == option.expiry_time()
            || !engine.volatility_affects_price(option, context)
        {
            return Err(Error::UnsupportedOperation(
                "volatility does not affect the remaining payoff".into(),
            ));
        }
        return Ok(0.0);
    }
    let s = engine.prepare_implied_volatility(option, context, s)?;
    let (mut a, mut b) = (s.lower_bound, s.upper_bound);
    let (mut fa, mut fb) = (
        if a == 0.0 {
            match zero_residual {
                Some(value) => value,
                None => evaluate(a)?,
            }
        } else {
            evaluate(a)?
        },
        evaluate(b)?,
    );
    if context.valuation_time() == option.expiry_time()
        || !engine.volatility_affects_price(option, context)
    {
        return Err(Error::UnsupportedOperation(
            "volatility does not affect the remaining payoff".into(),
        ));
    }
    if fa.abs() <= s.price_tolerance {
        return Ok(a);
    }
    if fb.abs() <= s.price_tolerance {
        return Ok(b);
    }
    if (fa < 0.0) == (fb < 0.0) {
        return Err(Error::RootNotBracketed(
            "price is not bracketed by volatility bounds".into(),
        ));
    }
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let (mut c, mut fc, mut d, mut bisected) = (a, fa, a, true);
    for _ in 0..s.max_iterations {
        crate::cancellation::check()?;
        let scale = fa.abs().max(fb.abs()).max(fc.abs());
        let (ya, yb, yc) = (fa / scale, fb / scale, fc / scale);
        let mut trial = a + (b - a) * (ya / (ya - yb));
        if fa != fc && fb != fc {
            trial = a * (yb / (ya - yb)) * (yc / (ya - yc))
                + b * (ya / (yb - ya)) * (yc / (yb - yc))
                + c * (ya / (yc - ya)) * (yb / (yc - yb));
        }
        let quarter = a + (b - a) * 0.25;
        if !trial.is_finite()
            || trial <= quarter.min(b)
            || trial >= quarter.max(b)
            || (bisected && (trial - b).abs() >= 0.5 * (b - c).abs())
            || (!bisected && (trial - b).abs() >= 0.5 * (c - d).abs())
            || (bisected && (b - c).abs() <= s.parameter_tolerance)
            || (!bisected && (c - d).abs() <= s.parameter_tolerance)
        {
            trial = a + (b - a) * 0.5;
            bisected = true;
        } else {
            bisected = false;
        }
        let residual = evaluate(trial)?;
        if residual.abs() <= s.price_tolerance {
            return Ok(trial);
        }
        if (b - a).abs() <= s.parameter_tolerance {
            return Err(Error::NonConvergence(
                "volatility bracket converged without matching the observed price".into(),
            ));
        }
        d = c;
        c = b;
        fc = fb;
        if (fa < 0.0) == (residual < 0.0) {
            a = trial;
            fa = residual;
        } else {
            b = trial;
            fb = residual;
        }
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }
    Err(Error::NonConvergence(
        "implied-volatility solver did not converge".into(),
    ))
}
