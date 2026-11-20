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
    rhs: Vec<f64>,
    active: usize,
}

impl Stepper {
    const FACTOR_CACHE_LEN: usize = 3;
    // Each factor has three coefficients and four solve values per row. A miss
    // builds the new factor before eviction, with a separate right-hand side.
    pub(super) const PEAK_BYTES_PER_NODE: u64 =
        ((Self::FACTOR_CACHE_LEN + 1) * size_of::<[f64; 7]>() + size_of::<f64>()) as u64;

    pub fn new(count: usize) -> Self {
        Self {
            factors: Vec::new(),
            rhs: vec![0.0; count - 1],
            active: 0,
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
            rhs: vec![0.0; size],
            active: 0,
        }
    }

    pub fn advance_local(
        &mut self,
        old: &[f64],
        next: &mut [f64],
        coefficients: impl Fn(usize) -> [f64; 3],
        [dt, theta]: [f64; 2],
        boundaries: [f64; 2],
    ) -> Result<()> {
        let factors = &mut self.factors[0];
        let size = factors.coefficients.len();
        for i in 0..size {
            let [a, b, c] = coefficients(i + 1);
            if [a, b, c].iter().any(|v| !v.is_finite()) {
                return Err(unstable());
            }
            if theta == 0.0 && -dt * b > 1.0 {
                return Err(invalid_grid("explicit finite-difference grid is unstable"));
            }
            factors.coefficients[i] = [a, b, c];
            if theta != 0.0 {
                factors.factor_row(i, dt, theta)?;
            }
        }
        self.apply(old, next, dt, theta, boundaries, 0)
    }

    #[expect(
        clippy::float_cmp,
        reason = "Factorization cache keys must match exactly before reusing a matrix solve."
    )]
    pub fn advance(
        &mut self,
        old: &[f64],
        next: &mut [f64],
        grid: &SpatialGrid,
        sigma: f64,
        key: [f64; 4],
        boundaries: [f64; 2],
    ) -> Result<()> {
        let [dt, theta, rate, dividend] = key;
        let key = [dt, theta, rate, dividend, sigma];
        let count = grid.count;
        let size = count - 1;
        let active = if let Some(index) = self.factors.iter().position(|f| f.key == key) {
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
        };
        self.apply(old, next, dt, theta, boundaries, active)
    }

    #[expect(
        clippy::float_cmp,
        reason = "Theta equal to one selects the exact implicit Euler scheme."
    )]
    fn apply(
        &mut self,
        old: &[f64],
        next: &mut [f64],
        dt: f64,
        theta: f64,
        boundaries: [f64; 2],
        active: usize,
    ) -> Result<()> {
        let count = next.len() - 1;
        let size = count - 1;
        let factors = &self.factors[active];
        next[0] = boundaries[0];
        next[count] = boundaries[1];
        for i in 0..size {
            let [a, b, c] = factors.coefficients[i];
            // Avoid evaluating an unused overflowing explicit term in implicit Euler.
            self.rhs[i] = if theta == 1.0 {
                old[i + 1]
            } else {
                old[i + 1] + (1.0 - theta) * dt * (a * old[i] + b * old[i + 1] + c * old[i + 2])
            };
            if i == 0 {
                self.rhs[i] += theta * dt * a * boundaries[0];
            }
            if i + 1 == size {
                self.rhs[i] += theta * dt * c * boundaries[1];
            }
        }
        if theta != 0.0 {
            for i in 1..size {
                self.rhs[i] -= factors.multipliers[i] * self.rhs[i - 1];
            }
            for i in (0..size).rev() {
                let value = self.rhs[i]
                    - if i + 1 < size {
                        factors.upper[i] * self.rhs[i + 1]
                    } else {
                        0.0
                    };
                self.rhs[i] = if factors.inverse[i].is_finite() {
                    value * factors.inverse[i]
                } else {
                    value / factors.diagonal[i]
                };
            }
        }
        next[1..count].copy_from_slice(&self.rhs);
        if next.iter().any(|v| !v.is_finite()) {
            return Err(unstable());
        }
        Ok(())
    }
}
