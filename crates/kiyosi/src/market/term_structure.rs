use std::sync::Arc;

use crate::{Error, Result, Timestamp, TradingCalendar, error::finite, time::MICROS_PER_DAY};

/// Owns a term structure of continuously compounded rates.
///
/// Uses Actual/365 Fixed by default. Factor nodes interpolate log-linearly
/// from an implicit factor of one at the reference time.
/// [`Self::bounded_flat_with_calendar`] supports calendar-based accrual.
/// Dated curves reject queries before their reference; bounded flat curves and
/// node-based curves also enforce their upper coverage bounds. Negative rates
/// are allowed. Clones share immutable node storage; parallel shifts retain that
/// storage and own their shift. See [`Self::flat`], [`Self::bounded_flat`] and [`Self::new`].
#[derive(Clone, Debug, PartialEq)]
pub struct RateTermStructure {
    reference: Option<Timestamp>,
    upper_bound: Option<Timestamp>,
    rate: Option<f64>,
    nodes: Arc<[Timestamp]>,
    factors: Arc<[f64]>,
    forwards: Arc<[f64]>,
    node_flat_rate: Option<f64>,
    spread: f64,
    calendar_accrual: Option<CalendarAccrual>,
}

#[derive(Clone, Debug, PartialEq)]
struct CalendarAccrual {
    calendar: TradingCalendar,
    rate: f64,
    starts_open: bool,
    open_micros_at_nodes: Arc<[i64]>,
}

impl CalendarAccrual {
    fn is_open(&self, segment: usize) -> bool {
        self.starts_open == (segment % 2 == 0)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "Elapsed microseconds intentionally round to fractional f64 years."
    )]
    fn year_fraction(&self, open_micros: i64) -> f64 {
        open_micros as f64
            / (MICROS_PER_DAY as f64 * f64::from(self.calendar.trading_days_per_year()))
    }

    fn forward_rate(&self, segment: usize) -> f64 {
        if self.is_open(segment) {
            self.rate * (365.0 / f64::from(self.calendar.trading_days_per_year()))
        } else {
            0.0
        }
    }

    fn validate_rate(&self, has_transitions: bool) -> Result<()> {
        finite(self.rate, false, "rate")?;
        finite(self.forward_rate(0), false, "forward_rate")?;
        if has_transitions {
            finite(self.forward_rate(1), false, "forward_rate")?;
        }
        Ok(())
    }
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
    /// use kiyosi::{Date, RateTermStructure};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let curve = RateTermStructure::flat(0.05, Some(start))?;
    /// assert!((curve.factor(start, end)? - (-0.05_f64).exp()).abs() < 1e-14);
    /// # Ok(()) }
    /// ```
    pub fn flat(rate: f64, reference: Option<Timestamp>) -> Result<Self> {
        finite(rate, false, "rate")?;
        Ok(Self {
            reference,
            upper_bound: None,
            rate: Some(rate),
            nodes: Arc::default(),
            factors: Arc::default(),
            forwards: Arc::default(),
            node_flat_rate: None,
            spread: 0.0,
            calendar_accrual: None,
        })
    }

    /// Creates an exactly constant rate with inclusive lower and upper coverage bounds.
    ///
    /// Preserves the supplied rate without reconstructing it from rounded discount
    /// factors. Flat-rate engines accept this representation and retain its coverage.
    /// Nodes and node factors are empty, as for [`Self::flat`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for a non-finite rate or an upper bound before
    /// the reference. Equal bounds allow queries only at that instant.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, RateTermStructure};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = start.add_days(365)?;
    /// let curve = RateTermStructure::bounded_flat(0.05, start, end)?;
    /// assert_eq!(curve.exact_flat_rate(), Some(0.05));
    /// assert_eq!(curve.max_time(), Some(end));
    /// assert!(curve.forward_rate(end.add_days(1)?).is_err());
    /// # Ok(()) }
    /// ```
    pub fn bounded_flat(rate: f64, reference: Timestamp, max_time: Timestamp) -> Result<Self> {
        let mut curve = Self::flat(rate, Some(reference))?;
        if max_time < reference {
            return Err(Error::InvalidInput(
                "curve max_time precedes reference_time".into(),
            ));
        }
        curve.upper_bound = Some(max_time);
        Ok(curve)
    }

    /// Creates a bounded constant annual quote that accrues on the supplied calendar.
    ///
    /// A full open date accrues `rate / calendar.trading_days_per_year()` and
    /// partial dates accrue proportionally to their elapsed time. Closed dates
    /// contribute zero. Both bounds are inclusive. [`TradingCalendar::all_days`]
    /// gives the same curve as [`Self::bounded_flat`]. Shifts apply to the quote.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, RateTermStructure, TradingCalendar};
    /// # fn main() -> kiyosi::Result<()> {
    /// let friday = Date::new(2025, 1, 3)?.start_of_day();
    /// let saturday = friday.add_days(1)?;
    /// let monday = friday.add_days(3)?;
    /// let curve = RateTermStructure::bounded_flat_with_calendar(
    ///     0.05, friday, monday, TradingCalendar::weekdays(),
    /// )?;
    /// assert_eq!(curve.factor(saturday, monday)?, 1.0);
    /// assert!((curve.log_factor(friday, monday)? + 0.05 / 261.0).abs() < 1e-15);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for reversed bounds, a non-finite quote,
    /// or a non-finite Actual/365 Fixed forward. Cancellation returns [`Error::Io`]
    /// with [`std::io::ErrorKind::Interrupted`].
    ///
    /// # Panics
    ///
    /// Propagates panics from a custom calendar predicate.
    pub fn bounded_flat_with_calendar(
        rate: f64,
        reference: Timestamp,
        max_time: Timestamp,
        calendar: TradingCalendar,
    ) -> Result<Self> {
        let mut curve = Self::bounded_flat(rate, reference, max_time)?;
        if calendar == TradingCalendar::all_days() {
            return Ok(curve);
        }
        let starts_open = calendar.is_trading_day(reference.date());
        let nodes = calendar.transitions(reference, max_time, starts_open)?;
        let mut open_micros_at_nodes = Vec::with_capacity(nodes.len());
        let mut previous = reference;
        let mut open_micros = 0;
        for (index, &node) in nodes.iter().enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            if starts_open == (index % 2 == 0) {
                open_micros += previous.micros_until(node);
            }
            open_micros_at_nodes.push(open_micros);
            previous = node;
        }
        let accrual = CalendarAccrual {
            calendar,
            rate,
            starts_open,
            open_micros_at_nodes: open_micros_at_nodes.into(),
        };
        accrual.validate_rate(!nodes.is_empty())?;
        curve.rate = None;
        curve.nodes = nodes.into();
        curve.calendar_accrual = Some(accrual);
        Ok(curve)
    }

    /// Creates strictly increasing nodes after the reference with positive finite factors.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for empty or mismatched sequences,
    /// unordered nodes, nonpositive or non-finite factors or non-finite forwards.
    /// Cancellation returns [`Error::Io`] with [`std::io::ErrorKind::Interrupted`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, RateTermStructure};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let curve = RateTermStructure::new(start, vec![end], vec![0.95])?;
    /// assert!((curve.factor(start, end)? - 0.95).abs() < 1e-14);
    /// assert_eq!(curve.nodes(), &[end]);
    /// assert_eq!(curve.factors()?, vec![0.95]);
    /// assert_eq!(curve.reference_time(), Some(start));
    /// assert_eq!(curve.max_time(), Some(end));
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::float_cmp,
        reason = "Flat-rate algorithms require exactly constant forwards."
    )]
    pub fn new(reference: Timestamp, nodes: Vec<Timestamp>, factors: Vec<f64>) -> Result<Self> {
        if nodes.is_empty() || nodes.len() != factors.len() {
            return Err(Error::InvalidInput(
                "curve nodes and factors must have the same nonzero size".into(),
            ));
        }
        let mut previous = reference;
        let mut previous_factor = 1.0;
        let mut forwards = Vec::with_capacity(nodes.len());
        let mut node_flat_rate = None;
        for (index, (&node, &factor)) in nodes.iter().zip(&factors).enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            if node <= previous {
                return Err(Error::InvalidInput(
                    "curve nodes must increase after the reference".into(),
                ));
            }
            finite(factor, true, "factors")?;
            let forward =
                -crate::numerics::log_ratio(factor, previous_factor) / previous.years_until(node);
            finite(forward, false, "forward_rate")?;
            node_flat_rate = if index == 0 {
                Some(forward)
            } else {
                node_flat_rate.filter(|&rate| rate == forward)
            };
            forwards.push(forward);
            previous = node;
            previous_factor = factor;
        }
        Ok(Self {
            reference: Some(reference),
            upper_bound: None,
            rate: None,
            nodes: nodes.into(),
            factors: factors.into(),
            forwards: forwards.into(),
            node_flat_rate,
            spread: 0.0,
            calendar_accrual: None,
        })
    }

    /// Returns an exactly constant rate, or `None` when any implied forwards differ.
    ///
    /// Node curves qualify only when every implied forward is exactly equal;
    /// their coverage and representation remain unchanged. Rounded factors from a
    /// flat rate may not qualify. Calendar quotes qualify if zero or if their
    /// coverage contains no change in the effective forward rate. See
    /// [`Self::bounded_flat`] and [`Self::bounded_flat_with_calendar`].
    pub fn exact_flat_rate(&self) -> Option<f64> {
        if let Some(accrual) = &self.calendar_accrual {
            return (accrual.rate == 0.0 || self.nodes.is_empty()).then(|| accrual.forward_rate(0));
        }
        self.rate
            .or_else(|| self.node_flat_rate.map(|rate| rate + self.spread))
    }

    pub(super) fn require_flat_rate(&self, field: &str) -> Result<f64> {
        self.exact_flat_rate().ok_or_else(|| {
            Error::UnsupportedOperation(format!(
                "{field} curve is not exactly constant; use an Actual/365 Fixed flat curve (bounded_flat preserves coverage)"
            ).into())
        })
    }

    /// Returns the inclusive lower coverage bound, or `None` for undated flat rates.
    ///
    /// See [`Self::new`] for a dated curve example.
    pub fn reference_time(&self) -> Option<Timestamp> {
        self.reference
    }
    /// Returns the inclusive upper coverage bound, or `None` for unbounded flat rates.
    ///
    /// See [`Self::new`] for a dated curve example.
    pub fn max_time(&self) -> Option<Timestamp> {
        self.upper_bound.or_else(|| self.nodes.last().copied())
    }
    /// Returns an explicit annual quote and its calendar, if present.
    ///
    /// The calendar borrow is valid for the lifetime of the curve borrow. See
    /// [`Self::bounded_flat_with_calendar`] for construction. The all-days
    /// Actual/365 Fixed calendar uses the ordinary bounded-flat representation.
    pub fn calendar_quote(&self) -> Option<(f64, &TradingCalendar)> {
        self.calendar_accrual
            .as_ref()
            .map(|accrual| (accrual.rate, &accrual.calendar))
    }
    /// Borrows factor nodes or calendar transition times, excluding the reference.
    ///
    /// Storage remains valid while the curve is borrowed; [`Self::shifted`] creates
    /// a separate curve without invalidating the borrow. See [`Self::new`].
    pub fn nodes(&self) -> &[Timestamp] {
        &self.nodes
    }
    /// Borrows the original node factors before parallel shifts; see [`Self::shifted`].
    ///
    /// Together with [`Self::parallel_shift`], these preserve the exact construction
    /// representation of a factor-node curve. Quote-based curves have no supplied
    /// node factors. Storage remains valid for this borrow.
    pub fn base_factors(&self) -> &[f64] {
        &self.factors
    }
    /// Returns the accumulated node-curve parallel shift; see [`Self::shifted`].
    ///
    /// Quote-based curves incorporate shifts into their rate and return zero here.
    pub fn parallel_shift(&self) -> f64 {
        self.spread
    }
    /// Returns owned node factors including any parallel shift.
    ///
    /// Calendar curves return factors at their transition times. Underflow to zero
    /// is allowed, as in [`Self::factor`]. Rebuild factor-node curves with
    /// [`Self::base_factors`] and [`Self::parallel_shift`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::ComputationFailure`] for a non-finite log factor or overflow.
    /// Cancellation returns [`Error::Io`] with [`std::io::ErrorKind::Interrupted`].
    pub fn factors(&self) -> Result<Vec<f64>> {
        crate::cancellation::check()?;
        if let Some(accrual) = &self.calendar_accrual {
            return accrual
                .open_micros_at_nodes
                .iter()
                .enumerate()
                .map(|(index, &micros)| {
                    node_factor(index, -accrual.rate * accrual.year_fraction(micros))
                })
                .collect();
        }
        let Some(reference) = self.reference.filter(|_| self.spread != 0.0) else {
            return Ok(self.factors.to_vec());
        };
        self.nodes
            .iter()
            .zip(self.factors.iter())
            .enumerate()
            .map(|(index, (&node, &factor))| {
                node_factor(
                    index,
                    factor.ln() - self.spread * reference.years_until(node),
                )
            })
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
    /// For factor-node curves, intervals between quoted points use their
    /// supplied factors directly, including the reference factor of one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for uncovered endpoints or a reversed
    /// interval, and [`Error::ComputationFailure`] for a non-finite result.
    /// See [`Self::factor`] for interval usage.
    pub fn log_factor(&self, start_time: Timestamp, end_time: Timestamp) -> Result<f64> {
        self.validate_time(start_time)?;
        self.validate_time(end_time)?;
        if end_time < start_time {
            return Err(Error::InvalidInput("curve interval is reversed".into()));
        }
        let mut value = -self.rate.unwrap_or(self.spread) * start_time.years_until(end_time);
        if let Some(accrual) = &self.calendar_accrual {
            value = -accrual.rate * self.accrual_year_fraction(start_time, end_time);
        } else if let Some(quoted) = self.quoted_log_factor(start_time, end_time) {
            value = quoted;
        } else if self.rate.is_none() && start_time < end_time {
            let first = self.nodes.partition_point(|&node| node <= start_time);
            let last = self.nodes.partition_point(|&node| node < end_time);
            if first == last {
                value -= self.forwards[first] * start_time.years_until(end_time);
            } else {
                // Use quoted factors for whole segments and local widths for the two ends.
                let middle =
                    crate::numerics::log_ratio(self.factors[last - 1], self.factors[first]);
                let left = -self.forwards[first] * start_time.years_until(self.nodes[first]);
                let right = -self.forwards[last] * self.nodes[last - 1].years_until(end_time);
                let mut sum = crate::numerics::Sum::default();
                for term in [value, left, middle, right] {
                    sum.add(term);
                }
                value = sum.total();
            }
        }
        if !value.is_finite() {
            return Err(Error::ComputationFailure(
                "log_factor must be finite".into(),
            ));
        }
        Ok(value)
    }

    pub(crate) fn quoted_log_factor(&self, start: Timestamp, end: Timestamp) -> Option<f64> {
        Some(
            crate::numerics::log_ratio(self.quoted_factor_at(end)?, self.quoted_factor_at(start)?)
                - self.spread * start.years_until(end),
        )
    }

    fn quoted_factor_at(&self, time: Timestamp) -> Option<f64> {
        if self.factors.is_empty() {
            None
        } else if self.reference == Some(time) {
            Some(1.0)
        } else {
            self.nodes
                .binary_search(&time)
                .ok()
                .map(|i| self.factors[i])
        }
    }

    pub(crate) fn unshifted_factor_quotes(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> Option<[f64; 2]> {
        if self.rate == Some(0.0) || self.calendar_quote().is_some_and(|(rate, _)| rate == 0.0) {
            return Some([1.0, 1.0]);
        }
        if self.spread != 0.0 {
            return None;
        }
        Some([self.quoted_factor_at(start)?, self.quoted_factor_at(end)?])
    }

    pub(crate) fn accrual_year_fraction(&self, start: Timestamp, end: Timestamp) -> f64 {
        let Some(accrual) = &self.calendar_accrual else {
            return start.years_until(end);
        };
        let open_micros_until = |time| {
            let index = self.nodes.partition_point(|&node| node <= time);
            let (previous, accrued) = if index == 0 {
                (self.reference.unwrap_or(time), 0)
            } else {
                (
                    self.nodes[index - 1],
                    accrual.open_micros_at_nodes[index - 1],
                )
            };
            accrued + i64::from(accrual.is_open(index)) * previous.micros_until(time)
        };
        accrual.year_fraction(open_micros_until(end) - open_micros_until(start))
    }
    /// Returns the interval factor, allowing underflow to zero.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::log_factor`] errors; overflow returns
    /// [`Error::ComputationFailure`]. See [`Self::flat`] for an example.
    pub fn factor(&self, start_time: Timestamp, end_time: Timestamp) -> Result<f64> {
        let value = self.log_factor(start_time, end_time)?.exp();
        if !value.is_finite() {
            return Err(Error::ComputationFailure("factor must be finite".into()));
        }
        Ok(value)
    }
    /// Returns the right-hand instantaneous forward rate in Actual/365 Fixed units.
    ///
    /// The coverage endpoint uses its left interval. See [`Self::new`] and
    /// [`Self::bounded_flat_with_calendar`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] outside curve coverage.
    pub fn forward_rate(&self, time: Timestamp) -> Result<f64> {
        self.validate_time(time)?;
        if let Some(accrual) = &self.calendar_accrual {
            return Ok(accrual.forward_rate(self.nodes.partition_point(|&node| node <= time)));
        }
        Ok(self.rate.unwrap_or_else(|| {
            let index = self
                .nodes
                .partition_point(|&node| node <= time)
                .min(self.nodes.len() - 1);
            self.forwards[index] + self.spread
        }))
    }
    pub(crate) fn shift_origin(&self) -> f64 {
        self.calendar_quote()
            .map(|(rate, _)| rate)
            .or(self.rate)
            .unwrap_or(self.spread)
    }

    /// Returns an owned parallel shift with unchanged coverage.
    ///
    /// The shift uses the calendar's annualization basis for calendar quotes
    /// and Actual/365 Fixed units for other curves.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite shifts or resulting rates.
    /// Cancellation during node validation returns [`Error::Io`] with
    /// [`std::io::ErrorKind::Interrupted`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::RateTermStructure;
    /// # fn main() -> kiyosi::Result<()> {
    /// let curve = RateTermStructure::flat(0.05, None)?;
    /// let shifted = curve.shifted(0.01)?;
    /// assert_eq!(shifted.exact_flat_rate(), Some(0.05 + 0.01));
    /// assert_eq!(curve.exact_flat_rate(), Some(0.05));
    /// # Ok(()) }
    /// ```
    pub fn shifted(&self, shift: f64) -> Result<Self> {
        finite(shift, false, "shift")?;
        let mut copy = self.clone();
        if let Some(accrual) = &mut copy.calendar_accrual {
            accrual.rate += shift;
            accrual.validate_rate(!self.nodes.is_empty())?;
        } else if let Some(rate) = self.rate {
            let rate = rate + shift;
            finite(rate, false, "rate")?;
            copy.rate = Some(rate);
        } else {
            copy.spread += shift;
            finite(copy.spread, false, "parallel_shift")?;
            for rates in self.forwards.chunks(1024) {
                crate::cancellation::check()?;
                for rate in rates {
                    finite(rate + copy.spread, false, "forward_rate")?;
                }
            }
        }
        Ok(copy)
    }
    pub(crate) fn nodes_after(&self, time: Timestamp) -> &[Timestamp] {
        &self.nodes[self.nodes.partition_point(|&node| node <= time)..]
    }

    pub(crate) fn validate_horizon(&self, valuation: Timestamp, end: Timestamp) -> Result<()> {
        self.validate_time(end)?;
        let mut previous = 0.0;
        let mut previous_time = valuation;
        for (index, &node) in self
            .nodes_after(valuation)
            .iter()
            .take_while(|&&node| node <= end)
            .enumerate()
        {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
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

fn node_factor(index: usize, log: f64) -> Result<f64> {
    if index % 1024 == 0 {
        crate::cancellation::check()?;
    }
    let factor = log.exp();
    if log.is_finite() && factor.is_finite() {
        Ok(factor)
    } else {
        Err(Error::ComputationFailure(
            "curve node factor cannot be represented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CancellationToken, Date};

    #[test]
    fn curve_node_construction_checks_cancellation() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(365)?;
        let token = CancellationToken::default();
        assert!(matches!(
            token.run(|| {
                token.cancel();
                let curve = RateTermStructure::new(start, vec![end], vec![0.95]);
                assert!(matches!(
                    &curve,
                    Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted
                ));
                curve
            }),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted
        ));
        Ok(())
    }

    #[test]
    fn node_curve_shifts_observe_active_cancellation() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let times = (1..=2048)
            .map(|offset| start.add_micros(offset * 1_000_000))
            .collect::<Result<Vec<_>>>()?;
        let curve = RateTermStructure::new(start, times, vec![1.0; 2048])?;
        let token = CancellationToken::default();
        assert!(matches!(
            token.run(|| {
                token.cancel();
                assert!(matches!(
                    curve.shifted(0.01),
                    Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted
                ));
                Ok(())
            }),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted
        ));
        assert_eq!(curve.parallel_shift(), 0.0);
        assert_eq!(curve.shifted(0.01)?.parallel_shift(), 0.01);
        Ok(())
    }

    #[test]
    fn curve_factor_reads_observe_active_cancellation() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(365)?;
        let base = RateTermStructure::new(start, vec![end], vec![0.95])?;
        for curve in [
            base.shifted(0.01)?,
            base,
            RateTermStructure::bounded_flat_with_calendar(
                0.05,
                start,
                end,
                TradingCalendar::weekdays(),
            )?,
        ] {
            let token = CancellationToken::default();
            token
                .run(|| {
                    token.cancel();
                    assert!(matches!(
                        curve.factors(),
                        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted
                    ));
                    Ok(())
                })
                .unwrap_err();
            assert_eq!(curve.factors()?.len(), curve.nodes().len());
        }
        Ok(())
    }

    #[test]
    fn curve_horizon_scan_observes_active_cancellation() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(365)?;
        let curve = RateTermStructure::new(start, vec![end], vec![0.95])?;
        let token = CancellationToken::default();
        let interrupted = token.run(|| {
            token.cancel();
            let error = curve.validate_horizon(start, end);
            assert!(matches!(&error, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted));
            error
        });
        assert!(
            matches!(interrupted, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        Ok(())
    }
}
