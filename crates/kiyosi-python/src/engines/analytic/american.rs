use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::american::AmericanOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;
/// Bjerksund-Stensland 2002 American approximation with flat rates.
///
/// See Also
/// --------
/// AmericanOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct BjerksundStenslandVanillaEngine;

numerical_greeks_methods!(
    PyRef<'_, AmericanOption>,
    |_binding, _py, instrument, context, request, shifts| {
        Ok(risk::PricingResult(domain(
            kiyosi::BjerksundStenslandVanillaEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request,
                shifts,
            ),
        )?))
    },
    [],
    engine_methods!(
        BjerksundStenslandVanillaEngine,
        #[new]
        fn new() -> Self {
            Self
        },
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
                AmericanOption,
            >,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

impl BjerksundStenslandVanillaEngine {
    pub(crate) fn evaluate(
        &self,
        py: Python<'_>,
        instrument: PyRef<'_, AmericanOption>,
        context: &PricingContext,
        request: ScalarRequest,
    ) -> PyResult<f64> {
        let option = &instrument.0;
        let context = &context.0;
        let calculate =
            || request.evaluate(&kiyosi::BjerksundStenslandVanillaEngine, option, context);
        // Keep the GIL for short scalar formulas, but release it for iterative solves.
        domain(if matches!(request, ScalarRequest::Price) {
            calculate()
        } else {
            py.detach(calculate)
        })
    }
}
