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
    pub fn ends(&self) -> impl Iterator<Item = f64> + '_ {
        self.segments.iter().map(|segment| segment.end)
    }

    pub fn new(
        curve: &crate::RateTermStructure,
        valuation: crate::Timestamp,
        expiry: crate::Timestamp,
    ) -> Result<Self> {
        curve.validate_horizon(valuation, expiry)?;
        let mut segments = Vec::new();
        let mut previous = valuation;
        for end in curve
            .nodes_after(valuation)
            .iter()
            .copied()
            .take_while(|&t| t < expiry)
            .chain([expiry])
        {
            segments.push(Segment {
                start: valuation.years_until(previous),
                end: valuation.years_until(end),
                // Keep the segment width independent of rounding in the absolute coordinates.
                width: previous.years_until(end),
                rate: curve.forward_rate(previous)?,
            });
            previous = end;
        }
        let mut from_start = Vec::with_capacity(segments.len());
        let mut log = 0.0;
        for segment in &segments {
            from_start.push(log);
            log -= segment.rate * segment.width;
        }
        let mut to_end = vec![0.0; segments.len()];
        log = 0.0;
        for index in (0..segments.len()).rev() {
            to_end[index] = log;
            log -= segments[index].rate * segments[index].width;
        }
        Ok(Self {
            segments,
            from_start,
            to_end,
        })
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
