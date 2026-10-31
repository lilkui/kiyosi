use crate::conversions::boolean;
use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;
/// Analytic European vanilla pricing with deterministic rate curves.
///
/// See Also
/// --------
/// EuropeanOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticEuropeanEngine;

#[pymethods]
impl AnalyticEuropeanEngine {
    #[new]
    fn new() -> Self {
        Self
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
    ///     If requests, pricing or requested derivatives fail.
    #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false))]
    fn price_with_greeks(
        &self,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = boolean)] all_greeks: bool,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks, all_greeks)?;
        Ok(risk::PricingResult(domain(
            kiyosi::AnalyticEuropeanEngine.price_with_greeks(&instrument.0, &context.0, request),
        )?))
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
            EuropeanOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        self.evaluate(py, instrument, context, ScalarRequest::Price)
    }
}

impl AnalyticEuropeanEngine {
    pub(crate) fn evaluate(
        &self,
        py: Python<'_>,
        instrument: PyRef<'_, EuropeanOption>,
        context: &PricingContext,
        request: ScalarRequest,
    ) -> PyResult<f64> {
        let option = &instrument.0;
        let context = &context.0;
        let calculate = || request.evaluate(&kiyosi::AnalyticEuropeanEngine, option, context);
        // Keep the GIL for short scalar formulas, but release it for iterative solves.
        domain(if matches!(request, ScalarRequest::Price) {
            calculate()
        } else {
            py.detach(calculate)
        })
    }
}
