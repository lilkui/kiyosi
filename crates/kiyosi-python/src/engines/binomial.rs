use crate::conversions::integer;
use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::american;
use crate::instruments::barrier::BarrierOption;
use crate::instruments::digital;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
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
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct CoxRossRubinsteinVanillaEngine(pub(crate) kiyosi::CoxRossRubinsteinVanillaEngine);

numerical_greeks_methods!(
    VanillaInstrument<'_>,
    |binding, py, instrument, context, request, shifts| {
        let engine = binding.0;
        let context = &context.0;
        with_vanilla!(
            instrument,
            option,
            Ok(risk::PricingResult(domain(py.detach(|| {
                engine.price_with_greeks(option, context, request, shifts)
            }))?))
        )
    },
    [],
    engine_methods!(
        CoxRossRubinsteinVanillaEngine,
        #[new]
        #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
        fn new(#[pyo3(from_py_with = integer)] step_count: i32) -> PyResult<Self> {
            Ok(Self(domain(kiyosi::CoxRossRubinsteinVanillaEngine::new(
                kiyosi::BinomialSettings { step_count },
            ))?))
        },
        /// Return the configured grid size.
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
    ($(#[$doc:meta])* $name:ident, $instrument:ty, |$py:ident, $option:ident, $engine:ident, $context:ident, $request:ident| $price:expr, $analysis:expr) => {
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
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, step_count=kiyosi::BinomialSettings::default().step_count))]
            fn new(
                #[pyo3(from_py_with = integer)] step_count: i32,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(kiyosi::BinomialSettings { step_count }))?))
            }

            /// Return the configured grid size.
            #[getter]
            fn step_count(&self) -> i32 {
                self.0.settings().step_count
            }

            fn __repr__(&self) -> String {
                format!("{}(step_count={})", stringify!($name), self.step_count())
            }

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

            /// Return the price and selected sensitivities.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Supported contract; see the engine class for accepted types.
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            /// greeks : str or iterable of str, optional
            ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
            /// all_greeks : bool, optional
            ///     Request every measure; omit `greeks` when True. Defaults to False.
            ///
            /// Returns
            /// -------
            /// PricingResult
            ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
            ///     including every Greek at expiry. Units follow PricingResult.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If request validation or pricing fails.
            #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false))]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
            ) -> PyResult<risk::PricingResult> {
                let $py = py;
                let $option = instrument;
                let $engine = self.0;
                let $context = &context.0;
                let $request = risk::greeks_request(greeks, all_greeks)?;
                Ok(risk::PricingResult(domain($analysis)?))
            }
        }
        impl $name {
            pub(crate) fn evaluate(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: ScalarRequest,
            ) -> PyResult<f64> {
                let $request = request;
                let $py = py;
                let $option = instrument;
                let $engine = self.0;
                let $context = &context.0;
                domain($price)
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
    |py, option, engine, context, request| {
        let option = option.contract();
        py.detach(|| request.evaluate(&engine, option, context))
    },
    {
        let option = option.contract();
        py.detach(|| engine.price_with_greeks(option, context, request))
    }
);
binomial_engine!(
    ///
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    CoxRossRubinsteinBarrierEngine,
    PyRef<'_, BarrierOption>,
    |py, option, engine, context, request| {
        let option = &option.0;
        py.detach(|| request.evaluate(&engine, option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request))
    }
);

impl CoxRossRubinsteinVanillaEngine {
    pub(crate) fn evaluate(
        &self,
        py: Python<'_>,
        instrument: VanillaInstrument<'_>,
        context: &PricingContext,
        request: ScalarRequest,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = &context.0;
        with_vanilla!(
            instrument,
            option,
            domain(py.detach(|| request.evaluate(&engine, option, context)))
        )
    }
}
