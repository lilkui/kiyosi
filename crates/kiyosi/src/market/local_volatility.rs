use crate::VarianceTimeConvention;
use std::sync::Arc;

use crate::{Error, Rate, RateTermStructure, Result, Timestamp, cancellation, error::finite};

fn has_unordered_pair<T: PartialOrd>(nodes: &[T]) -> Result<bool> {
    for (index, pair) in nodes.windows(2).enumerate() {
        if index % 1024 == 0 {
            cancellation::check()?;
        }
        if pair[0] >= pair[1] {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Checks a row count or width against the corresponding surface axis.
///
/// See [`LocalVolatilitySurface::new`] for the required layout.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] if the count differs from the axis length.
#[doc(hidden)]
pub fn validate_local_volatility_surface_dimension(count: usize, axis: usize) -> Result<()> {
    if count != axis {
        return Err(Error::InvalidInput(
            "values shape must match times and spots".into(),
        ));
    }
    Ok(())
}

fn invalid_axis(axis: &str) -> Error {
    Error::InvalidInput(
        format!("{axis} must contain at least two strictly increasing nodes").into(),
    )
}

/// Checks that a local-volatility time node follows its predecessor.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] for a duplicate or decreasing timestamp.
pub fn validate_local_volatility_time_node(previous: Timestamp, node: Timestamp) -> Result<()> {
    if node <= previous {
        return Err(invalid_axis("times"));
    }
    Ok(())
}

/// Owns a local-volatility table in absolute time and spot coordinates.
///
/// Rows correspond to times and columns to spots. Local volatility interpolates
/// bilinearly using Actual/365 Fixed year fractions. Extrapolation in the spot
/// dimension holds the nearest endpoint volatility constant; time extrapolation
/// is rejected. The first time is the reference. Clones share immutable table storage;
/// accessors borrow from the surface. See [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct LocalVolatilitySurface {
    times: Arc<[Timestamp]>,
    years: Arc<[f64]>,
    spots: Arc<[f64]>,
    values: Arc<[Box<[f64]>]>,
    row_groups: Arc<[usize]>,
    extrema: Arc<[[f64; 2]]>,
}

impl LocalVolatilitySurface {
    /// Creates a validated table of positive annualized volatilities in decimal units.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for fewer than two nodes on either axis,
    /// unordered or numerically indistinguishable time nodes, a nonrectangular
    /// table, or nonpositive or non-finite prices or volatilities.
    /// Cancellation returns [`Error::Io`] with [`std::io::ErrorKind::Interrupted`].
    /// No partially validated value is returned.
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
        Self::validate_axis_count(times.len(), "times")?;
        for (index, pair) in times.windows(2).enumerate() {
            if index % 1024 == 0 {
                cancellation::check()?;
            }
            validate_local_volatility_time_node(pair[0], pair[1])?;
        }
        Self::validate_axis_count(spots.len(), "spots")?;
        let mut previous = None;
        for chunk in spots.chunks(1024) {
            cancellation::check()?;
            for &spot in chunk {
                Self::validate_spot_node(previous, spot)?;
                previous = Some(spot);
            }
        }
        validate_local_volatility_surface_dimension(values.len(), times.len())?;
        for rows in values.chunks(1024) {
            cancellation::check()?;
            for row in rows {
                validate_local_volatility_surface_dimension(row.len(), spots.len())?;
            }
        }
        let rows = times.len();
        // The leaves hold rows in time order and the interior holds range extrema.
        let mut extrema = vec![[f64::INFINITY, 0.0_f64]; 2 * rows];
        let mut row_groups = Vec::with_capacity(times.len());
        for (row, current) in values.iter().enumerate() {
            let mut shared = row > 0;
            let [minimum, maximum] = &mut extrema[rows + row];
            for (column, &value) in current.iter().enumerate() {
                if column % 1024 == 0 {
                    cancellation::check()?;
                }
                if !value.is_finite() || value <= 0.0 {
                    return Err(Error::InvalidInput(
                        format!("values[{row}][{column}] must be finite and positive").into(),
                    ));
                }
                *maximum = maximum.max(value);
                *minimum = minimum.min(value);
                shared &= row > 0 && value.to_bits() == values[row - 1][column].to_bits();
            }
            row_groups.push(if shared { row_groups[row - 1] } else { row });
        }
        for i in (1..rows).rev() {
            if i % 1024 == 0 {
                cancellation::check()?;
            }
            let [left_min, left_max] = extrema[2 * i];
            let [right_min, right_max] = extrema[2 * i + 1];
            extrema[i] = [left_min.min(right_min), left_max.max(right_max)];
        }
        let mut years = Vec::with_capacity(times.len());
        for chunk in times.chunks(1024) {
            cancellation::check()?;
            years.extend(chunk.iter().map(|&t| times[0].years_until(t)));
        }
        if has_unordered_pair(&years)? {
            return Err(Error::InvalidInput(
                "times collapse in floating-point year fractions".into(),
            ));
        }
        let values = values
            .into_iter()
            .map(|row| {
                cancellation::check()?;
                Ok(row.into_boxed_slice())
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            times: times.into(),
            years: years.into(),
            spots: spots.into(),
            values,
            row_groups: row_groups.into(),
            extrema: extrema.into(),
        })
    }

    /// Checks the minimum size of an axis for [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] if the named axis has fewer than two nodes.
    #[doc(hidden)]
    pub fn validate_axis_count(count: usize, axis: &str) -> Result<()> {
        if count < 2 {
            return Err(invalid_axis(axis));
        }
        Ok(())
    }

    /// Checks one spot node for [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a nonpositive or non-finite spot,
    /// or for a node that does not strictly follow its predecessor.
    #[doc(hidden)]
    pub fn validate_spot_node(previous: Option<f64>, spot: f64) -> Result<()> {
        finite(spot, true, "spots")?;
        if previous.is_some_and(|previous| spot <= previous) {
            return Err(invalid_axis("spots"));
        }
        Ok(())
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
    pub fn values(&self) -> impl ExactSizeIterator<Item = &[f64]> {
        self.values.iter().map(Box::as_ref)
    }

    /// Returns the largest supplied volatility; see [`Self::new`] for units.
    pub fn maximum_volatility(&self) -> f64 {
        self.extrema[1][1]
    }

    pub(crate) fn constant_volatility(&self) -> Option<f64> {
        let [minimum, maximum] = self.extrema[1];
        (minimum.to_bits() == maximum.to_bits()).then_some(maximum)
    }

    pub(crate) fn constant_volatility_between(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<Option<f64>> {
        if self.validate_time(start).is_err() || self.validate_time(end).is_err() || start >= end {
            return Ok(None);
        }
        if let Some(volatility) = self.constant_volatility() {
            return Ok(Some(volatility));
        }
        // Include the rows that interpolate either edge of the requested interval.
        let first = self.times.partition_point(|&time| time <= start) - 1;
        let last = self.times.partition_point(|&time| time < end);
        cancellation::check()?;
        let [minimum, maximum] = self.extrema[self.times.len() + first];
        Ok((self.row_groups[first] == self.row_groups[last]
            && minimum.to_bits() == maximum.to_bits())
        .then_some(maximum))
    }

    pub(crate) fn maximum_between(&self, start: Timestamp, end: Timestamp) -> Result<f64> {
        if self.times.first() == Some(&start) && self.times.last() == Some(&end) {
            return Ok(self.maximum_volatility());
        }
        let offset = self.times[0].years_until(start);
        let duration = start.years_until(end);
        let rows = self.times.len();
        let mut left = rows + self.times.partition_point(|&node| node <= start);
        let mut right = rows
            + self
                .times
                .partition_point(|&node| start.years_until(node) < duration);
        let mut maximum = self
            .row_maximum(offset)?
            .max(self.row_maximum(offset + duration)?);
        while left < right {
            if left % 2 != 0 {
                maximum = maximum.max(self.extrema[left][1]);
                left += 1;
            }
            if right % 2 != 0 {
                right -= 1;
                maximum = maximum.max(self.extrema[right][1]);
            }
            left /= 2;
            right /= 2;
        }
        Ok(maximum)
    }

    fn row_maximum(&self, time: f64) -> Result<f64> {
        let (row, weight) = bracket(&self.years, time);
        let rows = self.times.len();
        if weight <= 0.0 || self.row_groups[row] == self.row_groups[row + 1] {
            return Ok(self.extrema[rows + row][1]);
        }
        if weight >= 1.0 {
            return Ok(self.extrema[rows + row + 1][1]);
        }
        let (left, right) = (&self.values[row], &self.values[row + 1]);
        let mut maximum = 0.0_f64;
        for (left, right) in left.chunks(1024).zip(right.chunks(1024)) {
            cancellation::check()?;
            for (&left, &right) in left.iter().zip(right) {
                maximum = maximum.max(interpolate(left, right, weight));
            }
        }
        Ok(maximum)
    }

    /// Interpolates sigma at an absolute time and positive spot.
    ///
    /// Price extrapolation uses the nearest price endpoint without moving the
    /// supplied spot. See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside time coverage or for a
    /// nonpositive or non-finite spot.
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
        let (row, time_weight) = bracket(&self.years, time);
        let location = bracket(&self.spots, spot);
        interpolate(
            self.at_spot(row, location),
            self.at_spot(row + 1, location),
            time_weight,
        )
    }

    pub(crate) fn stationary_row(&self, time: f64) -> Option<usize> {
        let (row, _) = bracket(&self.years, time);
        (self.row_groups[row] == self.row_groups[row + 1]).then_some(self.row_groups[row])
    }

    fn at_spot(&self, row: usize, (column, weight): (usize, f64)) -> f64 {
        let values = &self.values[row];
        interpolate(values[column], values[column + 1], weight)
    }
}

fn bracket(nodes: &[f64], value: f64) -> (usize, f64) {
    let left = nodes
        .partition_point(|&node| node <= value)
        .saturating_sub(1)
        .min(nodes.len() - 2);
    let weight = ((value - nodes[left]) / (nodes[left + 1] - nodes[left])).clamp(0.0, 1.0);
    (left, weight)
}

fn interpolate(low: f64, high: f64, weight: f64) -> f64 {
    // Endpoint-relative interpolation preserves positive subnormal constants.
    if weight <= 0.5 {
        low + weight * (high - low)
    } else {
        high + (1.0 - weight) * (low - high)
    }
}

pub(crate) struct LocalVolatilityProjection {
    locations: Vec<(usize, f64)>,
    rows: [(usize, Vec<f64>); 2],
}

impl LocalVolatilityProjection {
    pub(crate) fn new(surface: &LocalVolatilitySurface, spots: &[f64]) -> Result<Self> {
        let mut locations = Vec::with_capacity(spots.len());
        for chunk in spots.chunks(1024) {
            cancellation::check()?;
            locations.extend(chunk.iter().map(|&spot| bracket(&surface.spots, spot)));
        }
        Ok(Self {
            locations,
            rows: std::array::from_fn(|_| (usize::MAX, vec![0.0; spots.len()])),
        })
    }

    pub(crate) fn at_year(
        &mut self,
        surface: &LocalVolatilitySurface,
        time: f64,
    ) -> Result<impl Fn(usize) -> f64 + '_> {
        let (row, weight) = bracket(&surface.years, time);
        if self.rows[0].0 == row + 1 || self.rows[1].0 == row {
            self.rows.swap(0, 1);
        }
        for (index, (loaded, values)) in self.rows.iter_mut().enumerate() {
            let source = row + index;
            if *loaded != source {
                for (values, locations) in values.chunks_mut(1024).zip(self.locations.chunks(1024))
                {
                    cancellation::check()?;
                    for (value, &location) in values.iter_mut().zip(locations) {
                        *value = surface.at_spot(source, location);
                    }
                }
                *loaded = source;
            }
        }
        let [(_, lower), (_, upper)] = &self.rows;
        Ok(move |index| interpolate(lower[index], upper[index], weight))
    }
}

/// Owns rate curves and a validated local-volatility surface.
///
/// Uses the same rate conventions as [`crate::BlackScholesMertonParameters`].
/// See [`Self::new`] for a complete construction example.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalVolatilityParameters {
    variance_time_convention: VarianceTimeConvention,
    risk_free_curve: RateTermStructure,
    dividend_curve: RateTermStructure,
    surface: LocalVolatilitySurface,
}

/// Selects local-volatility replacements. Absent fields default to the source values.
///
/// See [`LocalVolatilityParameters::replace`].
#[derive(Clone, Debug, Default)]
pub struct LocalVolatilityParameterChanges {
    /// Replaces the variance clock without converting volatility values.
    pub variance_time_convention: Option<VarianceTimeConvention>,
    /// Replaces the risk-free rate or curve; see [`LocalVolatilityParameters::new`].
    pub risk_free_rate: Option<Rate>,
    /// Replaces the dividend yield or curve; see [`LocalVolatilityParameters::new`].
    pub dividend_yield: Option<Rate>,
    /// Replaces the owned surface; see [`LocalVolatilityParameters::new`].
    pub surface: Option<LocalVolatilitySurface>,
}

impl LocalVolatilityParameters {
    /// Borrows the variance clock. The rate curves define `r` and `q` accrual.
    ///
    /// See [`Self::new_with_variance_time_convention`] for construction.
    pub fn variance_time_convention(&self) -> &VarianceTimeConvention {
        &self.variance_time_convention
    }

    /// Returns a validated copy with the supplied model parameters.
    ///
    /// See [`Self::new`] for construction and units.
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] validation for replacement rates.
    pub fn replace(&self, changes: LocalVolatilityParameterChanges) -> Result<Self> {
        Self::new_with_variance_time_convention(
            changes
                .risk_free_rate
                .unwrap_or_else(|| self.risk_free_curve.clone().into()),
            changes
                .dividend_yield
                .unwrap_or_else(|| self.dividend_curve.clone().into()),
            changes.surface.unwrap_or_else(|| self.surface.clone()),
            changes
                .variance_time_convention
                .unwrap_or_else(|| self.variance_time_convention.clone()),
        )
    }
    /// Creates local-volatility parameters from scalar rates or owned curves.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite scalar rates, validating
    /// the risk-free rate before the dividend yield. Coverage is checked by
    /// [`crate::PricingContext`] and pricing.
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
        Self::new_with_variance_time_convention(
            risk_free_rate,
            dividend_yield,
            surface,
            VarianceTimeConvention::default(),
        )
    }

    /// Creates a model with an explicit volatility annualization convention.
    ///
    /// Scalar rates and dividend yields use A/365F. Volatility values are interpreted
    /// in the supplied convention without conversion; see [`Self::new`].
    ///
    /// # Errors
    ///
    /// Reuses [`Self::new`] rate and volatility validation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, LocalVolatilityParameters, LocalVolatilitySurface,
    ///     VarianceTimeConvention, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2026, 11, 13)?.start_of_day();
    /// let surface = LocalVolatilitySurface::new(vec![start, start.add_days(3)?],
    ///     vec![80.0, 120.0], vec![vec![0.2; 2]; 2])?;
    /// let model = LocalVolatilityParameters::new_with_variance_time_convention(0.05, 0.02, surface,
    ///     VarianceTimeConvention::Business(TradingCalendar::weekdays()))?;
    /// assert!(model.variance_time_convention().calendar().is_some());
    /// # Ok(()) }
    /// ```
    pub fn new_with_variance_time_convention(
        risk_free_rate: impl Into<Rate>,
        dividend_yield: impl Into<Rate>,
        surface: LocalVolatilitySurface,
        variance_time_convention: VarianceTimeConvention,
    ) -> Result<Self> {
        Ok(Self {
            variance_time_convention,
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
        self.risk_free_curve.require_flat_rate("risk-free")
    }
    /// Returns the constant scalar dividend yield, including dated curves.
    ///
    /// Use [`Self::dividend_curve`] when preserving curve coverage; see [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for nonconstant curves.
    pub fn flat_dividend_yield(&self) -> Result<f64> {
        self.dividend_curve.require_flat_rate("dividend")
    }
    /// Borrows the fixed surface; see [`Self::new`] for construction.
    pub fn surface(&self) -> &LocalVolatilitySurface {
        &self.surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_extrema_match_bilinear_values_at_knots_and_clipped_edges() -> Result<()> {
        let start = crate::Date::new(2025, 1, 1)?.start_of_day();
        let times = (0..5)
            .map(|i| start.add_days(2 * i))
            .collect::<Result<Vec<_>>>()?;
        let surface = LocalVolatilitySurface::new(
            times.clone(),
            vec![80.0, 100.0, 110.0],
            vec![
                vec![0.2, 0.2, 0.2],
                vec![0.2, 0.2, 0.2],
                vec![0.4, 0.9, 0.3],
                vec![0.8, 0.2, 0.3],
                vec![0.3, 0.2, 1.0],
            ],
        )?;
        for first in 0..8 {
            for last in first + 1..=8 {
                let from = start.add_days(first)?;
                let to = start.add_days(last)?;
                let offset = start.years_until(from);
                let duration = from.years_until(to);
                let mut expected = 0.0_f64;
                for time in [offset, offset + duration].into_iter().chain(
                    times
                        .iter()
                        .filter(|&&time| from < time && time < to)
                        .map(|&time| start.years_until(time)),
                ) {
                    for &spot in surface.spots() {
                        expected = expected.max(surface.at_year(time, spot));
                    }
                }
                assert_eq!(
                    surface.maximum_between(from, to)?.to_bits(),
                    expected.to_bits()
                );
            }
        }
        assert_eq!(
            surface
                .constant_volatility_between(start, times[1])?
                .map(f64::to_bits),
            Some(0.2_f64.to_bits())
        );
        assert!(
            surface
                .constant_volatility_between(start, start.add_days(3)?)?
                .is_none()
        );
        Ok(())
    }
}
