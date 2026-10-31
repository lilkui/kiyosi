use crate::numerics::{Sum, log_interval_mass, scaled_cash};
const LOG_DENSITY: f64 = -0.9189385332046727;

pub(super) fn scaled(amount: f64, exponent: f64) -> f64 {
    amount.signum() * scaled_cash(amount.abs(), exponent)
}

pub(super) struct TerminalPiece {
    pub lower: f64,
    pub upper: f64,
    pub cash: f64,
    pub asset: f64,
}

pub(super) fn integrate_terminal(
    pieces: &[TerminalPiece],
    log_spot: f64,
    anchor: f64,
    drift: f64,
    deviation: f64,
    discount: f64,
) -> f64 {
    let mut sum = Sum::default();
    let mean = log_spot + drift;
    for piece in pieces {
        let lower = (piece.lower - mean) / deviation;
        let upper = (piece.upper - mean) / deviation;
        if piece.cash != 0.0 {
            sum.add(scaled(
                piece.cash,
                discount + log_interval_mass(lower, upper),
            ));
        }
        if piece.asset != 0.0 {
            sum.add(scaled(
                piece.asset,
                anchor.ln()
                    + mean
                    + 0.5 * deviation * deviation
                    + discount
                    + log_interval_mass(lower - deviation, upper - deviation),
            ));
        }
    }
    sum.total()
}

pub(super) type Polynomial = [f64; 4];

#[derive(Default)]
pub(super) struct Layer {
    pub cells: Vec<Polynomial>,
    pub exact: Vec<TerminalPiece>,
}
impl Layer {
    pub fn evaluate(
        &self,
        nodes: &[f64],
        current: f64,
        anchor: f64,
        drift: f64,
        deviation: f64,
        discount: f64,
    ) -> f64 {
        let mut sum = Sum::default();
        sum.add(integrate_terminal(
            &self.exact,
            current,
            anchor,
            drift,
            deviation,
            discount,
        ));
        if !self.cells.is_empty() {
            sum.add(integrate_curve(
                &self.cells,
                nodes,
                current + drift,
                deviation,
                discount,
            ));
        }
        sum.total()
    }
}

pub(super) struct IntervalMoments {
    mass: f64,
    log_mass: f64,
    conditional: [f64; 3],
}
impl IntervalMoments {
    fn integrate(&self, polynomial: &Polynomial) -> f64 {
        if self.log_mass < -1500.0 {
            return 0.0;
        }
        let mean = polynomial[0]
            + polynomial[1] * self.conditional[0]
            + polynomial[2] * self.conditional[1]
            + polynomial[3] * self.conditional[2];
        if self.mass == 0.0 && mean.abs() < 1e20 && self.log_mass < -800.0 {
            return 0.0;
        }
        if self.mass > 0.0 {
            mean * self.mass
        } else {
            scaled(mean, self.log_mass)
        }
    }
}

// Moments of u=(x-left)/(right-left), conditional on x lying in this cell.
pub(super) fn cell_moments(left: f64, right: f64, mean: f64, deviation: f64) -> IntervalMoments {
    let width = right - left;
    let center = (left.midpoint(right) - mean) / deviation;
    let half = (0.5 * width) / deviation;
    let (log_mass, conditional) = if half > 0.0 && center.abs() * half + 0.5 * half * half < 0.25 {
        // Centered density series avoids subtracting almost equal CDFs and raw moments.
        let mut integrals = [0.0; 4];
        let mut previous = 0.0;
        let mut term = 1.0;
        for order in 0..24 {
            for (power, integral) in integrals.iter_mut().enumerate() {
                if (order + power) % 2 == 0 {
                    *integral += 2.0 * term / (order + power + 1) as f64;
                }
            }
            let next = (-center * half * term - half * half * previous) / (order + 1) as f64;
            previous = term;
            term = next;
        }
        let first = integrals[1] / integrals[0];
        let second = integrals[2] / integrals[0];
        let third = integrals[3] / integrals[0];
        (
            -0.5 * center * center + LOG_DENSITY + half.ln() + integrals[0].ln(),
            [
                0.5 * (1.0 + first),
                0.25 * (1.0 + 2.0 * first + second),
                0.125 * (1.0 + 3.0 * first + 3.0 * second + third),
            ],
        )
    } else {
        let lower = (left - mean) / deviation;
        let upper = (right - mean) / deviation;
        let log_mass = log_interval_mass(lower, upper);
        if log_mass < -1500.0 {
            return IntervalMoments {
                mass: 0.0,
                log_mass,
                conditional: [0.0; 3],
            };
        }
        let weighted_density = |z: f64| {
            if !z.is_finite() {
                return [0.0; 3];
            }
            let density = (-0.5 * z * z + LOG_DENSITY - log_mass).exp();
            [density, z * density, (z * z + 2.0) * density]
        };
        let low = weighted_density(lower);
        let high = weighted_density(upper);
        let first = low[0] - high[0];
        let second = 1.0 + low[1] - high[1];
        let third = low[2] - high[2];
        let offset = (mean - left) / width;
        let scale = deviation / width;
        (
            log_mass,
            [
                offset + scale * first,
                offset * offset + 2.0 * offset * scale * first + scale * scale * second,
                offset * offset * offset
                    + 3.0 * offset * offset * scale * first
                    + 3.0 * offset * scale * scale * second
                    + scale * scale * scale * third,
            ],
        )
    };
    IntervalMoments {
        mass: log_mass.exp(),
        log_mass,
        conditional,
    }
}

pub(super) fn integration_nodes(
    lower: f64,
    upper: f64,
    steps: i32,
    breaks: &[f64],
) -> crate::Result<Vec<f64>> {
    let invalid = || {
        crate::Error::ComputationFailure("quadrature integration grid is not representable".into())
    };
    if !lower.is_finite() || !upper.is_finite() || lower >= upper {
        return Err(invalid());
    }
    let mut nodes = Vec::with_capacity(steps as usize + 1 + breaks.len());
    for index in 0..=steps {
        let t = f64::from(index) / f64::from(steps);
        nodes.push(if lower <= 0.0 && upper >= 0.0 {
            t * upper + (1.0 - t) * lower
        } else if index == steps {
            upper
        } else {
            lower + t * (upper - lower)
        });
    }
    nodes.extend_from_slice(breaks);
    nodes.sort_by(f64::total_cmp);
    // Avoid roundoff-sized cells without collapsing the resolution of narrow grids.
    let minimum_spacing = (64.0 * f64::EPSILON * lower.abs().max(upper.abs()).max(1.0))
        .min(0.5 * (upper - lower) / f64::from(steps));
    nodes.dedup_by(|next, previous| (*next - *previous).abs() <= minimum_spacing);
    if nodes.len() < 4 {
        return Err(invalid());
    }
    Ok(nodes)
}

struct Cell {
    indices: [usize; 4],
    coordinates: [f64; 4],
    divisors: [f64; 6],
}
pub(super) struct CubicInterpolation(Vec<Cell>);
impl CubicInterpolation {
    pub fn new(nodes: &[f64]) -> Self {
        Self(
            (0..nodes.len() - 1)
                .map(|cell| {
                    let first = cell.saturating_sub(1).min(nodes.len() - 4);
                    let mut indices = std::array::from_fn(|k| first + k);
                    let gaps = &nodes[first..first + 4];
                    let minimum_spacing = gaps
                        .windows(2)
                        .map(|pair| pair[1] - pair[0])
                        .fold(0.0_f64, f64::max)
                        * (8.0 * f64::EPSILON.sqrt().sqrt());
                    if gaps
                        .windows(2)
                        .any(|pair| pair[1] - pair[0] < minimum_spacing)
                    {
                        // Closely clustered knots must not determine a wide cell's derivatives.
                        let middle = nodes[cell].midpoint(nodes[cell + 1]);
                        let mut selected = [cell; 4];
                        let mut left = cell.checked_sub(1);
                        let mut right = cell + 1;
                        let mut lower = nodes[cell];
                        let mut upper = lower;
                        let mut complete = true;
                        for slot in selected.iter_mut().skip(1) {
                            while let Some(index) = left {
                                if lower - nodes[index] > minimum_spacing {
                                    break;
                                }
                                left = index.checked_sub(1);
                            }
                            while right < nodes.len() && nodes[right] - upper <= minimum_spacing {
                                right += 1;
                            }
                            if let Some(index) = left.filter(|&index| {
                                right == nodes.len()
                                    || middle - nodes[index] <= nodes[right] - middle
                            }) {
                                *slot = index;
                                lower = nodes[index];
                                left = index.checked_sub(1);
                            } else if right < nodes.len() {
                                *slot = right;
                                upper = nodes[right];
                                right += 1;
                            } else {
                                complete = false;
                                break;
                            }
                        }
                        if complete {
                            selected.sort_unstable();
                            indices = selected;
                        }
                    }
                    let width = nodes[cell + 1] - nodes[cell];
                    let coordinates =
                        std::array::from_fn(|k| (nodes[indices[k]] - nodes[cell]) / width);
                    let mut divisors = [0.0; 6];
                    let mut index = 0;
                    for order in 1..4 {
                        for k in (order..4).rev() {
                            divisors[index] = coordinates[k] - coordinates[k - order];
                            index += 1;
                        }
                    }
                    Cell {
                        indices,
                        coordinates,
                        divisors,
                    }
                })
                .collect(),
        )
    }
    pub fn interpolate(&self, values: &[f64]) -> Vec<Polynomial> {
        self.0
            .iter()
            .map(|stencil| {
                let mut differences: [f64; 4] = std::array::from_fn(|k| values[stencil.indices[k]]);
                let mut index = 0;
                for order in 1..4 {
                    for k in (order..4).rev() {
                        differences[k] =
                            (differences[k] - differences[k - 1]) / stencil.divisors[index];
                        index += 1;
                    }
                }
                let mut polynomial = [differences[3], 0.0, 0.0, 0.0];
                for k in (0..3).rev() {
                    let mut next = [0.0; 4];
                    for power in 0..4 {
                        next[power] -= stencil.coordinates[k] * polynomial[power];
                        if power != 0 {
                            next[power] += polynomial[power - 1];
                        }
                    }
                    next[0] += differences[k];
                    polynomial = next;
                }
                polynomial
            })
            .collect()
    }
}

fn active_cells(nodes: &[f64], mean: f64, deviation: f64) -> std::ops::Range<usize> {
    // Beyond 56 deviations the Gaussian tail is below the -1500 cutoff.
    let radius = 56.0 * deviation;
    let first = nodes
        .partition_point(|&x| x <= mean - radius)
        .saturating_sub(1)
        .min(nodes.len() - 1);
    let last = nodes
        .partition_point(|&x| x < mean + radius)
        .min(nodes.len() - 1);
    first..last
}

pub(super) fn integrate_curve(
    curve: &[Polynomial],
    nodes: &[f64],
    mean: f64,
    deviation: f64,
    discount: f64,
) -> f64 {
    let mut sum = Sum::default();
    let factor = discount.exp();
    for cell in active_cells(nodes, mean, deviation) {
        let value =
            cell_moments(nodes[cell], nodes[cell + 1], mean, deviation).integrate(&curve[cell]);
        sum.add(if factor.is_normal() {
            value * factor
        } else {
            scaled(value, discount)
        });
    }
    sum.total()
}

#[derive(Default)]
pub(super) struct TransitionCache(Vec<(f64, Transition)>);

impl TransitionCache {
    pub fn get(&mut self, time: f64, nodes: &[f64], drift: f64, deviation: f64) -> &Transition {
        let index = if let Some(index) = self.0.iter().position(|(duration, _)| *duration == time) {
            index
        } else {
            // Retain common event gaps without accumulating matrices for irregular schedules.
            if self.0.len() == 3 {
                self.0.remove(0);
            }
            self.0
                .push((time, Transition::new(nodes, drift, deviation)));
            self.0.len() - 1
        };
        &self.0[index].1
    }
}

pub(super) struct Transition {
    moments: Vec<IntervalMoments>,
    rows: Vec<(usize, Vec<usize>)>,
}
impl Transition {
    pub fn new(nodes: &[f64], drift: f64, deviation: f64) -> Self {
        let mut moments = Vec::new();
        let mut geometries = std::collections::HashMap::new();
        let rows = nodes
            .iter()
            .map(|&node| {
                let mean = node + drift;
                let range = active_cells(nodes, mean, deviation);
                let first = range.start;
                let indices = range
                    .map(|cell| {
                        // Reuse only exactly translated cells, including midpoint rounding.
                        let key = [
                            nodes[cell + 1] - nodes[cell],
                            nodes[cell].midpoint(nodes[cell + 1]) - mean,
                            nodes[cell] - mean,
                            nodes[cell + 1] - mean,
                        ]
                        .map(f64::to_bits);
                        *geometries.entry(key).or_insert_with(|| {
                            let index = moments.len();
                            moments.push(cell_moments(
                                nodes[cell],
                                nodes[cell + 1],
                                mean,
                                deviation,
                            ));
                            index
                        })
                    })
                    .collect();
                (first, indices)
            })
            .collect();
        Self { moments, rows }
    }
    pub fn integrate(&self, curve: &[Polynomial], discount: f64, values: &mut [f64]) {
        let factor = discount.exp();
        for (value, (first, indices)) in values.iter_mut().zip(&self.rows) {
            let mut sum = Sum::default();
            for (cell, &index) in indices.iter().enumerate() {
                let value = self.moments[index].integrate(&curve[first + cell]);
                sum.add(if factor.is_normal() {
                    value * factor
                } else {
                    scaled(value, discount)
                });
            }
            *value = sum.total();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_nodes_merge_roundoff_without_collapsing_narrow_grids() {
        let nodes = integration_nodes(-0.2, 0.2, 512, &[0.0, 1.4e-16]).unwrap();
        assert_eq!(nodes.len(), 513);
        assert!(nodes.windows(2).all(|pair| pair[1] - pair[0] > 1e-14));

        let tiny = integration_nodes(-1e-20, 1e-20, 8, &[0.0]).unwrap();
        assert_eq!(tiny.len(), 9);
        assert_eq!(tiny[0], -1e-20);
        assert_eq!(tiny[tiny.len() - 1], 1e-20);
        let values: Vec<_> = tiny.iter().map(|&node| 1.0 + node / 1e-20).collect();
        let curve = CubicInterpolation::new(&tiny).interpolate(&values);
        assert!((integrate_curve(&curve, &tiny, 2e-21, 1e-21, 0.0) - 1.2).abs() < 1e-12);
    }

    #[test]
    fn cubic_stencils_preserve_polynomials_with_clustered_knots() {
        let nodes = [-1.0, -0.5, 0.0, 1e-13, 0.5, 1.0];
        let values: Vec<_> = nodes.iter().map(|&x| 1.0 + x + x * x).collect();
        let curve = CubicInterpolation::new(&nodes).interpolate(&values);
        assert!((integrate_curve(&curve, &nodes, 0.2, 0.1, 0.0) - 1.25).abs() < 1e-12);
    }

    #[test]
    fn transition_cache_bounds_storage_and_preserves_integrals_after_eviction() {
        let nodes = [-1.0, 0.0, 1.0];
        let curve = [[1.0, 0.0, 0.0, 0.0]; 2];
        let mut cache = TransitionCache::default();
        for time in [1.0_f64, 2.0, 3.0, 4.0, 1.0] {
            let deviation = time.sqrt();
            let mut values = [0.0; 3];
            cache
                .get(time, &nodes, 0.0, deviation)
                .integrate(&curve, 0.0, &mut values);
            for (&node, value) in nodes.iter().zip(values) {
                let expected = 0.5
                    * (libm::erf((1.0 - node) / (2.0_f64.sqrt() * deviation))
                        - libm::erf((-1.0 - node) / (2.0_f64.sqrt() * deviation)));
                assert!((value - expected).abs() < 1e-14);
            }
            assert!(cache.0.len() <= 3);
        }
        assert_eq!(
            cache.0.iter().map(|(time, _)| *time).collect::<Vec<_>>(),
            [3.0, 4.0, 1.0]
        );
        let retained = cache.get(1.0, &nodes, 0.0, 1.0) as *const Transition;
        assert!(std::ptr::eq(retained, cache.get(1.0, &nodes, 0.0, 1.0)));
        assert_eq!(cache.0.len(), 3);
    }
}
