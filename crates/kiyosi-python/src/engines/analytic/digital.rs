use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::digital::DigitalInstrument;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;
/// Analytic European digital pricing with deterministic rate curves.
///
/// See Also
/// --------
/// CashOrNothingOption : Supported contract.
/// AssetOrNothingOption : Supported contract.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct AnalyticDigitalEngine;

numerical_greeks_methods!(
    DigitalInstrument<'_>,
    |_binding, _py, instrument, context, request| {
        Ok(risk::PricingResult(domain(
            kiyosi::AnalyticDigitalEngine.price_with_greeks(
                instrument.contract(),
                &context.0,
                request),
        )?))
    },
    [
        ///
        /// Notes
        /// -----
        /// Delta and gamma are analytic at positive volatility before expiry.
        /// At zero volatility they use scalar differences. Other Greeks use numerical
        /// differences with engine defaults. At expiry spot Greeks use default
        /// terminal-payoff stencils. Use numerical_greeks for configurable scalar differences.
    ],
    engine_methods!(
        AnalyticDigitalEngine,
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
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: DigitalInstrument<
                '_,
            >,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

impl AnalyticDigitalEngine {
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
        instrument: DigitalInstrument<'_>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let option = instrument.contract();
        let context = &context.0;
        let calculate = || request.evaluate(&kiyosi::AnalyticDigitalEngine, option, context);
        // Keep the GIL for short scalar formulas, but release it for iterative solves.
        domain(if request.is_price() {
            calculate()
        } else {
            py.detach(calculate)
        })
    }
}
