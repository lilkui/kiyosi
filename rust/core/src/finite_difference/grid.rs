use super::*;

pub(super) struct SpatialGrid {
    pub upper: f64,
    pub spacing: f64,
    pub count: usize,
    pub scale: f64,
    lower: f64,
    pub nodes: Vec<f64>,
}

impl SpatialGrid {
    pub fn new(
        settings: FiniteDifferenceSettings,
        upper: f64,
        scale: f64,
        level: f64,
    ) -> Result<Self> {
        let mut grid = Self {
            upper,
            spacing: 0.0,
            count: settings.asset_step_count as usize,
            scale,
            lower: 0.0,
            nodes: Vec::new(),
        };
        grid.spacing = grid.coordinate(upper) / grid.count as f64;
        if !upper.is_finite() || upper <= level || !grid.spacing.is_finite() || grid.spacing <= 0.0
        {
            return Err(invalid_grid(
                "finite-difference boundary or spacing is invalid",
            ));
        }
        Ok(grid)
    }

    pub fn barrier_domain(&mut self, lower: f64, upper: f64) -> Result<()> {
        self.count = (((upper - lower) / self.spacing).ceil() as usize).clamp(3, self.count);
        self.uniform_domain(lower, upper)
    }

    pub fn uniform_domain(&mut self, lower: f64, upper: f64) -> Result<()> {
        self.lower = lower;
        self.upper = upper;
        self.spacing = (upper - lower) / self.count as f64;
        let mut previous = lower;
        for index in 1..=self.count {
            let asset = self.asset(index);
            if !asset.is_finite() || asset <= previous {
                return Err(invalid_grid(
                    "barrier grid nodes are not representably distinct",
                ));
            }
            previous = asset;
        }
        Ok(())
    }

    pub fn linear(&self, values: &[f64], asset: f64) -> f64 {
        let (index, weight) = self.locate(asset);
        (1.0 - weight) * values[index] + weight * values[index + 1]
    }

    pub fn coordinate(&self, asset: f64) -> f64 {
        if self.scale == 0.0 {
            return asset - self.lower;
        }
        let ratio = asset / self.scale;
        if ratio.is_finite() {
            ratio.asinh()
        } else {
            asset.ln() - self.scale.ln() + std::f64::consts::LN_2
        }
    }

    pub fn asset(&self, index: usize) -> f64 {
        if !self.nodes.is_empty() {
            return self.nodes[index];
        }
        if index == self.count {
            return self.upper;
        }
        self.asset_of_coordinate(self.spacing * index as f64)
    }

    pub fn asset_of_coordinate(&self, x: f64) -> f64 {
        if self.scale == 0.0 {
            self.lower + x
        } else if x > 700.0 {
            (self.scale.ln() + x - std::f64::consts::LN_2).exp()
        } else {
            self.scale * x.sinh()
        }
    }

    pub fn coefficients(&self, index: usize, rate: f64, dividend: f64, sigma: f64) -> [f64; 3] {
        if !self.nodes.is_empty() {
            let asset = self.nodes[index];
            let left = asset - self.nodes[index - 1];
            let right = self.nodes[index + 1] - asset;
            let total = self.nodes[index + 1] - self.nodes[index - 1];
            let drift = rate - dividend;
            let variance = (sigma * sigma).max(if drift >= 0.0 {
                drift * (right / asset)
            } else {
                -drift * (left / asset)
            });
            let a = (asset / left) * (variance * (asset / total) - drift * (right / total));
            let c = (asset / right) * (variance * (asset / total) + drift * (left / total));
            return [a, -a - c - rate, c];
        }
        let node = index as f64 + self.lower / self.spacing;
        let (drift, scaled_sigma) = if self.scale == 0.0 {
            ((rate - dividend) * node, sigma * node)
        } else {
            let tangent = (node * self.spacing).tanh();
            (
                ((rate - dividend) - 0.5 * sigma * sigma * tangent * tangent) * tangent
                    / self.spacing,
                sigma * tangent / self.spacing,
            )
        };
        // One-sided drift preserves nonnegative off-diagonals at low volatility.
        let diffusion = (0.5 * scaled_sigma * scaled_sigma).max(0.5 * drift.abs());
        [
            diffusion - 0.5 * drift,
            -2.0 * diffusion - rate,
            diffusion + 0.5 * drift,
        ]
    }

    fn locate(&self, spot: f64) -> (usize, f64) {
        if !self.nodes.is_empty() {
            let index = self
                .nodes
                .partition_point(|&n| n <= spot)
                .saturating_sub(1)
                .min(self.count - 1);
            return (
                index,
                (spot - self.nodes[index]) / (self.nodes[index + 1] - self.nodes[index]),
            );
        }
        let position = self.coordinate(spot) / self.spacing;
        let index = position.floor().clamp(0.0, (self.count - 1) as f64) as usize;
        (index, position - index as f64)
    }

    fn width(&self, index: usize) -> f64 {
        if self.nodes.is_empty() {
            1.0
        } else {
            (self.nodes[index + 1] - self.nodes[index]) / self.spacing
        }
    }

    pub fn curvature(&self, values: &[f64]) -> Vec<f64> {
        let mut curvature = vec![0.0; values.len()];
        let mut factors = vec![0.0; values.len()];
        for node in 1..self.count {
            let left = self.width(node - 1);
            let right = self.width(node);
            let total = left + right;
            let a = left / total;
            let diagonal = 2.0 - a * factors[node - 1];
            factors[node] = (right / total) / diagonal;
            curvature[node] = (6.0
                * (((values[node + 1] - values[node]) / right
                    - (values[node] - values[node - 1]) / left)
                    / total)
                - a * curvature[node - 1])
                / diagonal;
        }
        for node in (1..self.count).rev() {
            curvature[node] -= factors[node] * curvature[node + 1];
        }
        curvature
    }

    pub fn sample(&self, values: &[f64], curvature: &[f64], spot: f64) -> [f64; 3] {
        let (index, weight) = self.locate(spot);
        let h = self.width(index);
        let left = 1.0 - weight;
        let lerp = |a: f64, b: f64| a + weight * (b - a);
        let linear = lerp(values[index], values[index + 1]);
        let correction = ((left * left * left - left) * curvature[index]
            + (weight * weight * weight - weight) * curvature[index + 1])
            / 6.0;
        let interpolated = linear + correction * h * h;
        let first_control = values[index] + (values[index + 1] - values[index]) / 3.0
            - (2.0 * curvature[index] + curvature[index + 1]) * h * h / 18.0;
        let second_control = values[index] + 2.0 * (values[index + 1] - values[index]) / 3.0
            - (curvature[index] + 2.0 * curvature[index + 1]) * h * h / 18.0;
        let minimum = values[index].min(values[index + 1]);
        let maximum = values[index].max(values[index + 1]);
        let overshoot = if self.nodes.is_empty() {
            interpolated < minimum || interpolated > maximum
        } else {
            first_control.min(second_control) < minimum
                || first_control.max(second_control) > maximum
        };
        if overshoot {
            let slope = (values[index + 1] - values[index]) / h;
            let before = if index > 0 {
                (values[index] - values[index - 1]) / self.width(index - 1)
            } else {
                slope
            };
            let after = if index + 2 < values.len() {
                (values[index + 2] - values[index + 1]) / self.width(index + 1)
            } else {
                slope
            };
            let first = slope - h * (2.0 * curvature[index] + curvature[index + 1]) / 6.0;
            let last = slope + h * (curvature[index] + 2.0 * curvature[index + 1]) / 6.0;
            let maximum_turn = before > 0.0
                && after < 0.0
                && curvature[index] <= 0.0
                && curvature[index + 1] <= 0.0
                && first <= before
                && last >= after;
            let minimum_turn = before < 0.0
                && after > 0.0
                && curvature[index] >= 0.0
                && curvature[index + 1] >= 0.0
                && first >= before
                && last <= after;
            if !maximum_turn && !minimum_turn {
                if self.nodes.is_empty() {
                    return [interpolated.clamp(minimum, maximum), 0.0, 0.0];
                }
                let global_min = values.iter().copied().fold(f64::INFINITY, f64::min);
                let global_max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if first_control.min(second_control) < global_min
                    || first_control.max(second_control) > global_max
                {
                    return [linear, slope / self.spacing, 0.0];
                }
            }
        }
        let price = if weight == 0.0 {
            values[index]
        } else if weight == 1.0 {
            values[index + 1]
        } else {
            interpolated
        };
        let derivative = ((values[index + 1] - values[index]) / h
            + h * ((1.0 - 3.0 * left * left) * curvature[index]
                + (3.0 * weight * weight - 1.0) * curvature[index + 1])
                / 6.0)
            / self.spacing;
        let second = lerp(curvature[index], curvature[index + 1]) / self.spacing / self.spacing;
        if self.scale == 0.0 {
            [price, derivative, second]
        } else {
            let jacobian = self.scale.hypot(spot);
            [
                price,
                derivative / jacobian,
                (second - derivative * self.coordinate(spot).tanh()) / jacobian / jacobian,
            ]
        }
    }
}

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

pub(super) struct TimeGrid {
    pub times: Vec<f64>,
    pub widths: Vec<f64>,
}

impl TimeGrid {
    pub fn validate_barrier_stability(
        &self,
        space: &SpatialGrid,
        sigma: f64,
        theta: f64,
        scheduled: bool,
        risk: &Curve,
        dividend: &Curve,
    ) -> Result<()> {
        if theta == 0.0 {
            for (index, &dt) in self.widths.iter().enumerate() {
                let middle = self.times[index].midpoint(self.times[index + 1]);
                let rate = risk.rate(middle);
                let q = dividend.rate(middle);
                let decay = if scheduled {
                    let n = space.count as f64;
                    (sigma * sigma * n * n).max((rate - q).abs() * n) + rate
                } else {
                    (1..space.count)
                        .map(|i| -space.coefficients(i, rate, q, sigma)[1])
                        .fold(0.0, f64::max)
                };
                if dt * decay > 1.0 {
                    return Err(invalid_grid("explicit finite-difference grid is unstable"));
                }
            }
        }
        Ok(())
    }

    pub fn new(time: f64, steps: i32, risk: &Curve, dividend: &Curve) -> Self {
        Self::with_events(time, steps, risk, dividend, &[])
    }

    pub fn with_events(
        time: f64,
        steps: i32,
        risk: &Curve,
        dividend: &Curve,
        observations: &[f64],
    ) -> Self {
        let mut events: Vec<_> = risk
            .segments
            .iter()
            .chain(&dividend.segments)
            .map(|&(end, _)| end)
            .collect();
        events.extend_from_slice(observations);
        events.push(0.0);
        events.sort_by(f64::total_cmp);
        events.dedup();
        let mut times = events.clone();
        let tolerance = 4.0 * f64::EPSILON * time;
        for index in 1..steps {
            let node = time * f64::from(index) / f64::from(steps);
            if !events.iter().any(|event| (event - node).abs() <= tolerance) {
                times.push(node);
            }
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        let dt = time / f64::from(steps);
        let mut uniform_index = 1;
        let mut uniform_start = 0.0;
        let mut widths = Vec::with_capacity(times.len() - 1);
        for pair in times.windows(2) {
            let [start, end] = [pair[0], pair[1]];
            let uniform_end = |i| {
                if i == steps {
                    time
                } else {
                    time * f64::from(i) / f64::from(steps)
                }
            };
            while uniform_index < steps && uniform_end(uniform_index) < end {
                uniform_start = uniform_end(uniform_index);
                uniform_index += 1;
            }
            widths.push(
                if start == uniform_start && end == uniform_end(uniform_index) {
                    dt
                } else {
                    end - start
                },
            );
            if end == uniform_end(uniform_index) && uniform_index < steps {
                uniform_start = end;
                uniform_index += 1;
            }
        }
        Self { times, widths }
    }
}
