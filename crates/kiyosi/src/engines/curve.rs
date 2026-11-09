use crate::Result;

pub(super) struct Curve {
    segments: Vec<(f64, f64)>,
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
        Ok(Self { segments })
    }

    pub fn rate(&self, time: f64) -> f64 {
        self.segments[self
            .segments
            .partition_point(|&(end, _)| end <= time)
            .min(self.segments.len() - 1)]
        .1
    }

    pub fn log_factor(&self, start: f64, end: f64) -> f64 {
        let mut previous: f64 = 0.0;
        let mut log = 0.0;
        for &(next, rate) in &self.segments {
            let width = end.min(next) - start.max(previous);
            if width > 0.0 {
                log -= rate * width;
            }
            previous = next;
        }
        log
    }
}
