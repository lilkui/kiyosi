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
    /// greeks : str or iterable of str
    ///     Greek names or "all" to request every measure; duplicates are ignored.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     with time Greeks absent at expiry; other terminal Greeks follow the payoff.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, pricing or requested derivatives fail.
    #[pyo3(signature = (instrument, context, greeks))]
    #[expect(
        clippy::unused_self,
        reason = "uniform instance API for pricing engines"
    )]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts an owned PyRef guard"
    )]
    ///
    /// See Also
    /// --------
    /// numerical_greeks : Uniform numerical bump controls for supported BSM pairs.
    ///     Heston and local-volatility models are unsupported by this helper.
    fn price_with_greeks(
        &self,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
        greeks: &Bound<'_, PyAny>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks)?;
        Ok(risk::PricingResult(domain(
            kiyosi::AnalyticEuropeanEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request,
                Default::default(),
            ),
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
    #[expect(
        clippy::unused_self,
        reason = "uniform instance API for pricing engines"
    )]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "retain the extracted Python borrow during evaluation"
    )]
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: PyRef<'_, EuropeanOption>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let option = &instrument.0;
        let context = &context.0;
        let calculate = || request.evaluate(&kiyosi::AnalyticEuropeanEngine, option, context);
        // Keep the GIL for short scalar formulas, but release it for iterative solves.
        domain(if request.is_price() {
            calculate()
        } else {
            py.detach(calculate)
        })
    }
}
