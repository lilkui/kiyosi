use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use pyo3::prelude::*;
/// Analytic European vanilla pricing with deterministic rate curves.
///
/// See Also
/// --------
/// EuropeanOption : Supported contract.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct AnalyticEuropeanEngine;

numerical_greeks_methods!(
    PyRef<'_, EuropeanOption>,
    [
        ///
        /// Notes
        /// -----
        /// Uses analytic Greeks at positive volatility before expiry, scalar differences
        /// at zero volatility, and default terminal stencils at expiry.
        /// Use numerical_greeks for configurable scalar differences.
    ],
    engine_methods!(
        AnalyticEuropeanEngine,
        #[new]
        fn new() -> Self {
            Self
        },
        /// Returns the price.
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
    )
);

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
        domain(if request.is_short() {
            calculate()
        } else {
            request.run(py, calculate)?
        })
    }
}
