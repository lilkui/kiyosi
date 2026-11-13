use crate::conversions::integer;
use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::american;
use crate::instruments::barrier::BarrierOption;
use crate::instruments::digital;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

#[derive(FromPyObject)]
pub(crate) enum VanillaInstrument<'py> {
    European(PyRef<'py, EuropeanOption>),
    American(PyRef<'py, american::AmericanOption>),
}

macro_rules! with_vanilla {
    ($instrument:expr, $option:ident, $body:expr) => {{
        match $instrument {
            $crate::engines::binomial::VanillaInstrument::European(option) => {
                let $option = &option.0;
                $body
            }
            $crate::engines::binomial::VanillaInstrument::American(option) => {
                let $option = &option.0;
                $body
            }
        }
    }};
}
pub(crate) use with_vanilla;

macro_rules! binomial_engine {
    ($(#[$doc:meta])* $name:ident, $instrument:ty, $with:path) => {
        binomial_engine!(
            $(#[$doc])* $name, $instrument, $with,
            [
                ///
                /// Notes
                /// -----
                /// At positive volatility before expiry delta and gamma use tree nodes. Zero
                /// volatility uses scalar differences. Other Greeks are unsupported. At expiry
                /// spot Greeks use default terminal-payoff stencils. Use numerical_greeks for
                /// configurable scalar differences.
                ///
            ]
        );
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty, $with:path,
     [$(#[$greeks_doc:meta])*]) => {
        /// Flat-rate Cox-Ross-Rubinstein tree pricing.
        ///
        /// Parameters
        /// ----------
        /// step_count : int, optional
        ///     Number of tree time steps, from 1 through 1,000,000; defaults to 256.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If step_count is outside 1 through 1,000,000.
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        numerical_greeks_methods!(
            $instrument,
            [$(#[$greeks_doc])*],
            engine_methods!(
                $name,
                #[new]
                #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
                fn new(
                    #[pyo3(from_py_with = integer)] step_count: i32,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(kiyosi::BinomialSettings { step_count }))?))
                },

                /// Return the configured number of binomial time steps.
                #[getter]
                fn step_count(&self) -> i32 {
                    self.0.settings().step_count
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
                    self.evaluate(py, instrument, context, ScalarRequest::Price)
                }
            )
        );
        impl $name {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: impl crate::engines::PricingRequest<R>,
            ) -> PyResult<R> {
                $with!(&instrument, option, crate::engines::evaluate(py, &self.0, option, context, request))
            }
        }
    };
}

binomial_engine!(
    ///
    /// Notes
    /// -----
    /// Accepts EuropeanOption and AmericanOption.
    ///
    CoxRossRubinsteinVanillaEngine,
    VanillaInstrument<'_>,
    with_vanilla,
    [
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use tree nodes;
        /// one-step gamma and other Greeks use numerical differences with engine
        /// defaults. At expiry spot Greeks use default terminal-payoff stencils. Use
        /// numerical_greeks for configurable scalar differences. Zero volatility uses
        /// scalar differences.
        ///
    ]
);

binomial_engine!(
    ///
    /// See Also
    /// --------
    /// CashOrNothingOption : Supported contract.
    /// AssetOrNothingOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Provides node delta/gamma for its product family; other Greeks are absent.
    ///
    CoxRossRubinsteinDigitalEngine,
    digital::DigitalInstrument<'_>,
    digital::with_digital
);
binomial_engine!(
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    ///
    /// Notes
    /// -----
    /// Provides node delta/gamma for its product family; other Greeks are absent.
    ///
    CoxRossRubinsteinBarrierEngine,
    PyRef<'_, BarrierOption>,
    with_contract
);
