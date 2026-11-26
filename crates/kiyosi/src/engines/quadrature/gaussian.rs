use crate::numerics::{
    Sum, discounted_product, log_interval_mass, log_narrow_interval_mass, scaled_signed_cash,
};
use crate::{Error, Result, Timestamp};
use rustfft::{Fft, FftPlanner, num_complex::Complex64};
use std::{
    cell::{Cell as Counter, RefCell},
    collections::hash_map::{Entry, HashMap},
    ops::Range,
    rc::{Rc, Weak},
    sync::Arc,
};

const LOG_DENSITY: f64 = -0.918_938_533_204_672_7;
const LOG_HALF_SUBNORMAL: f64 = -745.133_219_101_941_2;
const WORKSPACE_BUDGET: usize = 256 << 20;
const WORK_BUDGET: usize = 1_000_000_000;
// Includes the hash entry and transient capacity growth during construction.
const BYTES_PER_CACHED_MOMENT: usize = 256;
const MAX_TRANSITIONS: usize = 8;
const MAX_PLANNED_GAPS: usize = 64;

fn workspace_error() -> Error {
    Error::InvalidInput(
        "Gaussian quadrature workspace exceeds 256 MiB; reduce asset_step_count".into(),
    )
}

#[derive(PartialEq)]
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

pub(super) struct CubicPiece {
    pub bounds: [f64; 2],
    pub coefficients: Polynomial,
}

impl CubicPiece {
    pub fn restricted(
        bounds: [f64; 2],
        [left, right]: [f64; 2],
        [constant, linear, quadratic, cubic]: Polynomial,
    ) -> Self {
        let width = right - left;
        let offset = (bounds[0] - left) / width;
        let ratio = (bounds[1] - bounds[0]) / width;
        Self {
            bounds,
            coefficients: [
                constant + offset * (linear + offset * (quadratic + offset * cubic)),
                ratio * (linear + offset * (2.0 * quadratic + 3.0 * offset * cubic)),
                ratio * ratio * (quadratic + 3.0 * offset * cubic),
                ratio * ratio * ratio * cubic,
            ],
        }
    }
}

pub(super) fn integrate_cubics(
    pieces: &[CubicPiece],
    mean: f64,
    deviation: f64,
    discount: f64,
) -> f64 {
    let mut total = Sum::default();
    let radius = 56.0 * deviation;
    for piece in pieces {
        let [left, right] = piece.bounds;
        if left < mean + radius && right > mean - radius {
            let weighted =
                cell_moments(left, right, mean, deviation).integrate(&piece.coefficients);
            total.add(scaled_signed_cash(weighted, discount));
        }
    }
    total.total()
}

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
    layers: usize,
) -> crate::Result<Vec<f64>> {
    let invalid = || {
        crate::Error::ComputationFailure("quadrature integration grid is not representable".into())
    };
    if !lower.is_finite() || !upper.is_finite() || lower >= upper {
        return Err(invalid());
    }
    let points = (steps as usize + 1).saturating_add(breaks.len());
    if grid_bytes(points, layers) > WORKSPACE_BUDGET {
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
    nodes.sort_unstable_by(f64::total_cmp);
    // Avoid roundoff-sized cells without collapsing the resolution of narrow grids.
    let minimum_spacing = (64.0 * f64::EPSILON * lower.abs().max(upper.abs()).max(1.0))
        .min(0.5 * (upper - lower) / f64::from(steps));
    nodes.dedup_by(|next, previous| (*next - *previous).abs() <= minimum_spacing);
    if nodes.len() < 4 {
        return Err(invalid());
    }
    Ok(nodes)
}

fn grid_bytes(points: usize, layers: usize) -> usize {
    // Old and new polynomial layers can coexist, along with values and intrinsic payoffs.
    let per_point = size_of::<f64>()
        + size_of::<Cell>()
        + layers * 2 * (size_of::<Polynomial>() + size_of::<f64>());
    points.saturating_mul(per_point)
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
    cells_within(nodes, mean, 56.0 * deviation)
}

fn cells_within(nodes: &[f64], mean: f64, radius: f64) -> Range<usize> {
    let first = nodes
        .partition_point(|&x| x <= mean - radius)
        .saturating_sub(1)
        .min(nodes.len() - 1);
    let last = nodes
        .partition_point(|&x| x < mean + radius)
        .min(nodes.len() - 1);
    first..last
}

fn excessive_rollback_work() -> Error {
    Error::InvalidInput(
        "quadrature rollback exceeds one billion node and cell visits; reduce step counts".into(),
    )
}

fn remaining_rollback_work(nodes: &[f64], layers: usize, visits: usize) -> Result<usize> {
    WORK_BUDGET
        .checked_sub(nodes.len().saturating_mul(layers).saturating_mul(visits))
        .ok_or_else(excessive_rollback_work)
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

pub(super) struct TransitionCache {
    entries: Vec<(f64, Transition)>,
    budget: usize,
    grids: Vec<(Range<usize>, Arc<ConvolutionGrid>)>,
    tail_work: Rc<Counter<usize>>,
    scheduled_gaps: HashMap<u64, (usize, usize)>,
    fft_workspace: Weak<RefCell<FftWorkspace>>,
}

impl TransitionCache {
    pub fn new(
        nodes: &[f64],
        node_capacity: usize,
        layers: usize,
        asset_steps: i32,
    ) -> Result<Self> {
        Self::with_patches(
            nodes,
            node_capacity,
            layers,
            asset_steps,
            std::slice::from_ref(&(0..nodes.len())),
        )
    }

    pub fn with_patches(
        nodes: &[f64],
        node_capacity: usize,
        layers: usize,
        asset_steps: i32,
        patches: &[Range<usize>],
    ) -> Result<Self> {
        let mut budget = WORKSPACE_BUDGET
            .checked_sub(grid_bytes(node_capacity, layers))
            .ok_or_else(workspace_error)?;
        let cells = asset_steps.unsigned_abs() as usize;
        let mut grids = Vec::new();
        for patch in patches {
            if (cells + 33).saturating_mul(size_of::<usize>()) > budget {
                break;
            }
            if let Some(grid) = ConvolutionGrid::new(&nodes[patch.clone()], cells)? {
                budget -= (grid.regular.capacity() + grid.split.capacity()) * size_of::<usize>();
                grids.push((patch.clone(), Arc::new(grid)));
            }
        }
        Ok(Self {
            entries: Vec::new(),
            budget,
            grids,
            tail_work: Rc::new(Counter::new(WORK_BUDGET)),
            scheduled_gaps: HashMap::new(),
            fft_workspace: Weak::new(),
        })
    }

    fn convolutions(&self, deviation: f64) -> &[(Range<usize>, Arc<ConvolutionGrid>)] {
        let bytes = self
            .grids
            .iter()
            .map(|(_, grid)| grid.bytes(deviation))
            .sum::<usize>();
        if bytes > self.budget
            || self
                .grids
                .iter()
                .any(|(_, grid)| deviation < grid.spacing / 8.0)
        {
            &[]
        } else {
            &self.grids
        }
    }

    fn transition_work(&self, nodes: &[f64], drift: f64, deviation: f64) -> Result<usize> {
        let grids = self.convolutions(deviation);
        let fft_work = grids
            .iter()
            .map(|(_, grid)| grid.work(deviation))
            .sum::<usize>();
        if covers_grid(grids, nodes.len()) {
            return Ok(fft_work);
        }
        nodes
            .iter()
            .enumerate()
            .try_fold(fft_work, |work, (index, &node)| {
                crate::cancellation::check()?;
                let active = planned_cells(nodes, node + drift, deviation, grids.len());
                Ok(direct_ranges(active, index, grids)
                    .into_iter()
                    .fold(work, |work, range| work.saturating_add(range.len())))
            })
    }

    pub fn validate_rollback_work(
        &self,
        nodes: &[f64],
        drift: f64,
        deviation: f64,
        intervals: i32,
    ) -> Result<()> {
        let exercise_steps = (intervals.unsigned_abs() - 1) as usize;
        let mut work = remaining_rollback_work(nodes, 1, exercise_steps)?;
        // The first rollback integrates terminal pieces directly, without interpolated cells.
        let continuation_steps = exercise_steps.saturating_sub(1);
        if continuation_steps > 0 {
            work = work
                .checked_sub(
                    self.transition_work(nodes, drift, deviation)?
                        .saturating_mul(continuation_steps),
                )
                .ok_or_else(excessive_rollback_work)?;
        }
        self.tail_work.set(work);
        Ok(())
    }

    pub fn validate_scheduled_work(
        &mut self,
        nodes: &[f64],
        dates: &[Timestamp],
        layers: usize,
        terminal_pieces: usize,
        split_pieces: usize,
        transition: impl Fn(f64) -> Result<(f64, f64, f64)>,
    ) -> Result<()> {
        let visits = dates
            .len()
            .saturating_add(terminal_pieces.saturating_sub(1))
            .saturating_add(split_pieces);
        let fixed_work = WORK_BUDGET - remaining_rollback_work(nodes, layers, visits)?;
        let mut work = self
            .tail_work
            .get()
            .checked_sub(fixed_work)
            .ok_or_else(excessive_rollback_work)?;
        // A terminal payoff with exact pieces does not integrate interpolated cells.
        let intervals = dates
            .len()
            .saturating_sub(1 + usize::from(terminal_pieces > 0));
        for pair in dates.windows(2).take(intervals) {
            crate::cancellation::check()?;
            let time = pair[0].years_until(pair[1]);
            let key = time.to_bits();
            let cells = if let Some((cells, uses)) = self.scheduled_gaps.get_mut(&key) {
                *uses += 1;
                *cells
            } else {
                let (drift, deviation, _) = transition(time)?;
                let cells = self.transition_work(nodes, drift, deviation)?;
                if self.scheduled_gaps.len() < MAX_PLANNED_GAPS {
                    self.scheduled_gaps.insert(key, (cells, 1));
                }
                cells
            };
            work = work
                .checked_sub(cells.saturating_mul(layers))
                .ok_or_else(excessive_rollback_work)?;
        }
        self.tail_work.set(work);
        Ok(())
    }

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
    ) -> crate::Result<&mut Transition> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(duration, _)| *duration == time)
        {
            let entry = self.entries.remove(index);
            self.entries.push(entry);
        } else {
            let grids = self.convolutions(deviation).to_vec();
            let bytes = grids
                .iter()
                .map(|(_, grid)| grid.bytes(deviation))
                .sum::<usize>();
            let row_bytes = if covers_grid(&grids, nodes.len()) {
                0
            } else {
                Transition::row_bytes(nodes, drift, deviation, self.budget - bytes, &grids)?
            };
            let minimum = bytes + row_bytes;
            let mut retained = self
                .entries
                .iter()
                .map(|(_, matrix)| matrix.storage_bytes())
                .sum::<usize>();
            while self.entries.len() >= MAX_TRANSITIONS || retained + minimum > self.budget {
                let least_used = self
                    .entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, (gap, _))| {
                        self.scheduled_gaps
                            .get(&gap.to_bits())
                            .map_or(0, |&(_, uses)| uses)
                    })
                    .map_or(0, |(index, _)| index);
                retained -= self.entries.remove(least_used).1.storage_bytes();
            }
            let index_bytes = row_bytes.saturating_sub(nodes.len() * size_of::<Row>());
            let max_moments = (self.budget - retained - minimum) / BYTES_PER_CACHED_MOMENT;
            let mut transition = Transition::new(
                nodes,
                drift,
                deviation,
                max_moments,
                index_bytes != 0,
                &grids,
                Rc::clone(&self.tail_work),
            )?;
            for (patch, grid) in grids {
                let workspace = grid.use_fft(deviation).then(|| {
                    self.fft_workspace.upgrade().unwrap_or_else(|| {
                        // All patches in this request use the same FFT size.
                        let workspace = Rc::new(RefCell::new(FftWorkspace::new(grid.size)));
                        self.fft_workspace = Rc::downgrade(&workspace);
                        workspace
                    })
                });
                let fft = GaussianConvolution::new(
                    grid,
                    &nodes[patch.clone()],
                    drift,
                    deviation,
                    Rc::clone(&self.tail_work),
                    workspace,
                )?;
                transition.convolutions.push((patch, fft));
            }
            self.entries.push((time, transition));
        }
        if let Some((_, uses)) = self.scheduled_gaps.get_mut(&time.to_bits()) {
            *uses = uses.saturating_sub(1);
        }
        Ok(&mut self.entries.last_mut().expect("transition was appended").1)
    }
}

fn covers_grid(grids: &[(Range<usize>, Arc<ConvolutionGrid>)], nodes: usize) -> bool {
    matches!(grids, [(patch, _)] if *patch == (0..nodes))
}

fn planned_cells(nodes: &[f64], mean: f64, deviation: f64, patches: usize) -> Range<usize> {
    if patches > 1 {
        cells_within(nodes, mean, 10.0 * deviation)
    } else {
        active_cells(nodes, mean, deviation)
    }
}

fn direct_ranges<T>(
    active: Range<usize>,
    node: usize,
    grids: &[(Range<usize>, T)],
) -> [Range<usize>; 2] {
    if let Some((patch, _)) = grids.iter().find(|(patch, _)| patch.contains(&node)) {
        [
            active.start..active.end.min(patch.start),
            active.start.max(patch.end - 1)..active.end,
        ]
    } else {
        [active, 0..0]
    }
}

type Row = ([Range<usize>; 2], Vec<usize>);

pub(super) struct Transition {
    moments: Vec<IntervalMoments>,
    rows: Vec<Row>,
    drift: f64,
    deviation: f64,
    convolutions: Vec<(Range<usize>, GaussianConvolution)>,
    tail_work: Rc<Counter<usize>>,
}
impl Transition {
    fn storage_bytes(&self) -> usize {
        self.convolutions
            .iter()
            .map(|(_, fft)| fft.grid.bytes(self.deviation))
            .sum::<usize>()
            + self.rows.capacity() * size_of::<Row>()
            + self.moments.capacity() * size_of::<IntervalMoments>()
            + self
                .rows
                .iter()
                .map(|(_, indices)| indices.capacity() * size_of::<usize>())
                .sum::<usize>()
    }

    fn row_bytes(
        nodes: &[f64],
        drift: f64,
        deviation: f64,
        budget: usize,
        grids: &[(Range<usize>, Arc<ConvolutionGrid>)],
    ) -> Result<usize> {
        let row_bytes = nodes.len().saturating_mul(size_of::<Row>());
        if row_bytes > budget {
            return Err(workspace_error());
        }
        let index_budget =
            (budget - row_bytes).min(nodes.len().saturating_mul(256 * size_of::<usize>()));
        let mut bytes = 0;
        for (index, &node) in nodes.iter().enumerate() {
            crate::cancellation::check()?;
            let active = planned_cells(nodes, node + drift, deviation, grids.len());
            let cells = direct_ranges(active, index, grids)
                .iter()
                .map(ExactSizeIterator::len)
                .sum::<usize>();
            bytes += cells * size_of::<usize>();
            if bytes > index_budget {
                return Ok(row_bytes);
            }
        }
        Ok(row_bytes + bytes)
    }

    fn new(
        nodes: &[f64],
        drift: f64,
        deviation: f64,
        max_moments: usize,
        cache_indices: bool,
        grids: &[(Range<usize>, Arc<ConvolutionGrid>)],
        tail_work: Rc<Counter<usize>>,
    ) -> Result<Self> {
        let mut moments = Vec::new();
        let mut geometries = HashMap::new();
        let mut rows = Vec::new();
        if !covers_grid(grids, nodes.len()) {
            rows.reserve_exact(nodes.len());
        }
        let row_count = if covers_grid(grids, nodes.len()) {
            0
        } else {
            nodes.len()
        };
        for (index, &node) in nodes.iter().take(row_count).enumerate() {
            crate::cancellation::check()?;
            let mean = node + drift;
            let active = planned_cells(nodes, mean, deviation, grids.len());
            let ranges = direct_ranges(active, index, grids);
            let cells = ranges.iter().cloned().flatten();
            let mut indices = Vec::new();
            if cache_indices {
                indices.reserve_exact(ranges.iter().map(ExactSizeIterator::len).sum());
            }
            for cell in cells.take(if cache_indices { usize::MAX } else { 0 }) {
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
            rows.push((ranges, indices));
        }
        Ok(Self {
            moments,
            rows,
            drift,
            deviation,
            convolutions: Vec::new(),
            tail_work,
        })
    }
    pub fn integrate(
        &mut self,
        nodes: &[f64],
        curve: &[Polynomial],
        discount: f64,
        values: &mut [f64],
    ) -> crate::Result<()> {
        if !self.convolutions.is_empty() {
            values.fill(0.0);
        }
        for (patch, convolution) in &mut self.convolutions {
            convolution.integrate(
                &nodes[patch.clone()],
                &curve[patch.start..patch.end - 1],
                [self.drift, self.deviation, discount],
                &mut values[patch.clone()],
            )?;
        }
        let factor = discount.exp();
        let pruned = self.convolutions.len() > 1;
        let threshold = if pruned {
            // A unit-cell cubic is bounded by four times its largest coefficient.
            // Bound the sum of both tails and leave slack for rounding the edges.
            let scale = curve
                .iter()
                .flatten()
                .try_fold(0.0_f64, |max, v| v.is_finite().then_some(max.max(v.abs())))
                .unwrap_or(f64::INFINITY);
            if scale == 0.0 {
                values.fill(0.0);
                return Ok(());
            }
            (8.0 * (-49.0_f64).exp() / f64::EPSILON) * scale * factor
        } else {
            0.0
        };
        for (row, ((value, (ranges, indices)), &node)) in
            values.iter_mut().zip(&self.rows).zip(nodes).enumerate()
        {
            crate::cancellation::check()?;
            let mut sum = Sum::default();
            if !self.convolutions.is_empty() {
                sum.add(*value);
            }
            for (offset, cell) in ranges.iter().cloned().flatten().enumerate() {
                if offset % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                let index = indices.get(offset).copied().unwrap_or(usize::MAX);
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
            if pruned && (!threshold.is_normal() || sum.total().abs() < threshold) {
                let mean = node + self.drift;
                let full = direct_ranges(
                    active_cells(nodes, mean, self.deviation),
                    row,
                    &self.convolutions,
                );
                for (full, near) in full.into_iter().zip(ranges) {
                    let left = full.start..full.end.min(near.start);
                    let right = full.start.max(near.end.max(left.end))..full.end;
                    let work = left.len() + right.len();
                    self.tail_work.set(
                        self.tail_work
                            .get()
                            .checked_sub(work)
                            .ok_or_else(excessive_rollback_work)?,
                    );
                    for cell in left.chain(right) {
                        if cell % 1024 == 0 {
                            crate::cancellation::check()?;
                        }
                        let term = cell_moments(nodes[cell], nodes[cell + 1], mean, self.deviation)
                            .integrate(&curve[cell]);
                        sum.add(if factor.is_normal() {
                            term * factor
                        } else {
                            scaled_signed_cash(term, discount)
                        });
                    }
                }
            }
            *value = sum.total();
        }
        Ok(())
    }
}

struct ConvolutionGrid {
    size: usize,
    spacing: f64,
    regular: Vec<usize>,
    split: Vec<usize>,
    extras: usize,
}

impl ConvolutionGrid {
    #[expect(
        clippy::cast_precision_loss,
        reason = "Validated i32 grid sizes fit exactly in f64."
    )]
    fn new(nodes: &[f64], cells: usize) -> Result<Option<Self>> {
        let Some(extras) = nodes.len().checked_sub(cells + 1) else {
            return Ok(None);
        };
        if cells < 256 || extras > 16 {
            return Ok(None);
        }
        let spacing = (nodes[nodes.len() - 1] - nodes[0]) / cells as f64;
        let tolerance = 16.0 * f64::EPSILON * nodes[0].abs().max(nodes[nodes.len() - 1].abs());
        if !spacing.is_normal() || tolerance >= 1e-7 * spacing {
            return Ok(None);
        }
        let mut regular = Vec::with_capacity(cells + 1);
        let mut next = 0;
        for index in 0..=cells {
            if index % 256 == 0 {
                crate::cancellation::check()?;
            }
            let coordinate = nodes[0] + index as f64 * spacing;
            while next < nodes.len() && nodes[next] < coordinate - tolerance {
                next += 1;
            }
            if next == nodes.len() || (nodes[next] - coordinate).abs() > tolerance {
                return Ok(None);
            }
            regular.push(next);
            next += 1;
        }
        let split = regular
            .windows(2)
            .filter(|pair| pair[1] - pair[0] > 1)
            .flat_map(|pair| pair[0]..pair[1])
            .collect();
        Ok(Some(Self {
            size: (2 * cells).next_power_of_two(),
            spacing,
            regular,
            split,
            extras,
        }))
    }

    fn correction_count(&self) -> usize {
        self.regular.len().saturating_mul(self.split.len())
            + self.extras.saturating_mul(self.regular.len() + self.extras)
    }

    fn use_fft(&self, deviation: f64) -> bool {
        deviation >= 2.0 * self.spacing
    }

    fn bytes(&self, deviation: f64) -> usize {
        // Spectra, plans, scratch, and sparse exact rows for cells split by breakpoints.
        let spectra = if self.use_fft(deviation) {
            self.size.saturating_mul(22 * size_of::<Complex64>())
        } else {
            self.regular.len() * 2 * size_of::<IntervalMoments>()
        };
        let corrections = self
            .correction_count()
            .saturating_mul(2 * size_of::<(usize, IntervalMoments)>());
        let rows = (self.regular.len() + self.extras) * size_of::<Vec<(usize, IntervalMoments)>>();
        spectra.saturating_add(corrections).saturating_add(rows)
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "The band has at most 226 cells."
    )]
    fn work(&self, deviation: f64) -> usize {
        let regular = if self.use_fft(deviation) {
            5 * self.size * self.size.ilog2() as usize
        } else {
            ((112.0 * deviation / self.spacing).ceil() as usize + 2) * self.regular.len()
        };
        regular.saturating_add(self.correction_count())
    }
}

struct GaussianConvolution {
    grid: Arc<ConvolutionGrid>,
    fft: Option<Spectrum>,
    moments: Vec<IntervalMoments>,
    corrections: Vec<Vec<(usize, IntervalMoments)>>,
    tail_work: Rc<Counter<usize>>,
}

struct Spectrum {
    kernels: [Vec<Complex64>; 4],
    workspace: Rc<RefCell<FftWorkspace>>,
}

struct FftWorkspace {
    forward: Arc<dyn Fft<f64>>,
    inverse: Arc<dyn Fft<f64>>,
    input: Vec<Complex64>,
    output: Vec<Complex64>,
    scratch: Vec<Complex64>,
    tail_values: Vec<f64>,
    tail: Option<TailFft>,
}

struct TailFft {
    forward: Arc<dyn Fft<f64>>,
    inverse: Arc<dyn Fft<f64>>,
    input: Vec<Complex64>,
    output: Vec<Complex64>,
    kernel: Vec<Complex64>,
    scratch: Vec<Complex64>,
    weights: Vec<[f64; 4]>,
}

impl GaussianConvolution {
    #[expect(
        clippy::cast_precision_loss,
        reason = "workspace-bounded offsets and transform length fit in f64"
    )]
    fn new(
        grid: Arc<ConvolutionGrid>,
        nodes: &[f64],
        drift: f64,
        deviation: f64,
        tail_work: Rc<Counter<usize>>,
        workspace: Option<Rc<RefCell<FftWorkspace>>>,
    ) -> Result<Self> {
        crate::cancellation::check()?;
        let spacing = grid.spacing;
        let cells = grid.regular.len() - 1;
        let mut cell_weights = Vec::with_capacity(2 * cells);
        for offset in 0..2 * cells {
            if offset % 128 == 0 {
                crate::cancellation::check()?;
            }
            let left = (offset as f64 - cells as f64) * spacing;
            cell_weights.push(cell_moments(left, left + spacing, drift, deviation));
        }
        let fft = workspace
            .map(|workspace| Spectrum::new(workspace, cells, &cell_weights))
            .transpose()?;
        let mut corrections = Vec::new();
        if grid.extras > 0 {
            corrections.reserve_exact(nodes.len());
            for (index, &node) in nodes.iter().enumerate() {
                crate::cancellation::check()?;
                let mean = node + drift;
                let active = active_cells(nodes, mean, deviation);
                let row = |cell| {
                    (
                        cell,
                        cell_moments(nodes[cell], nodes[cell + 1], mean, deviation),
                    )
                };
                corrections.push(if grid.regular.binary_search(&index).is_ok() {
                    grid.split
                        .iter()
                        .copied()
                        .filter(|cell| active.contains(cell))
                        .map(row)
                        .collect()
                } else {
                    active.map(row).collect()
                });
            }
        }
        Ok(Self {
            grid,
            fft,
            moments: cell_weights,
            corrections,
            tail_work,
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "Workspace bounds support indices and positive widths convert to clamped counts."
    )]
    fn integrate(
        &mut self,
        nodes: &[f64],
        curve: &[Polynomial],
        [drift, deviation, discount]: [f64; 3],
        values: &mut [f64],
    ) -> Result<()> {
        if self.fft.is_none() {
            return self.integrate_band(nodes, curve, [drift, deviation, discount], values);
        }
        let factor = discount.exp();
        let scale = curve
            .iter()
            .flatten()
            .try_fold(0.0_f64, |scale, value| {
                value.is_finite().then_some(scale.max(value.abs()))
            })
            .unwrap_or(f64::INFINITY);
        if scale == 0.0 {
            values.fill(0.0);
            return Ok(());
        }
        let charge = |work| {
            self.tail_work.set(
                self.tail_work
                    .get()
                    .checked_sub(work)
                    .ok_or_else(excessive_rollback_work)?,
            );
            Ok::<_, Error>(())
        };
        if !(1e-100..=1e100).contains(&scale) || !(1e-100..=1e100).contains(&factor) {
            for (value, &node) in values.iter_mut().zip(nodes) {
                crate::cancellation::check()?;
                charge(active_cells(nodes, node + drift, deviation).len())?;
                *value = integrate_curve(curve, nodes, node + drift, deviation, discount);
            }
            return Ok(());
        }
        let regular = &self.grid.regular;
        let cells = regular.len() - 1;
        let nonzero = |&cell: &usize| curve[cell].iter().any(|&coefficient| coefficient != 0.0);
        let support = &regular[..cells];
        let first = support.iter().position(nonzero).unwrap_or(cells);
        let last = support.iter().rposition(nonzero).map_or(cells, |i| i + 1);
        let fft = self.fft.as_ref().expect("FFT selected above");
        let mut workspace = fft.workspace.borrow_mut();
        let FftWorkspace {
            forward,
            inverse,
            input,
            output,
            scratch,
            tail_values,
            tail,
        } = &mut *workspace;
        output.fill(Complex64::default());
        for (power, kernel) in fft.kernels.iter().enumerate() {
            crate::cancellation::check()?;
            for (value, pair) in input.iter_mut().zip(regular.windows(2)) {
                *value = Complex64::new(
                    if pair[1] == pair[0] + 1 {
                        curve[pair[0]][power]
                    } else {
                        0.0
                    },
                    0.0,
                );
            }
            input[cells..].fill(Complex64::default());
            forward.process_with_scratch(input, scratch);
            for ((value, coefficient), weight) in output.iter_mut().zip(input.iter()).zip(kernel) {
                *value += coefficient * weight;
            }
        }
        inverse.process_with_scratch(output, scratch);
        let roundoff = 128.0 * f64::EPSILON * scale;
        let log_scale = (8.0 * scale).ln();
        let relative_error = (4.0 * f64::EPSILON).ln();
        let width = deviation / self.grid.spacing;
        let drift_cells = drift / self.grid.spacing;
        let floor = LOG_HALF_SUBNORMAL - discount.max(0.0);
        tail_values[..=cells].fill(f64::NAN);
        // Small direct bands cost less than a local transform. Scale the
        // convolution only when the spatial grid makes a band wide enough.
        if first < last && width >= 32.0 && width.is_finite() {
            let coefficients = |cell: usize| {
                let index = regular[cell];
                if regular[cell + 1] == index + 1 {
                    curve[index]
                } else {
                    [0.0; 4]
                }
            };
            for (side, target, anchor) in [(1.0, 0..first, first), (-1.0, last..cells + 1, last)] {
                let rows = ((4.0 * width).ceil() as usize).min(cells + 1);
                let source_len = ((12.0 * width).ceil() as usize).min(cells);
                let columns = if side > 0.0 {
                    first..first.saturating_add(source_len).min(last)
                } else {
                    last.saturating_sub(source_len).max(first)..last
                };
                let omitted = if side > 0.0 {
                    (columns.end < last).then_some(columns.end)
                } else {
                    (columns.start > first).then_some(columns.start)
                };
                let size = (columns.len() + rows).next_power_of_two();
                // Large diffusion bands use the direct repair. Small local
                // transforms share the outer transition's workspace budget.
                if size > output.len() / 4 {
                    continue;
                }
                let work = 5 * size * size.ilog2() as usize;
                for start in (target.start..target.end).step_by(rows.max(1)) {
                    let end = start.saturating_add(rows).min(target.end);
                    let center = (start as f64).midpoint(end as f64);
                    let tilt = (anchor as f64 - center - drift_cells) / width;
                    if !(6.0..=38.0).contains(&(tilt * side)) {
                        continue;
                    }
                    if !output[start..end]
                        .iter()
                        .any(|value| value.re.abs() <= roundoff)
                    {
                        continue;
                    }
                    if work > self.tail_work.get() {
                        continue;
                    }
                    charge(work)?;
                    if tail.as_ref().is_none_or(|fft| fft.input.len() != size) {
                        *tail = Some(TailFft::new(size));
                    }
                    let Some(fft) = tail.as_mut() else {
                        unreachable!("tail transform is initialized")
                    };
                    fft.convolve(
                        &self.moments,
                        cells,
                        columns.clone(),
                        start..end,
                        coefficients,
                        [anchor as f64, drift_cells, width, tilt],
                    )?;
                    for (i, value) in tail_values.iter_mut().enumerate().take(end).skip(start) {
                        let sample = fft.output[i - start].re;
                        // Accept only rows with enough signal on the tilted scale.
                        if sample.abs() <= 1e-4 * scale {
                            continue;
                        }
                        let undo = (0.5 * tilt
                            + ((i as f64 - anchor as f64 + drift_cells) / width))
                            * tilt
                            + tilt.abs() / width;
                        let log_value = sample.abs().ln() + undo;
                        if omitted.is_some_and(|edge| {
                            let distance = ((edge as f64 - i as f64 - drift_cells).abs() - 1.0)
                                .max(0.0)
                                / width;
                            log_scale - 0.5 * distance.powi(2) > log_value + relative_error
                        }) {
                            continue;
                        }
                        *value = scaled_signed_cash(sample, undo);
                    }
                }
            }
        }
        let mut index = 0;
        for (node_index, (value, &node)) in values.iter_mut().zip(nodes).enumerate() {
            if node_index % 256 == 0 {
                crate::cancellation::check()?;
            }
            let exact = self.corrections.get(node_index).into_iter().flatten();
            let mut sum = Sum::default();
            for &(cell, ref moments) in exact {
                sum.add(moments.integrate(&curve[cell]));
            }
            if regular[index] != node_index {
                *value = sum.total() * factor;
                continue;
            }
            let sample = output[index].re;
            let estimate = sample + sum.total();
            let repaired = tail_values[index];
            let combined = repaired + sum.total();
            *value = if estimate.is_finite() && estimate.abs() > roundoff && sample.abs() > roundoff
            {
                factor * estimate
            } else if combined.is_finite()
                && combined.abs() > repaired.abs().midpoint(sum.total().abs())
            {
                factor * combined
            } else {
                // Direct summation recovers relative accuracy in small tails.
                crate::cancellation::check()?;
                let active = active_cells(nodes, node + drift, deviation);
                let (start, end) = if self.grid.extras == 0 {
                    (active.start, active.end)
                } else {
                    (
                        regular.partition_point(|&i| i < active.start),
                        regular.partition_point(|&i| i < active.end).min(cells),
                    )
                };
                let end = end.min(last);
                let start = start.max(first).min(end);
                let offset = cells - index;
                let add = |sum: &mut Sum, range: std::ops::Range<usize>| {
                    charge(range.len())?;
                    for cell in range {
                        if regular[cell] + 1 == regular[cell + 1] {
                            sum.add(self.moments[offset + cell].integrate(&curve[regular[cell]]));
                        }
                    }
                    Ok::<_, Error>(())
                };
                if start < end {
                    let support = start..end;
                    let center = index as f64 + drift_cells;
                    let distance = center.clamp(start as f64, end as f64) - center;
                    // Reject unrepresentable Gaussian mass before reading the support.
                    let nearest = (distance.abs() - 1.0).max(0.0) / width;
                    if log_scale - 0.5 * nearest.powi(2) >= floor {
                        let (mut left, mut right, mut bound) = Self::tail_band(
                            center,
                            support.clone(),
                            width,
                            distance,
                            8.0,
                            log_scale,
                        );
                        add(&mut sum, left..right)?;
                        // Widen only as far as the compensated sum needs.
                        for radius in [12.0, 20.0, 36.0, f64::INFINITY] {
                            let threshold = (sum.total().abs().ln() + relative_error).max(floor);
                            if bound <= threshold {
                                break;
                            }
                            let (lo, hi, remaining) = Self::tail_band(
                                center,
                                support.clone(),
                                width,
                                distance,
                                radius,
                                log_scale,
                            );
                            add(&mut sum, lo..left)?;
                            add(&mut sum, right..hi)?;
                            (left, right, bound) = (lo, hi, remaining);
                        }
                    }
                }
                sum.total() * factor
            };
            index += 1;
        }
        Ok(())
    }

    fn integrate_band(
        &self,
        nodes: &[f64],
        curve: &[Polynomial],
        [drift, deviation, discount]: [f64; 3],
        values: &mut [f64],
    ) -> Result<()> {
        let regular = &self.grid.regular;
        let cells = regular.len() - 1;
        let factor = discount.exp();
        if !factor.is_normal() {
            for (value, &node) in values.iter_mut().zip(nodes) {
                crate::cancellation::check()?;
                let remaining = self
                    .tail_work
                    .get()
                    .checked_sub(active_cells(nodes, node + drift, deviation).len())
                    .ok_or_else(excessive_rollback_work)?;
                self.tail_work.set(remaining);
                *value = integrate_curve(curve, nodes, node + drift, deviation, discount);
            }
            return Ok(());
        }
        let mut row = 0;
        for (index, (value, &node)) in values.iter_mut().zip(nodes).enumerate() {
            if index % 256 == 0 {
                crate::cancellation::check()?;
            }
            let mut sum = Sum::default();
            for &(cell, ref moments) in self.corrections.get(index).into_iter().flatten() {
                sum.add(moments.integrate(&curve[cell]));
            }
            if regular[row] == index {
                let active = active_cells(nodes, node + drift, deviation);
                let left = regular.partition_point(|&cell| cell < active.start);
                let right = regular
                    .partition_point(|&cell| cell < active.end)
                    .min(cells);
                for column in left..right {
                    let cell = regular[column];
                    if regular[column + 1] == cell + 1 {
                        sum.add(self.moments[cells - row + column].integrate(&curve[cell]));
                    }
                }
                row += 1;
            }
            *value = sum.total() * factor;
        }
        Ok(())
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Workspace bounds make grid indices exact and the candidate band is clamped to them."
    )]
    fn tail_band(
        center: f64,
        active: Range<usize>,
        width: f64,
        distance: f64,
        deviations: f64,
        log_scale: f64,
    ) -> (usize, usize, f64) {
        let low = active.start as f64;
        let high = active.end as f64;
        let radius = distance.hypot(deviations * width);
        let left = (center - radius).floor().clamp(low, high) as usize;
        let right = (center + radius).ceil().clamp(low, high) as usize;
        let edge = |cell: usize, side: f64| {
            // Leave one cell of slack for rounding of the regular-lattice moments.
            ((side * (cell as f64 - center) - 1.0) / width).max(0.0)
        };
        let left_z = if left > active.start {
            edge(left, -1.0)
        } else {
            f64::INFINITY
        };
        let right_z = if right < active.end {
            edge(right, 1.0)
        } else {
            f64::INFINITY
        };
        // Each cubic is bounded by four coefficients on its unit cell. Each
        // omitted Gaussian tail has probability at most exp(-z²/2).
        let bound = log_scale - 0.5 * left_z.min(right_z).powi(2);
        (left, right, bound)
    }
}

impl FftWorkspace {
    fn new(size: usize) -> Self {
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let scratch = vec![
            Complex64::default();
            forward
                .get_inplace_scratch_len()
                .max(inverse.get_inplace_scratch_len())
        ];
        let blank = || vec![Complex64::default(); size];
        Self {
            forward,
            inverse,
            input: blank(),
            output: blank(),
            scratch,
            tail_values: vec![f64::NAN; size],
            tail: None,
        }
    }
}

impl TailFft {
    fn new(size: usize) -> Self {
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let blank = || vec![Complex64::default(); size];
        Self {
            input: blank(),
            output: blank(),
            kernel: blank(),
            scratch: vec![
                Complex64::default();
                forward
                    .get_inplace_scratch_len()
                    .max(inverse.get_inplace_scratch_len())
            ],
            weights: vec![[0.0; 4]; size],
            forward,
            inverse,
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "The workspace bounds all transform indices."
    )]
    fn convolve(
        &mut self,
        moments: &[IntervalMoments],
        cells: usize,
        columns: Range<usize>,
        rows: Range<usize>,
        coefficients: impl Fn(usize) -> Polynomial,
        [anchor, drift, width, tilt]: [f64; 4],
    ) -> Result<()> {
        let size = self.input.len();
        let log_ratio = tilt / width;
        let normalize = (0.5 * tilt + drift / width) * tilt + log_ratio.abs();
        let origin = cells + columns.start - rows.start;
        self.weights.fill([0.0; 4]);
        // Multiplying coefficients and the kernel by the same geometric
        // progression scales convolution row i by that progression at i.
        for offset in 0..columns.len() {
            let moment = &moments[origin + offset];
            let delta = (origin + offset) as f64 - cells as f64;
            let mass = (moment.log_mass + log_ratio * delta - normalize).exp();
            self.weights[(size - offset) % size] = [
                mass,
                mass * moment.conditional[0],
                mass * moment.conditional[1],
                mass * moment.conditional[2],
            ];
        }
        for offset in 1..rows.len() {
            let moment = &moments[origin - offset];
            let delta = (origin - offset) as f64 - cells as f64;
            let mass = (moment.log_mass + log_ratio * delta - normalize).exp();
            self.weights[offset] = [
                mass,
                mass * moment.conditional[0],
                mass * moment.conditional[1],
                mass * moment.conditional[2],
            ];
        }
        self.output.fill(Complex64::default());
        for power in [0, 2] {
            crate::cancellation::check()?;
            self.input.fill(Complex64::default());
            for (value, j) in self.input.iter_mut().zip(columns.clone()) {
                let coefficient = coefficients(j);
                let weight = (-log_ratio * (j as f64 - anchor)).exp();
                *value = Complex64::new(coefficient[power], coefficient[power + 1]) * weight;
            }
            for (value, weights) in self.kernel.iter_mut().zip(&self.weights) {
                *value = Complex64::new(weights[power], weights[power + 1]);
            }
            self.forward
                .process_with_scratch(&mut self.input, &mut self.scratch);
            self.forward
                .process_with_scratch(&mut self.kernel, &mut self.scratch);
            // Two real sequences share a forward transform. Recover their
            // spectra using conjugate symmetry before multiplying them.
            let split = |values: &[Complex64], i: usize, mirror: usize| {
                let forward = values[i];
                let reverse = values[mirror].conj();
                let difference = forward - reverse;
                [
                    (forward + reverse) * 0.5,
                    Complex64::new(difference.im, -difference.re) * 0.5,
                ]
            };
            for (i, value) in self.output.iter_mut().enumerate() {
                let mirror = (size - i) % size;
                let [a, b] = split(&self.input, i, mirror);
                let [c, d] = split(&self.kernel, i, mirror);
                *value += a * c + b * d;
            }
        }
        self.inverse
            .process_with_scratch(&mut self.output, &mut self.scratch);
        for value in &mut self.output[..rows.len()] {
            *value /= size as f64;
        }
        Ok(())
    }
}

impl Spectrum {
    #[expect(
        clippy::cast_precision_loss,
        reason = "The workspace bounds the transform length."
    )]
    fn new(
        workspace: Rc<RefCell<FftWorkspace>>,
        cells: usize,
        moments: &[IntervalMoments],
    ) -> Result<Self> {
        let mut work = workspace.borrow_mut();
        let size = work.input.len();
        let mut kernels = std::array::from_fn(|_| vec![Complex64::default(); size]);
        for (offset, moments) in moments.iter().enumerate() {
            // The correlation offset is j - i. The convolution kernel uses i - j.
            let index = (cells + size - offset) % size;
            kernels[0][index].re = moments.mass;
            if moments.mass != 0.0 {
                for (kernel, conditional) in kernels[1..].iter_mut().zip(moments.conditional) {
                    kernel[index].re = moments.mass * conditional;
                }
            }
        }
        let normalization = 1.0 / size as f64;
        let FftWorkspace {
            forward, scratch, ..
        } = &mut *work;
        for kernel in &mut kernels {
            crate::cancellation::check()?;
            forward.process_with_scratch(kernel, scratch);
            for value in kernel {
                *value *= normalization;
            }
        }
        drop(work);
        Ok(Self { kernels, workspace })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_nodes_merge_roundoff_without_collapsing_narrow_grids() {
        let nodes = integration_nodes(-0.2, 0.2, 512, &[0.0, 1.4e-16], 1).unwrap();
        assert_eq!(nodes.len(), 513);
        assert!(nodes.windows(2).all(|pair| pair[1] - pair[0] > 1e-14));

        let tiny = integration_nodes(-1e-20, 1e-20, 8, &[0.0], 1).unwrap();
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
        let mut transition = Transition::new(
            &nodes,
            0.25,
            0.5,
            0,
            false,
            &[],
            Rc::new(Counter::new(WORK_BUDGET)),
        )?;
        let mut values = [0.0; 3];
        transition.integrate(&nodes, &curve, -0.05, &mut values)?;
        for (&node, actual) in nodes.iter().zip(values) {
            let expected = integrate_curve(&curve, &nodes, node + 0.25, 0.5, -0.05);
            assert!((actual - expected).abs() < 1e-13);
        }
        Ok(())
    }

    #[test]
    fn transition_rows_must_fit_in_the_budget_left_by_the_grid() -> Result<()> {
        let nodes = [-1.0, -0.75, -0.5, -0.25, 0.25, 0.5, 0.75, 1.0];
        let capacity = WORKSPACE_BUDGET / grid_bytes(1, 1);
        let mut cache = TransitionCache::new(&nodes, capacity, 1, 7)?;
        assert!(matches!(
            cache.get(1.0, &nodes, 0.0, 1.0),
            Err(Error::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn four_recurring_gaps_reuse_kernels_and_share_fft_workspace() -> Result<()> {
        let nodes = integration_nodes(-1.5, 1.5, 512, &[], 1)?;
        let mut cache = TransitionCache::new(&nodes, nodes.len(), 1, 512)?;
        let mut kernels = [std::ptr::null(); 4];
        for duration in 0..8 {
            let gap = duration % 4;
            let time = f64::from(gap + 1);
            let transition = cache.get(time, &nodes, 0.0, 0.05 * time.sqrt())?;
            let spectrum = transition.convolutions[0].1.fft.as_ref().unwrap();
            let kernel = &mut kernels[usize::try_from(gap).unwrap()];
            if duration < 4 {
                *kernel = spectrum.kernels[0].as_ptr();
            } else {
                assert_eq!(*kernel, spectrum.kernels[0].as_ptr());
            }
        }
        let workspace = cache.fft_workspace.upgrade().unwrap();
        assert_eq!(cache.entries.len(), 4);
        assert!(cache.entries.iter().all(|(_, transition)| {
            let spectrum = transition.convolutions[0].1.fft.as_ref().unwrap();
            Rc::ptr_eq(&workspace, &spectrum.workspace)
        }));
        Ok(())
    }

    #[test]
    fn tilted_tail_convolution_matches_direct_moments_on_both_sides() -> Result<()> {
        let cells = 256;
        let spacing = 0.01;
        let drift = 0.013;
        let deviation = 0.12;
        let width = deviation / spacing;
        let drift_cells = drift / spacing;
        let moments: Vec<_> = (-256..256)
            .map(|i| {
                let left = f64::from(i) * spacing;
                cell_moments(left, left + spacing, drift, deviation)
            })
            .collect();
        let coefficients = |_| [0.6, 0.1, -0.3, 0.08];
        let mut fft = TailFft::new(128);
        for (rows, columns, anchor, tilt) in [
            (30..55, 130..170, 130.0, 7.0),
            (210..237, 110..150, 150.0, -6.0),
        ] {
            fft.convolve(
                &moments,
                cells,
                columns.clone(),
                rows.clone(),
                coefficients,
                [anchor, drift_cells, width, tilt],
            )?;
            for (i, row) in rows.enumerate() {
                let row_coordinate = f64::from(u32::try_from(row).expect("256 cells"));
                let undo = (0.5 * tilt + (row_coordinate - anchor + drift_cells) / width) * tilt
                    + tilt.abs() / width;
                let actual = scaled_signed_cash(fft.output[i].re, undo);
                let expected: f64 = columns
                    .clone()
                    .map(|j| moments[cells + j - row].integrate(&coefficients(j)))
                    .sum();
                assert!(
                    (actual - expected).abs() < 2e-11 * expected,
                    "{actual} vs {expected}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn convolution_matches_direct_cubic_moments_with_and_without_split_cells() -> Result<()> {
        for breaks in [&[][..], &[-0.723, 0.013, 0.015][..]] {
            let nodes = integration_nodes(-1.5, 1.5, 384, breaks, 1)?;
            let curve: Vec<_> = nodes[..nodes.len() - 1]
                .iter()
                .map(|&x| [(2.0 * x).sin(), x * x, -0.2, 0.1])
                .collect();
            for (drift, deviation) in [(-0.14, 0.09), (0.19, 0.3), (0.6, 0.09), (0.01, 0.004)] {
                let mut cache = TransitionCache::new(&nodes, nodes.len(), 1, 384)?;
                let transition = cache.get(1.0, &nodes, drift, deviation)?;
                assert!(!transition.convolutions.is_empty());
                let mut actual = vec![0.0; nodes.len()];
                transition.integrate(&nodes, &curve, -0.03, &mut actual)?;
                for (&node, value) in nodes.iter().zip(actual) {
                    let expected = integrate_curve(&curve, &nodes, node + drift, deviation, -0.03);
                    assert!(
                        (value - expected).abs() < 8e-14,
                        "node {node}, got {value}, expected {expected}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn transitions_match_normal_probabilities_across_repeated_intervals() {
        let nodes = [-1.0, 0.0, 1.0];
        let curve = [[1.0, 0.0, 0.0, 0.0]; 2];
        let mut cache = TransitionCache::new(&nodes, nodes.len(), 1, 2).unwrap();
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
