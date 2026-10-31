use crate::conversions::{integer, real};
use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::{accumulator::Accumulator, autocallable, barrier::BarrierOption};
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

macro_rules! quadrature_engine {
    ($(#[$doc:meta])* $name:ident, $instrument:ty $(,)?) => {
        quadrature_engine!($(#[$doc])* $name, $instrument, with_contract);
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty,
     $with:path $(,)?) => {
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

        numerical_greeks_methods!(
            $instrument,
            |binding, py, instrument, context, request, shifts| {
                let engine = binding.0;
                let context = &context.0;
                $with!(&instrument, option, Ok(risk::PricingResult(domain(py.detach(|| {
                    engine.price_with_greeks(option, context, request, shifts)
                }))?)))
            },
            [],
            engine_methods!(
                $name,
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
                },
                /// Returns the configured spatial grid budget.
                #[getter]
                fn asset_step_count(&self) -> i32 {
                    self.0.settings().asset_step_count
                },
                /// Returns the integration extent in standard deviations.
                #[getter]
                fn integration_standard_deviations(&self) -> f64 {
                    self.0.settings().integration_standard_deviations
                },
                fn __repr__(&self) -> String {
                    format!(
                        concat!(
                            stringify!($name),
                            "(asset_step_count={}, integration_standard_deviations={})"
                        ),
                        self.asset_step_count(),
                        self.integration_standard_deviations()
                    )
                },
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
            )
        );
        impl $name {
            pub(crate) fn evaluate(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: ScalarRequest,
            ) -> PyResult<f64> {
                let engine = self.0;
                let context = &context.0;
                $with!(&instrument, option, domain(py.detach(|| {
                    request.evaluate(&engine, option, context)
                })))
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
);

quadrature_engine!(
    /// See Also
    /// --------
    /// Accumulator : Supported contract.
    QuadratureAccumulatorEngine,
    PyRef<'_, Accumulator>,
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
    autocallable::with_note,
);
