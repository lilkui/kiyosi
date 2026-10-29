use crate::{Error, Result, Timestamp, error::finite};

/// Owns an immutable continuous-rate curve using Actual/365 Fixed.
///
/// Node factors interpolate linearly in log space from an implicit factor of one
/// at the reference. Dated curves reject queries before their reference; node
/// curves also reject queries after their final node. Negative rates are allowed.
/// See [`Self::flat`] and [`Self::new`] for construction.
#[derive(Clone, Debug, PartialEq)]
pub struct RateTermStructure {
    reference: Option<Timestamp>,
    rate: Option<f64>,
    nodes: Vec<Timestamp>,
    factors: Vec<f64>,
    forwards: Vec<f64>,
    spread: f64,
}

impl RateTermStructure {
    /// Creates a finite flat rate, optionally restricted to times at or after a reference.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite rate.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{Date, RateTermStructure};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let curve = RateTermStructure::flat(0.05, Some(start))?;
    /// assert!((curve.factor(start, end)? - (-0.05_f64).exp()).abs() < 1e-14);
    /// # Ok(()) }
    /// ```
    pub fn flat(rate: f64, reference: Option<Timestamp>) -> Result<Self> {
        finite(rate, false)?;
        Ok(Self {
            reference,
            rate: Some(rate),
            nodes: vec![],
            factors: vec![],
            forwards: vec![],
            spread: 0.0,
        })
    }

    /// Creates strictly increasing nodes after the reference with positive finite factors.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for empty or mismatched sequences,
    /// unordered nodes, nonpositive/non-finite factors or non-finite forwards.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::{Date, RateTermStructure};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let curve = RateTermStructure::new(start, vec![end], vec![0.95])?;
    /// assert!((curve.factor(start, end)? - 0.95).abs() < 1e-14);
    /// assert_eq!(curve.nodes(), &[end]);
    /// assert_eq!(curve.factors(), vec![0.95]);
    /// assert_eq!(curve.reference_time(), Some(start));
    /// assert_eq!(curve.max_time(), Some(end));
    /// # Ok(()) }
    /// ```
    pub fn new(reference: Timestamp, nodes: Vec<Timestamp>, factors: Vec<f64>) -> Result<Self> {
        if nodes.is_empty() || nodes.len() != factors.len() {
            return Err(Error::InvalidInput(
                "curve nodes and factors must have the same nonzero size".into(),
            ));
        }
        let mut previous = reference;
        let mut previous_log = 0.0;
        let mut forwards = Vec::with_capacity(nodes.len());
        for (&node, &factor) in nodes.iter().zip(&factors) {
            if node <= previous {
                return Err(Error::InvalidInput(
                    "curve nodes must increase after the reference".into(),
                ));
            }
            finite(factor, true)?;
            let log = factor.ln();
            let forward = (previous_log - log) / previous.years_until(node);
            finite(forward, false)?;
            forwards.push(forward);
            previous = node;
            previous_log = log;
        }
        Ok(Self {
            reference: Some(reference),
            rate: None,
            nodes,
            factors,
            forwards,
            spread: 0.0,
        })
    }

    /// Reports whether the curve is explicitly flat; constant node curves are not flat.
    ///
    /// See [`Self::flat`] and [`Self::new`] for the two representations.
    pub fn is_flat(&self) -> bool {
        self.rate.is_some()
    }
    /// Returns the explicitly configured flat rate, or `None` for node curves.
    ///
    /// See [`Self::shifted`] for a flat-rate replacement example.
    pub fn flat_rate(&self) -> Option<f64> {
        self.rate
    }
    /// Returns the inclusive lower coverage bound, or `None` for undated flat rates.
    ///
    /// See [`Self::new`] for a dated curve example.
    pub fn reference_time(&self) -> Option<Timestamp> {
        self.reference
    }
    /// Returns the inclusive upper coverage bound, or `None` for flat rates.
    ///
    /// See [`Self::new`] for a dated curve example.
    pub fn max_time(&self) -> Option<Timestamp> {
        self.nodes.last().copied()
    }
    /// Borrows the supplied nodes, excluding the implicit reference.
    ///
    /// Storage remains valid while the curve is borrowed; [`Self::shifted`] creates
    /// a separate curve without invalidating the borrow. See [`Self::new`].
    pub fn nodes(&self) -> &[Timestamp] {
        &self.nodes
    }
    /// Returns owned node factors including any parallel shift.
    ///
    /// Flat curves return an empty vector. Shifted factors may underflow to zero
    /// or overflow to infinity. See [`Self::new`] for node access.
    pub fn factors(&self) -> Vec<f64> {
        let Some(reference) = self.reference.filter(|_| self.spread != 0.0) else {
            return self.factors.clone();
        };
        self.nodes
            .iter()
            .zip(&self.factors)
            .map(|(&node, &factor)| (factor.ln() - self.spread * reference.years_until(node)).exp())
            .collect()
    }
    /// Checks whether a timestamp lies within the curve's inclusive coverage.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside the curve's bounds.
    /// See [`Self::flat`] and [`Self::new`] for coverage examples.
    pub fn validate_time(&self, time: Timestamp) -> Result<()> {
        if self.reference.is_some_and(|reference| time < reference)
            || self.max_time().is_some_and(|end| time > end)
        {
            return Err(Error::InvalidInput(
                "timestamp is outside curve coverage".into(),
            ));
        }
        Ok(())
    }
    /// Returns the log factor over an ordered, covered interval.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for uncovered endpoints or a reversed
    /// interval, and [`Error::ComputationFailure`] for a non-finite result.
    /// See [`Self::factor`] for interval usage.
    pub fn log_factor(&self, start: Timestamp, end: Timestamp) -> Result<f64> {
        self.validate_time(start)?;
        self.validate_time(end)?;
        if end < start {
            return Err(Error::InvalidInput("curve interval is reversed".into()));
        }
        let mut value = -self.rate.unwrap_or(self.spread) * start.years_until(end);
        if !self.is_flat() {
            let mut index = self
                .nodes
                .partition_point(|&node| node <= start)
                .min(self.nodes.len() - 1);
            let mut current = start;
            while current < end {
                let next = end.min(self.nodes[index]);
                // Subtract timestamps before conversion to preserve distant microsecond intervals.
                value -= self.forwards[index] * current.years_until(next);
                current = next;
                index += 1;
            }
        }
        if !value.is_finite() {
            return Err(Error::ComputationFailure(
                "value must be finite and in its valid range".into(),
            ));
        }
        Ok(value)
    }
    /// Returns the interval factor, allowing underflow to zero.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::log_factor`] errors; overflow returns
    /// [`Error::ComputationFailure`]. See [`Self::flat`] for an example.
    pub fn factor(&self, start: Timestamp, end: Timestamp) -> Result<f64> {
        let value = self.log_factor(start, end)?.exp();
        if !value.is_finite() {
            return Err(Error::ComputationFailure(
                "value must be finite and in its valid range".into(),
            ));
        }
        Ok(value)
    }
    /// Returns the right-hand instantaneous forward rate.
    ///
    /// The last node uses its left interval; see [`Self::new`] for a node curve.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside curve coverage.
    pub fn forward_rate(&self, time: Timestamp) -> Result<f64> {
        self.validate_time(time)?;
        Ok(self.rate.unwrap_or_else(|| {
            let index = self
                .nodes
                .partition_point(|&node| node <= time)
                .min(self.nodes.len() - 1);
            self.forwards[index] + self.spread
        }))
    }
    /// Returns an owned parallel shift with unchanged coverage.
    ///
    /// The original curve is unchanged on success and failure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite shifts or resulting rates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi_core::RateTermStructure;
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let curve = RateTermStructure::flat(0.05, None)?;
    /// let shifted = curve.shifted(0.01)?;
    /// assert_eq!(shifted.flat_rate(), Some(0.05 + 0.01));
    /// assert_eq!(curve.flat_rate(), Some(0.05));
    /// # Ok(()) }
    /// ```
    pub fn shifted(&self, shift: f64) -> Result<Self> {
        finite(shift, false)?;
        let mut copy = self.clone();
        if let Some(rate) = self.rate {
            let rate = rate + shift;
            finite(rate, false)?;
            copy.rate = Some(rate);
        } else {
            copy.spread += shift;
            finite(copy.spread, false)?;
            for rate in &self.forwards {
                finite(rate + copy.spread, false)?;
            }
        }
        Ok(copy)
    }
    pub(crate) fn validate_horizon(&self, valuation: Timestamp, end: Timestamp) -> Result<()> {
        self.validate_time(end)?;
        let mut previous = 0.0;
        let mut previous_time = valuation;
        for &node in self
            .nodes
            .iter()
            .filter(|&&node| node > valuation && node <= end)
        {
            let time = valuation.years_until(node);
            if time <= previous {
                return Err(Error::ComputationFailure(
                    "curve knots are not distinct in pricing time".into(),
                ));
            }
            previous = time;
            previous_time = node;
        }
        if previous_time < end && valuation.years_until(end) <= previous {
            return Err(Error::ComputationFailure(
                "pricing horizon is not distinct from its preceding knot".into(),
            ));
        }
        Ok(())
    }
}
