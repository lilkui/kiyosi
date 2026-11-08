use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::binary_barrier::{self, BinaryInstrument};
use crate::market::PricingContext;
use pyo3::prelude::*;
/// Flat-rate binary-barrier and single-touch analytics.
///
/// See Also
/// --------
/// BinaryBarrierOption : Supported contract.
/// TouchOption : Supported contract.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct AnalyticBinaryBarrierEngine;
numerical_greeks_methods!(
    BinaryInstrument<'_>,
    [
        ///
        /// Notes
        /// -----
        /// Before expiry requested Greeks use scalar numerical differences and the
        /// engine defaults. Use numerical_greeks for configurable scalar differences.
        ///
    ],
    engine_methods!(
        AnalyticBinaryBarrierEngine,
        #[new]
        fn new() -> Self {
            Self
        },
        /// Return the price.
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
        ///
        fn price(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: BinaryInstrument<'_>,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

impl AnalyticBinaryBarrierEngine {
    #[expect(
        clippy::unused_self,
        reason = "uniform instance API for pricing engines"
    )]
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: BinaryInstrument<'_>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let context = &context.0;
        binary_barrier::with_binary!(
            instrument,
            option,
            domain(request.run(py, || request.evaluate(
                &kiyosi::AnalyticBinaryBarrierEngine,
                option,
                context
            ))?)
        )
    }
}
