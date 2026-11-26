use crate::numerics::{log_ratio, scaled_cash};

use super::{Curve, FiniteDifferenceScheme, FiniteDifferenceSettings, Result, invalid_grid};

pub(super) fn project_in_coordinates<const N: usize>(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64) -> [f64; N],
) -> [f64; N] {
    project_grid_node(space, breaks, index, |spot, _| value(spot))
}

pub(super) fn project_grid_node<const N: usize>(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64, Option<usize>) -> [f64; N],
) -> [f64; N] {
    let center = space.nodes[index];
    if index == 0 || index == space.count {
        return value(space.asset(index), Some(index));
    }
    project_interval(
        [
            space.nodes[index - 1].midpoint(center),
            center,
            center.midpoint(space.nodes[index + 1]),
        ],
        breaks
            .iter()
            .filter(|&&b| b > 0.0)
            .map(|&b| space.coordinate(b)),
        |x| {
            if x.to_bits() == center.to_bits() {
                value(space.asset(index), Some(index))
            } else {
                value(space.asset_of_coordinate(x), None)
            }
        },
    )
}

pub(super) fn project_payoff(
    space: &SpatialGrid,
    breaks: &[f64],
    index: usize,
    value: impl Fn(f64) -> f64,
) -> f64 {
    project_in_coordinates(space, breaks, index, |spot| [value(spot)])[0]
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

// Leave room for the lower tail even in the small-volatility limit.
pub(super) fn spot_floor(spot: f64, level: f64, width: f64) -> f64 {
    let margin = width.max(1e-3);
    let wings = scaled_cash(level, -margin);
    let query = scaled_cash(spot, -0.25 * margin);
    wings.min(query).max(f64::from_bits(1))
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SpatialGrid {
    pub lower: f64,
    pub upper: f64,
    pub spacing: f64,
    pub count: usize,
    pub nodes: Vec<f64>,
    reference: f64,
    pub(super) spots: Vec<f64>,
    stencils: Vec<LogStencil>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LogStencil {
    left: f64,
    right: f64,
    down: f64,
    up: f64,
}

impl LogStencil {
    fn new(left: f64, right: f64) -> Self {
        Self {
            left,
            right,
            down: -(-left).exp_m1(),
            up: right.exp_m1(),
        }
    }

    fn coefficients(self, rate: f64, dividend: f64, sigma: f64) -> [f64; 3] {
        let carry = rate - dividend;
        let variance = sigma * sigma;
        let drift = carry - 0.5 * variance;
        let down_slope = self.down / self.left;
        let up_slope = self.up / self.right;
        let difference = up_slope - down_slope;
        // Fit the log drift and discounted spot moments. Switch to one-sided drift
        // when both moments would require a negative jump rate.
        let (a, c) = if difference > 8.0 * f64::EPSILON && self.up.is_finite() {
            let first =
                (0.5 * variance * up_slope + carry * (1.0 - up_slope)) / difference / self.left;
            let a = first.max(-carry / self.down).max(0.0);
            (a, ((a * self.down + carry) / self.up).max(0.0))
        } else {
            let variance = variance.max((drift * self.right).max(-drift * self.left));
            let width = self.left + self.right;
            (
                (variance - drift * self.right) / width / self.left,
                (variance + drift * self.left) / width / self.right,
            )
        };
        [a, -a - c - rate, c]
    }
}

#[derive(PartialEq)]
pub(super) struct Spline {
    values: Vec<f64>,
    curvature: Vec<f64>,
    bounds: [f64; 2],
    scale: f64,
}

impl SpatialGrid {
    pub fn knock_in_continuation(
        settings: FiniteDifferenceSettings,
        bounds: [f64; 2],
        strike: f64,
        diffusion_width: f64,
    ) -> Result<Self> {
        let mut space = Self::new(settings.asset_step_count, bounds, strike)?;
        let scale = (0.1 * diffusion_width).max(1e-4);
        if settings.scheme != FiniteDifferenceScheme::ExplicitEuler && space.spacing > scale {
            concentrate(&mut space, vec![strike], scale)?;
        }
        Ok(space)
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "Validated positive i32 grid counts fit usize."
    )]
    pub fn new(count: i32, [lower, upper]: [f64; 2], reference: f64) -> Result<Self> {
        if !lower.is_finite() || lower <= 0.0 || !upper.is_finite() || upper <= lower {
            return Err(invalid_grid("finite-difference boundaries are invalid"));
        }
        let reference = reference.clamp(lower, upper);
        let low = log_ratio(lower, reference);
        let high = log_ratio(upper, reference);
        let spacing = (high - low) / f64::from(count);
        if !spacing.is_finite() || spacing <= 0.0 {
            return Err(invalid_grid("finite-difference spacing is invalid"));
        }
        let mut grid = Self {
            lower,
            upper,
            spacing,
            count: count as usize,
            reference,
            nodes: Vec::new(),
            spots: Vec::new(),
            stencils: Vec::new(),
        };
        let mut nodes = Vec::with_capacity(grid.count + 1);
        for i in 0..count {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            nodes.push(low + spacing * f64::from(i));
        }
        nodes.push(high);
        grid.set_nodes(nodes, &[], true)?;
        Ok(grid)
    }

    pub fn resolve_drift_band(&mut self, strike: f64, carry: f64, width: f64) -> Result<()> {
        let left = self.nodes[0];
        let right = self.nodes[self.count];
        let x = self.coordinate(strike);
        let band = [
            (x - carry.max(0.0) - 5.0 * width).max(left),
            (x - carry.min(0.0) + 5.0 * width).min(right),
        ];
        if band[0] >= band[1] {
            return Ok(());
        }
        let tail = self.count.div_ceil(32);
        let left_steps = if band[0] > left { tail } else { 0 };
        let right_steps = if band[1] < right { tail } else { 0 };
        let intervals = [
            (left, band[0], left_steps),
            (band[0], band[1], self.count - left_steps - right_steps),
            (band[1], right, right_steps),
        ];
        let mut nodes = vec![left];
        for (start, end, count) in intervals {
            #[expect(
                clippy::cast_precision_loss,
                reason = "Counts are bounded by validated i32 settings."
            )]
            for i in 1..count {
                if i % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                nodes.push((end - start).mul_add(i as f64 / count as f64, start));
            }
            if count > 0 {
                nodes.push(end);
            }
        }
        self.set_nodes(nodes, &[], false)
    }

    fn set_nodes(&mut self, nodes: Vec<f64>, anchors: &[(f64, f64)], uniform: bool) -> Result<()> {
        let mut spots = Vec::with_capacity(nodes.len());
        spots.push(self.lower);
        for (i, &x) in nodes.iter().enumerate().take(nodes.len() - 1).skip(1) {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let spot = anchors
                .binary_search_by(|a| a.0.total_cmp(&x))
                .map_or_else(|_| self.asset_of_coordinate(x), |index| anchors[index].1);
            spots.push(spot);
        }
        spots.push(self.upper);
        if nodes.len() < 4
            || nodes.windows(2).any(|pair| pair[0] >= pair[1])
            || spots
                .windows(2)
                .any(|pair| !pair[1].is_finite() || pair[0] >= pair[1])
        {
            return Err(invalid_grid(
                "finite-difference grid nodes are not representably distinct",
            ));
        }
        self.stencils = if uniform {
            vec![LogStencil::new(self.spacing, self.spacing)]
        } else {
            let mut stencils = Vec::with_capacity(nodes.len() - 2);
            for (i, cell) in nodes.windows(3).enumerate() {
                if i % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                stencils.push(LogStencil::new(cell[1] - cell[0], cell[2] - cell[1]));
            }
            stencils
        };
        self.count = nodes.len() - 1;
        self.nodes = nodes;
        self.spots = spots;
        Ok(())
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "Grid counts are bounded by validated i32 settings."
    )]
    pub fn resolve_barriers(
        &mut self,
        barriers: [Option<f64>; 2],
        strike: Option<f64>,
        time: f64,
        [risk, dividend]: [&Curve; 2],
        volatility_at: impl Fn(f64) -> f64,
    ) -> Result<()> {
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
        // A single cell across a diffusion width or drift layer smears the payoff.
        let resolved_width = 8.0 * self.spacing;
        let mut scale = f64::INFINITY;
        let mut resolve_strike = false;
        for barrier in barriers.into_iter().flatten() {
            let sigma = volatility_at(barrier);
            let layer = sigma * sigma / carry;
            let width = sigma * time.sqrt();
            let narrow = width < resolved_width;
            if narrow || layer < resolved_width {
                resolve_strike |= narrow;
                scale = scale.min(
                    (if narrow { width } else { 2.0 * width })
                        .min(layer * (self.count as f64 / 32.0)),
                );
            }
        }
        if scale.is_finite() {
            // Refining either edge must keep fine cells at the opposite barrier.
            let mut levels: Vec<_> = barriers.into_iter().flatten().collect();
            if resolve_strike {
                levels.extend(strike.filter(|&level| level > self.lower && level < self.upper));
            }
            concentrate_levels(self, levels, scale.max(f64::EPSILON.sqrt()))?;
        }
        Ok(())
    }

    pub fn linear(&self, values: &[f64], asset: f64) -> f64 {
        let (index, weight) = self.locate(asset);
        (1.0 - weight) * values[index] + weight * values[index + 1]
    }

    pub fn coordinate(&self, asset: f64) -> f64 {
        log_ratio(asset, self.reference)
    }

    pub fn split_cell_at(&self, asset: f64) -> Option<usize> {
        let x = self.coordinate(asset);
        let right = self.nodes.partition_point(|&node| node <= x);
        if right == 0 || right > self.count {
            return None;
        }
        [right - 1, right].into_iter().find(|&i| {
            i > 0
                && i < self.count
                && self.nodes[i - 1].midpoint(self.nodes[i]) < x
                && x < self.nodes[i].midpoint(self.nodes[i + 1])
        })
    }

    pub fn asset(&self, index: usize) -> f64 {
        self.spots[index]
    }

    fn asset_of_coordinate(&self, x: f64) -> f64 {
        if x.abs() < 0.5 {
            self.reference.mul_add(x.exp_m1(), self.reference)
        } else {
            scaled_cash(self.reference, x)
        }
    }

    pub fn coefficients(&self, index: usize, rate: f64, dividend: f64, sigma: f64) -> [f64; 3] {
        self.stencils[if self.is_uniform() { 0 } else { index - 1 }]
            .coefficients(rate, dividend, sigma)
    }

    pub fn is_uniform(&self) -> bool {
        self.stencils.len() == 1
    }

    fn locate(&self, spot: f64) -> (usize, f64) {
        let index = self
            .spots
            .partition_point(|&node| node <= spot)
            .saturating_sub(1)
            .min(self.count - 1);
        let weight = if spot.to_bits() == self.spots[index + 1].to_bits() {
            1.0
        } else if spot.to_bits() == self.spots[index].to_bits() {
            0.0
        } else {
            (self.coordinate(spot) - self.nodes[index])
                / (self.nodes[index + 1] - self.nodes[index])
        };
        (index, weight)
    }

    pub fn nearest(&self, spot: f64) -> usize {
        let (index, weight) = self.locate(spot);
        index + usize::from(weight >= 0.5)
    }

    pub fn exercise_cell(&self, spline: &Spline, spot: f64, payoff: impl Fn(f64) -> f64) -> bool {
        let (i, _) = self.locate(spot);
        [i, i + 1]
            .into_iter()
            .all(|j| spline.values[j].to_bits() == (payoff(self.asset(j)) / spline.scale).to_bits())
    }

    fn width(&self, index: usize) -> f64 {
        (self.nodes[index + 1] - self.nodes[index]) / self.spacing
    }

    pub fn discount_affine_edges(&self, values: &[f64], cash: f64, asset: f64) -> [f64; 2] {
        let edge = |outer: usize, inner: usize| {
            let relative_change = (self.nodes[inner] - self.nodes[outer]).exp_m1();
            values[outer] * cash
                + (values[inner] - values[outer]) / relative_change * (asset - cash)
        };
        [edge(0, 1), edge(self.count, self.count - 1)]
    }

    pub fn spline(&self, mut values: Vec<f64>) -> Spline {
        let mut bounds = values
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], &value| {
                [lo.min(value), hi.max(value)]
            });
        let peak = bounds[0].abs().max(bounds[1].abs());
        let scale = if peak.is_finite() && peak > 1e280 {
            peak
        } else {
            1.0
        };
        if scale > 1.0 {
            for value in &mut values {
                *value /= scale;
            }
            bounds = bounds.map(|value| value / scale);
        }
        let curvature = self.curvature(&values);
        Spline {
            values,
            curvature,
            bounds,
            scale,
        }
    }

    fn curvature(&self, values: &[f64]) -> Vec<f64> {
        let mut curvature = vec![0.0; values.len()];
        let mut factors = vec![0.0; values.len()];
        // The first two and last two cells share a cubic at each endpoint.
        let end_ratio = |outer, inner| {
            let ratio = self.width(outer) / self.width(inner);
            (0.25..=4.0).contains(&ratio).then_some(ratio)
        };
        let first = end_ratio(0, 1);
        let last = end_ratio(self.count - 1, self.count - 2);
        for node in 1..self.count {
            let left = self.width(node - 1);
            let right = self.width(node);
            let total = left + right;
            let mut a = left / total;
            let mut c = right / total;
            let mut diagonal = 2.0;
            if let Some(ratio) = first.filter(|_| node == 1) {
                diagonal += a * (1.0 + ratio);
                c -= a * ratio;
                a = 0.0;
            }
            if let Some(ratio) = last.filter(|_| node == self.count - 1) {
                diagonal += c * (1.0 + ratio);
                a -= c * ratio;
                c = 0.0;
            }
            diagonal -= a * factors[node - 1];
            factors[node] = c / diagonal;
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
        if let Some(ratio) = first {
            curvature[0] = curvature[1] + ratio * (curvature[1] - curvature[2]);
        }
        if let Some(ratio) = last {
            let n = self.count;
            curvature[n] = curvature[n - 1] + ratio * (curvature[n - 1] - curvature[n - 2]);
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

    pub fn sample(&self, spline: &Spline, spot: f64) -> [f64; 3] {
        let [price, first, second] = self.sample_log(spline, spot);
        [price, first / spot, (second - first) / spot / spot]
    }

    #[expect(
        clippy::float_cmp,
        reason = "Exact endpoint weights select one-sided formulas without mixing adjacent cells."
    )]
    pub fn sample_log(&self, spline: &Spline, spot: f64) -> [f64; 3] {
        let Spline {
            values,
            curvature,
            bounds: [global_min, global_max],
            scale,
        } = spline;
        let (index, weight) = self.locate(spot);
        if index > 0 && index + 2 <= self.count {
            let slope = |i| (values[i + 1] - values[i]) / (self.spots[i + 1] - self.spots[i]);
            let slope_here: f64 = slope(index);
            let tolerance = 5e-9 * slope_here.abs();
            if slope_here.is_finite()
                && (slope(index - 1) - slope_here).abs() <= tolerance
                && (slope(index + 1) - slope_here).abs() <= tolerance
            {
                let price = slope_here.mul_add(spot - self.spots[index], values[index]) * scale;
                let derivative = slope_here * spot * scale;
                return [price, derivative, derivative];
            }
        }
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
        let overshoot = if self.is_uniform() {
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
                if self.is_uniform() {
                    return [interpolated.clamp(minimum, maximum) * scale, 0.0, 0.0];
                }
                if first_control.min(second_control) < *global_min
                    || first_control.max(second_control) > *global_max
                {
                    return [linear * scale, slope / self.spacing * scale, 0.0];
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
        let derivative = (values[index + 1] - values[index]) / h
            + h * ((1.0 - 3.0 * left * left) * curvature[index]
                + (3.0 * weight * weight - 1.0) * curvature[index + 1])
                / 6.0;
        let second = lerp(curvature[index], curvature[index + 1]);
        [
            price * scale,
            derivative / self.spacing * scale,
            second / self.spacing / self.spacing * scale,
        ]
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
    let anchors: Vec<_> = levels
        .into_iter()
        .map(|spot| (space.coordinate(spot), spot))
        .collect();
    let mut levels: Vec<_> = anchors.iter().map(|&(x, _)| x).collect();
    let lower = space.nodes[0];
    let upper = space.nodes[space.count];
    let gap = scale * ((upper - lower) / scale).asinh() / space.count as f64;
    let mut previous = lower;
    levels.retain(|&l| {
        if l == lower || l == upper {
            true
        } else if l - previous < gap || upper - l < gap {
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
    if levels[0] > lower {
        segments.push((lower, levels[0], false, 1_usize, 0.0));
    }
    for pair in levels.windows(2) {
        let middle = pair[0].midpoint(pair[1]);
        segments.push((pair[0], middle, true, 1, 0.0));
        segments.push((middle, pair[1], false, 1, 0.0));
    }
    let last = levels[levels.len() - 1];
    if last < upper {
        segments.push((last, upper, true, 1, 0.0));
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
    let mut nodes = vec![lower];
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
    space.set_nodes(nodes, &anchors, false)
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

    pub fn validate_stability(
        &self,
        space: &SpatialGrid,
        sigma: f64,
        theta: f64,
        risk: &Curve,
        dividend: &Curve,
    ) -> Result<()> {
        if theta == 0.0 {
            for (index, &dt) in self.widths.iter().enumerate() {
                crate::cancellation::check()?;
                let middle = self.times[index].midpoint(self.times[index + 1]);
                let rate = risk.rate(middle);
                let q = dividend.rate(middle);
                let end = if space.is_uniform() { 2 } else { space.count };
                for i in 1..end {
                    if i % 1024 == 0 {
                        crate::cancellation::check()?;
                    }
                    if -dt * space.coefficients(i, rate, q, sigma)[1] > 1.0 {
                        return Err(invalid_grid("explicit finite-difference grid is unstable"));
                    }
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
        let space = SpatialGrid::new(800, [50.0, 200.0], 100.0)?;
        let token = crate::CancellationToken::default();
        let result = token.run(|| {
            token.cancel();
            grid.validate_stability(&space, 0.000_001, 0.0, &curve, &curve)
        });
        assert!(
            matches!(result, Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
        );
        Ok(())
    }

    #[test]
    fn nonuniform_spline_uses_global_bounds_for_overshoot() {
        let mut grid = SpatialGrid::new(4, [1.0, 5.0], 1.0).unwrap();
        grid.set_nodes(
            vec![0.0, 2.0_f64.ln(), 2.5_f64.ln(), 4.0_f64.ln(), 5.0_f64.ln()],
            &[],
            false,
        )
        .unwrap();
        let spline = grid.spline(vec![0.0, 0.0, 1.0, 2.0, 3.0]);
        let sample = grid.sample(&spline, 1.5);
        assert!(sample.into_iter().all(|value| value.abs() < f64::EPSILON));
    }

    #[test]
    fn speed_uses_gamma_differences_only_strictly_inside_grid() {
        let grid = SpatialGrid::new(4, [10.0, 20.0], 10.0).unwrap();
        let spline = Spline {
            values: grid.nodes.iter().map(|x| x.powi(3)).collect(),
            curvature: grid
                .nodes
                .iter()
                .map(|x| 6.0 * x * grid.spacing.powi(2))
                .collect(),
            bounds: [0.0, 1.0],
            scale: 1.0,
        };
        let gamma = |spot| {
            let x = log_ratio(spot, 10.0);
            (6.0 * x - 3.0 * x * x) / spot / spot
        };
        for spot in [11.25, 15.0, 18.75] {
            let expected = (gamma(spot + 1.0) - gamma(spot - 1.0)) / 2.0;
            assert!((grid.speed(&spline, spot, 1.0).unwrap() - expected).abs() < 1e-12);
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
