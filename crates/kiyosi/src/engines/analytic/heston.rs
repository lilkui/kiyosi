use std::{
    cell::Cell,
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    f64::consts::PI,
};

use rustfft::num_complex::Complex64;

use super::european::lognormal_price;
use crate::instruments::monitoring::validate_life;
use crate::numerics::{Sum, checked_with_scale, scaled_cash};
use crate::{
    Error, EuropeanOption, GreeksRequest, HestonParameters, OptionType, PricingContext,
    PricingEngine, PricingResult, Result,
};

mod fft;

const MAX_BATCH_STRIKES: usize = 1 << 20;
const BATCH_WORK_LIMIT: usize = 8_000_000;
const PANEL_SAMPLES: usize = 15;
const PANEL_CACHE_CAPACITY: usize = 16_384 / PANEL_SAMPLES;
type PanelSamples = [FourierSample; PANEL_SAMPLES];
type PanelCache = HashMap<[u64; 2], PanelSamples>;

struct BatchIntegration {
    geometry: Option<PanelCache>,
    remaining: Cell<usize>,
}

fn charge_batch_work(remaining: &Cell<usize>, work: usize) -> Result<()> {
    remaining.set(remaining.get().checked_sub(work).ok_or_else(|| {
        Error::InvalidInput("analytic Heston strike batch exceeds its Fourier work budget".into())
    })?);
    Ok(())
}

/// Checks the total input count for an analytic Heston strike batch.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] above 1,048,576 strikes, including duplicates.
#[doc(hidden)]
pub fn validate_heston_strike_batch_size(count: usize) -> Result<()> {
    if count > MAX_BATCH_STRIKES {
        return Err(Error::InvalidInput(
            "analytic Heston strike batch exceeds 1,048,576 strikes".into(),
        ));
    }
    Ok(())
}

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
    /// Caps characteristic-function calculations per quote, including in strike batches.
    ///
    /// Counts both Heston and control-variate characteristic-function evaluations.
    /// Defaults to 100,000. Expiry and deterministic-variance branches consume no evaluations.
    pub max_evaluations: usize,
}

impl Default for HestonIntegrationSettings {
    fn default() -> Self {
        Self {
            absolute_tolerance: 1e-8,
            relative_tolerance: 1e-8,
            max_evaluations: 100_000,
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
/// variance control variate. The integration combines Gauss–Kronrod error
/// estimates over sampled frequencies with an analytic bound on the remaining
/// tail. It accepts when their sum is at most one quarter of the target price
/// error defined by [`HestonIntegrationSettings`].
/// Zero variance parameters and correlation endpoints are valid without the Feller condition.
/// Supports deterministic rate/dividend curves and prices only; Greeks and
/// implied-parameter solvers are unsupported. Owns its settings and supports concurrent
/// calls on shared instances, with request-local integration state.
/// See [`Self::new`] for a complete example.
///
/// # Greeks
///
/// Returns the price with requested Greeks marked unsupported; see [`Self::new`].
///
/// Use [`PricingResult::require`] to reject an unavailable Greek.
/// The analytic estimator ignores valid numerical shifts.
///
/// ## Errors
///
/// Checks Heston applicability and propagates [`Self::price`] errors.
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
    /// use kiyosi::engines::HestonIntegrationSettings;
    /// use kiyosi::{AnalyticHestonEuropeanEngine, Date, EuropeanOption, HestonParameters,
    ///     OptionType, PricingContext, PricingEngine};
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
    /// assert_eq!(engine.settings().max_evaluations, 100_000);
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

    /// Prices European calls or puts across strikes with fixed dates and market inputs.
    ///
    /// Uses the template's direction and dates. The result preserves input order
    /// and duplicates, with each distinct strike priced once. The empty batch
    /// validates the market and returns an empty vector. Groups of up to 16,384
    /// distinct strikes can share a fast Fourier transform with interpolation
    /// when the estimated error fits the price tolerances. Other
    /// quotes share Fourier samples across adaptive integration panels. The
    /// evaluation budget applies per strike. The batch also limits total Fourier work.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] above 1,048,576 strikes (including duplicates),
    /// for nonpositive or non-finite strikes, or when the batch exceeds its Fourier work budget.
    /// Propagates scalar pricing errors and cancellation without a partial result.
    /// An output reservation failure returns [`Error::ComputationFailure`].
    ///
    /// # Examples
    ///
    /// Given the option and context from [`Self::new`], price a call spread.
    ///
    /// ```
    /// # use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// # let start = Date::new(2025, 1, 1)?.start_of_day();
    /// # let option = EuropeanOption::new(OptionType::Call, 100.0, start, start.add_days(365)?)?;
    /// # let context = PricingContext::new(HestonParameters::new(HestonParameterInputs {
    /// #     risk_free_rate: 0.05.into(), dividend_yield: 0.02.into(), initial_variance: 0.04,
    /// #     long_run_variance: 0.04, mean_reversion_speed: 1.5, vol_of_vol: 0.3, correlation: -0.7,
    /// # })?, 100.0, start)?;
    /// let engine = AnalyticHestonEuropeanEngine::default();
    /// let prices = engine.price_strikes(&option, &context, &[90.0, 110.0])?;
    /// let spread = prices[0] - prices[1];
    /// assert!(spread > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price_strikes(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        validate_heston_strike_batch_size(strikes.len())?;
        let model = validate_market(option, context)?;
        crate::cancellation::check()?;
        for &strike in strikes {
            crate::cancellation::check()?;
            crate::error::finite(strike, true, "strike")?;
        }
        crate::engines::quote_distinct_strikes(strikes, |unique| {
            self.price_distinct_strikes(option, context, model, unique)
        })
    }

    fn price_distinct_strikes(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        model: &HestonParameters,
        strikes: &[f64],
    ) -> Result<Vec<f64>> {
        let mut prices = Vec::new();
        prices.try_reserve_exact(strikes.len()).map_err(|_| {
            Error::ComputationFailure("cannot reserve Heston strike batch output".into())
        })?;
        let mut batch = BatchIntegration {
            geometry: (strikes.len() > 1).then(PanelCache::new),
            remaining: Cell::new(BATCH_WORK_LIMIT),
        };
        let mut panels = BinaryHeap::new();
        for chunk in strikes.chunks(fft::MAX_STRIKES) {
            let transformed = fft::price_strikes(
                option,
                context,
                model,
                chunk,
                self.settings,
                &batch.remaining,
            )?;
            for (index, &strike) in chunk.iter().enumerate() {
                crate::cancellation::check()?;
                if let Some(price) = transformed.as_ref().and_then(|prices| prices[index]) {
                    prices.push(price);
                    continue;
                }
                prices.push(self.price_one(
                    option,
                    context,
                    model,
                    strike,
                    Some(&mut batch),
                    &mut panels,
                )?);
            }
        }
        Ok(prices)
    }

    fn price_one(
        &self,
        option: &EuropeanOption,
        context: &PricingContext,
        model: &HestonParameters,
        strike: f64,
        batch: Option<&mut BatchIntegration>,
        panels: &mut BinaryHeap<Panel>,
    ) -> Result<f64> {
        let start = context.valuation_time();
        let end = option.expiry_time();
        let spot = context.spot_price();
        let sign = if option.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        };
        let time = start.years_until(end);
        if time == 0.0 {
            return Ok((sign * (spot - strike)).max(0.0));
        }
        let absorbing = model.initial_variance() == 0.0
            && (model.mean_reversion_speed() == 0.0 || model.long_run_variance() == 0.0);
        if absorbing {
            return crate::engines::deterministic::vanilla(
                option,
                crate::engines::Payoff::vanilla(option.option_type(), strike, false),
                context,
            );
        }
        let risk = model.risk_free_curve().log_factor(start, end)?;
        let dividend = model.dividend_curve().log_factor(start, end)?;
        let scale = scaled_cash(spot, dividend).max(scaled_cash(strike, risk));
        let deterministic = model.vol_of_vol() == 0.0;
        if !deterministic && (!scale.is_finite() || scale == 0.0) {
            return Err(computation());
        }
        let variance = integrated_variance(model, time);
        let deviation = if deterministic && !variance.is_normal() {
            deterministic_deviation(model, time)
        } else {
            variance.sqrt()
        };
        if !deviation.is_finite() || deviation <= 0.0 {
            return Err(computation());
        }
        let log_moneyness = context.log_forward_moneyness(end, strike, dividend - risk);
        let base = lognormal_price(
            spot,
            strike,
            log_moneyness,
            (dividend, risk),
            deviation,
            1.0,
            sign,
        )?;
        if deterministic {
            return checked_with_scale(base, scale);
        }
        let weight = (spot.ln() + dividend - scale.ln())
            .midpoint(strike.ln() + risk - scale.ln())
            .exp()
            / PI;
        let (geometry, batch_work) = batch.map_or((None, None), |batch| {
            (batch.geometry.as_mut(), Some(&batch.remaining))
        });
        let mut integrand = Integrand {
            model,
            time,
            variance,
            // Keep the payoff transform's unit frequency scale visible when rare paths inflate variance.
            frequency_scale: (1.0 / variance.sqrt()).max(1.0),
            log_moneyness,
            weight,
            remaining: self.settings.max_evaluations,
            geometry,
            batch_work,
        };
        integrate(&mut integrand, base, self.settings, scale, panels)
    }
}

fn validate_market<'a>(
    option: &EuropeanOption,
    context: &'a PricingContext,
) -> Result<&'a HestonParameters> {
    let model = context.model_parameters().as_heston()?;
    validate_life(option, context)?;
    context.validate_curve_coverage(option.expiry_time())?;
    Ok(model)
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
    /// Prices within the permitted valuation bounds, returning intrinsic value at expiry.
    ///
    /// Validates model applicability, permitted valuation bounds and rate/dividend
    /// coverage in that order. Valuation may precede the effective time, with curves
    /// covering valuation through expiry; see [`crate::instruments::OptionContract::valuation_start`].
    /// Initial variance is at valuation. See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for other models,
    /// [`Error::InvalidInput`] after expiry or outside curve coverage,
    /// [`Error::NonConvergence`] when the request-wide budget or floating-point
    /// resolution cannot meet the estimated error target, and
    /// [`Error::ComputationFailure`] for unrepresentable intermediates or invalid
    /// results. Only rounding-sized negative prices may be clipped.
    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64> {
        let model = validate_market(option, context)?;
        self.price_one(
            option,
            context,
            model,
            option.strike(),
            None,
            &mut BinaryHeap::new(),
        )
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
        PricingResult::new(self.price(option, context)?)
            .map(|result| result.with_request(request, GreeksRequest::new([])))
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
        let sum = small_reversion_coefficient(x);
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

fn small_reversion_coefficient(x: f64) -> f64 {
    let mut term = 0.5;
    let mut sum = term;
    for n in 3..=12 {
        term *= -x / f64::from(n);
        sum += term;
    }
    sum
}

fn deterministic_deviation(model: &HestonParameters, time: f64) -> f64 {
    let rate = model.mean_reversion_speed();
    let x = rate * time;
    // Take square roots before restoring the scale of the two variance terms.
    let (initial_weight, long_run_weight) = if x < 0.01 {
        let sum = small_reversion_coefficient(x);
        (
            time * (1.0 - x * sum),
            rate.ln().midpoint(sum.ln()).exp() * time,
        )
    } else {
        let change = -(-x).exp_m1();
        (change / rate, ((1.0 - change / x) * time).sqrt())
    };
    (model.initial_variance().sqrt() * initial_weight.sqrt())
        .hypot(model.long_run_variance().sqrt() * long_run_weight)
}

// Compute the small remainders directly: subtracting one loses short-time variance.
fn exprel_minus_one(z: Complex64) -> Complex64 {
    if z.norm_sqr() < 0.25 {
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
    if z.norm_sqr() < 0.015_625 {
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
    let small_time = (d * time).norm_sqr() < 0.25;
    let (h, em1) = if small_time {
        let em1 = exprel_minus_one(-d * time);
        (time * (1.0 + em1), em1)
    } else {
        ((1.0 - (-d * time).exp()) / d, Complex64::new(0.0, 0.0))
    };
    let z = -0.5 * xi * (xi * c) * h;
    let drift = if kappa == 0.0 || model.long_run_variance() == 0.0 {
        Complex64::new(0.0, 0.0)
    } else {
        let remainder = if z.norm_sqr() < 0.015_625 {
            let lm1 = logrel_minus_one(z);
            if small_time {
                time * (em1 * (1.0 + lm1) + lm1)
            } else {
                h * (1.0 + lm1) - time
            }
        } else {
            h * ((1.0 + z).ln() / z) - time
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
    geometry: Option<&'a mut PanelCache>,
    batch_work: Option<&'a Cell<usize>>,
}

#[derive(Clone, Copy)]
struct FourierSample {
    frequency: f64,
    transform: Complex64,
    payoff_denominator: f64,
    jacobian_denominator: f64,
}

fn fourier_sample(
    model: &HestonParameters,
    time: f64,
    variance: f64,
    frequency_scale: f64,
    t: f64,
) -> FourierSample {
    let u = frequency_scale * t / (1.0 - t);
    let transform = characteristic_difference(model, time, variance, u);
    FourierSample {
        frequency: u,
        transform,
        payoff_denominator: u * u + 0.25,
        jacobian_denominator: (1.0 - t).powi(2),
    }
}

fn characteristic_difference(
    model: &HestonParameters,
    time: f64,
    variance: f64,
    u: f64,
) -> Complex64 {
    let log_heston = log_characteristic(model, time, u);
    let log_black = -0.5 * variance * (u * u + 0.25);
    let difference = log_heston - log_black;
    if difference.norm_sqr() < 0.25 {
        -log_black.exp() * complex_expm1(difference)
    } else {
        Complex64::new(log_black.exp(), 0.0) - log_heston.exp()
    }
}

fn tail_modulus_bound(f: &Integrand<'_>, frequency: f64) -> f64 {
    // Allow for roundoff in the exponent and keep the bound positive on underflow.
    let damping = |exponent: f64| {
        (-exponent * (1.0 - 64.0 * f64::EPSILON))
            .exp()
            .max(f64::MIN_POSITIVE)
    };
    let black = damping(0.5 * (f.variance.sqrt() * frequency).powi(2));
    let rho = f.model.correlation().abs();
    let initial = f.model.initial_variance();
    let s = ((1.0 - rho) * (1.0 + rho) * frequency) * frequency;
    if !s.is_normal() || initial == 0.0 {
        return 1.0 + black;
    }
    // Conditioning on the variance path and applying Cauchy-Schwarz bounds the
    // squared half-moment transform by E[exp(-s * integrated variance)].
    // The CIR Laplace transform is at most exp(-initial * loading), since its
    // mean-reversion contribution is nonpositive.
    let kappa = f.model.mean_reversion_speed();
    let gamma = kappa.hypot(f.model.vol_of_vol() * s.sqrt() * std::f64::consts::SQRT_2);
    let elapsed = gamma * f.time;
    let ratio = kappa / gamma;
    let loading = (s / gamma) * (-2.0 * (-elapsed).exp_m1())
        / (1.0 + ratio + (1.0 - ratio) * (-elapsed).exp());
    let heston = if loading.is_normal() && elapsed.is_normal() {
        damping(initial * loading * 0.5)
    } else {
        1.0
    };
    heston + black
}

struct Panel {
    left: f64,
    right: f64,
    value: f64,
    error: f64,
}

impl PartialEq for Panel {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Panel {}

impl PartialOrd for Panel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Panel {
    fn cmp(&self, other: &Self) -> Ordering {
        self.error
            .total_cmp(&other.error)
            .then_with(|| other.left.total_cmp(&self.left))
    }
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
        // Both modulus bounds decrease with frequency. Integrate the payoff
        // denominator exactly over the unresolved tail.
        return Ok(Panel {
            left,
            right,
            value: 0.0,
            error: 2.0 * f.weight * 0.5_f64.atan2(frequency) * tail_modulus_bound(f, frequency),
        });
    }
    crate::cancellation::check()?;
    f.remaining = f
        .remaining
        .checked_sub(2 * PANEL_SAMPLES)
        .ok_or_else(nonconvergence)?;
    if let Some(remaining) = f.batch_work {
        charge_batch_work(remaining, 2 * PANEL_SAMPLES)?;
    }
    let evaluate = || {
        std::array::from_fn(|i| {
            let t = if i == 0 {
                center
            } else {
                let offset = half * X[(i - 1) / 2];
                if i % 2 == 1 {
                    center - offset
                } else {
                    center + offset
                }
            };
            fourier_sample(f.model, f.time, f.variance, f.frequency_scale, t)
        })
    };
    let key = [left.to_bits(), right.to_bits()];
    let scratch;
    let samples = match f.geometry.as_mut() {
        Some(cache) if cache.len() < PANEL_CACHE_CAPACITY => {
            cache.entry(key).or_insert_with(evaluate)
        }
        cache => {
            if let Some(samples) = cache.and_then(|cache| cache.get(&key)) {
                samples
            } else {
                scratch = evaluate();
                &scratch
            }
        }
    };
    let values = samples.map(|sample| {
        let phase = sample.frequency * f.log_moneyness;
        f.weight * (Complex64::from_polar(1.0, phase) * sample.transform).re
            / sample.payoff_denominator
            * f.frequency_scale
            / sample.jacobian_denominator
    });
    if values.iter().any(|value| !value.is_finite()) {
        return Err(computation());
    }
    let middle = values[0];
    let mut kronrod = W[7] * middle;
    let mut gauss = G[3] * middle;
    let mut absolute = W[7] * middle.abs();
    let mut pairs = [(0.0, 0.0); 7];
    for i in 0..7 {
        let a = values[2 * i + 1];
        let b = values[2 * i + 2];
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
    panels: &mut BinaryHeap<Panel>,
) -> Result<f64> {
    let normalized_base = base / scale;
    panels.clear();
    let mut sum = Sum::default();
    let mut error = Sum::default();
    let ends = [0.0, 0.25, 0.5, 0.75, 0.9, 0.99, 1.0];
    for pair in ends.windows(2) {
        let panel = panel(f, pair[0], pair[1])?;
        sum.add(panel.value);
        error.add(panel.error);
        panels.push(panel);
    }
    loop {
        let correction = sum.total();
        let tolerance = settings.absolute_tolerance / scale
            + settings.relative_tolerance * (normalized_base + correction).abs();
        let estimated_error = error.total();
        if !correction.is_finite() || !estimated_error.is_finite() || estimated_error < 0.0 {
            return Err(computation());
        }
        if estimated_error <= 0.25 * tolerance {
            let price = base + scale * correction;
            match checked_with_scale(price, scale) {
                // The target can admit a negative estimate beyond roundoff near zero.
                Err(_) if price < 0.0 && -price / scale <= estimated_error => {}
                result => return result,
            }
        }
        let old = panels.pop().ok_or_else(computation)?;
        let mid = old.left.midpoint(old.right);
        sum.add(-old.value);
        error.add(-old.error);
        for (left, right) in [(old.left, mid), (mid, old.right)] {
            let panel = panel(f, left, right)?;
            sum.add(panel.value);
            error.add(panel.error);
            panels.push(panel);
        }
    }
}
