use crate::error::domain;
use crate::instruments::binary_barrier::BinaryInstrument;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
/// Flat-rate binary-barrier and single-touch analytics.
///
/// See Also
/// --------
/// BinaryBarrierOption : Supported contract.
/// TouchOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticBinaryBarrierEngine;
#[pymethods]
impl AnalyticBinaryBarrierEngine {
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: BinaryInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let context = &context.0;
        match instrument {
            BinaryInstrument::Binary(option) => {
                let option = &option.0;
                domain(
                    py,
                    py.detach(|| core::AnalyticBinaryBarrierEngine.price(option, context)),
                )
            }
            BinaryInstrument::Touch(option) => {
                let option = &option.0;
                domain(
                    py,
                    py.detach(|| core::AnalyticBinaryBarrierEngine.price(option, context)),
                )
            }
        }
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
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: BinaryInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let context = &context.0;
        let result = match instrument {
            BinaryInstrument::Binary(option) => {
                let option = &option.0;
                py.detach(|| {
                    core::AnalyticBinaryBarrierEngine
                        .price_with_greeks(option, context, request, shifts)
                })
            }
            BinaryInstrument::Touch(option) => {
                let option = &option.0;
                py.detach(|| {
                    core::AnalyticBinaryBarrierEngine
                        .price_with_greeks(option, context, request, shifts)
                })
            }
        };
        Ok(risk::PricingResult(domain(py, result)?))
    }
}
