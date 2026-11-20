use crate::numerics::{
    Sum, discounted_product, log_interval_mass, log_narrow_interval_mass, scaled_signed_cash,
};
use crate::{Error, Result, Timestamp};
use std::collections::hash_map::{Entry, HashMap};

const LOG_DENSITY: f64 = -0.918_938_533_204_672_7;
const WORKSPACE_BUDGET: usize = 256 << 20;
// Includes the hash entry and transient capacity growth during construction.
const BYTES_PER_CACHED_MOMENT: usize = 256;

fn workspace_error() -> Error {
    Error::InvalidInput(
        "Gaussian quadrature workspace exceeds 256 MiB; reduce asset_step_count".into(),
    )
}

pub(super) struct TerminalPiece {
    pub lower: f64,
    pub upper: f64,
    pub cash: f64,
    pub asset: f64,
}

#[expect(
    clippy::float_cmp,
    reason = "Only an exact strike cash leg identifies a payoff anchored at zero."
)]
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
    let forward = (0.5 * deviation).mul_add(deviation, mean);
    for piece in pieces {
        let lower = (piece.lower - mean) / deviation;
        let upper = (piece.upper - mean) / deviation;
        let log_mass = || log_interval_mass(lower, upper);
        if deviation < 1e-5
            && forward.abs() < 0.5
            && piece.asset != 0.0
            && piece.cash == -piece.asset * anchor
        {
            // Factor the small forward payoff and integrate the two CDF shifts as narrow strips.
            let strip = |edge: f64| {
                log_narrow_interval_mass(edge - 0.5 * deviation, deviation)
                    .unwrap_or_else(|| log_interval_mass(edge - deviation, edge))
            };
            let asset = -piece.cash;
            sum.add(discounted_product(
                asset.abs(),
                asset.signum() * forward.exp_m1(),
                discount + log_mass(),
            ));
            sum.add(scaled_signed_cash(asset, discount + forward + strip(lower)));
            sum.add(scaled_signed_cash(
                -asset,
                discount + forward + strip(upper),
            ));
            continue;
        }
        if piece.cash != 0.0 {
            sum.add(scaled_signed_cash(piece.cash, discount + log_mass()));
        }
        if piece.asset != 0.0 {
            sum.add(scaled_signed_cash(
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
            scaled_signed_cash(mean, self.log_mass)
        }
    }
}

// Moments of u=(x-left)/(right-left), conditional on x lying in this cell.
#[expect(
    clippy::cast_precision_loss,
    reason = "Series orders are below 28 and convert exactly to f64."
)]
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
                f64::midpoint(1.0, first),
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

#[expect(
    clippy::cast_sign_loss,
    reason = "Callers pass positive grid sizes from validated quadrature settings."
)]
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
    let points = (steps as usize + 1).saturating_add(breaks.len());
    if points.saturating_mul(size_of::<Cell>()) > WORKSPACE_BUDGET {
        return Err(workspace_error());
    }
    let mut nodes = Vec::with_capacity(points);
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

pub(super) fn validate_rollback_work(
    nodes: &[f64],
    drift: f64,
    deviation: f64,
    intervals: i32,
) -> Result<()> {
    let exercise_steps = (intervals.unsigned_abs() - 1) as usize;
    let work = remaining_rollback_work(nodes, 1, exercise_steps)?;
    // The first rollback integrates terminal pieces directly, without interpolated cells.
    let continuation_steps = exercise_steps.saturating_sub(1);
    if continuation_steps > 0 {
        work.checked_sub(
            active_cell_visits(nodes, drift, deviation)?.saturating_mul(continuation_steps),
        )
        .ok_or_else(excessive_rollback_work)?;
    }
    Ok(())
}

fn excessive_rollback_work() -> Error {
    Error::InvalidInput(
        "quadrature rollback exceeds one billion node and cell visits; reduce step counts".into(),
    )
}

fn remaining_rollback_work(nodes: &[f64], layers: usize, visits: usize) -> Result<usize> {
    const WORK_BUDGET: usize = 1_000_000_000;
    WORK_BUDGET
        .checked_sub(nodes.len().saturating_mul(layers).saturating_mul(visits))
        .ok_or_else(excessive_rollback_work)
}

fn active_cell_visits(nodes: &[f64], drift: f64, deviation: f64) -> Result<usize> {
    nodes.iter().try_fold(0_usize, |count, &node| {
        crate::cancellation::check()?;
        Ok(count.saturating_add(active_cells(nodes, node + drift, deviation).len()))
    })
}

pub(super) fn validate_scheduled_work(
    nodes: &[f64],
    dates: &[Timestamp],
    layers: usize,
    terminal_pieces: usize,
    transition: impl Fn(f64) -> Result<(f64, f64, f64)>,
) -> Result<()> {
    let visits = dates
        .len()
        .saturating_add(terminal_pieces.saturating_sub(1));
    let mut work = remaining_rollback_work(nodes, layers, visits)?;
    let mut last = None;
    // A terminal payoff with exact pieces does not integrate interpolated cells.
    let intervals = dates
        .len()
        .saturating_sub(1 + usize::from(terminal_pieces > 0));
    for pair in dates.windows(2).take(intervals) {
        crate::cancellation::check()?;
        let time = pair[0].years_until(pair[1]);
        let key = time.to_bits();
        let cells = if let Some((_, cells)) = last.filter(|&(previous, _)| previous == key) {
            cells
        } else {
            let (drift, deviation, _) = transition(time)?;
            let cells = active_cell_visits(nodes, drift, deviation)?;
            last = Some((key, cells));
            cells
        };
        work = work
            .checked_sub(cells.saturating_mul(layers))
            .ok_or_else(excessive_rollback_work)?;
    }
    Ok(())
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
            scaled_signed_cash(value, discount)
        });
    }
    sum.total()
}

#[derive(Default)]
pub(super) struct TransitionCache(Vec<(f64, Transition)>);

impl TransitionCache {
    #[expect(
        clippy::float_cmp,
        reason = "Transition cache durations must match exactly before reusing an integration matrix."
    )]
    pub fn get(
        &mut self,
        time: f64,
        nodes: &[f64],
        drift: f64,
        deviation: f64,
    ) -> crate::Result<&Transition> {
        let index = if let Some(index) = self.0.iter().position(|(duration, _)| *duration == time) {
            index
        } else {
            // Retain common event gaps without accumulating matrices for irregular schedules.
            if self.0.len() == 3 {
                self.0.remove(0);
            }
            let row_bytes = Transition::row_bytes(nodes, drift, deviation)?;
            let mut retained = self
                .0
                .iter()
                .map(|(_, matrix)| matrix.storage_bytes())
                .sum::<usize>();
            while retained + row_bytes > WORKSPACE_BUDGET {
                retained -= self.0.remove(0).1.storage_bytes();
            }
            let max_moments = (WORKSPACE_BUDGET - retained - row_bytes) / BYTES_PER_CACHED_MOMENT;
            self.0
                .push((time, Transition::new(nodes, drift, deviation, max_moments)?));
            self.0.len() - 1
        };
        Ok(&self.0[index].1)
    }
}

type Row = (usize, Vec<usize>);

pub(super) struct Transition {
    moments: Vec<IntervalMoments>,
    rows: Vec<Row>,
    drift: f64,
    deviation: f64,
}
impl Transition {
    fn storage_bytes(&self) -> usize {
        self.rows.capacity() * size_of::<Row>()
            + self.moments.capacity() * size_of::<IntervalMoments>()
            + self
                .rows
                .iter()
                .map(|(_, indices)| indices.capacity() * size_of::<usize>())
                .sum::<usize>()
    }

    fn row_bytes(nodes: &[f64], drift: f64, deviation: f64) -> Result<usize> {
        nodes.iter().try_fold(
            nodes.len().saturating_mul(size_of::<Row>()),
            |bytes, &node| {
                crate::cancellation::check()?;
                let cells = active_cells(nodes, node + drift, deviation).len();
                let bytes = bytes.saturating_add(cells.saturating_mul(size_of::<usize>()));
                (bytes <= WORKSPACE_BUDGET)
                    .then_some(bytes)
                    .ok_or_else(workspace_error)
            },
        )
    }

    fn new(nodes: &[f64], drift: f64, deviation: f64, max_moments: usize) -> Result<Self> {
        let mut moments = Vec::new();
        let mut geometries = HashMap::new();
        let mut rows = Vec::with_capacity(nodes.len());
        for &node in nodes {
            crate::cancellation::check()?;
            let mean = node + drift;
            let range = active_cells(nodes, mean, deviation);
            let first = range.start;
            let mut indices = Vec::with_capacity(range.len());
            for cell in range {
                // Reuse only exactly translated cells, including midpoint rounding.
                let key = [
                    nodes[cell + 1] - nodes[cell],
                    nodes[cell].midpoint(nodes[cell + 1]) - mean,
                    nodes[cell] - mean,
                    nodes[cell + 1] - mean,
                ]
                .map(f64::to_bits);
                let index = match geometries.entry(key) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) if moments.len() < max_moments => {
                        let index = moments.len();
                        moments.push(cell_moments(nodes[cell], nodes[cell + 1], mean, deviation));
                        *entry.insert(index)
                    }
                    // Keep the grid resolution when there is no room to cache another moment.
                    Entry::Vacant(_) => usize::MAX,
                };
                indices.push(index);
            }
            rows.push((first, indices));
        }
        Ok(Self {
            moments,
            rows,
            drift,
            deviation,
        })
    }
    pub fn integrate(
        &self,
        nodes: &[f64],
        curve: &[Polynomial],
        discount: f64,
        values: &mut [f64],
    ) -> crate::Result<()> {
        let factor = discount.exp();
        for ((value, (first, indices)), &node) in values.iter_mut().zip(&self.rows).zip(nodes) {
            crate::cancellation::check()?;
            let mut sum = Sum::default();
            for (cell, &index) in indices.iter().enumerate() {
                let cell = first + cell;
                let value = if index == usize::MAX {
                    cell_moments(
                        nodes[cell],
                        nodes[cell + 1],
                        node + self.drift,
                        self.deviation,
                    )
                    .integrate(&curve[cell])
                } else {
                    self.moments[index].integrate(&curve[cell])
                };
                sum.add(if factor.is_normal() {
                    value * factor
                } else {
                    scaled_signed_cash(value, discount)
                });
            }
            *value = sum.total();
        }
        Ok(())
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
    fn transition_integrates_when_there_is_no_room_to_cache_moments() -> Result<()> {
        let nodes = [-1.0, 0.0, 1.0];
        let curve = [[1.0, 0.0, 0.0, 0.0]; 2];
        let transition = Transition::new(&nodes, 0.25, 0.5, 0)?;
        let mut values = [0.0; 3];
        transition.integrate(&nodes, &curve, -0.05, &mut values)?;
        for (&node, actual) in nodes.iter().zip(values) {
            let expected = integrate_curve(&curve, &nodes, node + 0.25, 0.5, -0.05);
            assert!((actual - expected).abs() < 1e-13);
        }
        Ok(())
    }

    #[test]
    fn transitions_match_normal_probabilities_across_repeated_intervals() {
        let nodes = [-1.0, 0.0, 1.0];
        let curve = [[1.0, 0.0, 0.0, 0.0]; 2];
        let mut cache = TransitionCache::default();
        for time in [1.0_f64, 2.0, 3.0, 4.0, 1.0] {
            let deviation = time.sqrt();
            let mut values = [0.0; 3];
            cache
                .get(time, &nodes, 0.0, deviation)
                .unwrap()
                .integrate(&nodes, &curve, 0.0, &mut values)
                .unwrap();
            for (&node, value) in nodes.iter().zip(values) {
                let expected = 0.5
                    * (libm::erf((1.0 - node) / (2.0_f64.sqrt() * deviation))
                        - libm::erf((-1.0 - node) / (2.0_f64.sqrt() * deviation)));
                assert!((value - expected).abs() < 1e-14);
            }
        }
    }
}
