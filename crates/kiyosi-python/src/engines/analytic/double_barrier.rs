use crate::error::domain;
use crate::instruments::double_barrier::DoubleBarrierOption;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
/// Flat-rate continuous double-barrier analytics.
///
/// See Also
/// --------
/// DoubleBarrierOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticDoubleBarrierEngine;

#[pymethods]
impl AnalyticDoubleBarrierEngine {
    #[new]
    fn new() -> Self {
        Self
    }
    /// Return the price without changing the instrument or context.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    ///
    /// Returns
    /// -------
    /// float
    ///     Present value of remaining contractual payments.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If domain validation, engine applicability or numerical pricing fails.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            DoubleBarrierOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(
            py,
            py.detach(|| core::AnalyticDoubleBarrierEngine.price(&option, &context)),
        )
    }
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            DoubleBarrierOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| {
                core::AnalyticDoubleBarrierEngine
                    .price_with_greeks(&option, &context, request, shifts)
            }),
        )?))
    }
}
