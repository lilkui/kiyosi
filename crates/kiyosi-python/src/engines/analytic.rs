macro_rules! analytic_engine {
    ($(#[$doc:meta])* $engine:ident, $instrument:ty, $with:path) => {
        analytic_engine!($(#[$doc])* $engine, $instrument, $with, |_option, _context| false);
    };
    ($(#[$doc:meta])* $engine:ident, $instrument:ty, $with:path,
     |$contract:ident, $market:ident| $short:expr) => {
        analytic_engine!(
            $(#[$doc])* $engine, $instrument, $with, |$contract, $market| $short,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry, requested Greeks use scalar numerical differences with
                /// the request's shift controls. Omitted controls use engine defaults.
                ///
            ]
        );
    };
    ($(#[$doc:meta])* $engine:ident, $instrument:ty, $with:path,
     |$contract:ident, $market:ident| $short:expr,
     [$(#[$greeks_doc:meta])*]) => {
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $engine;
        numerical_greeks_methods!(
            $instrument,
            [
                $(#[$greeks_doc])*
            ],
            engine_methods!(
                $engine,
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
                    #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                    context: &PricingContext,
                ) -> PyResult<f64> {
                    self.evaluate(py, instrument, context, $crate::engines::ScalarRequest::Price)
                }
            )
        );
        impl $engine {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: impl crate::engines::PricingRequest<R>,
            ) -> PyResult<R> {
                let $market = &context.0;
                let short = {
                    let $contract = &instrument;
                    $short
                };
                $with!(&instrument, $contract, domain(request.run(py,
                    short || kiyosi::OptionContract::expiry_time($contract) == $market.valuation_time(), || {
                    request.evaluate(&kiyosi::$engine, $contract, $market)
                })?))
            }
        }
    };
}

pub(crate) mod american;
pub(crate) mod asian;
pub(crate) mod barrier;
pub(crate) mod binary_barrier;
pub(crate) mod digital;
pub(crate) mod double_barrier;
pub(crate) mod double_touch;
pub(crate) mod european;
pub(crate) mod heston;
pub(crate) mod range_accrual;
