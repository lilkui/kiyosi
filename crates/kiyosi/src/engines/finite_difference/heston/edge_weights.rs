use crate::{Result, numerics::Sum};

use super::super::Curve;

pub(super) const BYTES_PER_STEP: u64 = 256;

type Boundary = (usize, f64, f64);

/// Accumulates boundary weights by their strike cutoffs in valuation-date units.
pub(super) struct EdgeWeights {
    boundaries: Vec<[Boundary; 2]>,
    cutoffs: Vec<f64>,
    tree: Vec<[Sum; 4]>,
    sign: f64,
}

impl EdgeWeights {
    pub(super) fn new(
        times: &[f64],
        [risk, dividend]: [&Curve; 2],
        bounds: [f64; 2],
        sign: f64,
    ) -> Result<Option<Self>> {
        let mut terms = Vec::with_capacity(times.len());
        let mut cutoffs = Vec::with_capacity(2 * times.len());
        for &time in times {
            crate::cancellation::check()?;
            // P(t,T) = P(0,T)/P(0,t). Factoring P(0,T) out of each
            // leg leaves a fixed cutoff at each boundary and time.
            let cash = (-risk.log_factor(0.0, time)).exp();
            let asset = (-dividend.log_factor(0.0, time)).exp();
            let pair = bounds.map(|spot| {
                let asset = spot * asset;
                [sign * (asset / cash), asset, cash]
            });
            if pair.iter().flatten().any(|value| !value.is_normal()) {
                return Ok(None);
            }
            cutoffs.extend(pair.map(|[cutoff, _, _]| cutoff));
            terms.push(pair);
        }
        cutoffs.sort_unstable_by(|a, b| b.total_cmp(a));
        cutoffs.dedup_by(|a, b| a.to_bits() == b.to_bits());
        let boundaries = terms
            .into_iter()
            .map(|pair| {
                crate::cancellation::check()?;
                Ok(pair.map(|[cutoff, asset, cash]| {
                    let index = cutoffs
                        .binary_search_by(|value| cutoff.total_cmp(value))
                        .expect("boundary cutoff is indexed");
                    (index, asset, cash)
                }))
            })
            .collect::<Result<_>>()?;
        Ok(Some(Self {
            boundaries,
            tree: vec![[Sum::default(); 4]; cutoffs.len() + 1],
            cutoffs,
            sign,
        }))
    }

    pub(super) fn add(&mut self, step: usize, weights: [f64; 2]) -> Option<()> {
        for ((index, asset, cash), weight) in self.boundaries[step].into_iter().zip(weights) {
            let [asset, cash] = [weight * asset, weight * cash];
            if !asset.is_finite() || !cash.is_finite() {
                return None;
            }
            let mut node = index + 1;
            while node < self.tree.len() {
                for (sum, term) in
                    self.tree[node]
                        .iter_mut()
                        .zip([asset, cash, asset.abs(), cash.abs()])
                {
                    sum.add(term);
                }
                node += node & node.wrapping_neg();
            }
        }
        Some(())
    }

    pub(super) fn at(&self, strike: f64, [risk, dividend]: [f64; 2]) -> Option<[f64; 2]> {
        let normalized = strike * (risk / dividend);
        if !risk.is_normal() || !dividend.is_normal() || !normalized.is_normal() {
            return None;
        }
        let mut node = self
            .cutoffs
            .partition_point(|&cutoff| cutoff > self.sign * normalized);
        let mut sums = [Sum::default(); 4];
        while node != 0 {
            for (sum, partial) in sums.iter_mut().zip(self.tree[node]) {
                sum.merge(partial);
            }
            node &= node - 1;
        }
        let [asset, cash, abs_asset, abs_cash] = sums.map(|sum| sum.total());
        let value = self.sign * dividend * (-normalized).mul_add(cash, asset);
        let scale = dividend * normalized.mul_add(abs_cash, abs_asset);
        (value.is_finite() && scale.is_finite()).then_some([value, scale])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Date, RateTermStructure};

    #[test]
    fn indexed_boundary_terms_match_each_expirys_discounted_payoffs() -> Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let dates = (1..=4)
            .map(|year| start.add_days(365 * year))
            .collect::<Result<Vec<_>>>()?;
        let risk = RateTermStructure::new(start, dates.clone(), vec![0.6, 1.0, 0.85, 1.2])?;
        let dividend = RateTermStructure::new(start, dates.clone(), vec![0.9, 0.75, 1.1, 1.0])?;
        let end = dates[3];
        let risk = Curve::new(&risk, start, end)?;
        let dividend = Curve::new(&dividend, start, end)?;
        let curves = [&risk, &dividend];
        let times = [0.0, 1.0, 2.0, 3.0];
        let bounds = [75.0, 125.0];
        let weights = [[0.5, 0.08], [0.2, -0.07], [-0.2, 0.2], [0.05, 0.09]];
        for sign in [-1.0, 1.0] {
            let mut index = EdgeWeights::new(&times, curves, bounds, sign)?.unwrap();
            for (step, weight) in weights.into_iter().enumerate() {
                index.add(step, weight).unwrap();
                let expiry = times[step] + 1.0;
                let factors = curves.map(|curve| curve.log_factor(0.0, expiry).exp());
                for strike in [60.0, 75.0, 100.0, 125.0, 150.0] {
                    let mut expected = Sum::default();
                    for (&time, weights) in times[..=step].iter().zip(weights) {
                        let [cash, asset] =
                            curves.map(|curve| curve.log_factor(time, expiry).exp());
                        for (spot, weight) in bounds.into_iter().zip(weights) {
                            expected.add(weight * (sign * (spot * asset - strike * cash)).max(0.0));
                        }
                    }
                    let [actual, scale] = index.at(strike, factors).unwrap();
                    assert!((actual - expected.total()).abs() <= 1e-14 * scale.max(1.0));
                }
            }
        }
        Ok(())
    }
}
