use super::{
    Curve, Diffusion, FiniteDifferenceScheme, Result, SpatialGrid, TimeGrid, invalid_grid,
    unstable, validate_work, validate_workspace,
};

#[expect(
    clippy::too_many_arguments,
    reason = "explicit grid, model and barrier event inputs"
)]
pub(super) fn march<D: Diffusion>(
    mut values: Vec<f64>,
    space: &SpatialGrid,
    grid: &TimeGrid,
    diffusion: D,
    mut scheme: FiniteDifferenceScheme,
    [risk, dividend]: [&Curve; 2],
    observations: &[f64],
    implicit_fallback: bool,
    edges: impl Fn(f64) -> [f64; 2],
    observe: impl Fn(&mut [f64], f64),
) -> Result<Vec<f64>> {
    let mut next = vec![0.0; values.len()];
    let mut stepper = diffusion.stepper(space.count);
    let mut damping: u32 = 2;
    for step in (0..grid.widths.len()).rev() {
        crate::cancellation::check()?;
        for (current, dt, weight) in grid.substeps(step, scheme, &mut damping) {
            let middle = current + 0.5 * dt;
            let boundaries = edges(current);
            let mut key = [dt, weight, risk.rate(middle), dividend.rate(middle)];
            loop {
                diffusion.advance(
                    &mut stepper,
                    &values,
                    &mut next,
                    space,
                    middle,
                    key,
                    boundaries,
                )?;
                if !use_implicit_fallback(
                    &mut scheme,
                    implicit_fallback,
                    key[1],
                    &next[1..space.count],
                ) {
                    break;
                }
                key[1] = scheme.theta();
            }
            std::mem::swap(&mut values, &mut next);
        }
        if observations
            .binary_search_by(|event| event.total_cmp(&grid.times[step]))
            .is_ok()
        {
            observe(&mut values, grid.times[step]);
            damping = 2;
        }
    }
    Ok(values)
}

fn use_implicit_fallback(
    scheme: &mut FiniteDifferenceScheme,
    enabled: bool,
    weight: f64,
    interior: &[f64],
) -> bool {
    if enabled
        && *scheme == FiniteDifferenceScheme::Rannacher
        && weight < 1.0
        && interior.iter().any(|&value| value < 0.0)
    {
        // Keep damping the oscillation after retrying this step.
        *scheme = FiniteDifferenceScheme::ImplicitEuler;
        return true;
    }
    false
}

struct Factors {
    key: [f64; 5],
    coefficients: Vec<[f64; 3]>,
    upper: Vec<f64>,
    multipliers: Vec<f64>,
    diagonal: Vec<f64>,
    inverse: Vec<f64>,
}

impl Factors {
    fn factor_row(&mut self, i: usize, dt: f64, theta: f64) -> Result<()> {
        let [a, b, c] = self.coefficients[i];
        self.upper[i] = -theta * dt * c;
        self.diagonal[i] = 1.0 - theta * dt * b;
        if i > 0 {
            self.multipliers[i] = -theta * dt * a / self.diagonal[i - 1];
            self.diagonal[i] -= self.multipliers[i] * self.upper[i - 1];
        }
        if !self.diagonal[i].is_finite() || self.diagonal[i] == 0.0 {
            return Err(unstable());
        }
        self.inverse[i] = 1.0 / self.diagonal[i];
        Ok(())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "two domains share diffusion, time grid and event rules"
)]
pub(super) fn march_with_continuation<D: Diffusion>(
    mut values: Vec<f64>,
    space: &SpatialGrid,
    mut continuation: Vec<f64>,
    full_space: &SpatialGrid,
    grid: &TimeGrid,
    diffusion: D,
    mut scheme: FiniteDifferenceScheme,
    [risk, dividend]: [&Curve; 2],
    observations: &[f64],
    implicit_fallback: bool,
    full_edges: impl Fn(f64) -> [f64; 2],
    edges: impl Fn(f64, &[f64]) -> [f64; 2],
    observe: impl Fn(&mut [f64], &[f64], f64),
) -> Result<Vec<f64>> {
    validate_workspace(
        scheme,
        (values.len() + continuation.len()) as u64,
        grid.widths.len() as u64,
        risk.workspace_bytes() + dividend.workspace_bytes(),
    )?;
    validate_work(
        (space.count + full_space.count) as u64,
        grid.rollback_steps(scheme, observations)? + u64::from(implicit_fallback),
    )?;
    let mut next = vec![0.0; values.len()];
    let mut next_continuation = vec![0.0; continuation.len()];
    let mut stepper = diffusion.stepper(space.count);
    let mut full_stepper = diffusion.stepper(full_space.count);
    let mut damping = 2;
    for step in (0..grid.widths.len()).rev() {
        crate::cancellation::check()?;
        for (current, dt, weight) in grid.substeps(step, scheme, &mut damping) {
            let middle = current + 0.5 * dt;
            let boundaries = full_edges(current);
            let mut key = [dt, weight, risk.rate(middle), dividend.rate(middle)];
            loop {
                diffusion.advance(
                    &mut full_stepper,
                    &continuation,
                    &mut next_continuation,
                    full_space,
                    middle,
                    key,
                    boundaries,
                )?;
                diffusion.advance(
                    &mut stepper,
                    &values,
                    &mut next,
                    space,
                    middle,
                    key,
                    edges(current, &next_continuation),
                )?;
                if !use_implicit_fallback(
                    &mut scheme,
                    implicit_fallback,
                    key[1],
                    &next[1..space.count],
                ) {
                    break;
                }
                key[1] = scheme.theta();
            }
            std::mem::swap(&mut continuation, &mut next_continuation);
            std::mem::swap(&mut values, &mut next);
        }
        if observations
            .binary_search_by(|event| event.total_cmp(&grid.times[step]))
            .is_ok()
        {
            observe(&mut values, &continuation, grid.times[step]);
            damping = 2;
        }
    }
    Ok(values)
}

pub(super) struct Stepper {
    factors: Vec<Factors>,
    active: usize,
    local: Option<crate::market::LocalVolatilityProjection>,
    local_stencils: Option<Vec<super::grid::PreparedLogStencil>>,
    local_operator: Option<(usize, [u64; 3])>,
}

pub(super) struct PackedStep<'a> {
    factors: &'a Factors,
    dt: f64,
    theta: f64,
}

impl PackedStep<'_> {
    pub fn apply<const N: usize>(
        &self,
        old: &[[f64; N]],
        next: &mut [[f64; N]],
        [left, right]: [[f64; N]; 2],
    ) -> Result<()> {
        let Self { factors, dt, theta } = *self;
        let count = old.len() - 1;
        let size = count - 1;
        next[0] = left;
        next[count] = right;
        let explicit = (1.0 - theta) * dt;
        let implicit = theta * dt;
        let mut previous = [0.0; N];
        for (i, &[a, b, c]) in factors.coefficients.iter().enumerate() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let multiplier = factors.multipliers[i];
            let rhs = std::array::from_fn(|lane| {
                let mut value = old[i + 1][lane];
                if theta < 1.0 {
                    value += explicit * (a * old[i][lane] + b * value + c * old[i + 2][lane]);
                }
                if i == 0 {
                    value += implicit * a * left[lane];
                }
                if i + 1 == size {
                    value += implicit * c * right[lane];
                }
                value - multiplier * previous[lane]
            });
            next[i + 1] = rhs;
            previous = rhs;
        }
        let mut following = [0.0; N];
        for i in (0..size).rev() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let upper = factors.upper[i];
            let inverse = factors.inverse[i];
            let diagonal = factors.diagonal[i];
            let solve = |rhs| {
                if inverse.is_finite() {
                    rhs * inverse
                } else {
                    rhs / diagonal
                }
            };
            let rhs = next[i + 1];
            let values = std::array::from_fn(|lane| solve(rhs[lane] - upper * following[lane]));
            next[i + 1] = values;
            following = values;
        }
        if next.as_flattened().iter().any(|v| !v.is_finite()) {
            return Err(unstable());
        }
        Ok(())
    }
}

impl Stepper {
    const FACTOR_CACHE_LEN: usize = 3;
    // Each factor has three coefficients and four solve values per row. A miss
    // builds the new factor before eviction.
    // One local factor, the projection and prepared stencils fit within this peak.
    // Keep the grid admission budget conservative with one spare value per node.
    pub(super) const PEAK_BYTES_PER_NODE: u64 =
        ((Self::FACTOR_CACHE_LEN + 1) * size_of::<[f64; 7]>() + size_of::<f64>()) as u64;

    pub fn new() -> Self {
        Self {
            factors: Vec::new(),
            active: 0,
            local: None,
            local_stencils: None,
            local_operator: None,
        }
    }

    pub fn new_local(count: usize) -> Self {
        let size = count - 1;
        Self {
            factors: vec![Factors {
                key: [0.0; 5],
                coefficients: vec![[0.0; 3]; size],
                upper: vec![0.0; size],
                multipliers: vec![0.0; size],
                diagonal: vec![0.0; size],
                inverse: vec![0.0; size],
            }],
            active: 0,
            local: None,
            local_stencils: None,
            local_operator: None,
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "local step uses a fixed grid and dynamic surface and boundary inputs"
    )]
    pub fn advance_local(
        &mut self,
        old: &[&[f64]],
        next: &mut [&mut [f64]],
        space: &SpatialGrid,
        surface: &crate::LocalVolatilitySurface,
        [time, scale]: [f64; 2],
        [dt, theta, rate, dividend]: [f64; 4],
        boundaries: &[[f64; 2]],
    ) -> Result<()> {
        self.prepare_local(space, surface, [time, scale], [dt, theta, rate, dividend])?;
        for ((old, next), &boundaries) in old.iter().zip(next).zip(boundaries) {
            self.apply(old, next, dt, theta, boundaries, 0)?;
        }
        Ok(())
    }

    pub fn prepare_packed(
        &mut self,
        space: &SpatialGrid,
        diffusion: impl Diffusion,
        middle: f64,
        key: [f64; 4],
    ) -> Result<PackedStep<'_>> {
        let [dt, theta, _, _] = key;
        let scale = diffusion.variance_rate(middle).sqrt();
        let active = if let Some((surface, offset)) = diffusion.surface() {
            self.prepare_local(space, surface, [offset + middle, scale], key)?;
            0
        } else {
            self.prepare_constant(space, diffusion.sigma(middle, 0.0) * scale, key)?
        };
        Ok(PackedStep {
            factors: &self.factors[active],
            dt,
            theta,
        })
    }

    fn prepare_local(
        &mut self,
        space: &SpatialGrid,
        surface: &crate::LocalVolatilitySurface,
        [time, scale]: [f64; 2],
        [dt, theta, rate, dividend]: [f64; 4],
    ) -> Result<()> {
        if self.local.is_none() {
            if theta != 0.0 {
                self.local_stencils = space.prepared_stencils()?;
            }
            self.local = Some(crate::market::LocalVolatilityProjection::new(
                surface,
                &space.spots,
            )?);
        }
        let operator = surface
            .stationary_row(time)
            .map(|row| (row, [scale, rate, dividend].map(f64::to_bits)));
        let same_operator = operator.is_some() && self.local_operator == operator;
        let factors = &mut self.factors[0];
        let key = [dt, theta, rate, dividend, scale];
        let same_factors = same_operator && factors.key.map(f64::to_bits) == key.map(f64::to_bits);
        if !same_factors {
            let projection = self
                .local
                .as_mut()
                .expect("local projection is initialized");
            let volatility = (!same_operator)
                .then(|| projection.at_year(surface, time))
                .transpose()?;
            let carry = rate - dividend;
            let prepared = self.local_stencils.as_deref().filter(|_| {
                theta != 0.0 && super::grid::PreparedLogStencil::bounded([rate, carry])
            });
            for i in 0..factors.coefficients.len() {
                if i % 1024 == 0 {
                    crate::cancellation::check()?;
                }
                if let Some(volatility) = &volatility {
                    let sigma = volatility(i + 1) * scale;
                    let variance = sigma * sigma;
                    let row = if let Some(stencils) =
                        prepared.filter(|_| super::grid::PreparedLogStencil::bounded([variance]))
                    {
                        stencils[i].coefficients(rate, carry, variance)
                    } else {
                        space.coefficients(i + 1, rate, dividend, sigma)
                    };
                    if row.iter().any(|v| !v.is_finite()) {
                        return Err(unstable());
                    }
                    factors.coefficients[i] = row;
                }
                if theta == 0.0 && -dt * factors.coefficients[i][1] > 1.0 {
                    return Err(invalid_grid("explicit finite-difference grid is unstable"));
                }
                if theta != 0.0 {
                    factors.factor_row(i, dt, theta)?;
                }
            }
            self.local_operator = operator;
            factors.key = key;
        }
        Ok(())
    }

    pub fn transpose_local(
        &mut self,
        weights: &mut [f64],
        previous: &mut [f64],
        space: &SpatialGrid,
        surface: &crate::LocalVolatilitySurface,
        time_and_scale: [f64; 2],
        key: [f64; 4],
    ) -> Result<[f64; 2]> {
        self.prepare_local(space, surface, time_and_scale, key)?;
        Self::transpose(&self.factors[0], weights, previous, key)
    }

    pub fn transpose_constant(
        &mut self,
        weights: &mut [f64],
        previous: &mut [f64],
        space: &SpatialGrid,
        sigma: f64,
        key: [f64; 4],
    ) -> Result<[f64; 2]> {
        let active = self.prepare_constant(space, sigma, key)?;
        Self::transpose(&self.factors[active], weights, previous, key)
    }

    fn transpose(
        factors: &Factors,
        weights: &mut [f64],
        previous: &mut [f64],
        key: [f64; 4],
    ) -> Result<[f64; 2]> {
        let [dt, theta, _, _] = key;
        let count = weights.len() - 1;
        let mut edges = [weights[0], weights[count]];
        let rhs = &mut weights[1..count];
        // A = L U. Applying A^(-T) solves with U^T first, then L^T.
        for i in 0..rhs.len() {
            if i % 1024 == 0 {
                crate::cancellation::check()?;
            }
            let value = rhs[i]
                - if i == 0 {
                    0.0
                } else {
                    factors.upper[i - 1] * rhs[i - 1]
                };
            rhs[i] = if factors.inverse[i].is_finite() {
                value * factors.inverse[i]
            } else {
                value / factors.diagonal[i]
            };
        }
        for i in (1..rhs.len()).rev() {
            rhs[i - 1] -= factors.multipliers[i] * rhs[i];
        }
        edges[0] += theta * dt * factors.coefficients[0][0] * rhs[0];
        edges[1] += theta * dt * factors.coefficients[count - 2][2] * rhs[count - 2];
        previous.fill(0.0);
        let explicit = (1.0 - theta) * dt;
        for (i, (&weight, &[a, b, c])) in rhs.iter().zip(&factors.coefficients).enumerate() {
            previous[i] += explicit * a * weight;
            previous[i + 1] += (1.0 + explicit * b) * weight;
            previous[i + 2] += explicit * c * weight;
        }
        if previous
            .iter()
            .chain(&edges)
            .any(|value| !value.is_finite())
        {
            return Err(unstable());
        }
        Ok(edges)
    }

    pub fn advance(
        &mut self,
        old: &[f64],
        next: &mut [f64],
        grid: &SpatialGrid,
        sigma: f64,
        key: [f64; 4],
        boundaries: [f64; 2],
    ) -> Result<()> {
        let active = self.prepare_constant(grid, sigma, key)?;
        self.apply(old, next, key[0], key[1], boundaries, active)
    }

    #[expect(
        clippy::float_cmp,
        reason = "Factorization cache keys must match exactly before reusing a matrix solve."
    )]
    fn prepare_constant(&mut self, grid: &SpatialGrid, sigma: f64, key: [f64; 4]) -> Result<usize> {
        let [dt, theta, rate, dividend] = key;
        let key = [dt, theta, rate, dividend, sigma];
        let count = grid.count;
        let size = count - 1;
        Ok(
            if let Some(index) = self.factors.iter().position(|f| f.key == key) {
                index
            } else {
                let mut coefficients = Vec::with_capacity(size);
                for i in 1..count {
                    let row = grid.coefficients(i, rate, dividend, sigma);
                    if row.iter().any(|v| !v.is_finite()) {
                        return Err(unstable());
                    }
                    if theta == 0.0 && -dt * row[1] > 1.0 {
                        return Err(invalid_grid("explicit finite-difference grid is unstable"));
                    }
                    coefficients.push(row);
                }
                let mut factors = Factors {
                    key,
                    coefficients,
                    upper: vec![0.0; size],
                    multipliers: vec![0.0; size],
                    diagonal: vec![0.0; size],
                    inverse: vec![0.0; size],
                };
                if theta != 0.0 {
                    for i in 0..size {
                        factors.factor_row(i, dt, theta)?;
                    }
                }
                if self.factors.len() < Self::FACTOR_CACHE_LEN {
                    self.factors.push(factors);
                    self.factors.len() - 1
                } else {
                    self.active = (self.active + 1) % Self::FACTOR_CACHE_LEN;
                    self.factors[self.active] = factors;
                    self.active
                }
            },
        )
    }

    pub fn advance_tangent(
        &mut self,
        [old, before, after]: [&[f64]; 3],
        next: &mut [f64],
        [dt, theta, rate, dividend, sigma]: [f64; 5],
        boundaries: [f64; 2],
        derivative: &[[f64; 3]],
    ) -> Result<()> {
        let key = [dt, theta, rate, dividend, sigma].map(f64::to_bits);
        let active = self
            .factors
            .iter()
            .position(|f| f.key.map(f64::to_bits) == key)
            .ok_or_else(unstable)?;
        self.apply_with_source::<true>(old, next, dt, theta, boundaries, active, |i| {
            let apply = |values: &[f64]| {
                let [a, b, c] = derivative[i];
                a * values[i] + b * values[i + 1] + c * values[i + 2]
            };
            dt * ((1.0 - theta) * apply(before) + theta * apply(after))
        })
    }

    fn apply(
        &self,
        old: &[f64],
        next: &mut [f64],
        dt: f64,
        theta: f64,
        boundaries: [f64; 2],
        active: usize,
    ) -> Result<()> {
        self.apply_with_source::<false>(old, next, dt, theta, boundaries, active, |_| 0.0)
    }

    #[expect(
        clippy::float_cmp,
        clippy::too_many_arguments,
        reason = "Theta selects an exact scheme and tangents add a source to the same solve."
    )]
    fn apply_with_source<const TANGENT: bool>(
        &self,
        old: &[f64],
        next: &mut [f64],
        dt: f64,
        theta: f64,
        boundaries: [f64; 2],
        active: usize,
        source: impl Fn(usize) -> f64,
    ) -> Result<()> {
        let count = next.len() - 1;
        let size = count - 1;
        let factors = &self.factors[active];
        next[0] = boundaries[0];
        next[count] = boundaries[1];
        if theta == 0.0 {
            for i in 0..size {
                let [a, b, c] = factors.coefficients[i];
                let mut value = old[i + 1] + dt * (a * old[i] + b * old[i + 1] + c * old[i + 2]);
                if TANGENT {
                    value += source(i);
                }
                next[i + 1] = value;
            }
        } else {
            let mut previous = 0.0;
            for i in 0..size {
                let [a, b, c] = factors.coefficients[i];
                // Avoid evaluating an unused overflowing explicit term in implicit Euler.
                let mut value = if theta == 1.0 {
                    old[i + 1]
                } else {
                    old[i + 1] + (1.0 - theta) * dt * (a * old[i] + b * old[i + 1] + c * old[i + 2])
                };
                if TANGENT {
                    value += source(i);
                }
                if i == 0 {
                    value += theta * dt * a * boundaries[0];
                }
                if i + 1 == size {
                    value += theta * dt * c * boundaries[1];
                }
                if i != 0 {
                    value -= factors.multipliers[i] * previous;
                }
                next[i + 1] = value;
                previous = value;
            }
            let mut following = 0.0;
            for i in (0..size).rev() {
                let tail = if i + 1 < size {
                    factors.upper[i] * following
                } else {
                    0.0
                };
                let value = next[i + 1] - tail;
                let solved = if factors.inverse[i].is_finite() {
                    value * factors.inverse[i]
                } else {
                    value / factors.diagonal[i]
                };
                next[i + 1] = solved;
                following = solved;
            }
        }
        if next.iter().any(|v| !v.is_finite()) {
            return Err(unstable());
        }
        Ok(())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "shares a time grid and implicit operator across sensitivities"
)]
pub(super) fn march_tangents(
    mut values: Vec<f64>,
    space: &SpatialGrid,
    grid: &TimeGrid,
    [rate, dividend, sigma]: [f64; 3],
    scheme: FiniteDifferenceScheme,
    observations: &[f64],
    derivatives: &[Vec<[f64; 3]>; 3],
    edges: impl Fn(f64) -> [f64; 2],
    tangent_edges: impl Fn(f64, usize) -> [f64; 2],
    observe: impl Fn(&mut [f64], &mut [Vec<f64>; 3], f64),
) -> Result<(Vec<f64>, [Vec<f64>; 3])> {
    let mut next = vec![0.0; values.len()];
    let mut tangents = derivatives.each_ref().map(|row| {
        if row.is_empty() {
            Vec::new()
        } else {
            vec![0.0; values.len()]
        }
    });
    let mut next_tangents = tangents.clone();
    let mut stepper = Stepper::new();
    let mut damping = 2;
    for step in (0..grid.widths.len()).rev() {
        crate::cancellation::check()?;
        for (current, dt, theta) in grid.substeps(step, scheme, &mut damping) {
            stepper.advance(
                &values,
                &mut next,
                space,
                sigma,
                [dt, theta, rate, dividend],
                edges(current),
            )?;
            for (lane, derivative) in derivatives
                .iter()
                .enumerate()
                .filter(|(_, row)| !row.is_empty())
            {
                stepper.advance_tangent(
                    [&tangents[lane], &values, &next],
                    &mut next_tangents[lane],
                    [dt, theta, rate, dividend, sigma],
                    tangent_edges(current, lane),
                    derivative,
                )?;
            }
            std::mem::swap(&mut values, &mut next);
            std::mem::swap(&mut tangents, &mut next_tangents);
        }
        if observations
            .binary_search_by(|event| event.total_cmp(&grid.times[step]))
            .is_ok()
        {
            observe(&mut values, &mut tangents, grid.times[step]);
            damping = 2;
        }
    }
    Ok((values, tangents))
}
