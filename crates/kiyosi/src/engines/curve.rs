use crate::Result;

#[derive(PartialEq)]
pub(super) struct Curve {
    segments: Vec<Segment>,
    from_start: Vec<f64>,
    to_end: Vec<f64>,
}

#[derive(PartialEq)]
struct Segment {
    start: f64,
    end: f64,
    width: f64,
    rate: f64,
}

impl Segment {
    fn log_factor(&self, start: f64, end: f64) -> f64 {
        let width = if start <= self.start && end >= self.end {
            self.width
        } else {
            let overlap = end.min(self.end) - start.max(self.start);
            overlap * (self.width / (self.end - self.start))
        };
        -self.rate * width
    }
}

impl Curve {
    const BYTES_PER_INTERVAL: u64 = (size_of::<Segment>() + 2 * size_of::<f64>()) as u64;

    fn interior_nodes(
        curve: &crate::RateTermStructure,
        valuation: crate::Timestamp,
        expiry: crate::Timestamp,
    ) -> &[crate::Timestamp] {
        let nodes = curve.nodes_after(valuation);
        &nodes[..nodes.partition_point(|&node| node < expiry)]
    }

    pub fn interval_count(
        curve: &crate::RateTermStructure,
        valuation: crate::Timestamp,
        expiry: crate::Timestamp,
    ) -> u64 {
        Self::interior_nodes(curve, valuation, expiry).len() as u64 + 1
    }

    pub fn bytes_for_intervals(count: u64) -> u64 {
        count.saturating_mul(Self::BYTES_PER_INTERVAL)
    }

    pub fn workspace_bytes(&self) -> u64 {
        Self::bytes_for_intervals(self.segments.len() as u64)
    }

    pub fn ends(&self) -> impl ExactSizeIterator<Item = f64> + '_ {
        self.segments.iter().map(|segment| segment.end)
    }

    pub fn ends_between(&self, start: f64, end: f64) -> impl Iterator<Item = f64> + '_ {
        let first = self
            .segments
            .partition_point(|segment| segment.end <= start);
        self.segments[first..]
            .iter()
            .map(|segment| segment.end)
            .take_while(move |&knot| knot < end)
    }

    pub fn new(
        curve: &crate::RateTermStructure,
        valuation: crate::Timestamp,
        expiry: crate::Timestamp,
    ) -> Result<Self> {
        curve.validate_horizon(valuation, expiry)?;
        crate::cancellation::check()?;
        let nodes = Self::interior_nodes(curve, valuation, expiry);
        let count = nodes.len() + 1;
        let mut segments = Vec::with_capacity(count);
        let mut from_start = Vec::with_capacity(count);
        let mut previous = valuation;
        let mut log = 0.0;
        for (index, end) in nodes.iter().copied().chain([expiry]).enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let segment = Segment {
                start: valuation.years_until(previous),
                end: valuation.years_until(end),
                // Keep the segment width independent of rounding in the absolute coordinates.
                width: previous.years_until(end),
                rate: curve.forward_rate(previous)?,
            };
            from_start.push(log);
            log -= segment.rate * segment.width;
            segments.push(segment);
            previous = end;
        }
        let mut to_end = vec![0.0; count];
        log = 0.0;
        for index in (0..count).rev() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            to_end[index] = log;
            log -= segments[index].rate * segments[index].width;
        }
        Ok(Self {
            segments,
            from_start,
            to_end,
        })
    }

    pub fn rate_bounds(&self) -> [f64; 2] {
        self.segments.iter().fold(
            [f64::INFINITY, f64::NEG_INFINITY],
            |[low, high], segment| [low.min(segment.rate), high.max(segment.rate)],
        )
    }

    pub fn rate(&self, time: f64) -> f64 {
        self.segments[self
            .segments
            .partition_point(|segment| segment.end <= time)
            .min(self.segments.len() - 1)]
        .rate
    }

    #[expect(
        clippy::float_cmp,
        reason = "Exact curve endpoints select cached integrals."
    )]
    pub fn log_factor(&self, start: f64, end: f64) -> f64 {
        let horizon = self.segments[self.segments.len() - 1].end;
        let start = start.max(0.0);
        let end = end.min(horizon);
        if start >= end {
            return 0.0;
        }
        let first = self
            .segments
            .partition_point(|segment| segment.end <= start);
        let last = self.segments.partition_point(|segment| segment.end < end);
        if start == 0.0 && self.from_start[last].is_finite() {
            return self.from_start[last] + self.segments[last].log_factor(start, end);
        }
        if end == horizon && self.to_end[first].is_finite() {
            return self.segments[first].log_factor(start, end) + self.to_end[first];
        }
        self.segments[first..=last]
            .iter()
            .map(|segment| segment.log_factor(start, end))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Date, RateTermStructure};

    #[test]
    fn interval_integrals_preserve_knots_partial_segments_and_short_intervals() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(3 * 365)?;
        let rates = RateTermStructure::new(
            start,
            vec![start.add_days(365)?, start.add_days(2 * 365)?, end],
            vec![(-0.25_f64).exp(), 0.25_f64.exp(), (-1.25_f64).exp()],
        )?;
        let curve = Curve::new(&rates, start, end)?;
        for (from, to, expected) in [
            (0.0, 1.0, -0.25),
            (0.0, 2.0, 0.25),
            (0.0, 3.0, -1.25),
            (0.0, 1.5, 0.0),
            (1.0, 3.0, -1.0),
            (1.5, 3.0, -1.25),
            (0.5, 2.5, -0.375),
            (1.0, 2.0, 0.5),
            (2.0, 2.0, 0.0),
        ] {
            assert!((curve.log_factor(from, to) - expected).abs() < 1e-14);
        }
        let next = libm::nextafter(1.0, 2.0);
        assert!((curve.log_factor(1.0, next) - 0.5 * (next - 1.0)).abs() < 1e-30);
        let steep = Curve::new(&RateTermStructure::flat(1e100, None)?, start, end)?;
        assert!((steep.log_factor(1.0, next) / (-1e100 * (next - 1.0)) - 1.0).abs() < 1e-14);
        Ok(())
    }
}
