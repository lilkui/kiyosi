use crate::numerics::checked;
use crate::{Error, EuropeanOption, OptionContract, OptionType, PricingContext, Result, Timestamp};

/// Denotes no realized average before averaging starts; see [`GeometricAveragePriceOption::new`].
pub const DEFAULT_REALIZED_AVERAGE: Option<f64> = None;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AverageTerms {
    history_time: Option<Timestamp>,
    pub(crate) terms: EuropeanOption,
    pub(crate) averaging_start: Timestamp,
    pub(crate) realized: Option<f64>,
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
            history_time: None,
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
        if (valuation > self.averaging_start) != self.realized.is_some() {
            return Err(Error::InvalidInput(
                "realized average must match the elapsed averaging period".into(),
            ));
        }
        if self.realized.is_some() {
            let time = self.history_time.ok_or_else(|| {
                Error::InvalidInput("supplied history requires history_time".into())
            })?;
            if time != context.history_anchor.unwrap_or(valuation)
                || time < self.averaging_start
                || time > expiry
            {
                return Err(Error::InvalidInput(
                    "history_time must match the elapsed averaging period at valuation".into(),
                ));
            }
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
    ) -> Result<f64> {
        let model = context.bsm_parameters()?;
        let valuation = context.valuation_time();
        let expiry = self.terms.expiry_time();
        let carry = |time| -> Result<f64> {
            Ok(model.dividend_curve().log_factor(valuation, time)?
                - model.risk_free_curve().log_factor(valuation, time)?)
        };
        if self.period() == 0.0 {
            return checked(crate::numerics::scaled_cash(
                context.spot_price(),
                carry(expiry)?,
            ));
        }
        let start = self.averaging_start.max(valuation);
        let mut times = vec![start, expiry];
        for curve in [model.risk_free_curve(), model.dividend_curve()] {
            times.extend(
                curve
                    .nodes()
                    .iter()
                    .copied()
                    .filter(|&time| time > start && time < expiry),
            );
        }
        times.sort_unstable();
        times.dedup();
        let prior_weight = (1.0 - start.years_until(expiry) / self.period()).max(0.0);
        let mut total = crate::numerics::Sum::default();
        for pair in times.windows(2) {
            let a = carry(pair[0])?;
            let b = carry(pair[1])?;
            let weight = pair[0].years_until(pair[1]) / self.period();
            if geometric {
                total.add(a.midpoint(b) * weight);
            } else {
                let delta = b - a;
                let growth = if delta == 0.0 {
                    0.0
                } else if delta > 0.0 {
                    delta + (-(-delta).exp_m1() / delta).ln()
                } else {
                    (delta.exp_m1() / delta).ln()
                };
                total.add(crate::numerics::scaled_cash(context.spot_price(), a + growth) * weight);
            }
        }
        if geometric {
            if prior_weight > 0.0 {
                total.add(
                    prior_weight
                        * crate::numerics::log_ratio(
                            self.realized.unwrap_or(0.0),
                            context.spot_price(),
                        ),
                );
            }
            checked(crate::numerics::scaled_cash(
                context.spot_price(),
                total.total(),
            ))
        } else {
            total.add(self.realized.unwrap_or(0.0) * prior_weight);
            checked(total.total())
        }
    }

    pub(crate) fn adjusted_strike(&self, tau: f64) -> f64 {
        let elapsed = self.period() - tau;
        if elapsed > 0.0 {
            self.terms.strike()
                + elapsed / tau * (self.terms.strike() - self.realized.unwrap_or(0.0))
        } else {
            self.terms.strike()
        }
    }
}

macro_rules! average_option {
    ($name:ident, $description:literal, $geometric:literal) => {
        #[doc = $description]
        ///
        /// Owns validated continuously monitored averaging terms; a zero-length window fixes once at expiry.
        ///
        /// See [`Self::new`] for construction and [`Self::replace`] for dated history updates.
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub(crate) AverageTerms);
        impl $name {
            /// Returns a validated replacement, preserving omitted terms and the original.
            ///
            /// Valuation-relative history checks occur at pricing time.
            /// See [`GeometricAveragePriceOption::new`] for construction examples.
            ///
            /// # Errors
            ///
            /// Returns [`Error::InvalidInput`] when historical observation terms change
            /// with known history unless the history is explicitly replaced or cleared.
            /// See the [crate-level history contract](crate).
            ///
            /// Reuses constructor validation for the resulting contract. Failure leaves
            /// this value unchanged.
            ///
            /// # Examples
            ///
            /// Refreshes an observed average and its timestamp together for a valuation roll:
            ///
            /// ```rust
            /// use kiyosi::{AveragePriceOptionChanges, Date, GeometricAveragePriceOption,
            ///     OptionContract, OptionType};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 2)?.start_of_day();
            /// let at = start.add_days(4)?;
            /// let original = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, start.add_days(10)?, Some(100.0))?
            ///     .with_history_time(Some(at))?;
            /// let refreshed = original.replace(AveragePriceOptionChanges {
            ///     realized_average: Some(Some(104.0)),
            ///     history_time: Some(Some(at.add_days(1)?)), ..Default::default()
            /// })?;
            /// assert_eq!(refreshed.history_time(), Some(at.add_days(1)?));
            /// assert_eq!(refreshed.realized_average(), Some(104.0));
            /// assert_eq!(original.history_time(), Some(at));
            /// # Ok(()) }
            /// ```
            pub fn replace(&self, changes: AveragePriceOptionChanges) -> Result<Self> {
                let history_supplied = changes.realized_average.is_some();
                let updated = Self::new(
                    changes.option_type.unwrap_or(self.option_type()),
                    changes.strike.unwrap_or(self.strike()),
                    changes.effective_time.unwrap_or(self.effective_time()),
                    changes
                        .averaging_start_time
                        .unwrap_or(self.averaging_start_time()),
                    changes.expiry_time.unwrap_or(self.expiry_time()),
                    changes.realized_average.unwrap_or(self.realized_average()),
                )?;
                super::validate_history_replacement(
                    self.realized_average().is_some(),
                    history_supplied,
                    self.averaging_start_time() != updated.averaging_start_time(),
                    "realized_average",
                )?;
                let history_time = super::replacement_history_time(
                    self.history_time(),
                    changes.history_time,
                    history_supplied && updated.realized_average().is_none(),
                );
                updated.with_history_time(history_time)
            }
            /// Creates an average-price contract with explicit history.
            ///
            /// Use [`DEFAULT_REALIZED_AVERAGE`] before averaging starts. Positive history
            /// is required strictly after the averaging start, checked during pricing.
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
            ///     ArithmeticAveragePriceOption, DEFAULT_REALIZED_AVERAGE};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 1)?.start_of_day();
            /// let end = Date::new(2026, 1, 1)?.start_of_day();
            /// let geometric = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, end, DEFAULT_REALIZED_AVERAGE)?;
            /// let arithmetic = ArithmeticAveragePriceOption::new(OptionType::Put,
            ///     100.0, start, start, end, DEFAULT_REALIZED_AVERAGE)?;
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
            /// Returns a copy with a replaced average and matching history timestamp.
            ///
            /// Use [`Self::replace`] with [`AveragePriceOptionChanges::history_time`] and
            /// [`AveragePriceOptionChanges::realized_average`] to refresh a dated snapshot.
            ///
            /// # Errors
            ///
            /// Reuses [`Self::new`] validation; failures leave this contract unchanged.
            /// `None` restores unknown history; time consistency is checked at pricing.
            ///
            /// # Examples
            ///
            /// ```rust
            /// use kiyosi::{Date, OptionType, OptionContract, GeometricAveragePriceOption,
            ///     AnalyticGeometricAveragePriceEngine, BlackScholesMertonParameters, PricingContext};
            /// # fn main() -> kiyosi::Result<()> {
            /// let start = Date::new(2025, 1, 1)?.start_of_day();
            /// let original = GeometricAveragePriceOption::new(OptionType::Call,
            ///     100.0, start, start, Date::new(2026, 1, 1)?.start_of_day(), None)?;
            /// let at = start.add_days(30)?;
            /// let seasoned = original.replace_history(105.0)?.with_history_time(Some(at))?;
            /// assert_eq!(seasoned.realized_average(), Some(105.0));
            /// assert_eq!(original.realized_average(), None);
            /// let context = PricingContext::new(BlackScholesMertonParameters::new(0.0, 0.0, 0.0)?, 105.0, at)?;
            /// assert!((AnalyticGeometricAveragePriceEngine.price(&seasoned, &context)? - 5.0).abs() < 1e-12);
            /// # Ok(()) }
            /// ```
            ///
            /// Clearing the final history payload also clears its timestamp.
            pub fn replace_history(
                &self,
                realized_average: impl Into<Option<f64>>,
            ) -> Result<Self> {
                self.replace(AveragePriceOptionChanges {
                    realized_average: Some(realized_average.into()),
                    ..Default::default()
                })
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
            /// Returns the caller-supplied history, or `None` if unknown; see [`Self::replace_history`].
            pub fn realized_average(&self) -> Option<f64> {
                self.0.realized
            }
        }
        impl OptionContract for $name {
            fn history_time(&self) -> Option<Timestamp> {
                self.0.history_time
            }
            fn with_history_time(mut self, time: Option<Timestamp>) -> Result<Self> {
                crate::instruments::validate_history_time_bounds(&self, time)?;
                self.0.history_time = time;
                Ok(self)
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
                {
                    let average = |spot| -> Result<f64> {
                        let bumped = context.replace(crate::PricingContextChanges {
                            spot_price: Some(spot),
                            ..Default::default()
                        })?;
                        self.0.deterministic_average(&bumped, $geometric)
                    };
                    return average(context.spot_price() - radius)
                        .and_then(|low| {
                            average(context.spot_price() + radius).map(|high| (low, high))
                        })
                        .is_ok_and(|(low, high)| low <= self.strike() && self.strike() <= high);
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

/// Selects replacements for [`GeometricAveragePriceOption::replace`]; absent fields retain their values.
///
/// For history, `Some(None)` restores unknown history.
///
/// For nullable fields, `Some(None)` clears the value.
#[derive(Clone, Copy, Debug, Default)]
pub struct AveragePriceOptionChanges {
    /// Replaces or clears the history timestamp; see [`OptionContract::with_history_time`].
    pub history_time: Option<Option<Timestamp>>,
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
