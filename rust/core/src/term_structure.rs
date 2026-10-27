use crate::{Error, ErrorCategory, Result, Timestamp, error::finite};

/// Owning, immutable continuous-rate curve using Actual/365 Fixed.
/// Node factors interpolate linearly in log space from an implicit factor of one
/// at the reference. Dated curves reject queries before their reference; node
/// curves also reject queries after their final node. Negative rates are allowed.
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
    /// Non-finite rates return `InvalidParameter`.
    ///
    /// ```
    /// use kiyosi_core::{Date, RateTermStructure};
    /// # fn main() -> kiyosi_core::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let curve = RateTermStructure::flat(0.05, Some(start))?;
    /// assert!((curve.factor(start, end)? - (-0.05_f64).exp()).abs() < 1e-14);
    /// # Ok(()) }
    /// ```
    pub fn flat(rate: f64, reference: Option<Timestamp>) -> Result<Self> {
        finite(rate, ErrorCategory::InvalidParameter, false)?;
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
    /// Empty, mismatched or malformed nodes return `InvalidParameter`.
    pub fn new(reference: Timestamp, nodes: Vec<Timestamp>, factors: Vec<f64>) -> Result<Self> {
        if nodes.is_empty() || nodes.len() != factors.len() {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "curve nodes and factors must have the same nonzero size",
            ));
        }
        let mut previous = reference;
        let mut previous_log = 0.0;
        let mut forwards = Vec::with_capacity(nodes.len());
        for (&node, &factor) in nodes.iter().zip(&factors) {
            if node <= previous {
                return Err(Error::new(
                    ErrorCategory::InvalidParameter,
                    "curve nodes must increase after the reference",
                ));
            }
            finite(factor, ErrorCategory::InvalidParameter, true)?;
            let log = factor.ln();
            let forward = (previous_log - log) / previous.years_until(node);
            finite(forward, ErrorCategory::InvalidParameter, false)?;
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

    /// Whether the curve is explicitly flat (a constant node curve is not flat).
    pub fn is_flat(&self) -> bool {
        self.rate.is_some()
    }
    /// The explicitly configured flat rate, when present.
    pub fn flat_rate(&self) -> Option<f64> {
        self.rate
    }
    /// Inclusive lower coverage bound; absent for undated flat rates.
    pub fn reference_time(&self) -> Option<Timestamp> {
        self.reference
    }
    /// Inclusive upper coverage bound; absent for flat rates.
    pub fn max_time(&self) -> Option<Timestamp> {
        self.nodes.last().copied()
    }
    /// Supplied nodes, excluding the implicit reference; borrows this curve's storage.
    pub fn nodes(&self) -> &[Timestamp] {
        &self.nodes
    }
    /// Owning node factors including any parallel shift. Shifted factors may underflow or overflow.
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
    /// Validates coverage, returning `InvalidTimeRange` outside the curve's bounds.
    pub fn validate_time(&self, time: Timestamp) -> Result<()> {
        if self.reference.is_some_and(|reference| time < reference)
            || self.max_time().is_some_and(|end| time > end)
        {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "timestamp is outside curve coverage",
            ));
        }
        Ok(())
    }
    /// Log factor over an ordered, covered interval. Non-finite results return `InvalidResult`.
    pub fn log_factor(&self, start: Timestamp, end: Timestamp) -> Result<f64> {
        self.validate_time(start)?;
        self.validate_time(end)?;
        if end < start {
            return Err(Error::new(
                ErrorCategory::InvalidTimeRange,
                "curve interval is reversed",
            ));
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
        finite(value, ErrorCategory::InvalidResult, false)?;
        Ok(value)
    }
    /// Interval factor. Underflow to zero is allowed; overflow returns `InvalidResult`.
    pub fn factor(&self, start: Timestamp, end: Timestamp) -> Result<f64> {
        let value = self.log_factor(start, end)?.exp();
        finite(value, ErrorCategory::InvalidResult, false)?;
        Ok(value)
    }
    /// Right-hand instantaneous forward rate. The last node uses its left interval.
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
    /// Returns an owning parallel shift with unchanged coverage; never mutates this curve.
    /// Non-finite shifts or resulting rates return `InvalidParameter`.
    pub fn shifted(&self, shift: f64) -> Result<Self> {
        finite(shift, ErrorCategory::InvalidParameter, false)?;
        let mut copy = self.clone();
        if let Some(rate) = self.rate {
            let rate = rate + shift;
            finite(rate, ErrorCategory::InvalidParameter, false)?;
            copy.rate = Some(rate);
        } else {
            copy.spread += shift;
            finite(copy.spread, ErrorCategory::InvalidParameter, false)?;
            for rate in &self.forwards {
                finite(rate + copy.spread, ErrorCategory::InvalidParameter, false)?;
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
                return Err(Error::new(
                    ErrorCategory::InvalidResult,
                    "curve knots are not distinct in pricing time",
                ));
            }
            previous = time;
            previous_time = node;
        }
        if previous_time < end && valuation.years_until(end) <= previous {
            return Err(Error::new(
                ErrorCategory::InvalidResult,
                "pricing horizon is not distinct from its preceding knot",
            ));
        }
        Ok(())
    }
}
