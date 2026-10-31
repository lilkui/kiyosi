use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::binary_barrier::{self, BinaryInstrument};
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;
/// Flat-rate binary-barrier and single-touch analytics.
///
/// See Also
/// --------
/// BinaryBarrierOption : Supported contract.
/// TouchOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticBinaryBarrierEngine;
numerical_greeks_methods!(
    BinaryInstrument<'_>,
    |_binding, py, instrument, context, request, shifts| {
        let context = &context.0;
        let result = binary_barrier::with_binary!(
            instrument,
            option,
            py.detach(|| {
                kiyosi::AnalyticBinaryBarrierEngine
                    .price_with_greeks(option, context, request, shifts)
            })
        );
        Ok(risk::PricingResult(domain(result)?))
    },
    [],
    engine_methods!(
        AnalyticBinaryBarrierEngine,
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
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: BinaryInstrument<'_>,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

impl AnalyticBinaryBarrierEngine {
    pub(crate) fn evaluate(
        &self,
        py: Python<'_>,
        instrument: BinaryInstrument<'_>,
        context: &PricingContext,
        request: ScalarRequest,
    ) -> PyResult<f64> {
        let context = &context.0;
        binary_barrier::with_binary!(
            instrument,
            option,
            domain(py.detach(|| request.evaluate(
                &kiyosi::AnalyticBinaryBarrierEngine,
                option,
                context
            )))
        )
    }
}
