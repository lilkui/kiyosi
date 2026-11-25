use std::{any::Any, fmt, ops::RangeInclusive, sync::Arc};

use super::{SpatialGrid, grid::Spline, local_volatility};
use crate::{
    Error, EuropeanOption, Greek, GreeksRequest, NumericalShiftSettings, OptionContract,
    PricingContext, PricingContextChanges, PricingEngine, PricingResult, Result, Timestamp,
};

/// Prepares owned finite-difference solutions for a supported contract type.
///
/// Import this trait alongside [`PricingEngine`]. The instrument parameter follows
/// that trait's dispatch; other engine families do not implement this operation.
pub trait FiniteDifferenceEngine<I = EuropeanOption>: PricingEngine<I> {
    /// Solves for a finite, positive, strictly increasing closed spot interval.
    ///
    /// Fixes valuation time, model inputs, contract and supplied history. Current
    /// observations use each queried spot. The interval need not contain the
    /// context's spot. Multiple financial states may require multiple rollbacks.
    /// Range endpoints are in spot-price units. The solver uses log spot internally.
    /// Inputs remain unchanged; failure or cancellation returns no partial solution.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for an invalid interval or required grid
    /// coverage; propagates the engine's applicability, life, history, coverage,
    /// stability and numerical errors. Unused numerical bounds are not checked.
    ///
    /// # Examples
    ///
    /// ```
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let option = EuropeanOption::new(OptionType::Call, 100.0, start, end)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start)?;
    /// let solution = FiniteDifferenceVanillaEngine::default()
    ///     .solve(&option, &context, 80.0..=120.0)?;
    /// assert!(solution.price(105.0)? > solution.price(95.0)?);
    /// # Ok(()) }
    /// ```
    fn solve(
        &self,
        instrument: &I,
        context: &PricingContext,
        spot_range: RangeInclusive<f64>,
    ) -> Result<FiniteDifferenceSolution>;
}

/// Owns a fixed-time finite-difference solution for repeated spot queries.
///
/// Create through [`FiniteDifferenceEngine::solve`]. Queries never perform a
/// numerical rollback. Mesh accuracy and truncation require convergence checks;
/// rebuilding a scalar engine's mesh at another spot need not give identical bits.
/// Clones share immutable storage and support concurrent queries after the source
/// objects are dropped. See the library-wide cancellation and concurrency contract.
#[derive(Clone)]
pub struct FiniteDifferenceSolution {
    context: PricingContext,
    range: RangeInclusive<f64>,
    supported: GreeksRequest,
    query_work: usize,
    value: Arc<dyn Erased>,
}

impl PartialEq for FiniteDifferenceSolution {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context
            && self.range == other.range
            && self.supported == other.supported
            && (Arc::ptr_eq(&self.value, &other.value) || self.value.same(other.value.as_ref()))
    }
}

impl fmt::Debug for FiniteDifferenceSolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FiniteDifferenceSolution")
            .field("spot_range", &self.range)
            .field("valuation_time", &self.valuation_time())
            .field("supported", &self.supported)
            .finish_non_exhaustive()
    }
}

impl FiniteDifferenceSolution {
    pub(super) fn new<I, B>(
        instrument: I,
        context: &PricingContext,
        range: RangeInclusive<f64>,
        backend: B,
    ) -> Self
    where
        I: OptionContract + PartialEq + Send + Sync + 'static,
        B: Backend<I>,
    {
        let supported = if matches!(
            context.model_parameters(),
            crate::ModelParameters::Heston(_)
        ) {
            GreeksRequest::new([])
        } else if local_volatility::local_model(context).is_some() {
            GreeksRequest::new([Greek::Delta, Greek::Gamma])
        } else {
            GreeksRequest::new([Greek::Delta, Greek::Gamma, Greek::Speed])
        };
        let query_work = if backend.queries_may_scan_calendar()
            && context.valuation_time() < instrument.expiry_time()
            && context
                .model_parameters()
                .variance_time_convention()
                .calendar()
                .is_some()
        {
            // A closed calendar can require scanning the full horizon before pricing.
            usize::MAX
        } else {
            backend.query_work(&instrument, context)
        };
        Self {
            context: context.clone(),
            range,
            supported,
            query_work,
            value: Arc::new(Prepared {
                instrument,
                backend,
            }),
        }
    }

    /// Returns the closed query interval; see [`FiniteDifferenceEngine::solve`].
    pub fn spot_range(&self) -> RangeInclusive<f64> {
        self.range.clone()
    }

    /// Returns the fixed valuation timestamp; see [`Self::spot_range`].
    pub fn valuation_time(&self) -> Timestamp {
        self.context.valuation_time()
    }

    /// Returns potential capabilities; individual query statuses can be unavailable.
    ///
    /// BSM solutions support delta/gamma/speed, local volatility delta/gamma,
    /// and Heston no Greeks. See [`PricingResult::status`].
    pub fn supported(&self) -> GreeksRequest {
        self.supported
    }

    /// Estimates per-spot work for dispatching cancellable queries.
    #[doc(hidden)]
    pub fn query_work(&self) -> usize {
        self.query_work
    }

    /// Evaluates a spot without rollback; see [`FiniteDifferenceEngine::solve`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for non-finite, negative or out-of-interval
    /// spots. Propagates financial validation, numerical and cancellation errors.
    pub fn price(&self, spot_price: f64) -> Result<f64> {
        Ok(self
            .price_with_greeks(spot_price, GreeksRequest::new([]), None)?
            .price())
    }

    /// Evaluates spots in order, preserving duplicates; empty input is valid.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors, without partial output or mutation.
    /// All spot domains are validated before evaluation.
    pub fn price_many(&self, spot_prices: &[f64]) -> Result<Vec<f64>> {
        self.validate_spots(spot_prices)?;
        let request = GreeksRequest::new([]);
        let shifts = NumericalShiftSettings::default();
        spot_prices
            .iter()
            .map(|&spot| Ok(self.evaluate(spot, request, shifts)?.price()))
            .collect()
    }

    /// Evaluates price and requested spot Greeks on the saved solution.
    ///
    /// `None` uses core automatic spot shifts. Required difference samples must
    /// stay inside the query interval; unavailable stencils and mathematical
    /// discontinuities retain distinct [`PricingResult::status`] values.
    /// Other requested Greeks are unsupported, including all Heston Greeks.
    /// See [`Self::supported`] and [`FiniteDifferenceEngine::solve`].
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price`] errors and returns [`Error::InvalidInput`] for a
    /// nonpositive or non-finite explicit shift, even for unused controls.
    pub fn price_with_greeks(
        &self,
        spot_price: f64,
        request: GreeksRequest,
        spot_shift: Option<f64>,
    ) -> Result<PricingResult> {
        let shifts = NumericalShiftSettings {
            spot_shift,
            ..Default::default()
        };
        shifts.validate()?;
        self.validate_spot(spot_price)?;
        self.evaluate(spot_price, request, shifts)
    }

    #[expect(
        clippy::float_cmp,
        reason = "Exact equality verifies a representably symmetric spot stencil."
    )]
    fn evaluate(
        &self,
        spot_price: f64,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        crate::cancellation::check()?;
        let context = at_spot(&self.context, spot_price)?;
        let active = GreeksRequest::new(
            Greek::ALL
                .into_iter()
                .filter(|&g| request.has(g) && self.supported.has(g)),
        );
        if active == GreeksRequest::new([]) {
            return Ok(self
                .value
                .evaluate(&context, active, shifts)?
                .with_request(request, self.supported));
        }
        let h =
            crate::risk::numerical::symmetric_shift(spot_price, shifts.spot_shift_at(spot_price));
        let undefined = self.at_spot_discontinuity(&context, 0.0);
        let stencil = h > 0.0
            && self.range.contains(&(spot_price - h))
            && self.range.contains(&(spot_price + h))
            && !self.at_spot_discontinuity(&context, h);
        let native = GreeksRequest::new(
            Greek::ALL
                .into_iter()
                .filter(|&g| active.has(g) && !undefined && (g != Greek::Speed || stencil)),
        );
        let mut result = self.value.evaluate(&context, native, shifts)?;
        if undefined {
            return Ok(PricingResult::new(result.price())?.with_request(request, self.supported));
        }
        if stencil
            && [Greek::Delta, Greek::Gamma, Greek::Speed]
                .into_iter()
                .any(|g| active.has(g) && !result.has(g))
        {
            let sample_request = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
            let sample = |spot| {
                self.value
                    .evaluate(&at_spot(&context, spot)?, sample_request, shifts)
            };
            let up = sample(spot_price + h)?;
            let down = sample(spot_price - h)?;
            if active.has(Greek::Delta) && !result.has(Greek::Delta) {
                result.set(
                    active,
                    Greek::Delta,
                    (up.price() - down.price()) / (2.0 * h),
                )?;
            }
            if active.has(Greek::Gamma) && !result.has(Greek::Gamma) {
                result.set(
                    active,
                    Greek::Gamma,
                    ((up.price() - result.price()) - (result.price() - down.price())) / h / h,
                )?;
            }
            if active.has(Greek::Speed) && !result.has(Greek::Speed) {
                if let (Some(up), Some(down)) = (up.get(Greek::Gamma), down.get(Greek::Gamma)) {
                    result.set(active, Greek::Speed, (up - down) / (2.0 * h))?;
                } else {
                    let two_h = 2.0 * h;
                    if two_h.is_finite()
                        && self.range.contains(&(spot_price - two_h))
                        && self.range.contains(&(spot_price + two_h))
                        && crate::risk::numerical::symmetric_shift(spot_price, two_h) == two_h
                        && !self.at_spot_discontinuity(&context, two_h)
                    {
                        let up2 = sample(spot_price + two_h)?.price();
                        let down2 = sample(spot_price - two_h)?.price();
                        result.set(
                            active,
                            Greek::Speed,
                            ((up2 - up.price()) - (up.price() - down.price())
                                + (down.price() - down2))
                                / h
                                / h
                                / two_h,
                        )?;
                    }
                }
            }
        }
        Ok(result
            .with_request(request, self.supported)
            .with_unavailable_stencils(active))
    }

    /// Evaluates price/Greeks in input order with no partial result on failure.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::price_with_greeks`] errors. Controls and all spot domains
    /// are validated even for empty input, before any evaluation.
    pub fn price_many_with_greeks(
        &self,
        spot_prices: &[f64],
        request: GreeksRequest,
        spot_shift: Option<f64>,
    ) -> Result<Vec<PricingResult>> {
        let shifts = NumericalShiftSettings {
            spot_shift,
            ..Default::default()
        };
        shifts.validate()?;
        self.validate_spots(spot_prices)?;
        spot_prices
            .iter()
            .map(|&spot| self.evaluate(spot, request, shifts))
            .collect()
    }

    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.value.at_spot_discontinuity(context, radius)
    }

    fn validate_spots(&self, spot_prices: &[f64]) -> Result<()> {
        for &spot in spot_prices {
            crate::cancellation::check()?;
            self.validate_spot(spot)?;
        }
        crate::cancellation::check()
    }

    fn validate_spot(&self, spot: f64) -> Result<()> {
        crate::error::finite(spot, true, "spot_price")?;
        if !self.range.contains(&spot) {
            return Err(Error::InvalidInput(
                "spot_price is outside the solution interval".into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_range(range: &RangeInclusive<f64>) -> Result<()> {
    crate::error::finite(*range.start(), true, "spot_range lower bound")?;
    crate::error::finite(*range.end(), true, "spot_range upper bound")?;
    if range.start() >= range.end() {
        return Err(Error::InvalidInput(
            "spot_range must be strictly increasing".into(),
        ));
    }
    Ok(())
}

pub(super) fn at_spot(context: &PricingContext, spot: f64) -> Result<PricingContext> {
    context.replace(PricingContextChanges {
        spot_price: Some(spot),
        ..Default::default()
    })
}

pub(super) fn monitoring_query_work(
    monitoring: &crate::instruments::monitoring::Monitoring,
    context: &PricingContext,
) -> usize {
    let start = context.valuation_time();
    let end = monitoring.expiry_time();
    if start >= end || !crate::engines::deterministic::enabled(context) {
        return 0;
    }
    if monitoring.observation_mode() == crate::ObservationMode::Scheduled {
        let times = monitoring.observation_times();
        return times.len() - times.partition_point(|&time| time <= start);
    }
    let model = context.model_parameters();
    let intervals = [model.risk_free_curve(), model.dividend_curve()]
        .map(|curve| super::Curve::interval_count(curve, start, end));
    usize::try_from(intervals[0] + intervals[1]).unwrap_or(usize::MAX)
}

pub(super) fn probes(
    range: &RangeInclusive<f64>,
    levels: impl IntoIterator<Item = f64>,
) -> Vec<f64> {
    let mut points = vec![*range.start(), *range.end()];
    points.extend(levels.into_iter().filter(|s| range.contains(s)));
    points.sort_by(f64::total_cmp);
    points.dedup();
    let middle: Vec<_> = points
        .windows(2)
        .map(|pair| pair[0].midpoint(pair[1]))
        .collect();
    points.extend(middle);
    points
}

#[derive(PartialEq)]
pub(super) struct Grid {
    pub space: SpatialGrid,
    pub spline: Spline,
}
impl Grid {
    pub fn new(space: SpatialGrid, values: Vec<f64>) -> Self {
        let spline = space.spline(values);
        Self { space, spline }
    }
    pub fn sample(&self, spot: f64) -> [f64; 3] {
        self.space.sample(&self.spline, spot)
    }
    pub fn speed(&self, spot: f64, shift: f64) -> Option<f64> {
        self.space.speed(&self.spline, spot, shift)
    }
}

pub(super) trait Backend<I>: PartialEq + Send + Sync + 'static {
    fn at_spot_discontinuity(&self, instrument: &I, context: &PricingContext, radius: f64) -> bool
    where
        I: OptionContract,
    {
        instrument.at_spot_discontinuity(context, radius)
    }

    fn query_work(&self, _instrument: &I, _context: &PricingContext) -> usize {
        0
    }

    fn queries_may_scan_calendar(&self) -> bool {
        true
    }

    fn evaluate(
        &self,
        instrument: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult>;
}

#[derive(PartialEq)]
struct Prepared<I, B> {
    instrument: I,
    backend: B,
}

trait Erased: Send + Sync {
    fn evaluate(
        &self,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult>;
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn same(&self, other: &dyn Erased) -> bool;
}

impl<I, B> Erased for Prepared<I, B>
where
    I: OptionContract + PartialEq + Send + Sync + 'static,
    B: Backend<I>,
{
    fn evaluate(
        &self,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult> {
        self.backend
            .evaluate(&self.instrument, context, request, shifts)
    }
    fn at_spot_discontinuity(&self, context: &PricingContext, radius: f64) -> bool {
        self.backend
            .at_spot_discontinuity(&self.instrument, context, radius)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn same(&self, other: &dyn Erased) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
}
