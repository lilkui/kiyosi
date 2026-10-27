use crate::{
    BlackScholesMertonParameterChanges, Error, ErrorCategory, EuropeanOption,
    EuropeanPricingEngine, PricingContext, PricingContextChanges, Result,
};

/// Positive volatility bracket and convergence controls. Either tolerance stops iteration.
#[derive(Clone, Copy, Debug)]
pub struct ImpliedVolatilitySettings {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub price_tolerance: f64,
    pub parameter_tolerance: f64,
    pub max_iterations: i32,
}
impl Default for ImpliedVolatilitySettings {
    fn default() -> Self {
        Self {
            lower_bound: 0.0001,
            upper_bound: 4.0,
            price_tolerance: 1e-8,
            parameter_tolerance: 1e-8,
            max_iterations: 100,
        }
    }
}

/// Safeguarded Brent solve, retaining the caller's bracket. Endpoint pricing validates
/// the contract before expiry identifiability checks. Inputs remain unchanged on failure.
/// Pricing errors propagate; non-finite scalar prices/residuals use SolverNonFinite.
pub fn implied_volatility<E: EuropeanPricingEngine + ?Sized>(
    engine: &E,
    option: &EuropeanOption,
    context: &PricingContext,
    observed_price: f64,
    settings: ImpliedVolatilitySettings,
) -> Result<f64> {
    let s = settings;
    if !observed_price.is_finite()
        || !s.lower_bound.is_finite()
        || !s.upper_bound.is_finite()
        || s.lower_bound <= 0.0
        || s.lower_bound >= s.upper_bound
        || !s.price_tolerance.is_finite()
        || s.price_tolerance <= 0.0
        || !s.parameter_tolerance.is_finite()
        || s.parameter_tolerance <= 0.0
        || s.max_iterations <= 0
    {
        return Err(Error::new(
            ErrorCategory::InvalidParameter,
            "implied-volatility settings or price are invalid",
        ));
    }
    let evaluate = |volatility| -> Result<f64> {
        let model = context
            .model_parameters()
            .replace(BlackScholesMertonParameterChanges {
                volatility: Some(volatility),
                ..Default::default()
            })?;
        let shifted = context.replace(PricingContextChanges {
            model_parameters: Some(model),
            ..Default::default()
        })?;
        let residual = engine.price(option, &shifted)? - observed_price;
        if !residual.is_finite() {
            return Err(Error::new(
                ErrorCategory::SolverNonFinite,
                "non-finite solver evaluation",
            ));
        }
        Ok(residual)
    };
    let (mut a, mut b) = (s.lower_bound, s.upper_bound);
    let (mut fa, mut fb) = (evaluate(a)?, evaluate(b)?);
    if context.valuation_time() == option.expiry_date().start_of_day() {
        return Err(Error::new(
            ErrorCategory::UnsupportedOperation,
            "volatility does not affect expiry payoff",
        ));
    }
    if fa.abs() <= s.price_tolerance {
        return Ok(a);
    }
    if fb.abs() <= s.price_tolerance {
        return Ok(b);
    }
    if (fa < 0.0) == (fb < 0.0) {
        return Err(Error::new(
            ErrorCategory::UnbracketedVolatility,
            "price is not bracketed by volatility bounds",
        ));
    }
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let (mut c, mut fc, mut d, mut bisected) = (a, fa, a, true);
    for _ in 0..s.max_iterations {
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
        if residual.abs() <= s.price_tolerance || (b - a).abs() <= s.parameter_tolerance {
            return Ok(trial);
        }
        d = c;
        c = b;
        fc = fb;
        if (fa < 0.0) != (residual < 0.0) {
            b = trial;
            fb = residual;
        } else {
            a = trial;
            fa = residual;
        }
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }
    Err(Error::new(
        ErrorCategory::SolverNonConvergence,
        "implied-volatility solver did not converge",
    ))
}
