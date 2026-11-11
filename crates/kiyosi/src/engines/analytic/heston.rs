use std::f64::consts::PI;

use num_complex::Complex64;

use super::european::lognormal_price;
use crate::instruments::monitoring::validate_life;
use crate::numerics::{Sum, checked_with_scale, log_ratio, scaled_cash};
use crate::{
    Error, EuropeanOption, GreeksRequest, HestonParameters, OptionType, PricingContext,
    PricingEngine, PricingResult, Result,
};

/// Configures adaptive Heston Fourier integration; see [`AnalyticHestonEuropeanEngine::new`].
///
/// The target price error is `absolute_tolerance + relative_tolerance * abs(price)`.
/// Numerical error estimates do not guarantee accuracy for every valid model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HestonIntegrationSettings {
    /// Sets a finite nonnegative absolute tolerance in price units; defaults to `1e-8`.
    pub absolute_tolerance: f64,
    /// Sets a finite nonnegative relative tolerance; defaults to `1e-8`.
    pub relative_tolerance: f64,
    /// Caps characteristic-function calculations across the entire price request.
    ///
    /// Includes the analytic control variate and transformed tail integration;
    /// cached values are not counted twice. Defaults to 10,000. Expiry and
    /// deterministic-variance branches consume no evaluations.
    pub max_evaluations: usize,
}

impl Default for HestonIntegrationSettings {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1e-8,
            relative_tolerance: 1e-8,
            max_evaluations: 10_000,
        }
    }
}

impl HestonIntegrationSettings {
    /// Validates tolerances and the evaluation budget; see [`AnalyticHestonEuropeanEngine::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite or negative tolerances,
    /// two zero tolerances, or a zero evaluation budget.
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("absolute_tolerance", self.absolute_tolerance),
            ("relative_tolerance", self.relative_tolerance),
        ] {
            crate::error::finite(value, false, name)?;
            if value < 0.0 {
                return Err(Error::InvalidInput(
                    format!("{name} must be nonnegative").into(),
                ));
            }
        }
        if self.absolute_tolerance == 0.0 && self.relative_tolerance == 0.0 {
            return Err(Error::InvalidInput(
                "absolute_tolerance and relative_tolerance must not both be zero".into(),
            ));
        }
        if self.max_evaluations == 0 {
            return Err(Error::InvalidInput(
                "max_evaluations must be positive".into(),
            ));
        }
        Ok(())
    }
}

/// Prices European calls and puts under Heston using adaptive Fourier integration.
///
/// Uses a continuous square-root/logarithm representation and a deterministic
/// variance control variate. A variable transform covers the semi-infinite
/// frequency range; an analytic bound accounts for the unresolved tail.
/// Zero variance parameters
/// and correlation endpoints are valid, without imposing the Feller condition.
/// Supports deterministic rate/dividend curves and prices only; Greeks and
/// implied-parameter solvers are unsupported. Owns its settings and supports concurrent
/// calls on shared instances, with request-local integration state.
/// See [`Self::new`] for a complete example.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnalyticHestonEuropeanEngine {
    settings: HestonIntegrationSettings,
}

impl AnalyticHestonEuropeanEngine {
    /// Creates an engine after validating its integration settings.
    ///
    /// # Errors
    ///
    /// Propagates [`HestonIntegrationSettings::validate`] errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{AnalyticHestonEuropeanEngine, Date, EuropeanOption, HestonParameters,
    ///     HestonIntegrationSettings, OptionType, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start.add_days(31)?,
    ///     Date::new(2026, 1, 1)?.start_of_day())?;
    /// let model = HestonParameters::new(kiyosi::HestonParameterInputs {
    ///     risk_free_rate: 0.05.into(), dividend_yield: 0.02.into(),
    ///     initial_variance: 0.04, long_run_variance: 0.04,
    ///     mean_reversion_speed: 1.5, vol_of_vol: 0.3, correlation: -0.7,
    /// })?;
    /// let context = PricingContext::new(model, 100.0, start.into())?;
    /// let engine = AnalyticHestonEuropeanEngine::new(HestonIntegrationSettings::default())?;
    /// assert_eq!(engine.settings().max_evaluations, 10_000);
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: HestonIntegrationSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned integration settings; see [`Self::new`].
    pub fn settings(&self) -> HestonIntegrationSettings {
        self.settings
    }

    /// Prices within the permitted valuation bounds, returning intrinsic value at expiry.
    ///
    /// Validates model applicability, permitted valuation bounds and rate/dividend
    /// coverage in that order. Valuation may precede the effective time, with curves
    /// covering valuation through expiry; see [`crate::OptionContract::valuation_start`].
    /// Initial variance is at valuation. See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for other models,
    /// [`Error::InvalidInput`] after expiry or outside curve coverage,
    /// [`Error::NonConvergence`] when the request-wide budget or floating-point
    /// resolution cannot meet the estimated error target, and
    /// [`Error::ComputationFailure`] for unrepresentable intermediates or invalid
    /// results. Only rounding-sized negative prices may be clipped. Inputs and
    /// engine settings remain unchanged on success and failure.
    pub fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let model = context.model_parameters().as_heston()?;
        validate_life(option, context)?;
        let start = context.valuation_time();
        let end = option.expiry_time();
        model.risk_free_curve().validate_horizon(start, end)?;
        model.dividend_curve().validate_horizon(start, end)?;
        let spot = context.spot_price();
        let strike = option.strike();
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let time = start.years_until(end);
        if time == 0.0 {
            return Ok((sign * (spot - strike)).max(0.0));
        }
        let risk = model.risk_free_curve().log_factor(start, end)?;
        let dividend = model.dividend_curve().log_factor(start, end)?;
        let asset = scaled_cash(spot, dividend);
        let cash = scaled_cash(strike, risk);
        let scale = asset.max(cash);
        if !asset.is_finite() || !cash.is_finite() {
            return Err(computation());
        }
        let variance = integrated_variance(model, time);
        if !variance.is_finite() || variance < 0.0 {
            return Err(computation());
        }
        let absorbing = model.initial_variance() == 0.0
            && (model.mean_reversion_speed() == 0.0 || model.long_run_variance() == 0.0);
        let log_moneyness = log_ratio(spot, strike) + (dividend - risk);
        if absorbing {
            let value = if sign * log_moneyness > 0.0 {
                let amount = if sign > 0.0 { asset } else { cash };
                amount * -(-sign * log_moneyness).exp_m1()
            } else {
                0.0
            };
            return checked_with_scale(value, scale);
        }
        if variance == 0.0 {
            return Err(computation());
        }
        let base = lognormal_price(
            spot,
            strike,
            dividend - risk,
            (dividend, risk),
            variance.sqrt(),
            1.0,
            sign,
        )?;
        if model.vol_of_vol() == 0.0 {
            return checked_with_scale(base, scale);
        }
        if scale == 0.0 {
            return Err(computation());
        }
        let weight = (spot.ln() + dividend - scale.ln())
            .midpoint(strike.ln() + risk - scale.ln())
            .exp()
            / PI;
        let mut integrand = Integrand {
            model,
            time,
            variance,
            // Keep the payoff transform's unit frequency scale visible when rare paths inflate variance.
            frequency_scale: (1.0 / variance.sqrt()).max(1.0),
            log_moneyness,
            weight,
            remaining: self.settings.max_evaluations,
        };
        let correction = integrate(&mut integrand, base / scale, self.settings, scale)?;
        let price = base + scale * correction;
        checked_with_scale(price, scale)
    }

    /// Returns the price with requested Greeks marked unsupported; see [`Self::new`].
    ///
    /// Use [`PricingResult::require`] to reject an unavailable Greek.
    /// Inputs remain unchanged; valid numerical shifts are unused.
    ///
    /// # Errors
    ///
    /// Checks Heston applicability and propagates [`Self::price`] errors.
    pub fn price_with_greeks(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        request: GreeksRequest,
    ) -> Result<PricingResult> {
        let settings = crate::NumericalShiftSettings::default();
        self.validate_applicability(context)?;
        settings.validate()?;
        PricingResult::new(self.price(option, context)?)
            .map(|result| result.with_request(request, GreeksRequest::new([])))
    }
}

impl PricingEngine for AnalyticHestonEuropeanEngine {
    fn greek_method(
        &self,
        option: &crate::EuropeanOption,
        context: &PricingContext,
    ) -> Result<crate::GreekMethod> {
        <Self as PricingEngine<crate::EuropeanOption>>::validate_applicability(self, context)?;
        Ok(crate::GreekMethod::for_valuation(
            option,
            context,
            crate::GreekMethod::Unsupported,
        ))
    }

    fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
        context.model_parameters().as_heston()?;
        Ok(())
    }
    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }

    fn price_with_greeks_with_settings(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        request: GreeksRequest,
        settings: crate::NumericalShiftSettings,
    ) -> Result<PricingResult> {
        <Self as PricingEngine<EuropeanOption>>::validate_applicability(self, context)?;
        settings.validate()?;
        Self::price_with_greeks(self, option, context, request)
    }
}

fn computation() -> Error {
    Error::ComputationFailure("Heston calculation produced an invalid numerical result".into())
}
fn nonconvergence() -> Error {
    Error::NonConvergence("Heston integration did not reach the requested price accuracy".into())
}

pub(crate) fn integrated_variance(model: &HestonParameters, time: f64) -> f64 {
    let rate = model.mean_reversion_speed();
    let x = rate * time;
    if x < 0.01 {
        let mut term = 0.5;
        let mut sum = term;
        for n in 3..=12 {
            term *= -x / f64::from(n);
            sum += term;
        }
        let transient = time * (1.0 - x * sum);
        let growth_time = time * (time * sum);
        // Preserve kappa*theta before kappa*T/2 can underflow.
        let drift = rate * model.long_run_variance();
        let growth = if drift.is_normal() {
            drift * growth_time
        } else {
            scaled_cash(model.long_run_variance(), rate.ln() + growth_time.ln())
        };
        return model.initial_variance() * transient + growth;
    }
    let complement = 1.0 + (-x).exp_m1() / x;
    let transient = -(-x).exp_m1() / rate;
    model.initial_variance() * transient + model.long_run_variance() * (complement * time)
}

// Compute the small remainders directly: subtracting one loses short-time variance.
fn exprel_minus_one(z: Complex64) -> Complex64 {
    if z.norm() < 0.5 {
        let mut term = z / 2.0;
        let mut sum = term;
        for n in 3..=20 {
            term *= z / f64::from(n);
            sum += term;
        }
        sum
    } else {
        (z.exp() - 1.0) / z - 1.0
    }
}

fn logrel_minus_one(z: Complex64) -> Complex64 {
    if z.norm() < 0.125 {
        let mut power = -z;
        let mut sum = power / 2.0;
        for n in 3..=24 {
            power *= -z;
            sum += power / f64::from(n);
        }
        sum
    } else {
        (z + 1.0).ln() / z - 1.0
    }
}

// Rationalize the Riccati solution to avoid dividing by xi squared near deterministic variance.
fn log_characteristic(model: &HestonParameters, time: f64, u: f64) -> Complex64 {
    let xi = model.vol_of_vol();
    let kappa = model.mean_reversion_speed();
    let q = u * u + 0.25;
    let b = Complex64::new(
        kappa - 0.5 * model.correlation() * xi,
        -model.correlation() * xi * u,
    );
    let d = (b * b + xi * (xi * q)).sqrt();
    let c = q / (b + d);
    let em1 = exprel_minus_one(-d * time);
    let h = if (d * time).norm() < 0.5 {
        time * (1.0 + em1)
    } else {
        (1.0 - (-d * time).exp()) / d
    };
    let z = -0.5 * xi * (xi * c) * h;
    let drift = if kappa == 0.0 || model.long_run_variance() == 0.0 {
        Complex64::new(0.0, 0.0)
    } else {
        let lm1 = logrel_minus_one(z);
        let remainder = if (d * time).norm() < 0.5 && z.norm() < 0.125 {
            time * (em1 * (1.0 + lm1) + lm1)
        } else {
            let ratio = if z.norm() < 0.125 {
                1.0 + lm1
            } else {
                (1.0 + z).ln() / z
            };
            h * ratio - time
        };
        kappa * model.long_run_variance() * c * remainder
    };
    drift - 0.5 * q * h / (1.0 + z) * model.initial_variance()
}

fn complex_expm1(z: Complex64) -> Complex64 {
    Complex64::new(
        z.re.exp_m1() * z.im.cos() - 2.0 * (0.5 * z.im).sin().powi(2),
        z.re.exp() * z.im.sin(),
    )
}

struct Integrand<'a> {
    model: &'a HestonParameters,
    time: f64,
    variance: f64,
    frequency_scale: f64,
    log_moneyness: f64,
    weight: f64,
    remaining: usize,
}

impl Integrand<'_> {
    fn at(&mut self, t: f64) -> Result<f64> {
        crate::cancellation::check()?;
        if self.remaining < 2 {
            return Err(nonconvergence());
        }
        self.remaining -= 2;
        let u = self.frequency_scale * t / (1.0 - t);
        let log_heston = log_characteristic(self.model, self.time, u);
        let log_black = -0.5 * self.variance * (u * u + 0.25);
        let difference = log_heston - log_black;
        let transform = if difference.norm() < 0.5 {
            -log_black.exp() * complex_expm1(difference)
        } else {
            Complex64::new(log_black.exp(), 0.0) - log_heston.exp()
        };
        let value = self.weight
            * (Complex64::from_polar(1.0, u * self.log_moneyness) * transform).re
            / (u * u + 0.25)
            * self.frequency_scale
            / (1.0 - t).powi(2);
        if !value.is_finite() {
            return Err(computation());
        }
        Ok(value)
    }
}

struct Panel {
    left: f64,
    right: f64,
    value: f64,
    error: f64,
}

// The embedded 7/15 Gauss-Kronrod rule samples neither infinite-range endpoint.
#[expect(
    clippy::float_cmp,
    reason = "exact equality detects a floating-point interval that cannot be subdivided"
)]
fn panel(f: &mut Integrand<'_>, left: f64, right: f64) -> Result<Panel> {
    const X: [f64; 8] = [
        0.991_455_371_120_812_6,
        0.949_107_912_342_758_5,
        0.864_864_423_359_769_1,
        0.741_531_185_599_394_5,
        0.586_087_235_467_691_1,
        0.405_845_151_377_397_2,
        0.207_784_955_007_898_5,
        0.0,
    ];
    const W: [f64; 8] = [
        0.022_935_322_010_529_224,
        0.063_092_092_629_978_55,
        0.104_790_010_322_250_19,
        0.140_653_259_715_525_92,
        0.169_004_726_639_267_9,
        0.190_350_578_064_785_42,
        0.204_432_940_075_298_89,
        0.209_482_141_084_727_82,
    ];
    const G: [f64; 4] = [
        0.129_484_966_168_869_7,
        0.279_705_391_489_276_64,
        0.381_830_050_505_118_9,
        0.417_959_183_673_469_4,
    ];
    let center = left.midpoint(right);
    let half = (right - left) * 0.5;
    if center == left || center == right {
        return Err(nonconvergence());
    }
    if right == 1.0 {
        let frequency = f.frequency_scale * left / (1.0 - left);
        // Each half-moment characteristic function has modulus at most one.
        // Bound the unresolved tail instead of trusting quadrature samples to find it.
        return Ok(Panel {
            left,
            right,
            value: 0.0,
            error: 4.0 * f.weight * 0.5_f64.atan2(frequency),
        });
    }
    let middle = f.at(center)?;
    let mut kronrod = W[7] * middle;
    let mut gauss = G[3] * middle;
    let mut absolute = W[7] * middle.abs();
    let mut pairs = [(0.0, 0.0); 7];
    for i in 0..7 {
        let a = f.at(center - half * X[i])?;
        let b = f.at(center + half * X[i])?;
        pairs[i] = (a, b);
        kronrod += W[i] * (a + b);
        absolute += W[i] * (a.abs() + b.abs());
        if i % 2 == 1 {
            gauss += G[i / 2] * (a + b);
        }
    }
    let mean = 0.5 * kronrod;
    let mut asc = W[7] * (middle - mean).abs();
    for i in 0..7 {
        asc += W[i] * ((pairs[i].0 - mean).abs() + (pairs[i].1 - mean).abs());
    }
    let mut error = ((kronrod - gauss) * half).abs();
    asc *= half;
    if asc > 0.0 && error > 0.0 {
        error = asc * (200.0 * error / asc).powf(1.5).min(1.0);
    }
    error = error.max(50.0 * f64::EPSILON * absolute * half);
    Ok(Panel {
        left,
        right,
        value: kronrod * half,
        error,
    })
}

fn integrate(
    f: &mut Integrand<'_>,
    base: f64,
    settings: HestonIntegrationSettings,
    scale: f64,
) -> Result<f64> {
    let mut panels = Vec::new();
    let ends = [0.0, 0.25, 0.5, 0.75, 0.9, 0.99, 1.0];
    for pair in ends.windows(2) {
        panels.push(panel(f, pair[0], pair[1])?);
    }
    loop {
        let mut sum = Sum::default();
        let mut error = Sum::default();
        let mut worst = 0;
        for (i, p) in panels.iter().enumerate() {
            sum.add(p.value);
            error.add(p.error);
            if p.error > panels[worst].error {
                worst = i;
            }
        }
        let correction = sum.total();
        let tolerance = settings.absolute_tolerance / scale
            + settings.relative_tolerance * (base + correction).abs();
        if !correction.is_finite() || !error.total().is_finite() {
            return Err(computation());
        }
        if error.total() <= 0.25 * tolerance {
            return Ok(correction);
        }
        let old = panels.swap_remove(worst);
        let mid = old.left.midpoint(old.right);
        panels.push(panel(f, old.left, mid)?);
        panels.push(panel(f, mid, old.right)?);
    }
}
