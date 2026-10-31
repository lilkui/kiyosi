use crate::conversions::{integer, real};
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
use pyo3::prelude::*;

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
                let option = $contract;
                let context = &context.0;
                domain(py, py.detach(|| kiyosi::$engine.price(option, context)))
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
            /// spot_shift : float, optional
            ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
            /// volatility_shift : float, optional
            ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
            /// rate_shift : float, optional
            ///     Positive finite parallel risk-free-rate bump in decimal units;
            ///     defaults to 0.0001.
            /// time_shift_days : int, optional
            ///     Positive forward/backward time bump in calendar days; defaults to 1.
            ///     All bumps are validated even when analytic Greeks are available.
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
            #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
            #[expect(clippy::too_many_arguments, reason = "explicit Python pricing controls")]
            fn price_with_greeks(
                &self,
                py: Python<'_>,
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
                #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
                #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
            ) -> PyResult<risk::PricingResult> {
                let request = risk::greeks_request(greeks, all_greeks)?;
                let shifts = kiyosi::NumericalShiftSettings { spot_shift, volatility_shift, rate_shift, time_shift_days };
                let $option = instrument;
                let option = $contract;
                let context = &context.0;
                Ok(risk::PricingResult(domain(
                    py,
                    py.detach(|| {
                        kiyosi::$engine.price_with_greeks(option, context, request, shifts)
                    }),
                )?))
            }
        }
    };
}

quadrature_engine!(
    QuadratureVanillaEngine,
    PyRef<'_, EuropeanOption>,
    |option| &option.0
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
///     Spatial grid budget, at least 4; defaults to 512.
/// integration_standard_deviations : float, optional
///     Positive finite integration extent in standard deviations; defaults to 8.
///
/// Raises
/// ------
/// InvalidInputError
///     If asset_step_count is below 4 or integration_standard_deviations
///     is nonpositive or non-finite.
///
/// See Also
/// --------
/// BarrierOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureBarrierEngine(pub(crate) kiyosi::QuadratureBarrierEngine);

#[pymethods]
impl QuadratureBarrierEngine {
    #[new]
    #[pyo3(signature = (*, asset_step_count=kiyosi::QuadratureSettings::default().asset_step_count, integration_standard_deviations=kiyosi::QuadratureSettings::default().integration_standard_deviations))]
    fn new(
        py: Python<'_>,
        #[pyo3(from_py_with = integer)] asset_step_count: i32,
        #[pyo3(from_py_with = real)] integration_standard_deviations: f64,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            kiyosi::QuadratureBarrierEngine::new(kiyosi::QuadratureSettings {
                asset_step_count,
                integration_standard_deviations,
            }),
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
        let option = &instrument.0;
        let context = &context.0;
        domain(py, py.detach(|| engine.price(option, context)))
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
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
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
    #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python pricing controls"
    )]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, BarrierOption>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
        #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
        #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks, all_greeks)?;
        let shifts = kiyosi::NumericalShiftSettings {
            spot_shift,
            volatility_shift,
            rate_shift,
            time_shift_days,
        };
        let engine = self.0;
        let option = &instrument.0;
        let context = &context.0;
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(option, context, request, shifts)),
        )?))
    }
}
/// Flat-rate quadrature pricing with immutable integration settings.
///
/// Parameters
/// ----------
/// asset_step_count : int, optional
///     Spatial grid budget, at least 4; defaults to 512.
/// integration_standard_deviations : float, optional
///     Positive finite integration extent in standard deviations; defaults to 8.
///
/// Raises
/// ------
/// InvalidInputError
///     If asset_step_count is below 4 or integration_standard_deviations
///     is nonpositive or non-finite.
///
/// See Also
/// --------
/// Accumulator : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureAccumulatorEngine(pub(crate) kiyosi::QuadratureAccumulatorEngine);

#[pymethods]
impl QuadratureAccumulatorEngine {
    #[new]
    #[pyo3(signature = (*, asset_step_count=kiyosi::QuadratureSettings::default().asset_step_count, integration_standard_deviations=kiyosi::QuadratureSettings::default().integration_standard_deviations))]
    fn new(
        py: Python<'_>,
        #[pyo3(from_py_with = integer)] asset_step_count: i32,
        #[pyo3(from_py_with = real)] integration_standard_deviations: f64,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            kiyosi::QuadratureAccumulatorEngine::new(kiyosi::QuadratureSettings {
                asset_step_count,
                integration_standard_deviations,
            }),
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
        let option = &instrument.0;
        let context = &context.0;
        domain(py, py.detach(|| engine.price(option, context)))
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
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
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
    #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python pricing controls"
    )]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, Accumulator>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
        #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
        #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks, all_greeks)?;
        let shifts = kiyosi::NumericalShiftSettings {
            spot_shift,
            volatility_shift,
            rate_shift,
            time_shift_days,
        };
        let engine = self.0;
        let option = &instrument.0;
        let context = &context.0;
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(option, context, request, shifts)),
        )?))
    }
}
/// Flat-rate quadrature pricing with immutable integration settings.
///
/// Parameters
/// ----------
/// asset_step_count : int, optional
///     Spatial grid budget, at least 4; defaults to 512.
/// integration_standard_deviations : float, optional
///     Positive finite integration extent in standard deviations; defaults to 8.
///
/// Raises
/// ------
/// InvalidInputError
///     If asset_step_count is below 4 or integration_standard_deviations
///     is nonpositive or non-finite.
///
/// See Also
/// --------
/// SnowballOption : Supported contract.
/// BinarySnowballOption : Supported contract.
/// TernarySnowballOption : Supported contract.
/// PhoenixOption : Supported contract.
///
/// Notes
/// -----
/// Preserves cash and asset payments below the spatial grid, including
/// zero-lower-strike losses after large spot declines.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct QuadratureAutocallableEngine(pub(crate) kiyosi::QuadratureAutocallableEngine);

#[pymethods]
impl QuadratureAutocallableEngine {
    #[new]
    #[pyo3(signature = (*, asset_step_count=kiyosi::QuadratureSettings::default().asset_step_count, integration_standard_deviations=kiyosi::QuadratureSettings::default().integration_standard_deviations))]
    fn new(
        py: Python<'_>,
        #[pyo3(from_py_with = integer)] asset_step_count: i32,
        #[pyo3(from_py_with = real)] integration_standard_deviations: f64,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            kiyosi::QuadratureAutocallableEngine::new(kiyosi::QuadratureSettings {
                asset_step_count,
                integration_standard_deviations,
            }),
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
        let context = &context.0;
        if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
            let option = &note.0;
            return domain(py, py.detach(|| engine.price(option, context)));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
            let option = &note.0;
            return domain(py, py.detach(|| engine.price(option, context)));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
            let option = &note.0;
            return domain(py, py.detach(|| engine.price(option, context)));
        }
        let option = &crate::conversions::instrument::<PyRef<'_, SnowballOption>>(instrument)?.0;
        domain(py, py.detach(|| engine.price(option, context)))
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
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
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
    #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python pricing controls"
    )]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
        #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
        #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks, all_greeks)?;
        let shifts = kiyosi::NumericalShiftSettings {
            spot_shift,
            volatility_shift,
            rate_shift,
            time_shift_days,
        };
        let engine = self.0;
        let context = &context.0;
        if let Ok(note) = instrument.extract::<PyRef<'_, PhoenixOption>>() {
            let option = &note.0;
            return Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(option, context, request, shifts)),
            )?));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, BinarySnowballOption>>() {
            let option = &note.0;
            return Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(option, context, request, shifts)),
            )?));
        }
        if let Ok(note) = instrument.extract::<PyRef<'_, TernarySnowballOption>>() {
            let option = &note.0;
            return Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(option, context, request, shifts)),
            )?));
        }
        let option = &crate::conversions::instrument::<PyRef<'_, SnowballOption>>(instrument)?.0;
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(option, context, request, shifts)),
        )?))
    }
}
