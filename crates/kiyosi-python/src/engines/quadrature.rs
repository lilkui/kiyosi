use crate::conversions::{check_changes, integer, real};
use crate::error::domain;
use crate::instruments::digital;
use crate::instruments::vanilla::EuropeanOption;
use crate::instruments::{
    accumulator::Accumulator,
    autocallable::{BinarySnowballOption, PhoenixOption, SnowballOption, TernarySnowballOption},
    barrier::BarrierOption,
};
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;

macro_rules! quadrature_engine {
    ($engine:ident, $instrument:ty, |$option:ident| $contract:expr) => {
        /// Flat-rate terminal quadrature with a fixed 2,048-panel Simpson rule.
        ///
        /// Notes
        /// -----
        /// The vanilla engine accepts EuropeanOption; the digital engine accepts
        /// CashOrNothingOption and AssetOrNothingOption.
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $engine;

        #[pymethods]
        impl $engine {
            #[new]
            fn new() -> Self {
                Self
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
                let $option = instrument;
                let option = $contract.clone();
                let context = context.0.clone();
                domain(py, py.detach(|| core::$engine.price(&option, &context)))
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
                let request = risk::request(greeks, settings)?;
                let shifts = risk::shifts(settings)?;
                let $option = instrument;
                let option = $contract.clone();
                let context = context.0.clone();
                Ok(risk::PricingResult(domain(
                    py,
                    py.detach(|| {
                        core::$engine.price_with_greeks(&option, &context, request, shifts)
                    }),
                )?))
            }
        }
    };
}

quadrature_engine!(
    QuadratureVanillaEngine,
    PyRef<'_, EuropeanOption>,
    |option| option.0
);
quadrature_engine!(
    QuadratureDigitalEngine,
    digital::DigitalInstrument<'_>,
    |option| option.contract()
);
/// Flat-rate quadrature pricing with immutable integration settings.
///
/// Parameters
/// ----------
/// asset_step_count : int, optional
///     Spatial grid budget. Uses the core default when omitted.
/// integration_standard_deviations : float, optional
///     Positive finite integration extent in standard deviations.
///
/// See Also
/// --------
/// BarrierOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureBarrierEngine(pub(crate) core::QuadratureBarrierEngine);

#[pymethods]
impl QuadratureBarrierEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["asset_step_count", "integration_standard_deviations"],
        )?;
        let mut settings = core::QuadratureSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("asset_step_count"))
            .transpose()?
            .flatten()
        {
            settings.asset_step_count = integer(&value)?;
        }
        if let Some(value) = fields
            .map(|f| f.get_item("integration_standard_deviations"))
            .transpose()?
            .flatten()
        {
            settings.integration_standard_deviations = real(&value)?;
        }
        Ok(Self(domain(
            py,
            core::QuadratureBarrierEngine::new(settings),
        )?))
    }
    /// Return the configured spatial grid budget.
    #[getter]
    fn asset_step_count(&self) -> i32 {
        self.0.settings().asset_step_count
    }
    /// Return the integration extent in standard deviations.
    #[getter]
    fn integration_standard_deviations(&self) -> f64 {
        self.0.settings().integration_standard_deviations
    }
    fn __repr__(&self) -> String {
        format!(
            "QuadratureBarrierEngine(asset_step_count={}, integration_standard_deviations={})",
            self.asset_step_count(),
            self.integration_standard_deviations()
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
/// Flat-rate quadrature pricing with immutable integration settings.
///
/// Parameters
/// ----------
/// asset_step_count : int, optional
///     Spatial grid budget. Uses the core default when omitted.
/// integration_standard_deviations : float, optional
///     Positive finite integration extent in standard deviations.
///
/// See Also
/// --------
/// Accumulator : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureAccumulatorEngine(pub(crate) core::QuadratureAccumulatorEngine);

#[pymethods]
impl QuadratureAccumulatorEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["asset_step_count", "integration_standard_deviations"],
        )?;
        let mut settings = core::QuadratureSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("asset_step_count"))
            .transpose()?
            .flatten()
        {
            settings.asset_step_count = integer(&value)?;
        }
        if let Some(value) = fields
            .map(|f| f.get_item("integration_standard_deviations"))
            .transpose()?
            .flatten()
        {
            settings.integration_standard_deviations = real(&value)?;
        }
        Ok(Self(domain(
            py,
            core::QuadratureAccumulatorEngine::new(settings),
        )?))
    }
    /// Return the configured spatial grid budget.
    #[getter]
    fn asset_step_count(&self) -> i32 {
        self.0.settings().asset_step_count
    }
    /// Return the integration extent in standard deviations.
    #[getter]
    fn integration_standard_deviations(&self) -> f64 {
        self.0.settings().integration_standard_deviations
    }
    fn __repr__(&self) -> String {
        format!(
            "QuadratureAccumulatorEngine(asset_step_count={}, integration_standard_deviations={})",
            self.asset_step_count(),
            self.integration_standard_deviations()
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, Accumulator>,
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, Accumulator>,
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
/// Flat-rate quadrature pricing with immutable integration settings.
///
/// Parameters
/// ----------
/// asset_step_count : int, optional
///     Spatial grid budget. Uses the core default when omitted.
/// integration_standard_deviations : float, optional
///     Positive finite integration extent in standard deviations.
///
/// See Also
/// --------
/// SnowballOption : Supported contract.
/// BinarySnowballOption : Supported contract.
/// TernarySnowballOption : Supported contract.
/// PhoenixOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureAutocallableEngine(pub(crate) core::QuadratureAutocallableEngine);

#[pymethods]
impl QuadratureAutocallableEngine {
    #[new]
    #[pyo3(signature = (**fields))]
    fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["asset_step_count", "integration_standard_deviations"],
        )?;
        let mut settings = core::QuadratureSettings::default();
        if let Some(value) = fields
            .map(|f| f.get_item("asset_step_count"))
            .transpose()?
            .flatten()
        {
            settings.asset_step_count = integer(&value)?;
        }
        if let Some(value) = fields
            .map(|f| f.get_item("integration_standard_deviations"))
            .transpose()?
            .flatten()
        {
            settings.integration_standard_deviations = real(&value)?;
        }
        Ok(Self(domain(
            py,
            core::QuadratureAutocallableEngine::new(settings),
        )?))
    }
    /// Return the configured spatial grid budget.
    #[getter]
    fn asset_step_count(&self) -> i32 {
        self.0.settings().asset_step_count
    }
    /// Return the integration extent in standard deviations.
    #[getter]
    fn integration_standard_deviations(&self) -> f64 {
        self.0.settings().integration_standard_deviations
    }
    fn __repr__(&self) -> String {
        format!(
            "QuadratureAutocallableEngine(asset_step_count={}, integration_standard_deviations={})",
            self.asset_step_count(),
            self.integration_standard_deviations()
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
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
            let option = note.0.clone();
            return domain(py, py.detach(|| engine.price(&option, &context)));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
            let option = note.0.clone();
            return domain(py, py.detach(|| engine.price(&option, &context)));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
            let option = note.0.clone();
            return domain(py, py.detach(|| engine.price(&option, &context)));
        }
        let option = crate::conversions::instrument::<PyRef<'_, SnowballOption>>(instrument)?
            .0
            .clone();
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
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let context = context.0.clone();
        if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
            let option = note.0.clone();
            return Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
            )?));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
            let option = note.0.clone();
            return Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
            )?));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
            let option = note.0.clone();
            return Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
            )?));
        }
        let option = crate::conversions::instrument::<PyRef<'_, SnowballOption>>(instrument)?
            .0
            .clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
}
