use super::Pcg32;
use crate::{Error, Result};

const BITS: usize = 52;
const MASK: u64 = (1 << BITS) - 1;

include!("sobol_joe_kuo.rs");

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
    #[expect(
        clippy::cast_precision_loss,
        reason = "Sobol values contain at most 52 bits and convert exactly to f64."
    )]
    pub(super) fn normal(&mut self) -> f64 {
        // Midpoints keep every point, including index zero, strictly inside (0, 1).
        let probability = (self.values[self.dimension] as f64 + 0.5) / 4_503_599_627_370_496.0;
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
        -59.963_350_101_410_79,
        98.001_075_418_599_97,
        -56.676_285_746_907_03,
        13.931_260_938_727_968,
        -1.239_165_838_673_812_5,
    ];
    const Q0: &[f64] = &[
        1.954_488_583_381_417_6,
        4.676_279_128_988_815,
        86.360_242_139_089_05,
        -225.462_687_854_119_37,
        200.260_212_380_060_66,
        -82.037_225_616_833_34,
        15.905_622_512_621_17,
        -1.183_316_211_213_3,
    ];
    const P1: &[f64] = &[
        4.055_448_923_059_624_5,
        31.525_109_459_989_388,
        57.162_819_224_642_13,
        44.080_507_389_320_08,
        14.684_956_192_885_803,
        2.186_633_068_507_902_5,
        -0.140_256_079_171_354_5,
        -0.035_042_462_682_784_82,
        -0.000_857_456_785_154_685_4,
    ];
    const Q1: &[f64] = &[
        15.779_988_325_646_675,
        45.390_763_512_887_92,
        41.317_203_825_467_2,
        15.042_538_569_290_75,
        2.504_649_462_083_094,
        -0.142_182_922_854_787_79,
        -0.038_080_640_769_157_83,
        -0.000_933_259_480_895_457_4,
    ];
    const P2: &[f64] = &[
        3.237_748_917_769_460_3,
        6.915_228_890_689_842,
        3.938_810_252_924_744_4,
        1.333_034_608_158_075_5,
        0.201_485_389_549_179_08,
        0.012_371_663_481_782_003,
        0.000_301_581_553_508_235_43,
        2.658_069_746_867_375_5e-06,
        6.239_745_391_849_833e-09,
    ];
    const Q2: &[f64] = &[
        6.024_270_393_647_42,
        3.679_835_638_561_608_7,
        1.377_020_994_890_813_2,
        0.216_236_993_594_496_63,
        0.013_420_400_608_854_318,
        0.000_328_014_464_682_127_74,
        2.892_478_647_453_806_8e-06,
        6.790_194_080_099_813e-09,
    ];
    const TAIL_BOUNDARY: f64 = 0.135_335_283_236_612_7;
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
