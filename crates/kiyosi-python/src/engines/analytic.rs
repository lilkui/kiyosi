macro_rules! analytic_engine {
    ($(#[$doc:meta])* $engine:ident, $option:ident) => {
        analytic_engine!($(#[$doc])* $engine, $option, |_option, _context| false);
    };
    ($(#[$doc:meta])* $engine:ident, $option:ident, |$contract:ident, $market:ident| $short:expr) => {
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $engine;
        numerical_greeks_methods!(
            PyRef<'_, $option>,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry requested Greeks use scalar numerical differences and the
                /// engine defaults. Use numerical_greeks for configurable scalar differences.
                ///
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
                    #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
                        '_,
                        $option,
                    >,
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
                instrument: PyRef<'_, $option>,
                context: &PricingContext,
                request: impl crate::engines::PricingRequest<R>,
            ) -> PyResult<R> {
                let $contract = &instrument.0;
                let $market = &context.0;
                let calculate = || request.evaluate(&kiyosi::$engine, $contract, $market);
                domain(if request.is_short() && $short {
                    calculate()
                } else {
                    request.run(py, calculate)?
                })
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
