use crate::{Error, Rate, RateTermStructure, Result};

/// Selects Heston replacements; absent fields retain their values.
///
/// See [`HestonParameters::replace`] for an example.
#[derive(Clone, Debug, Default)]
pub struct HestonParameterChanges {
    /// Replaces the risk-free rate or curve.
    pub risk_free_rate: Option<Rate>,
    /// Replaces the dividend yield or curve.
    pub dividend_yield: Option<Rate>,
    /// Replaces the instantaneous variance at valuation.
    pub initial_variance: Option<f64>,
    /// Replaces the long-run variance.
    pub long_run_variance: Option<f64>,
    /// Replaces the mean-reversion speed per year.
    pub mean_reversion_speed: Option<f64>,
    /// Replaces the volatility-of-volatility parameter.
    pub vol_of_vol: Option<f64>,
    /// Replaces the correlation between the asset and variance Brownian motions.
    pub correlation: Option<f64>,
}

/// Supplies named inputs validated by [`HestonParameters::new`].
///
/// No model parameter has an implicit default. Rates use annual decimal units;
/// variance and mean-reversion inputs retain the units described by [`HestonParameters`].
/// See [`HestonParameters::new`] for a construction example.
#[derive(Clone, Debug)]
pub struct HestonParameterInputs {
    /// Supplies the risk-free rate or curve.
    pub risk_free_rate: Rate,
    /// Supplies the dividend yield or curve.
    pub dividend_yield: Rate,
    /// Sets instantaneous variance at valuation.
    pub initial_variance: f64,
    /// Sets the long-run variance level.
    pub long_run_variance: f64,
    /// Sets the mean-reversion speed per year.
    pub mean_reversion_speed: f64,
    /// Sets the volatility of volatility.
    pub vol_of_vol: f64,
    /// Sets asset/variance correlation, including endpoints of `[-1, 1]`.
    pub correlation: f64,
}

/// Owns constant Heston stochastic-volatility parameters under the risk-neutral measure.
///
/// Risk-free rates and dividend yields may follow deterministic term structures.
/// With time in years, `dv = kappa * (theta - v) dt + xi * sqrt(v) dW`.
/// Initial variance is measured at valuation; 20% initial volatility means 0.04
/// variance. Zero variance parameters and correlation endpoints are accepted;
/// the Feller condition, `2 * kappa * theta >= xi * xi`, is not required.
/// Here `v` is instantaneous variance and `xi` is the volatility-of-volatility
/// parameter. See [`Self::new`] for an example.
#[derive(Clone, Debug, PartialEq)]
pub struct HestonParameters {
    risk_free_curve: RateTermStructure,
    dividend_curve: RateTermStructure,
    initial_variance: f64,
    long_run_variance: f64,
    mean_reversion_speed: f64,
    vol_of_vol: f64,
    correlation: f64,
}

impl HestonParameters {
    /// Creates a model from rates or curves and explicitly supplied variance parameters.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for invalid rates, then for non-finite or
    /// negative variance parameters, then for correlation outside `[-1, 1]` or
    /// non-finite correlation. Validation follows the field order in [`HestonParameterInputs`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::HestonParameters;
    /// # fn main() -> kiyosi::Result<()> {
    /// let model = HestonParameters::new(kiyosi::HestonParameterInputs {
    ///     risk_free_rate: 0.05.into(), dividend_yield: 0.02.into(),
    ///     initial_variance: 0.04, long_run_variance: 0.04,
    ///     mean_reversion_speed: 1.5, vol_of_vol: 0.3, correlation: -0.7,
    /// })?;
    /// assert_eq!(model.initial_variance(), 0.04);
    /// assert_eq!(model.flat_risk_free_rate()?, 0.05);
    /// # Ok(()) }
    /// ```
    pub fn new(inputs: HestonParameterInputs) -> Result<Self> {
        let HestonParameterInputs {
            risk_free_rate,
            dividend_yield,
            initial_variance,
            long_run_variance,
            mean_reversion_speed,
            vol_of_vol,
            correlation,
        } = inputs;
        let risk_free_curve = risk_free_rate.into_curve("risk_free_rate")?;
        let dividend_curve = dividend_yield.into_curve("dividend_yield")?;
        for (name, value) in [
            ("initial_variance", initial_variance),
            ("long_run_variance", long_run_variance),
            ("mean_reversion_speed", mean_reversion_speed),
            ("vol_of_vol", vol_of_vol),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(Error::InvalidInput(
                    format!("{name} must be finite and nonnegative").into(),
                ));
            }
        }
        if !correlation.is_finite() || !(-1.0..=1.0).contains(&correlation) {
            return Err(Error::InvalidInput(
                "correlation must be finite and in [-1, 1]".into(),
            ));
        }
        Ok(Self {
            risk_free_curve,
            dividend_curve,
            initial_variance,
            long_run_variance,
            mean_reversion_speed,
            vol_of_vol,
            correlation,
        })
    }

    /// Returns a validated replacement, leaving this model unchanged on success or failure.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation for the resulting model.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{HestonParameterChanges, HestonParameters};
    /// # fn main() -> kiyosi::Result<()> {
    /// let model = HestonParameters::new(kiyosi::HestonParameterInputs {
    ///     risk_free_rate: 0.05.into(), dividend_yield: 0.02.into(),
    ///     initial_variance: 0.04, long_run_variance: 0.04,
    ///     mean_reversion_speed: 1.5, vol_of_vol: 0.3, correlation: -0.7,
    /// })?;
    /// let changed = model.replace(HestonParameterChanges {
    ///     initial_variance: Some(0.09), ..Default::default()
    /// })?;
    /// assert_eq!(changed.initial_variance(), 0.09);
    /// assert_eq!(model.initial_variance(), 0.04);
    /// # Ok(()) }
    /// ```
    pub fn replace(&self, changes: HestonParameterChanges) -> Result<Self> {
        Self::new(HestonParameterInputs {
            risk_free_rate: changes
                .risk_free_rate
                .unwrap_or_else(|| self.risk_free_curve.clone().into()),
            dividend_yield: changes
                .dividend_yield
                .unwrap_or_else(|| self.dividend_curve.clone().into()),
            initial_variance: changes.initial_variance.unwrap_or(self.initial_variance),
            long_run_variance: changes.long_run_variance.unwrap_or(self.long_run_variance),
            mean_reversion_speed: changes
                .mean_reversion_speed
                .unwrap_or(self.mean_reversion_speed),
            vol_of_vol: changes.vol_of_vol.unwrap_or(self.vol_of_vol),
            correlation: changes.correlation.unwrap_or(self.correlation),
        })
    }

    /// Borrows the risk-free curve for this model's lifetime; see [`Self::new`].
    pub fn risk_free_curve(&self) -> &RateTermStructure {
        &self.risk_free_curve
    }
    /// Borrows the dividend curve for this model's lifetime; see [`Self::new`].
    pub fn dividend_curve(&self) -> &RateTermStructure {
        &self.dividend_curve
    }
    /// Returns the constant risk-free rate; see [`Self::new`].
    ///
    /// This scalar omits coverage bounds; use [`Self::risk_free_curve`] when
    /// reconstructing a model.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves.
    pub fn flat_risk_free_rate(&self) -> Result<f64> {
        self.risk_free_curve
            .exact_flat_rate()
            .ok_or_else(|| Error::UnsupportedOperation("risk-free curve is not exactly constant; use an explicitly flat curve (bounded_flat preserves coverage)".into()))
    }
    /// Returns the constant dividend yield; see [`Self::new`].
    ///
    /// This scalar omits coverage bounds; use [`Self::dividend_curve`] when
    /// reconstructing a model.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant rate curves.
    pub fn flat_dividend_yield(&self) -> Result<f64> {
        self.dividend_curve
            .exact_flat_rate()
            .ok_or_else(|| Error::UnsupportedOperation("dividend curve is not exactly constant; use an explicitly flat curve (bounded_flat preserves coverage)".into()))
    }
    /// Returns instantaneous variance at valuation; see [`Self::new`].
    pub fn initial_variance(&self) -> f64 {
        self.initial_variance
    }
    /// Returns the long-run variance; see [`Self::new`].
    pub fn long_run_variance(&self) -> f64 {
        self.long_run_variance
    }
    /// Returns mean-reversion speed per year; see [`Self::new`].
    pub fn mean_reversion_speed(&self) -> f64 {
        self.mean_reversion_speed
    }
    /// Returns the volatility-of-volatility parameter xi; see [`Self::new`].
    pub fn vol_of_vol(&self) -> f64 {
        self.vol_of_vol
    }
    /// Returns the correlation between the asset and variance Brownian motions; see [`Self::new`].
    pub fn correlation(&self) -> f64 {
        self.correlation
    }
}
