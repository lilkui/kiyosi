use super::*;

struct Factors {
    key: [f64; 4],
    coefficients: Vec<[f64; 3]>,
    upper: Vec<f64>,
    multipliers: Vec<f64>,
    diagonal: Vec<f64>,
    inverse: Vec<f64>,
}

pub(super) struct Stepper {
    factors: Vec<Factors>,
    rhs: Vec<f64>,
    active: usize,
}

impl Stepper {
    pub fn new(count: usize) -> Self {
        Self {
            factors: Vec::new(),
            rhs: vec![0.0; count - 1],
            active: 0,
        }
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
        let [dt, theta, rate, dividend] = key;
        let count = grid.count;
        let size = count - 1;
        let active = if let Some(index) = self.factors.iter().position(|f| f.key == key) {
            index
        } else {
            let coefficients: Vec<_> = (1..count)
                .map(|i| grid.coefficients(i, rate, dividend, sigma))
                .collect();
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
                    let [a, b, c] = factors.coefficients[i];
                    factors.upper[i] = -theta * dt * c;
                    factors.diagonal[i] = 1.0 - theta * dt * b;
                    if i > 0 {
                        let pivot = factors.diagonal[i - 1];
                        factors.multipliers[i] = -theta * dt * a / pivot;
                        factors.diagonal[i] -= factors.multipliers[i] * factors.upper[i - 1];
                    }
                    if !factors.diagonal[i].is_finite() || factors.diagonal[i] == 0.0 {
                        return Err(unstable());
                    }
                    factors.inverse[i] = 1.0 / factors.diagonal[i];
                }
            }
            if self.factors.len() < 3 {
                self.factors.push(factors);
                self.factors.len() - 1
            } else {
                self.active = (self.active + 1) % 3;
                self.factors[self.active] = factors;
                self.active
            }
        };
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
