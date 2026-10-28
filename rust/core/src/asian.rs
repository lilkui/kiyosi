use crate::{
    Date, Error, ErrorCategory, EuropeanOption, GreeksRequest, NumericalShiftSettings,
    OptionContract, OptionType, PricingContext, PricingEngine, PricingResult, Result, Timestamp,
};

/// Denotes no realized average before averaging starts; see [`GeometricAveragePriceOption::new`].
pub const DEFAULT_REALIZED_AVERAGE: f64 = 0.0;

#[derive(Clone, Debug, PartialEq)]
struct AverageTerms {
    terms: EuropeanOption,
    averaging_start: Date,
    realized: f64,
}

impl AverageTerms {
    fn new(
        direction: OptionType,
        strike: f64,
        start: Date,
        averaging: Date,
        end: Date,
        realized: f64,
    ) -> Result<Self> {
        crate::error::finite(strike, ErrorCategory::InvalidStrike, true)?;
        if !realized.is_finite() || realized < 0.0 {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "realized average must be finite and non-negative",
            ));
        }
        let terms = EuropeanOption::new(direction, strike, start, end)?;
        if averaging < start || averaging > end {
            return Err(Error::new(
                ErrorCategory::InvalidSchedule,
                "averaging start must be within contract life",
            ));
        }
        Ok(Self {
            terms,
            averaging_start: averaging,
            realized,
        })
    }
    fn validate(&self, context: &PricingContext) -> Result<f64> {
        let valuation = context.valuation_time();
        let expiry = self.terms.expiry_date().start_of_day();
        if valuation < self.terms.effective_date().start_of_day() || valuation > expiry {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "valuation must be within contract life",
            ));
        }
        if (valuation > self.averaging_start.start_of_day()) == (self.realized == 0.0) {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "realized average must match the elapsed averaging period",
            ));
        }
        Ok(valuation.years_until(expiry))
    }
    fn period(&self) -> f64 {
        self.averaging_start
            .start_of_day()
            .years_until(self.terms.expiry_date().start_of_day())
    }
    fn sign(&self) -> f64 {
        if self.terms.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        }
    }
    fn adjusted_strike(&self, tau: f64) -> f64 {
        let elapsed = self.period() - tau;
        if elapsed > 0.0 {
            self.terms.strike() + elapsed / tau * (self.terms.strike() - self.realized)
        } else {
            self.terms.strike()
        }
    }
}

macro_rules! average_option {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        /// Owns validated continuous-average terms; a zero-length window fixes once at expiry.
        /// See [`Self::new`] for construction and [`Self::replace`] for history updates.
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(AverageTerms);
        impl $name {
            /// Creates an average-price contract with explicit history.
            /// Use [`DEFAULT_REALIZED_AVERAGE`] before averaging starts. Positive history
            /// is required strictly after the averaging start, checked during pricing.
            ///
            /// # Errors
            /// Returns [`ErrorCategory::InvalidStrike`] for invalid strike, then
            /// [`ErrorCategory::InvalidParameter`] for negative/non-finite history,
            /// [`ErrorCategory::InvalidTimeRange`] for reversed life dates, then
            /// [`ErrorCategory::InvalidSchedule`] for averaging outside contract life.
            ///
            /// # Examples
            /// ```
            /// use kiyosi_core::{Date, OptionType, GeometricAveragePriceOption,
            ///     ArithmeticAveragePriceOption, DEFAULT_REALIZED_AVERAGE};
            /// # fn main() -> kiyosi_core::Result<()> {
            /// let start = Date::new(2025, 1, 1)?;
            /// let end = Date::new(2026, 1, 1)?;
            /// let geometric = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, end, DEFAULT_REALIZED_AVERAGE)?;
            /// let arithmetic = ArithmeticAveragePriceOption::new(OptionType::Put,
            ///     100.0, start, start, end, DEFAULT_REALIZED_AVERAGE)?;
            /// assert_eq!(geometric.realized_average(), 0.0);
            /// assert_eq!(arithmetic.averaging_start_date(), start);
            /// # Ok(()) }
            /// ```
            pub fn new(
                option_type: OptionType,
                strike: f64,
                effective_date: Date,
                averaging_start_date: Date,
                expiry_date: Date,
                realized_average: f64,
            ) -> Result<Self> {
                Ok(Self(AverageTerms::new(
                    option_type,
                    strike,
                    effective_date,
                    averaging_start_date,
                    expiry_date,
                    realized_average,
                )?))
            }
            /// Returns an owning copy with replaced history, preserving all other terms.
            ///
            /// # Errors
            /// Reuses [`Self::new`] validation; failures leave this contract unchanged.
            /// Zero denotes no elapsed history, with time consistency checked at pricing.
            ///
            /// # Examples
            /// ```
            /// use kiyosi_core::{Date, OptionType, GeometricAveragePriceOption};
            /// # fn main() -> kiyosi_core::Result<()> {
            /// let start = Date::new(2025, 1, 1)?;
            /// let original = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, Date::new(2026, 1, 1)?, 0.0)?;
            /// let seasoned = original.replace(105.0)?;
            /// assert_eq!(seasoned.realized_average(), 105.0);
            /// assert_eq!(original.realized_average(), 0.0);
            /// # Ok(()) }
            /// ```
            pub fn replace(&self, realized_average: f64) -> Result<Self> {
                Self::new(
                    self.option_type(),
                    self.strike(),
                    self.effective_date(),
                    self.averaging_start_date(),
                    self.expiry_date(),
                    realized_average,
                )
            }
            /// Returns the direction; see [`Self::new`].
            pub fn option_type(&self) -> OptionType {
                self.0.terms.option_type()
            }
            /// Returns the positive strike; see [`Self::new`].
            pub fn strike(&self) -> f64 {
                self.0.terms.strike()
            }
            /// Returns the inclusive first date; see [`Self::new`].
            pub fn effective_date(&self) -> Date {
                self.0.terms.effective_date()
            }
            /// Returns expiry at midnight UTC; see [`Self::new`].
            pub fn expiry_date(&self) -> Date {
                self.0.terms.expiry_date()
            }
            /// Returns the averaging start; see [`Self::new`].
            pub fn averaging_start_date(&self) -> Date {
                self.0.averaging_start
            }
            /// Returns the caller-supplied history; see [`Self::replace`].
            pub fn realized_average(&self) -> f64 {
                self.0.realized
            }
        }
        impl OptionContract for $name {
            fn effective_date(&self) -> Date {
                self.effective_date()
            }
            fn expiry_date(&self) -> Date {
                self.expiry_date()
            }
            fn time_stencil_available(&self, before: Timestamp, after: Timestamp) -> bool {
                let start = self.averaging_start_date().start_of_day();
                if self.realized_average() == 0.0 {
                    after <= start
                } else {
                    before > start
                }
            }
        }
    };
}
average_option!(
    GeometricAveragePriceOption,
    "Defines a geometric-average price option."
);
average_option!(
    ArithmeticAveragePriceOption,
    "Defines an arithmetic-average price option."
);

/// Prices continuous geometric averages under BSM with deterministic rate curves.
/// Shared instances support concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalyticGeometricAveragePriceEngine;

/// Approximates continuous arithmetic averages with the existing moment matching.
/// Requires explicitly flat rates before expiry, including single-fixing contracts.
/// This is the Levy-compatible approximation, not an exact arithmetic-average price.
/// Shared instances support concurrent pricing; see [`Self::price`].
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnbullWakemanArithmeticAveragePriceEngine;

fn checked(value: f64) -> Result<f64> {
    if !value.is_finite() {
        return Err(Error::new(
            ErrorCategory::InvalidResult,
            "Asian pricing produced a non-finite result",
        ));
    }
    Ok(value.max(0.0))
}
fn coverage(option: &AverageTerms, context: &PricingContext) -> Result<()> {
    let expiry = option.terms.expiry_date().start_of_day();
    context
        .model_parameters()
        .risk_free_curve()
        .validate_horizon(context.valuation_time(), expiry)?;
    context
        .model_parameters()
        .dividend_curve()
        .validate_horizon(context.valuation_time(), expiry)
}
fn settlement(option: &AverageTerms, context: &PricingContext) -> Result<f64> {
    checked(
        option.sign()
            * (if option.realized > 0.0 {
                option.realized
            } else {
                context.spot_price()
            } - option.terms.strike()),
    )
}

impl AnalyticGeometricAveragePriceEngine {
    /// Prices geometric averaging, preserving forward starts, history and expiry settlement.
    ///
    /// # Errors
    /// Returns [`ErrorCategory::InvalidTimeRange`] outside life or curve coverage,
    /// [`ErrorCategory::InvalidParameter`] for inconsistent history, and
    /// [`ErrorCategory::InvalidResult`] for non-finite calculations. Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{AnalyticGeometricAveragePriceEngine, GeometricAveragePriceOption,
    ///     BlackScholesMertonParameters, PricingContext, Date, OptionType};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = GeometricAveragePriceOption::new(OptionType::Call, 100.0,
    ///     start, start, Date::new(2026, 1, 1)?, 0.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// assert!((AnalyticGeometricAveragePriceEngine.price(&option, &context)? - 5.035867546515384).abs() < 1e-12);
    /// # Ok(()) }
    /// ```
    pub fn price(
        &self,
        option: &GeometricAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        let o = &option.0;
        let tau = o.validate(context)?;
        if tau == 0.0 {
            return settlement(o, context);
        }
        coverage(o, context)?;
        if o.period() == 0.0 {
            return crate::AnalyticVanillaEngine.price(&o.terms, context);
        }
        let valuation = context.valuation_time();
        let start = o.averaging_start.start_of_day();
        let expiry = o.terms.expiry_date().start_of_day();
        let period = o.period();
        let lead = valuation.years_until(start).max(0.0);
        let future = valuation.max(start).years_until(expiry);
        let weight = future / period;
        let model = context.model_parameters();
        let sigma = model.volatility();
        let mut times = vec![valuation, start.max(valuation), expiry];
        for curve in [model.risk_free_curve(), model.dividend_curve()] {
            times.extend(
                curve
                    .nodes()
                    .iter()
                    .copied()
                    .filter(|t| *t > valuation && *t < expiry),
            );
        }
        times.sort_unstable();
        times.dedup();
        let mut carry = 0.0;
        for pair in times.windows(2) {
            let from = pair[0];
            let to = pair[1];
            let interval = -model.risk_free_curve().log_factor(from, to)?
                + model.dividend_curve().log_factor(from, to)?;
            let midpoint = valuation.years_until(from) + from.years_until(to) / 2.0;
            carry += interval * (tau - lead.max(midpoint)) / period;
        }
        let variance_time = lead + future / 3.0;
        let deviation = sigma * weight * variance_time.sqrt();
        let mut log_forward = carry - weight * 0.5 * sigma * sigma * (lead + future / 2.0)
            + 0.5 * deviation * deviation;
        if valuation > start {
            log_forward += (1.0 - weight) * log_ratio(o.realized, context.spot_price());
        }
        let log_discount = model.risk_free_curve().log_factor(valuation, expiry)?;
        checked(crate::pricing::lognormal_price(
            context.spot_price(),
            o.terms.strike(),
            log_forward,
            (log_forward + log_discount, log_discount),
            sigma * weight,
            variance_time.sqrt(),
            o.sign(),
        )?)
    }
}

fn log_ratio(value: f64, other: f64) -> f64 {
    let relative = (value - other) / other;
    if relative.abs() < 0.5 {
        relative.ln_1p()
    } else {
        value.ln() - other.ln()
    }
}
fn exprel(x: f64) -> f64 {
    if x == 0.0 { 1.0 } else { x.exp_m1() / x }
}
fn divided_exprel(x: f64, y: f64) -> f64 {
    if x.abs().max(y.abs()) < 0.5 {
        let (mut sum, mut homogeneous, mut power, mut factorial) = (0.5, 1.0, 1.0, 2.0);
        for n in 1..=16 {
            power *= y;
            homogeneous = x * homogeneous + power;
            factorial *= f64::from(n + 2);
            sum += homogeneous / factorial;
        }
        sum
    } else if (x - y).abs() < 1e-5 {
        let midpoint = (x + y) / 2.0;
        ((midpoint - 1.0) * midpoint.exp() + 1.0) / (midpoint * midpoint)
    } else {
        (exprel(x) - exprel(y)) / (x - y)
    }
}
fn divided_exprel_increment_ratio(x: f64, y: f64, h: f64) -> f64 {
    // Factor out h rather than subtracting nearly equal moments.
    if (x + h).abs().max(y.abs()) < 0.5 {
        let (mut sum, mut two, mut three, mut power, mut factorial) =
            (1.0 / 6.0, 1.0, 1.0, 1.0, 6.0);
        for n in 1..=16 {
            power *= y;
            two = x * two + power;
            three = (x + h) * three + two;
            factorial *= f64::from(n + 3);
            sum += three / factorial;
        }
        sum
    } else {
        let increment = (x.exp() * exprel(h) - exprel(x)) / (x + h);
        (increment - divided_exprel(x, y)) / (x + h - y)
    }
}

impl TurnbullWakemanArithmeticAveragePriceEngine {
    /// Prices the existing arithmetic approximation, settling realized history at expiry.
    /// See [`AnalyticGeometricAveragePriceEngine::price`] for the market setup.
    ///
    /// # Errors
    /// Reuses geometric life/history errors, then returns
    /// [`ErrorCategory::UnsupportedOperation`] for node curves before expiry,
    /// [`ErrorCategory::InvalidTimeRange`] for curve coverage, or
    /// [`ErrorCategory::InvalidResult`] for invalid moments or non-finite results.
    /// Inputs remain unchanged.
    ///
    /// # Examples
    /// ```
    /// use kiyosi_core::{TurnbullWakemanArithmeticAveragePriceEngine, ArithmeticAveragePriceOption,
    ///     BlackScholesMertonParameters, PricingContext, Date, OptionType, GreeksRequest, Greek};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let option = ArithmeticAveragePriceOption::new(OptionType::Call, 100.0,
    ///     start, start, Date::new(2026, 1, 1)?, 0.0)?;
    /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.04, 0.01, 0.2)?, 100.0, start.into())?;
    /// let result = TurnbullWakemanArithmeticAveragePriceEngine.price_with_greeks(
    ///     &option, &context, GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!((result.price() - 5.246501860362777).abs() < 1e-12);
    /// assert!(result.require(Greek::Delta)? > 0.0);
    /// # Ok(()) }
    /// ```
    pub fn price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        let o = &option.0;
        let tau = o.validate(context)?;
        if tau == 0.0 {
            return settlement(o, context);
        }
        let model = context.model_parameters();
        if !model.risk_free_curve().is_flat() || !model.dividend_curve().is_flat() {
            return Err(Error::new(
                ErrorCategory::UnsupportedOperation,
                "Turnbull-Wakeman requires flat rates",
            ));
        }
        coverage(o, context)?;
        if o.period() == 0.0 {
            return crate::AnalyticVanillaEngine.price(&o.terms, context);
        }
        let rate = model.risk_free_rate()?;
        let carry = rate - model.dividend_yield()?;
        let sigma = model.volatility();
        let period = o.period();
        let lead = (tau - period).max(0.0);
        let elapsed = period - tau;
        let delta = tau - lead;
        let m1 = (carry * lead).exp() * exprel(carry * delta);
        if !m1.is_finite() || m1 <= 0.0 {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "invalid arithmetic moment",
            ));
        }
        let strike = o.adjusted_strike(tau);
        let scale = if elapsed > 0.0 { tau / period } else { 1.0 };
        if elapsed > 0.0 && strike <= 0.0 {
            if o.sign() < 0.0 {
                return Ok(0.0);
            }
            let accrued = (o.realized * (elapsed / period) - o.terms.strike()).max(0.0);
            return checked(
                scaled_exp(accrued, -rate * tau)
                    + scaled_exp(context.spot_price(), m1.ln() + scale.ln() - rate * tau),
            );
        }
        let overflow = !strike.is_finite();
        let cash_strike = if overflow {
            (elapsed / period).mul_add(o.terms.strike() - o.realized, scale * o.terms.strike())
        } else {
            strike
        };
        let log_scale = if overflow { scale.ln() } else { 0.0 };
        let price_scale = if overflow { 1.0 } else { scale };
        let vol2 = sigma * sigma;
        let adjusted_vol = if vol2 * tau < 1e-4 {
            let mean = exprel(carry * delta);
            let coefficient = 2.0
                * delta
                * divided_exprel_increment_ratio(2.0 * carry * delta, carry * delta, vol2 * delta)
                / (mean * mean);
            let excess = vol2 * coefficient;
            let variance_time = lead
                + coefficient
                    * if excess == 0.0 {
                        1.0
                    } else {
                        excess.ln_1p() / excess
                    };
            let log_variance = vol2 * variance_time;
            if !log_variance.is_finite() || log_variance < -1e-12 {
                return Err(Error::new(
                    ErrorCategory::InvalidResult,
                    "invalid arithmetic variance",
                ));
            }
            sigma * (variance_time / tau).max(0.0).sqrt()
        } else {
            let m2 = 2.0
                * ((2.0 * carry + vol2) * lead).exp()
                * divided_exprel((2.0 * carry + vol2) * delta, carry * delta);
            let variance = m2.ln() - 2.0 * m1.ln();
            if !variance.is_finite() || variance < -1e-12 {
                return Err(Error::new(
                    ErrorCategory::InvalidResult,
                    "invalid arithmetic variance",
                ));
            }
            (variance.max(0.0) / tau).sqrt()
        };
        if !adjusted_vol.is_finite() {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "invalid arithmetic variance",
            ));
        }
        if adjusted_vol == 0.0 {
            if o.sign() * (log_ratio(context.spot_price(), cash_strike) + log_scale + m1.ln())
                <= 0.0
            {
                return Ok(0.0);
            }
            return checked(
                price_scale
                    * (o.sign()
                        * (scaled_exp(context.spot_price(), log_scale + m1.ln() - rate * tau)
                            - scaled_exp(cash_strike, -rate * tau)))
                    .max(0.0),
            );
        }
        checked(
            price_scale
                * crate::pricing::lognormal_price(
                    context.spot_price(),
                    cash_strike,
                    log_scale + m1.ln(),
                    (log_scale + m1.ln() - rate * tau, -rate * tau),
                    adjusted_vol,
                    tau.sqrt(),
                    o.sign(),
                )?,
        )
    }
}
fn scaled_exp(amount: f64, exponent: f64) -> f64 {
    let factor = exponent.exp();
    let value = amount * factor;
    if factor.is_normal() && value.is_finite() {
        value
    } else {
        (amount.ln() + exponent).exp()
    }
}

macro_rules! asian_analytics {
    ($engine:ident, $option:ident) => {
        impl $engine {
            /// Returns requested Greeks using shared scalar finite differences.
            /// Time Greeks are unavailable when a stencil crosses the history boundary;
            /// all Greeks are unavailable at expiry. See [`Self::price`] for a setup example.
            ///
            /// # Errors
            /// Returns [`ErrorCategory::InvalidParameter`] for empty requests or invalid shifts
            /// before pricing, [`ErrorCategory::InvalidResult`] for non-finite derivatives,
            /// and propagates pricing and feasible bumped-pricing errors.
            /// Inputs remain unchanged; unavailable stencils leave measures absent.
            pub fn price_with_greeks(
                &self,
                option: &$option,
                context: &PricingContext,
                request: GreeksRequest,
                settings: NumericalShiftSettings,
            ) -> Result<PricingResult> {
                crate::numerical_greeks(self, option, context, request, settings)
            }
        }
    };
}
asian_analytics!(
    AnalyticGeometricAveragePriceEngine,
    GeometricAveragePriceOption
);
asian_analytics!(
    TurnbullWakemanArithmeticAveragePriceEngine,
    ArithmeticAveragePriceOption
);
impl PricingEngine<GeometricAveragePriceOption> for AnalyticGeometricAveragePriceEngine {
    fn price(&self, option: &GeometricAveragePriceOption, context: &PricingContext) -> Result<f64> {
        Self::price(self, option, context)
    }
}
impl PricingEngine<ArithmeticAveragePriceOption> for TurnbullWakemanArithmeticAveragePriceEngine {
    fn price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> Result<f64> {
        Self::price(self, option, context)
    }
    fn volatility_affects_price(
        &self,
        option: &ArithmeticAveragePriceOption,
        context: &PricingContext,
    ) -> bool {
        let tau = context
            .valuation_time()
            .years_until(option.expiry_date().start_of_day());
        option.0.adjusted_strike(tau) > 0.0
    }
}
