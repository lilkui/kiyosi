use super::Pcg32;
use crate::{Error, Result};

const BITS: usize = 52;
const MASK: u64 = (1 << BITS) - 1;

include!("sobol_joe_kuo.rs");

// Ported from the existing CPU Sobol generator.
pub(super) struct SobolSequence {
    directions: Vec<u64>,
    shifts: Vec<u64>,
}

impl SobolSequence {
    pub(super) fn new(dimensions: usize, seed: u64) -> Result<Self> {
        if !(1..=21_201).contains(&dimensions) {
            return Err(Error::InvalidInput(
                "Sobol dimension must be in 1..=21,201".into(),
            ));
        }
        let mut directions = vec![0; dimensions * BITS];
        let mut shifts = Vec::with_capacity(dimensions);
        let mut polynomial = POLYNOMIAL_DATA.as_slice();
        for dimension in 0..dimensions {
            let mut raw = [0u64; BITS];
            if dimension == 0 {
                for (column, value) in raw.iter_mut().enumerate() {
                    *value = 1 << (BITS - column - 1);
                }
            } else {
                // The bundled table contains exactly 21,200 polynomial records.
                let degree = polynomial[0] as usize;
                let coefficients = polynomial[1];
                for (column, value) in raw.iter_mut().enumerate().take(degree) {
                    *value = u64::from(polynomial[column + 2]) << (BITS - column - 1);
                }
                polynomial = &polynomial[degree + 2..];
                for column in degree..BITS {
                    let mut direction = raw[column - degree] ^ (raw[column - degree] >> degree);
                    for offset in 1..degree {
                        if (coefficients >> (degree - offset - 1)) & 1 != 0 {
                            direction ^= raw[column - offset];
                        }
                    }
                    raw[column] = direction;
                }
            }
            let mut generator = Pcg32::new(seed, dimension as u64);
            let mut random_word =
                || (u64::from(generator.next()) << 32) | u64::from(generator.next());
            shifts.push(random_word() & MASK);
            let matrix: [u64; BITS] = std::array::from_fn(|row| {
                let bit = BITS - row - 1;
                (random_word() & (MASK << (bit + 1))) | (1 << bit)
            });
            for (column, value) in raw.iter().enumerate() {
                let mut scrambled = 0;
                for (row, mask) in matrix.iter().enumerate() {
                    scrambled |= u64::from((value & mask).count_ones() & 1) << (BITS - row - 1);
                }
                directions[column * dimensions + dimension] = scrambled;
            }
        }
        Ok(Self { directions, shifts })
    }

    pub(super) fn cursor(&self, index: usize) -> SobolCursor<'_> {
        let mut values = self.shifts.clone();
        let mut gray = index ^ (index >> 1);
        while gray != 0 {
            let column = gray.trailing_zeros() as usize;
            for (dimension, value) in values.iter_mut().enumerate() {
                *value ^= self.directions[column * self.shifts.len() + dimension];
            }
            gray &= gray - 1;
        }
        SobolCursor {
            sequence: self,
            values,
            index,
            dimension: 0,
        }
    }
}

pub(super) struct SobolCursor<'a> {
    sequence: &'a SobolSequence,
    values: Vec<u64>,
    index: usize,
    dimension: usize,
}

impl SobolCursor<'_> {
    pub(super) fn normal(&mut self) -> f64 {
        // Midpoints keep every point, including index zero, strictly inside (0, 1).
        let probability = (self.values[self.dimension] as f64 + 0.5) / 4503599627370496.0;
        self.dimension += 1;
        inverse_normal_cdf(probability)
    }

    pub(super) fn next_path(&mut self) {
        self.index += 1;
        let column = self.index.trailing_zeros() as usize;
        for (dimension, value) in self.values.iter_mut().enumerate() {
            *value ^= self.sequence.directions[column * self.sequence.shifts.len() + dimension];
        }
        self.dimension = 0;
    }
}

fn polynomial(value: f64, coefficients: &[f64], leading: f64) -> f64 {
    coefficients
        .iter()
        .fold(leading, |result, coefficient| result * value + coefficient)
}

// Cephes ndtri, Copyright 1984, 1987, 1989 Stephen L. Moshier;
// C++ translation Copyright 2024 SciPy.
fn inverse_normal_cdf(probability: f64) -> f64 {
    const P0: &[f64] = &[
        -59.96335010141079,
        98.00107541859997,
        -56.67628574690703,
        13.931260938727968,
        -1.2391658386738125,
    ];
    const Q0: &[f64] = &[
        1.9544885833814176,
        4.676279128988815,
        86.36024213908905,
        -225.46268785411937,
        200.26021238006066,
        -82.03722561683334,
        15.90562251262117,
        -1.1833162112133,
    ];
    const P1: &[f64] = &[
        4.0554489230596245,
        31.525109459989388,
        57.16281922464213,
        44.08050738932008,
        14.684956192885803,
        2.1866330685079025,
        -0.1402560791713545,
        -0.03504246268278482,
        -0.0008574567851546854,
    ];
    const Q1: &[f64] = &[
        15.779988325646675,
        45.39076351288792,
        41.3172038254672,
        15.04253856929075,
        2.504649462083094,
        -0.14218292285478779,
        -0.03808064076915783,
        -0.0009332594808954574,
    ];
    const P2: &[f64] = &[
        3.2377489177694603,
        6.915228890689842,
        3.9388102529247444,
        1.3330346081580755,
        0.20148538954917908,
        0.012371663481782003,
        0.00030158155350823543,
        2.6580697468673755e-06,
        6.239745391849833e-09,
    ];
    const Q2: &[f64] = &[
        6.02427039364742,
        3.6798356385616087,
        1.3770209948908132,
        0.21623699359449663,
        0.013420400608854318,
        0.00032801446468212774,
        2.8924786474538068e-06,
        6.790194080099813e-09,
    ];
    const TAIL_BOUNDARY: f64 = 0.1353352832366127;
    let upper = probability > 1.0 - TAIL_BOUNDARY;
    let tail = if upper {
        1.0 - probability
    } else {
        probability
    };
    if tail > TAIL_BOUNDARY {
        let centered = tail - 0.5;
        let square = centered * centered;
        return (centered
            + centered * square * polynomial(square, P0, 0.0) / polynomial(square, Q0, 1.0))
            * (2.0 * std::f64::consts::PI).sqrt();
    }
    let root = (-2.0 * tail.ln()).sqrt();
    let inverse = 1.0 / root;
    let correction = inverse
        * if root < 8.0 {
            polynomial(inverse, P1, 0.0) / polynomial(inverse, Q1, 1.0)
        } else {
            polynomial(inverse, P2, 0.0) / polynomial(inverse, Q2, 1.0)
        };
    let quantile = root - root.ln() / root - correction;
    if upper { quantile } else { -quantile }
}
