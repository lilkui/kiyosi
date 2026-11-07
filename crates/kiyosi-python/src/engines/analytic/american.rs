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
///
/// Notes
/// -----
/// Requires BSM with constant rates. At positive volatility before expiry,
/// negative transformed interest rates (risk-free rate for calls, dividend yield
/// for puts) and exercise boundaries at or below strike raise UnsupportedOperationError.
/// Zero volatility uses exact deterministic exercise; expiry returns intrinsic value.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct BjerksundStenslandVanillaEngine;

numerical_greeks_methods!(
    PyRef<'_, AmericanOption>,
    |_binding, _py, instrument, context, request| {
        Ok(risk::PricingResult(domain(
            kiyosi::BjerksundStenslandVanillaEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request),
        )?))
    },
    [
        ///
        /// Notes
        /// -----
        /// Before expiry requested Greeks use scalar numerical differences and the
        /// engine defaults. Use numerical_greeks for configurable scalar differences.
    ],
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
        instrument: PyRef<'_, AmericanOption>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let option = &instrument.0;
        let context = &context.0;
        let calculate =
            || request.evaluate(&kiyosi::BjerksundStenslandVanillaEngine, option, context);
        // Keep the GIL for short scalar formulas, but release it for iterative solves.
        domain(if request.is_price() {
            calculate()
        } else {
            crate::cancellation::run(py, calculate)?
        })
    }
}
