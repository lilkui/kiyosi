use crate::conversions::{check_changes, integer};
use crate::error::domain;
use crate::instruments::american;
use crate::instruments::barrier::BarrierOption;
use crate::instruments::digital;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;

#[derive(FromPyObject)]
pub(crate) enum VanillaInstrument<'py> {
    European(PyRef<'py, EuropeanOption>),
    American(PyRef<'py, american::AmericanOption>),
}

macro_rules! binomial_engine {
    ($name:ident, $instrument:ty, |$py:ident, $option:ident, $engine:ident, $context:ident, $request:ident, $shifts:ident| $price:expr, $analysis:expr) => {
        /// Flat-rate Cox-Ross-Rubinstein tree pricing.
        ///
        /// Parameters
        /// ----------
        /// step_count : int, optional
        ///     Grid size. Uses the core default when omitted.
        ///
        /// Notes
        /// -----
        /// The vanilla engine accepts EuropeanOption and AmericanOption; the
        /// digital engine accepts CashOrNothingOption and AssetOrNothingOption.
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $name(pub(crate) core::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (**fields))]
            fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                check_changes(fields, &["step_count"])?;
                let mut settings = core::BinomialSettings::default();
                if let Some(count) = fields
                    .map(|f| f.get_item("step_count"))
                    .transpose()?
                    .flatten()
                {
                    settings.step_count = integer(&count)?;
                }
                Ok(Self(domain(py, core::$name::new(settings))?))
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
                let $py = py;
                let $option = instrument;
                let $engine = self.0;
                let $context = context.0.clone();
                domain(py, $price)
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
            /// **settings
            ///     `all_greeks` requests every measure and requires omitting `greeks`.
            ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
            ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
            ///     defaults; analytic engines still validate them.
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
            ///     If requests, shifts, pricing or a feasible bumped valuation fails.
            #[pyo3(signature = (instrument, context, greeks=None, **settings))]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                settings: Option<&Bound<'_, PyDict>>,
            ) -> PyResult<risk::PricingResult> {
                let $py = py;
                let $option = instrument;
                let $engine = self.0;
                let $context = context.0.clone();
                let $request = risk::request(greeks, settings)?;
                let $shifts = risk::shifts(settings)?;
                Ok(risk::PricingResult(domain(py, $analysis)?))
            }
        }
    };
}

binomial_engine!(
    CoxRossRubinsteinVanillaEngine,
    VanillaInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        match option {
            VanillaInstrument::European(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price(&option, &context))
            }
            VanillaInstrument::American(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price(&option, &context))
            }
        }
    },
    {
        match option {
            VanillaInstrument::European(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
            }
            VanillaInstrument::American(option) => {
                let option = option.0.clone();
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
            }
        }
    }
);
binomial_engine!(
    CoxRossRubinsteinDigitalEngine,
    digital::DigitalInstrument<'_>,
    |py, option, engine, context, request, shifts| {
        let option = option.contract().clone();
        py.detach(|| engine.price(&option, &context))
    },
    {
        let option = option.contract().clone();
        py.detach(|| engine.price_with_greeks(&option, &context, request, shifts))
    }
);
/// Flat-rate Cox-Ross-Rubinstein tree pricing.
///
/// Parameters
/// ----------
/// step_count : int, optional
///     Grid size. Uses the core default when omitted.
///
/// See Also
/// --------
/// BarrierOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct CoxRossRubinsteinBarrierEngine(pub(crate) core::CoxRossRubinsteinBarrierEngine);

#[pymethods]
impl CoxRossRubinsteinBarrierEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(fields, &["step_count"])?;
        let mut settings = core::BinomialSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("step_count"))
            .transpose()?
            .flatten()
        {
            settings.step_count = integer(&value)?;
        }
        Ok(Self(domain(
            py,
            core::CoxRossRubinsteinBarrierEngine::new(settings),
        )?))
    }
    /// Return the configured grid size.
    #[getter]
    fn step_count(&self) -> i32 {
        self.0.settings().step_count
    }
    fn __repr__(&self) -> String {
        format!(
            "CoxRossRubinsteinBarrierEngine(step_count={})",
            self.step_count()
        )
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, BarrierOption>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, BarrierOption>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
}
