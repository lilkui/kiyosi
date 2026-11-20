use super::{Curve, FiniteDifferenceScheme, FiniteDifferenceSettings, Result, invalid_grid};

pub(super) fn project_in_coordinates(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64) -> [f64; 2],
) -> [f64; 2] {
    let value_at = |x| value(space.asset_of_coordinate(x));
    let center = space.coordinate(space.asset(index));
    if index == 0 || index == space.count {
        return value_at(center);
    }
    project_interval(
        [
            space.coordinate(space.asset(index - 1)).midpoint(center),
            center,
            center.midpoint(space.coordinate(space.asset(index + 1))),
        ],
        breaks.iter().map(|&b| space.coordinate(b)),
        value_at,
    )
}

pub(super) fn project_in_spot(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64) -> f64,
) -> f64 {
    let center = space.asset(index);
    if index == 0 || index == space.count {
        return value(center);
    }
    project_interval(
        [
            space.asset(index - 1).midpoint(center),
            center,
            space.asset(index + 1).midpoint(center),
        ],
        breaks.iter().copied(),
        |spot| [value(spot)],
    )[0]
}

fn project_interval<const N: usize>(
    [lower, center, upper]: [f64; 3],
    breaks: impl Iterator<Item = f64>,
    value: impl Fn(f64) -> [f64; N],
) -> [f64; N] {
    let mut points: Vec<_> = breaks.filter(|&v| v > lower && v < upper).collect();
    if points.is_empty() {
        return value(center);
    }
    points.extend([lower, center, upper]);
    points.sort_by(f64::total_cmp);
    points.dedup();
    let mut result = [0.0; N];
    for pair in points.windows(2) {
        let middle = pair[0].midpoint(pair[1]);
        let radius = (pair[1] - pair[0]) / (2.0 * 3.0_f64.sqrt());
        let low = value(middle - radius);
        let high = value(middle + radius);
        for layer in 0..N {
            result[layer] +=
                (pair[1] - pair[0]) / (upper - lower) * low[layer].midpoint(high[layer]);
        }
    }
    result
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SpatialGrid {
    pub upper: f64,
    pub spacing: f64,
    pub count: usize,
    pub scale: f64,
    lower: f64,
    pub nodes: Vec<f64>,
}

#[derive(PartialEq)]
pub(super) struct Spline {
    values: Vec<f64>,
    curvature: Vec<f64>,
    bounds: [f64; 2],
}

impl SpatialGrid {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "Validated positive i32 grid counts fit usize and are exactly representable in f64."
    )]
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

    #[expect(
        clippy::cast_precision_loss,
        reason = "Grid counts are bounded by validated i32 settings and fit exactly in f64."
    )]
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

    #[expect(
        clippy::cast_precision_loss,
        reason = "Grid counts are bounded by validated i32 settings."
    )]
    pub fn resolve_barrier(
        &mut self,
        up: bool,
        strike: Option<f64>,
        sigma: f64,
        time: f64,
        risk: &Curve,
        dividend: &Curve,
    ) -> Result<()> {
        let barrier = if up { self.upper } else { self.lower };
        let variance = sigma * sigma;
        let mut carry = 0.0_f64;
        for (index, at) in std::iter::once(0.0)
            .chain(risk.ends())
            .chain(dividend.ends())
            .enumerate()
        {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
            carry = carry.max((risk.rate(at) - dividend.rate(at)).abs());
        }
        let layer = (variance / carry) * barrier;
        let width = (sigma * time.sqrt()) * barrier;
        let narrow = width < self.spacing;
        if narrow || layer < self.spacing {
            let mut levels = vec![barrier];
            if narrow {
                // Short diffusion widths need fine cells at the payoff kink as well as the barrier.
                levels.extend(strike.filter(|&level| level > self.lower && level < self.upper));
            }
            let scale = (if narrow { width } else { 2.0 * width })
                .min(layer * (self.count as f64 / 32.0))
                .max(f64::EPSILON.sqrt() * self.upper);
            concentrate_levels(self, levels, scale)?;
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

    #[expect(
        clippy::cast_precision_loss,
        reason = "Grid indices are bounded by validated i32 settings and fit exactly in f64."
    )]
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

    #[expect(
        clippy::cast_precision_loss,
        reason = "Grid indices are bounded by validated i32 settings and fit exactly in f64."
    )]
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

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "The floored index is clamped to the i32-bounded grid; its conversion to f64 is exact."
    )]
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

    pub fn spline(&self, values: Vec<f64>) -> Spline {
        let bounds = if self.nodes.is_empty() {
            [f64::NEG_INFINITY, f64::INFINITY]
        } else {
            values
                .iter()
                .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], &value| {
                    [lo.min(value), hi.max(value)]
                })
        };
        let curvature = self.curvature(&values);
        Spline {
            values,
            curvature,
            bounds,
        }
    }

    fn curvature(&self, values: &[f64]) -> Vec<f64> {
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

    pub fn speed(&self, spline: &Spline, spot: f64, shift: f64) -> Option<f64> {
        if shift > 0.0 && spot - shift > self.asset(0) && spot + shift < self.upper {
            Some(
                (self.sample(spline, spot + shift)[2] - self.sample(spline, spot - shift)[2])
                    / (2.0 * shift),
            )
        } else {
            None
        }
    }

    #[expect(
        clippy::float_cmp,
        reason = "Exact endpoint weights select one-sided formulas without mixing adjacent cells."
    )]
    pub fn sample(&self, spline: &Spline, spot: f64) -> [f64; 3] {
        let Spline {
            values,
            curvature,
            bounds: [global_min, global_max],
        } = spline;
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
                if first_control.min(second_control) < *global_min
                    || first_control.max(second_control) > *global_max
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

pub(super) fn concentrate(space: &mut SpatialGrid, mut levels: Vec<f64>, scale: f64) -> Result<()> {
    levels.retain(|&l| l > space.lower && l < space.upper);
    concentrate_levels(space, levels, scale)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_cmp,
    reason = "Validated counts fit exactly; allocation floors proportions and exact endpoints select segment geometry."
)]
fn concentrate_levels(space: &mut SpatialGrid, mut levels: Vec<f64>, scale: f64) -> Result<()> {
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let gap = scale * ((space.upper - space.lower) / scale).asinh() / space.count as f64;
    let mut previous = space.lower;
    levels.retain(|&l| {
        if l == space.lower || l == space.upper {
            true
        } else if l - previous < gap || space.upper - l < gap {
            false
        } else {
            previous = l;
            true
        }
    });
    if levels.is_empty() {
        return Ok(());
    }
    let mut segments = Vec::new();
    if levels[0] > space.lower {
        segments.push((space.lower, levels[0], false, 1_usize, 0.0));
    }
    for pair in levels.windows(2) {
        let middle = pair[0].midpoint(pair[1]);
        segments.push((pair[0], middle, true, 1, 0.0));
        segments.push((middle, pair[1], false, 1, 0.0));
    }
    let last = levels[levels.len() - 1];
    if last < space.upper {
        segments.push((last, space.upper, true, 1, 0.0));
    }
    if segments.len() > space.count {
        return Ok(());
    }
    let width = |a: f64, b: f64| ((b - a) / scale).asinh();
    let total: f64 = segments.iter().map(|&(a, b, _, _, _)| width(a, b)).sum();
    let available = space.count - segments.len();
    let mut allocated = 0;
    for (a, b, _, steps, remainder) in &mut segments {
        let count = available as f64 * width(*a, *b) / total;
        *steps += count as usize;
        *remainder = count.fract();
        allocated += count as usize;
    }
    for _ in allocated..available {
        let mut best = 0;
        for i in 1..segments.len() {
            if segments[i].4 > segments[best].4 {
                best = i;
            }
        }
        segments[best].3 += 1;
        segments[best].4 = -1.0;
    }
    let mut spacing = total / space.count as f64;
    for &(a, b, _, steps, _) in &segments {
        if steps > 1 {
            spacing = spacing.min(width(a, b) / (steps as f64 - 0.5));
        }
    }
    let mut nodes = vec![space.lower];
    for (a, b, from_left, steps, _) in segments {
        for i in 1..=steps {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let x = spacing * if from_left { i } else { steps - i } as f64;
            let value = if i == steps {
                b
            } else if from_left {
                a + scale * x.sinh()
            } else {
                b - scale * x.sinh()
            };
            if value > nodes[nodes.len() - 1] {
                nodes.push(value);
            }
        }
    }
    if nodes.iter().any(|n| !n.is_finite()) {
        return Err(invalid_grid("concentrated grid is not representable"));
    }
    space.count = nodes.len() - 1;
    space.spacing = 1.0;
    space.scale = 0.0;
    space.nodes = nodes;
    Ok(())
}

pub(super) struct TimeGrid {
    pub times: Vec<f64>,
    pub widths: Vec<f64>,
}

impl TimeGrid {
    pub fn rollback_steps(
        &self,
        scheme: FiniteDifferenceScheme,
        sorted_observations: &[f64],
    ) -> Result<u64> {
        let intervals = self.widths.len();
        if scheme != FiniteDifferenceScheme::Rannacher {
            return Ok(intervals as u64);
        }
        let mut first_damped = intervals.saturating_sub(2);
        let mut extra = intervals - first_damped;
        for (i, time) in sorted_observations.iter().rev().enumerate() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            if let Ok(index) = self.times.binary_search_by(|node| node.total_cmp(time)) {
                // Each event damps the preceding two intervals. Count their union.
                let start = index.saturating_sub(2);
                extra += index.min(first_damped).saturating_sub(start);
                first_damped = first_damped.min(start);
            }
        }
        Ok(intervals as u64 + extra as u64)
    }

    pub fn substeps(
        &self,
        step: usize,
        scheme: FiniteDifferenceScheme,
        damping: &mut u32,
    ) -> impl Iterator<Item = (f64, f64, f64)> {
        let damp = scheme == FiniteDifferenceScheme::Rannacher && *damping > 0;
        *damping = damping.saturating_sub(1);
        let count = if damp { 2 } else { 1 };
        let dt = self.widths[step] / f64::from(count);
        let weight = if damp { 1.0 } else { scheme.theta() };
        let start = self.times[step];
        (0..count).map(move |substep| {
            let current = start + dt * f64::from(count - substep - 1);
            (current, dt, weight)
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "Grid counts are bounded by validated i32 settings and fit exactly in f64."
    )]
    pub fn validate_stability(
        &self,
        space: &SpatialGrid,
        sigma: f64,
        theta: f64,
        uniform: bool,
        risk: &Curve,
        dividend: &Curve,
    ) -> Result<()> {
        if theta == 0.0 {
            for (index, &dt) in self.widths.iter().enumerate() {
                crate::cancellation::check()?;
                let middle = self.times[index].midpoint(self.times[index + 1]);
                let rate = risk.rate(middle);
                let q = dividend.rate(middle);
                let decay = if uniform {
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

    #[expect(
        clippy::float_cmp,
        reason = "Exact uniform-grid endpoints identify reusable step widths without merging nearby events."
    )]
    pub fn with_events(
        time: f64,
        steps: i32,
        risk: &Curve,
        dividend: &Curve,
        observations: &[f64],
        validate_intervals: impl Fn(u64) -> Result<()>,
    ) -> Result<Self> {
        crate::cancellation::check()?;
        let mut events: Vec<_> = risk.ends().chain(dividend.ends()).collect();
        events.extend_from_slice(observations);
        events.push(0.0);
        events.sort_by(f64::total_cmp);
        events.dedup();
        let tolerance = 4.0 * f64::EPSILON * time;
        let uniform_node = |index| {
            let node = time * f64::from(index) / f64::from(steps);
            let next = events.partition_point(|&event| event < node);
            (!events[next.saturating_sub(1)..(next + 1).min(events.len())]
                .iter()
                .any(|event| (event - node).abs() <= tolerance))
            .then_some(node)
        };
        let uniform_nodes = || {
            (1..steps).map(|index| -> Result<Option<f64>> {
                if index % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                Ok(uniform_node(index))
            })
        };
        let event_intervals = events.len() - 1;
        let mut intervals = event_intervals + steps.unsigned_abs() as usize - 1;
        if validate_intervals(intervals as u64).is_err() {
            // Only count the exact grid near the limit, where events may replace uniform nodes.
            intervals = event_intervals;
            validate_intervals(intervals as u64)?;
            for node in uniform_nodes() {
                if node?.is_some() {
                    intervals += 1;
                    validate_intervals(intervals as u64)?;
                }
            }
        }
        let mut times = Vec::with_capacity(intervals + 1);
        times.extend_from_slice(&events);
        for node in uniform_nodes() {
            times.extend(node?);
        }
        drop(events);
        crate::cancellation::check()?;
        times.sort_by(f64::total_cmp);
        times.dedup();
        let dt = time / f64::from(steps);
        let mut uniform_index = 1;
        let mut uniform_start = 0.0;
        let mut widths = Vec::with_capacity(times.len() - 1);
        for (index, pair) in times.windows(2).enumerate() {
            if index % 1024 == 0 {
                crate::cancellation::check()?;
            }
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
        Ok(Self { times, widths })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_integrates_split_polynomials_and_payoffs_but_samples_smooth_cells() {
        let cell = [0.0, 0.5, 1.0];
        assert_eq!(project_interval(cell, [].into_iter(), |x| [x * x]), [0.25]);
        let moments = project_interval(cell, [0.25].into_iter(), |x| [x * x, x * x * x]);
        assert!((moments[0] - 1.0 / 3.0).abs() < 1e-15);
        assert!((moments[1] - 0.25).abs() < 1e-15);
        let payoff = project_interval(cell, [1.0, 0.75, -1.0, 0.25, 0.25].into_iter(), |x| {
            if x < 0.25 {
                [2.0, 0.0]
            } else if x < 0.75 {
                [0.0, 3.0]
            } else {
                [4.0, 0.0]
            }
        });
        assert_eq!(payoff, [1.5, 1.5]);
    }

    #[test]
    fn time_grid_preserves_events_and_only_replaces_uniform_nodes_within_tolerance() {
        let start = crate::Date::new(2025, 1, 1).unwrap().start_of_day();
        let end = start.add_days(365).unwrap();
        let curve = Curve::new(
            &crate::RateTermStructure::flat(0.0, None).unwrap(),
            start,
            end,
        )
        .unwrap();
        let events = [
            0.25 - 4.0 * f64::EPSILON,
            0.5 + 4.0 * f64::EPSILON,
            0.75 + 8.0 * f64::EPSILON,
        ];
        let validate = |limit| {
            move |steps| {
                if steps > limit {
                    return Err(invalid_grid("test workspace limit"));
                }
                Ok(())
            }
        };
        let grid = TimeGrid::with_events(1.0, 4, &curve, &curve, &events, validate(5)).unwrap();
        assert_eq!(
            grid.times,
            vec![0.0, events[0], events[1], 0.75, events[2], 1.0]
        );
        assert!(grid.widths.iter().all(|&width| width > 0.0));
        assert!((grid.widths.iter().sum::<f64>() - 1.0).abs() < f64::EPSILON);
        assert!(matches!(
            TimeGrid::with_events(1.0, 4, &curve, &curve, &events, validate(4)),
            Err(crate::Error::InvalidInput(message)) if message == "test workspace limit"
        ));
    }

    #[test]
    fn time_grid_construction_returns_interrupted_for_a_cancelled_request() -> Result<()> {
        let start = crate::Date::new(2025, 1, 1)?.start_of_day();
        let curve = Curve::new(
            &crate::RateTermStructure::flat(0.0, None)?,
            start,
            start.add_days(365)?,
        )?;
        let token = crate::CancellationToken::default();
        let result = token.run(|| {
            token.cancel();
            let grid = TimeGrid::with_events(1.0, 4, &curve, &curve, &[], |_| Ok(()));
            assert!(matches!(grid, Err(crate::Error::Io(ref error)) if error.kind() == std::io::ErrorKind::Interrupted));
            grid
        });
        assert!(
            matches!(result, Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        Ok(())
    }

    #[test]
    fn explicit_stability_scan_observes_cancellation() -> Result<()> {
        let start = crate::Date::new(2025, 1, 1)?.start_of_day();
        let curve = Curve::new(
            &crate::RateTermStructure::flat(0.0, None)?,
            start,
            start.add_days(365)?,
        )?;
        let grid = TimeGrid {
            times: vec![0.0, 1.0],
            widths: vec![1.0],
        };
        let space = SpatialGrid::new(FiniteDifferenceSettings::default(), 200.0, 100.0, 100.0)?;
        let token = crate::CancellationToken::default();
        let result = token.run(|| {
            token.cancel();
            grid.validate_stability(&space, 0.000_001, 0.0, false, &curve, &curve)
        });
        assert!(
            matches!(result, Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        Ok(())
    }

    #[test]
    fn nonuniform_spline_uses_global_bounds_for_overshoot() {
        let mut grid = SpatialGrid::new(
            FiniteDifferenceSettings {
                asset_step_count: 4,
                ..Default::default()
            },
            4.0,
            0.0,
            0.0,
        )
        .unwrap();
        grid.nodes = vec![0.0, 1.0, 1.5, 3.0, 4.0];
        let spline = grid.spline(vec![0.0, 0.0, 1.0, 2.0, 3.0]);
        let sample = grid.sample(&spline, 0.5);
        assert!(sample.into_iter().all(|value| value.abs() < f64::EPSILON));
    }

    #[test]
    fn speed_uses_gamma_differences_only_strictly_inside_grid() {
        let mut grid = SpatialGrid::new(
            FiniteDifferenceSettings {
                asset_step_count: 4,
                ..Default::default()
            },
            20.0,
            0.0,
            0.0,
        )
        .unwrap();
        grid.uniform_domain(10.0, 20.0).unwrap();
        let spline = Spline {
            values: (0..=grid.count).map(|i| grid.asset(i).powi(3)).collect(),
            curvature: (0..=grid.count)
                .map(|i| 6.0 * grid.asset(i) * grid.spacing.powi(2))
                .collect(),
            bounds: [1_000.0, 8_000.0],
        };
        for spot in [11.25, 15.0, 18.75] {
            assert!((grid.speed(&spline, spot, 1.0).unwrap() - 6.0).abs() < 1e-12);
        }
        for (spot, shift) in [
            (11.0, 1.0),
            (19.0, 1.0),
            (10.5, 1.0),
            (19.5, 1.0),
            (15.0, 0.0),
            (15.0, -1.0),
            (15.0, f64::NAN),
            (15.0, f64::INFINITY),
        ] {
            assert_eq!(grid.speed(&spline, spot, shift), None);
        }
    }

    #[test]
    fn rannacher_substeps_damp_two_intervals_and_restart_after_events() {
        let grid = TimeGrid {
            times: vec![0.0, 0.25, 0.5, 1.0],
            widths: vec![0.25, 0.25, 0.5],
        };
        assert_eq!(
            grid.rollback_steps(FiniteDifferenceScheme::Rannacher, &[])
                .unwrap(),
            5
        );
        assert_eq!(
            grid.rollback_steps(FiniteDifferenceScheme::Rannacher, &[0.25, 0.5, 0.5, 1.0])
                .unwrap(),
            6
        );
        let mut damping = 2;
        assert_eq!(
            grid.substeps(2, FiniteDifferenceScheme::Rannacher, &mut damping)
                .collect::<Vec<_>>(),
            vec![(0.75, 0.25, 1.0), (0.5, 0.25, 1.0)]
        );
        assert_eq!(
            grid.substeps(1, FiniteDifferenceScheme::Rannacher, &mut damping)
                .collect::<Vec<_>>(),
            vec![(0.375, 0.125, 1.0), (0.25, 0.125, 1.0)]
        );
        assert_eq!(
            grid.substeps(0, FiniteDifferenceScheme::Rannacher, &mut damping)
                .collect::<Vec<_>>(),
            vec![(0.0, 0.25, 0.5)]
        );
        damping = 2;
        assert_eq!(
            grid.substeps(0, FiniteDifferenceScheme::Rannacher, &mut damping)
                .collect::<Vec<_>>(),
            vec![(0.125, 0.125, 1.0), (0.0, 0.125, 1.0)]
        );
    }

    #[test]
    fn undamped_schemes_keep_intervals_whole_even_after_events() {
        let grid = TimeGrid {
            times: vec![0.0, 0.25, 0.5, 1.0],
            widths: vec![0.25, 0.25, 0.5],
        };
        for (scheme, theta) in [
            (FiniteDifferenceScheme::ExplicitEuler, 0.0),
            (FiniteDifferenceScheme::ImplicitEuler, 1.0),
            (FiniteDifferenceScheme::CrankNicolson, 0.5),
        ] {
            let mut damping = 2;
            for step in (0..3).rev() {
                assert_eq!(
                    grid.substeps(step, scheme, &mut damping)
                        .collect::<Vec<_>>(),
                    vec![(grid.times[step], grid.widths[step], theta)]
                );
                damping = 2;
            }
        }
    }
}
