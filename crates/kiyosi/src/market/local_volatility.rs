use crate::{Error, Rate, RateTermStructure, Result, Timestamp, error::finite};

/// Owns a local-volatility table in absolute time and spot coordinates.
///
/// Rows correspond to times and columns to spots. Local volatility interpolates
/// bilinearly using Actual/365 Fixed year fractions. Extrapolation in the spot
/// dimension holds the nearest endpoint volatility constant; time extrapolation
/// is rejected. The first time is the reference. Storage is immutable and borrowed
/// accessors remain valid for this surface's lifetime. See [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct LocalVolatilitySurface {
    times: Vec<Timestamp>,
    years: Vec<f64>,
    spots: Vec<f64>,
    values: Vec<f64>,
    maximum: f64,
}

impl LocalVolatilitySurface {
    /// Creates a validated table of positive annualized volatilities in decimal units.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for fewer than two nodes on either axis,
    /// unordered or numerically indistinguishable time nodes, a nonrectangular
    /// table, or nonpositive or non-finite prices
    /// or volatilities. No partially validated value is returned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, LocalVolatilitySurface};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let surface = LocalVolatilitySurface::new(vec![start, end],
    ///     vec![80.0, 120.0], vec![vec![0.2, 0.4], vec![0.2, 0.4]])?;
    /// assert!((surface.volatility(start, 100.0)? - 0.3).abs() < 1e-14);
    /// # Ok(()) }
    /// ```
    pub fn new(times: Vec<Timestamp>, spots: Vec<f64>, values: Vec<Vec<f64>>) -> Result<Self> {
        if times.len() < 2 || times.windows(2).any(|p| p[0] >= p[1]) {
            return Err(Error::InvalidInput(
                "times must contain at least two strictly increasing nodes".into(),
            ));
        }
        if spots.len() < 2 || spots.windows(2).any(|p| p[0] >= p[1]) {
            return Err(Error::InvalidInput(
                "spots must contain at least two strictly increasing nodes".into(),
            ));
        }
        for &spot in &spots {
            finite(spot, true, "spots")?;
        }
        if values.len() != times.len() || values.iter().any(|row| row.len() != spots.len()) {
            return Err(Error::InvalidInput(
                "values shape must match times and spots".into(),
            ));
        }
        for (row, values) in values.iter().enumerate() {
            for (column, &value) in values.iter().enumerate() {
                if !value.is_finite() || value <= 0.0 {
                    return Err(Error::InvalidInput(
                        format!("values[{row}][{column}] must be finite and positive").into(),
                    ));
                }
            }
        }
        let years: Vec<_> = times.iter().map(|&t| times[0].years_until(t)).collect();
        if years.windows(2).any(|p| p[0] >= p[1]) {
            return Err(Error::InvalidInput(
                "times collapse in floating-point year fractions".into(),
            ));
        }
        let values: Vec<_> = values.into_iter().flatten().collect();
        let maximum = values.iter().copied().fold(0.0, f64::max);
        Ok(Self {
            times,
            years,
            spots,
            values,
            maximum,
        })
    }

    /// Borrows the absolute time nodes; see [`Self::new`] for an example.
    pub fn times(&self) -> &[Timestamp] {
        &self.times
    }

    /// Borrows the absolute price nodes; see [`Self::new`] for an example.
    pub fn spots(&self) -> &[f64] {
        &self.spots
    }

    /// Borrows the volatility rows; see [`Self::new`] for their layout.
    pub fn values(&self) -> impl Iterator<Item = &[f64]> {
        self.values.chunks(self.spots.len())
    }

    /// Returns the largest supplied volatility; see [`Self::new`] for units.
    pub fn maximum_volatility(&self) -> f64 {
        self.maximum
    }

    /// Interpolates sigma at an absolute time and positive spot.
    ///
    /// Price extrapolation uses the nearest price endpoint without moving the
    /// supplied spot. See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside time coverage or for a
    /// nonpositive or non-finite spot. Inputs remain unchanged.
    pub fn volatility(&self, time: Timestamp, spot: f64) -> Result<f64> {
        self.validate_time(time)?;
        finite(spot, true, "spot")?;
        Ok(self.at_year(self.times[0].years_until(time), spot))
    }

    pub(crate) fn validate_time(&self, time: Timestamp) -> Result<()> {
        if time < self.times[0] || time > self.times[self.times.len() - 1] {
            return Err(Error::InvalidInput(
                "time outside local-volatility coverage".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn at_year(&self, time: f64, spot: f64) -> f64 {
        let i = self
            .years
            .partition_point(|&t| t <= time)
            .saturating_sub(1)
            .min(self.years.len() - 2);
        let j = self
            .spots
            .partition_point(|&s| s <= spot)
            .saturating_sub(1)
            .min(self.spots.len() - 2);
        let a = ((time - self.years[i]) / (self.years[i + 1] - self.years[i])).clamp(0.0, 1.0);
        let b = ((spot - self.spots[j]) / (self.spots[j + 1] - self.spots[j])).clamp(0.0, 1.0);
        let n = self.spots.len();
        // Endpoint-relative interpolation preserves positive subnormal constants.
        let interpolate = |low: f64, high: f64, weight: f64| {
            if weight <= 0.5 {
                low + weight * (high - low)
            } else {
                high + (1.0 - weight) * (low - high)
            }
        };
        let low = interpolate(self.values[i * n + j], self.values[i * n + j + 1], b);
        let high = interpolate(
            self.values[(i + 1) * n + j],
            self.values[(i + 1) * n + j + 1],
            b,
        );
        interpolate(low, high, a)
    }
}

/// Owns rate curves and a validated local-volatility surface.
///
/// Uses the same rate conventions as [`crate::BlackScholesMertonParameters`].
/// See [`Self::new`] for a complete construction example.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalVolatilityParameters {
    risk_free_curve: RateTermStructure,
    dividend_curve: RateTermStructure,
    surface: LocalVolatilitySurface,
}

/// Selects local-volatility replacements; absent fields retain their values.
///
/// See [`LocalVolatilityParameters::replace`].
#[derive(Clone, Debug, Default)]
pub struct LocalVolatilityParameterChanges {
    /// Replaces the risk-free rate or curve; see [`LocalVolatilityParameters::new`].
    pub risk_free_rate: Option<Rate>,
    /// Replaces the dividend yield or curve; see [`LocalVolatilityParameters::new`].
    pub dividend_yield: Option<Rate>,
    /// Replaces the owned surface; see [`LocalVolatilityParameters::new`].
    pub surface: Option<LocalVolatilitySurface>,
}

impl LocalVolatilityParameters {
    /// Returns a validated replacement without changing this model.
    ///
    /// See [`Self::new`] for construction and units.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation for replacement rates. The original
    /// retains its rates, surface and coverage on failure.
    pub fn replace(&self, changes: LocalVolatilityParameterChanges) -> Result<Self> {
        Self::new(
            changes
                .risk_free_rate
                .unwrap_or_else(|| self.risk_free_curve.clone().into()),
            changes
                .dividend_yield
                .unwrap_or_else(|| self.dividend_curve.clone().into()),
            changes.surface.unwrap_or_else(|| self.surface.clone()),
        )
    }
    /// Creates local-volatility parameters from scalar rates or owned curves.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite scalar rates, validating
    /// the risk-free rate before the dividend yield. Coverage is checked by
    /// [`crate::PricingContext`] and pricing, without changing the inputs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, LocalVolatilitySurface, LocalVolatilityParameters, PricingContext};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let surface = LocalVolatilitySurface::new(vec![start, end], vec![80.0, 120.0],
    ///     vec![vec![0.2, 0.2], vec![0.2, 0.2]])?;
    /// let model = LocalVolatilityParameters::new(0.05, 0.02, surface)?;
    /// let context = PricingContext::new(model, 100.0, start)?;
    /// assert_eq!(context.spot_price(), 100.0);
    /// # Ok(()) }
    /// ```
    pub fn new(
        risk_free_rate: impl Into<Rate>,
        dividend_yield: impl Into<Rate>,
        surface: LocalVolatilitySurface,
    ) -> Result<Self> {
        Ok(Self {
            risk_free_curve: risk_free_rate.into().into_curve("risk_free_rate")?,
            dividend_curve: dividend_yield.into().into_curve("dividend_yield")?,
            surface,
        })
    }
    /// Borrows the risk-free curve; see [`Self::new`] for construction.
    pub fn risk_free_curve(&self) -> &RateTermStructure {
        &self.risk_free_curve
    }
    /// Borrows the dividend curve; see [`Self::new`] for construction.
    pub fn dividend_curve(&self) -> &RateTermStructure {
        &self.dividend_curve
    }
    /// Returns the constant scalar risk-free rate, including dated curves.
    ///
    /// Use [`Self::risk_free_curve`] when preserving curve coverage; see [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant curves.
    pub fn flat_risk_free_rate(&self) -> Result<f64> {
        self.risk_free_curve
            .exact_flat_rate()
            .ok_or(Error::UnsupportedOperation(
                "risk-free curve is not exactly constant; use an explicitly flat curve (bounded_flat preserves coverage)".into(),
            ))
    }
    /// Returns the constant scalar dividend yield, including dated curves.
    ///
    /// Use [`Self::dividend_curve`] when preserving curve coverage; see [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant curves.
    pub fn flat_dividend_yield(&self) -> Result<f64> {
        self.dividend_curve
            .exact_flat_rate()
            .ok_or(Error::UnsupportedOperation(
                "dividend curve is not exactly constant; use an explicitly flat curve (bounded_flat preserves coverage)".into(),
            ))
    }
    /// Borrows the fixed surface; see [`Self::new`] for construction.
    pub fn surface(&self) -> &LocalVolatilitySurface {
        &self.surface
    }
}
