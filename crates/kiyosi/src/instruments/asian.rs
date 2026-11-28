use crate::numerics::{
    Sum, checked, discounted_product, finite_price, log_add, log_difference, log_mean_exponential,
    log_ratio, scaled_cash,
};
use crate::{
    Error, EuropeanOption, OptionType, PricingContext, Result, Timestamp,
    instruments::OptionContract,
};

/// Denotes no realized average before averaging starts; see [`GeometricAveragePriceOption::new`].
pub const DEFAULT_REALIZED_AVERAGE: Option<f64> = None;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AverageTerms {
    pub(crate) terms: EuropeanOption,
    pub(crate) averaging_start: Timestamp,
    pub(crate) realized: Option<f64>,
}

#[derive(Clone, Copy)]
pub(crate) enum DeterministicAverage {
    Cash(f64),
    Log(f64),
}

impl DeterministicAverage {
    fn distance(self, strike: f64) -> f64 {
        match self {
            Self::Cash(average) => average - strike,
            Self::Log(log_average) => log_average - strike.ln(),
        }
    }

    pub(crate) fn price(self, strike: f64, sign: f64, log_discount: f64) -> Result<f64> {
        let price = match self {
            Self::Cash(average) => {
                let payoff = (sign * (average - strike)).max(0.0);
                if log_discount == 0.0 {
                    payoff
                } else {
                    scaled_cash(payoff, log_discount)
                }
            }
            Self::Log(log_average) => {
                let log_strike = strike.ln();
                let (received, paid) = if sign > 0.0 {
                    (log_average, log_strike)
                } else {
                    (log_strike, log_average)
                };
                if received <= paid {
                    0.0
                } else {
                    (log_difference(received, paid) + log_discount).exp()
                }
            }
        };
        checked(price)
    }
}

impl AverageTerms {
    pub(crate) fn coverage(&self, context: &PricingContext) -> Result<()> {
        let expiry = self.terms.expiry_time();
        context
            .bsm_parameters()?
            .risk_free_curve()
            .validate_horizon(context.valuation_time(), expiry)?;
        context
            .bsm_parameters()?
            .dividend_curve()
            .validate_horizon(context.valuation_time(), expiry)
    }
    pub(crate) fn settlement(&self, context: &PricingContext) -> Result<f64> {
        checked(
            (self.sign() * (self.realized.unwrap_or(context.spot_price()) - self.terms.strike()))
                .max(0.0),
        )
    }
    fn new(
        direction: OptionType,
        strike: f64,
        start: Timestamp,
        averaging: Timestamp,
        end: Timestamp,
        realized: Option<f64>,
    ) -> Result<Self> {
        if realized.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(Error::InvalidInput(
                "realized average must be finite and positive".into(),
            ));
        }
        let terms = EuropeanOption::new(direction, strike, start, end)?;
        if averaging < start || averaging > end {
            return Err(Error::InvalidInput(
                "averaging start must be within contract life".into(),
            ));
        }
        Ok(Self {
            terms,
            averaging_start: averaging,
            realized,
        })
    }
    pub(crate) fn validate(&self, context: &PricingContext) -> Result<f64> {
        let valuation = context.valuation_time();
        let expiry = self.terms.expiry_time();
        if valuation < self.terms.effective_time() || valuation > expiry {
            return Err(Error::InvalidInput(
                "valuation must be within contract life".into(),
            ));
        }
        if valuation > self.averaging_start && self.realized.is_none() {
            return Err(Error::MissingHistory(
                "prior realized average is required".into(),
            ));
        }
        if valuation <= self.averaging_start && self.realized.is_some() {
            return Err(Error::InvalidInput(
                "realized average must match the elapsed averaging period".into(),
            ));
        }
        Ok(valuation.years_until(expiry))
    }
    pub(crate) fn period(&self) -> f64 {
        self.averaging_start.years_until(self.terms.expiry_time())
    }
    pub(crate) fn sign(&self) -> f64 {
        if self.terms.option_type() == OptionType::Call {
            1.0
        } else {
            -1.0
        }
    }
    pub(crate) fn deterministic_average(
        &self,
        context: &PricingContext,
        geometric: bool,
    ) -> Result<DeterministicAverage> {
        let model = context.bsm_parameters()?;
        let valuation = context.valuation_time();
        let expiry = self.terms.expiry_time();
        let carry = |from, to| -> Result<f64> {
            Ok(model.dividend_curve().log_factor(from, to)?
                - model.risk_free_curve().log_factor(from, to)?)
        };
        let start = self.averaging_start.max(valuation);
        let mut times = vec![start, expiry];
        for curve in [model.risk_free_curve(), model.dividend_curve()] {
            let nodes = curve.nodes_after(start);
            for chunk in nodes[..nodes.partition_point(|&time| time < expiry)].chunks(1024) {
                crate::cancellation::check()?;
                times.extend_from_slice(chunk);
            }
        }
        times.sort_unstable();
        times.dedup();
        let prior_weight = self.averaging_start.years_until(start) / self.period();
        let spot = context.spot_price();
        let mut total = Sum::default();
        let mut log_total = None;
        let mut add_cash = |amount: f64, weight: f64, growth: f64| {
            if let Some(log) = &mut log_total {
                *log = log_add(*log, amount.ln() + weight.ln() + growth);
            } else {
                let term = if growth == 0.0 {
                    amount * weight
                } else {
                    discounted_product(amount, weight, growth)
                };
                let subtotal = total.total();
                if term > 0.0 && (subtotal + term).is_finite() {
                    total.add(term);
                } else {
                    // Keep the average in log units when cash units lose its scale.
                    log_total = Some(log_add(subtotal.ln(), amount.ln() + weight.ln() + growth));
                }
            }
        };
        let mut log_growth = Sum::default();
        let mut cumulative = Sum::default();
        cumulative.add(carry(valuation, start)?);
        for pair in times.windows(2) {
            crate::cancellation::check()?;
            let a = cumulative.total();
            let delta = carry(pair[0], pair[1])?;
            cumulative.add(delta);
            let weight = pair[0].years_until(pair[1]) / self.period();
            if geometric {
                log_growth.add(a.midpoint(cumulative.total()) * weight);
            } else {
                add_cash(spot, weight, a + log_mean_exponential(delta));
            }
        }
        if geometric {
            if prior_weight > 0.0 {
                log_growth.add(prior_weight * log_ratio(self.realized.unwrap_or(0.0), spot));
            }
            let growth = log_growth.total();
            let average = if growth == 0.0 {
                spot
            } else {
                scaled_cash(spot, growth)
            };
            if average.is_finite() && average > 0.0 {
                Ok(DeterministicAverage::Cash(average))
            } else {
                finite_price(spot.ln() + growth).map(DeterministicAverage::Log)
            }
        } else {
            if prior_weight > 0.0 {
                add_cash(self.realized.unwrap_or(0.0), prior_weight, 0.0);
            }
            match log_total {
                Some(log) => finite_price(log).map(DeterministicAverage::Log),
                None => checked(total.total()).map(DeterministicAverage::Cash),
            }
        }
    }

    pub(crate) fn adjusted_strike(&self, valuation: Timestamp) -> f64 {
        self.adjusted_strike_for(valuation, self.terms.strike())
    }

    pub(crate) fn adjusted_strike_for(&self, valuation: Timestamp, strike: f64) -> f64 {
        let elapsed = self.averaging_start.years_until(valuation);
        if elapsed > 0.0 {
            let tau = valuation.years_until(self.terms.expiry_time());
            strike + elapsed / tau * (strike - self.realized.unwrap_or(0.0))
        } else {
            strike
        }
    }
}

macro_rules! average_option {
    ($name:ident, $description:literal, $geometric:literal) => {
        #[doc = $description]
        ///
        /// Owns validated continuously monitored averaging terms; a zero-length window fixes once at expiry.
        ///
        /// See [`Self::new`] for construction and [`Self::replace`] for history updates.
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub(crate) AverageTerms);
        impl $name {
            /// Returns a validated copy with the supplied fields applied.
            ///
            /// The supplied average applies to the elapsed averaging interval at valuation.
            /// Valuation-relative checks occur during pricing; changing terms retains history.
            ///
            /// # Errors
            ///
            /// See [`Self::new`] for validation rules and construction examples.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi::{AveragePriceOptionChanges, Date, GeometricAveragePriceOption,
            ///     OptionType};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 2)?.start_of_day();
            /// let original = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, start.add_days(10)?, None)?;
            /// let seasoned = original.replace(AveragePriceOptionChanges {
            ///     realized_average: Some(Some(104.0)), ..Default::default()
            /// })?;
            /// assert_eq!(seasoned.realized_average(), Some(104.0));
            /// assert_eq!(original.realized_average(), None);
            /// # Ok(()) }
            /// ```
            pub fn replace(&self, changes: AveragePriceOptionChanges) -> Result<Self> {
                Self::new(
                    changes.option_type.unwrap_or(self.option_type()),
                    changes.strike.unwrap_or(self.strike()),
                    changes.effective_time.unwrap_or(self.effective_time()),
                    changes
                        .averaging_start_time
                        .unwrap_or(self.averaging_start_time()),
                    changes.expiry_time.unwrap_or(self.expiry_time()),
                    changes.realized_average.unwrap_or(self.realized_average()),
                )
            }
            /// Creates an average-price contract.
            ///
            /// Use `None` when no realized average is known. Pricing requires a
            /// positive realized average strictly after the averaging start.
            /// History covers averaging strictly before the requested valuation time.
            /// The caller supplies the applicable average; no observations are reconstructed.
            ///
            /// # Errors
            ///
            /// Returns [`Error::InvalidInput`] for invalid strike, nonpositive or non-finite
            /// supplied history, reversed life times, or averaging outside contract life.
            /// Diagnostic order is unspecified when several inputs are invalid.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi::{Date, OptionType, GeometricAveragePriceOption,
            ///     ArithmeticAveragePriceOption};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 1)?.start_of_day();
            /// let end = Date::new(2026, 1, 1)?.start_of_day();
            /// let geometric = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, end, None)?;
            /// let arithmetic = ArithmeticAveragePriceOption::new(OptionType::Put,
            ///     100.0, start, start, end, None)?;
            /// assert_eq!(geometric.realized_average(), None);
            /// assert_eq!(arithmetic.averaging_start_time(), start);
            /// # Ok(()) }
            /// ```
            pub fn new(
                option_type: OptionType,
                strike: f64,
                effective_time: Timestamp,
                averaging_start_time: Timestamp,
                expiry_time: Timestamp,
                realized_average: impl Into<Option<f64>>,
            ) -> Result<Self> {
                Ok(Self(AverageTerms::new(
                    option_type,
                    strike,
                    effective_time,
                    averaging_start_time,
                    expiry_time,
                    realized_average.into(),
                )?))
            }
            /// Returns the direction; see [`Self::new`].
            pub fn option_type(&self) -> OptionType {
                self.0.terms.option_type()
            }
            /// Returns the positive strike; see [`Self::new`].
            pub fn strike(&self) -> f64 {
                self.0.terms.strike()
            }
            /// Returns the inclusive effective time; see [`Self::new`].
            pub fn effective_time(&self) -> Timestamp {
                self.0.terms.effective_time()
            }
            /// Returns the inclusive expiry timestamp; see [`Self::new`].
            pub fn expiry_time(&self) -> Timestamp {
                self.0.terms.expiry_time()
            }
            /// Returns the averaging start; see [`Self::new`].
            pub fn averaging_start_time(&self) -> Timestamp {
                self.0.averaging_start
            }
            /// Returns the caller-supplied history, or `None` if unknown; see [`Self::replace`].
            pub fn realized_average(&self) -> Option<f64> {
                self.0.realized
            }
        }
        impl OptionContract for $name {
            fn volatility_affects_price(&self, context: &PricingContext) -> bool {
                $geometric || self.0.adjusted_strike(context.valuation_time()) > 0.0
            }
            fn effective_time(&self) -> Timestamp {
                self.effective_time()
            }
            fn expiry_time(&self) -> Timestamp {
                self.expiry_time()
            }
            fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
                if context
                    .model_parameters()
                    .as_bsm()
                    .is_ok_and(|model| model.volatility() == 0.0)
                    && context.valuation_time() < self.expiry_time()
                    && self.0.period() > 0.0
                {
                    let distance = |spot| -> Result<f64> {
                        let bumped = context.replace(crate::PricingContextChanges {
                            spot_price: Some(spot),
                            ..Default::default()
                        })?;
                        Ok(self
                            .0
                            .deterministic_average(&bumped, $geometric)?
                            .distance(self.strike()))
                    };
                    return distance(context.spot_price() - radius)
                        .and_then(|low| {
                            distance(context.spot_price() + radius).map(|high| (low, high))
                        })
                        .is_ok_and(|(low, high)| low <= 0.0 && high >= 0.0);
                }
                self.realized_average().is_none()
                    && self.0.terms.at_spot_discontinuity(context, radius)
            }
            fn time_stencil_available(
                &self,
                _context: &PricingContext,
                before: Timestamp,
                after: Timestamp,
            ) -> bool {
                let start = self.averaging_start_time();
                if self.realized_average().is_none() {
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
    "Defines a geometric average-price Asian option.",
    true
);
average_option!(
    ArithmeticAveragePriceOption,
    "Defines an arithmetic average-price Asian option.",
    false
);

/// Selects replacements for continuous average-price Asian options.
///
/// Absent fields default to the source values. `Some(None)` clears realized history to
/// unknown. See [`GeometricAveragePriceOption::replace`] for an example.
#[derive(Clone, Copy, Debug, Default)]
pub struct AveragePriceOptionChanges {
    /// Replaces [`option_type`](GeometricAveragePriceOption::option_type).
    pub option_type: Option<OptionType>,
    /// Replaces [`strike`](GeometricAveragePriceOption::strike).
    pub strike: Option<f64>,
    /// Replaces [`effective_time`](GeometricAveragePriceOption::effective_time).
    pub effective_time: Option<Timestamp>,
    /// Replaces [`averaging_start_time`](GeometricAveragePriceOption::averaging_start_time).
    pub averaging_start_time: Option<Timestamp>,
    /// Replaces [`expiry_time`](GeometricAveragePriceOption::expiry_time).
    pub expiry_time: Option<Timestamp>,
    /// Replaces [`realized_average`](GeometricAveragePriceOption::realized_average).
    pub realized_average: Option<Option<f64>>,
}
