use crate::conversions::{integer, real};
use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::{accumulator::Accumulator, autocallable, barrier::BarrierOption};
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

macro_rules! quadrature_engine {
    ($(#[$doc:meta])* $name:ident, $instrument:ty,
     |$py:ident, $option:ident, $engine:ident, $context:ident, $request:ident, $shifts:ident| $price:expr, $analysis:expr) => {
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
        $(#[$doc])*
        #[pyclass(frozen, module = "kiyosi._native")]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, asset_step_count=kiyosi::QuadratureSettings::default().asset_step_count, integration_standard_deviations=kiyosi::QuadratureSettings::default().integration_standard_deviations))]
            fn new(
                #[pyo3(from_py_with = integer)] asset_step_count: i32,
                #[pyo3(from_py_with = real)] integration_standard_deviations: f64,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(
                    kiyosi::QuadratureSettings {
                        asset_step_count,
                        integration_standard_deviations,
                    },
                ))?))
            }
            /// Returns the configured spatial grid budget.
            #[getter]
            fn asset_step_count(&self) -> i32 {
                self.0.settings().asset_step_count
            }
            /// Returns the integration extent in standard deviations.
            #[getter]
            fn integration_standard_deviations(&self) -> f64 {
                self.0.settings().integration_standard_deviations
            }
            fn __repr__(&self) -> String {
                format!(
                    concat!(
                        stringify!($name),
                        "(asset_step_count={}, integration_standard_deviations={})"
                    ),
                    self.asset_step_count(),
                    self.integration_standard_deviations()
                )
            }
            /// Returns the price without changing the instrument or context.
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
            /// Returns the price and selected sensitivities.
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
                #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                context: &PricingContext,
                greeks: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
                #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
                #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
                #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
            ) -> PyResult<risk::PricingResult> {
                let $py = py;
                let $request = risk::greeks_request(greeks, all_greeks)?;
                let $shifts = kiyosi::NumericalShiftSettings {
                    spot_shift,
                    volatility_shift,
                    rate_shift,
                    time_shift_days,
                };
                let $option = instrument;
                let $engine = self.0;
                let $context = &context.0;
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

quadrature_engine!(
    /// See Also
    /// --------
    /// BarrierOption : Supported contract.
    QuadratureBarrierEngine,
    PyRef<'_, BarrierOption>,
    |py, option, engine, context, request, shifts| {
        let option = &option.0;
        py.detach(|| request.evaluate(&engine, option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

quadrature_engine!(
    /// See Also
    /// --------
    /// Accumulator : Supported contract.
    QuadratureAccumulatorEngine,
    PyRef<'_, Accumulator>,
    |py, option, engine, context, request, shifts| {
        let option = &option.0;
        py.detach(|| request.evaluate(&engine, option, context))
    },
    {
        let option = &option.0;
        py.detach(|| engine.price_with_greeks(option, context, request, shifts))
    }
);

quadrature_engine!(
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
    QuadratureAutocallableEngine,
    Bound<'_, PyAny>,
    |py, option, engine, context, request, shifts| {
        autocallable::with_note!(&option, note, py.detach(|| request.evaluate(&engine, note, context)))
    },
    {
        autocallable::with_note!(
            &option,
            note,
            py.detach(|| engine.price_with_greeks(note, context, request, shifts))
        )
    }
);
