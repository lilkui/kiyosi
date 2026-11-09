use crate::Result;

pub(super) struct Curve {
    segments: Vec<(f64, f64)>,
    from_start: Vec<f64>,
    to_end: Vec<f64>,
}

impl Curve {
    pub fn ends(&self) -> impl Iterator<Item = f64> + '_ {
        self.segments.iter().map(|&(end, _)| end)
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
            .nodes()
            .iter()
            .copied()
            .filter(|&t| t > valuation && t < expiry)
            .chain([expiry])
        {
            segments.push((valuation.years_until(end), curve.forward_rate(previous)?));
            previous = end;
        }
        let mut from_start = Vec::with_capacity(segments.len());
        let mut previous = 0.0;
        let mut log = 0.0;
        for &(end, rate) in &segments {
            from_start.push(log);
            log -= rate * (end - previous);
            previous = end;
        }
        let mut to_end = vec![0.0; segments.len()];
        log = 0.0;
        for index in (0..segments.len()).rev() {
            to_end[index] = log;
            let previous = if index == 0 {
                0.0
            } else {
                segments[index - 1].0
            };
            log -= segments[index].1 * (segments[index].0 - previous);
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
            .partition_point(|&(end, _)| end <= time)
            .min(self.segments.len() - 1)]
        .1
    }

    #[expect(
        clippy::float_cmp,
        reason = "Exact curve endpoints select cached integrals."
    )]
    pub fn log_factor(&self, start: f64, end: f64) -> f64 {
        let horizon = self.segments[self.segments.len() - 1].0;
        let start = start.max(0.0);
        let end = end.min(horizon);
        if start >= end {
            return 0.0;
        }
        let first = self.segments.partition_point(|&(end, _)| end <= start);
        let last = self.segments.partition_point(|&(next, _)| next < end);
        let mut previous = if first == 0 {
            0.0
        } else {
            self.segments[first - 1].0
        };
        if start == 0.0 && self.from_start[last].is_finite() {
            let previous = if last == 0 {
                0.0
            } else {
                self.segments[last - 1].0
            };
            return self.from_start[last] - self.segments[last].1 * (end - previous);
        }
        if end == horizon && self.to_end[first].is_finite() {
            return -self.segments[first].1 * (self.segments[first].0 - start) + self.to_end[first];
        }
        let mut log = 0.0;
        for &(next, rate) in &self.segments[first..=last] {
            let width = end.min(next) - start.max(previous);
            if width > 0.0 {
                log -= rate * width;
            }
            previous = next;
        }
        log
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
