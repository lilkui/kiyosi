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
            engine_methods!(@price
                $engine, $instrument,
                #[new]
                fn new() -> Self {
                    Self
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
