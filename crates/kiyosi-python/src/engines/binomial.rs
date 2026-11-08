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
///
/// Notes
/// -----
/// Accepts EuropeanOption and AmericanOption.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct CoxRossRubinsteinVanillaEngine(pub(crate) kiyosi::CoxRossRubinsteinVanillaEngine);

numerical_greeks_methods!(
    VanillaInstrument<'_>,
    [
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use tree nodes; one-step gamma and
        /// other Greeks use numerical differences with engine defaults. At expiry
        /// spot Greeks use default terminal-payoff stencils. Use numerical_greeks
        /// for configurable scalar differences. Zero volatility uses scalar differences.
    ],
    engine_methods!(
        CoxRossRubinsteinVanillaEngine,
        #[new]
        #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
        fn new(#[pyo3(from_py_with = integer)] step_count: i32) -> PyResult<Self> {
            Ok(Self(domain(kiyosi::CoxRossRubinsteinVanillaEngine::new(
                kiyosi::BinomialSettings { step_count },
            ))?))
        },
        /// Return the configured number of binomial time steps.
        #[getter]
        fn step_count(&self) -> i32 {
            self.0.settings().step_count
        },
        fn __repr__(&self) -> String {
            format!(
                "CoxRossRubinsteinVanillaEngine(step_count={})",
                self.step_count()
            )
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
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: VanillaInstrument<
                '_,
            >,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

macro_rules! binomial_engine {
    ($(#[$doc:meta])* $name:ident, $instrument:ty, |$value:ident| $contract:expr) => {
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
        ///
        /// Notes
        /// -----
        /// Provides node delta/gamma for its product family; other Greeks are absent.
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        numerical_greeks_methods!(
            $instrument,
            [
                ///
                /// Notes
                /// -----
                /// At positive volatility before expiry delta and gamma use tree nodes.
                /// Zero volatility uses scalar differences. Other Greeks are
                /// unsupported. At expiry spot Greeks use default terminal-payoff
                /// stencils. Use numerical_greeks for configurable scalar differences.
            ],
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

                fn __repr__(&self) -> String {
                    format!("{}(step_count={})", stringify!($name), self.step_count())
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
                let $value = instrument;
                let option = $contract;
                let engine = self.0;
                let context = &context.0;
                domain(request.run(py, || request.evaluate(&engine, option, context))?)
            }
        }
    };
}

binomial_engine!(
    ///
    /// See Also
    /// --------
    /// CashOrNothingOption : Supported contract.
    /// AssetOrNothingOption : Supported contract.
    CoxRossRubinsteinDigitalEngine,
    digital::DigitalInstrument<'_>,
    |instrument| instrument.contract()
);
binomial_engine!(
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    CoxRossRubinsteinBarrierEngine,
    PyRef<'_, BarrierOption>,
    |instrument| &instrument.0
);

impl CoxRossRubinsteinVanillaEngine {
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: VanillaInstrument<'_>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let engine = self.0;
        let context = &context.0;
        with_vanilla!(
            instrument,
            option,
            domain(request.run(py, || request.evaluate(&engine, option, context))?)
        )
    }
}
