use crate::conversions::{integer, real};
use crate::engines::ScalarRequest;
use crate::engines::binomial;
use crate::error::domain;
use crate::instruments::accumulator;
use crate::instruments::autocallable;
use crate::instruments::barrier;
use crate::instruments::binary_barrier;
use crate::instruments::digital;
use crate::instruments::double_barrier;
use crate::instruments::double_touch;
use crate::market::PricingContext;
use crate::risk;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn scheme(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::FiniteDifferenceScheme> {
    match value.extract::<&str>()? {
        "explicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ExplicitEuler),
        "implicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ImplicitEuler),
        "crank_nicolson" => Ok(kiyosi::FiniteDifferenceScheme::CrankNicolson),
        _ => Err(PyValueError::new_err("unknown finite-difference scheme")),
    }
}

macro_rules! fd_engine {
    ($name:ident, $instrument:ty $(,)?) => {
        fd_engine!($name, $instrument, with_contract);
    };
    ($name:ident, $instrument:ty, $with:path $(,)?) => {
        /// Finite-difference pricing with immutable grid settings.
        ///
        /// Parameters
        /// ----------
        /// asset_step_count : int, optional
        ///     Number of spatial steps, at least 3; defaults to 800.
        /// time_step_count : int, optional
        ///     Positive time-step budget; event dates and curve knots are added.
        #[doc = concat!("    Read `", stringify!($name), "().time_step_count` for the engine's default.")]
        /// scheme : FiniteDifferenceScheme, optional
        ///     Time-marching scheme; defaults to damped Crank-Nicolson.
        /// asset_upper_boundary : float or None, optional
        ///     Finite positive upper spot boundary, or None (default) for the
        ///     automatic domain. Must exceed spot, strike and barriers at pricing.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If asset_step_count is below 3, time_step_count is below 1, or an
        ///     explicit upper boundary is nonpositive or non-finite. Boundary coverage
        ///     and explicit-Euler stability are checked during pricing.
        ///
        /// Notes
        /// -----
        /// The engine name selects its product family. The vanilla engine accepts
        /// EuropeanOption and AmericanOption. The digital engine accepts
        /// CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
        /// also accepts TouchOption. The autocallable engine accepts all snowball
        /// variants and PhoenixOption. Other engines accept their named product.
        /// Digital pricing requires explicitly flat rates; other families accept
        /// deterministic rate curves.
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
                #[pyo3(signature = (*, asset_step_count=kiyosi::$name::default().settings().asset_step_count, time_step_count=kiyosi::$name::default().settings().time_step_count, scheme=kiyosi::$name::default().settings().scheme, asset_upper_boundary=None))]
                fn new(
                    #[pyo3(from_py_with = integer)] asset_step_count: i32,
                    #[pyo3(from_py_with = integer)] time_step_count: i32,
                    #[pyo3(from_py_with = scheme)] scheme: kiyosi::FiniteDifferenceScheme,
                    asset_upper_boundary: Option<&Bound<'_, PyAny>>,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(kiyosi::FiniteDifferenceSettings {
                        asset_step_count,
                        time_step_count,
                        scheme,
                        asset_upper_boundary: asset_upper_boundary.map(real).transpose()?,
                    }))?))
                },
                /// Return the configured spatial grid budget.
                #[getter]
                fn asset_step_count(&self) -> i32 {
                    self.0.settings().asset_step_count
                },
                /// Return the configured time-step budget before event insertion.
                #[getter]
                fn time_step_count(&self) -> i32 {
                    self.0.settings().time_step_count
                },
                /// Return the configured time-marching choice string.
                #[getter]
                fn scheme(&self) -> &'static str {
                    match self.0.settings().scheme {
                        kiyosi::FiniteDifferenceScheme::ExplicitEuler => "explicit_euler",
                        kiyosi::FiniteDifferenceScheme::ImplicitEuler => "implicit_euler",
                        kiyosi::FiniteDifferenceScheme::CrankNicolson => "crank_nicolson",
                    }
                },
                /// Return the explicit upper spot boundary, or None for automatic selection.
                #[getter]
                fn asset_upper_boundary(&self) -> Option<f64> {
                    self.0.settings().asset_upper_boundary
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

fd_engine!(
    FiniteDifferenceAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
);
fd_engine!(
    FiniteDifferenceVanillaEngine,
    binomial::VanillaInstrument<'_>,
    binomial::with_vanilla,
);
fd_engine!(
    FiniteDifferenceDigitalEngine,
    digital::DigitalInstrument<'_>,
    digital::with_digital,
);
fd_engine!(
    FiniteDifferenceBarrierEngine,
    PyRef<'_, barrier::BarrierOption>,
);
fd_engine!(
    FiniteDifferenceBinaryBarrierEngine,
    binary_barrier::BinaryInstrument<'_>,
    binary_barrier::with_binary,
);
fd_engine!(
    FiniteDifferenceDoubleBarrierEngine,
    PyRef<'_, double_barrier::DoubleBarrierOption>,
);
fd_engine!(
    FiniteDifferenceDoubleTouchEngine,
    PyRef<'_, double_touch::DoubleTouchOption>,
);
fd_engine!(
    FiniteDifferenceAutocallableEngine,
    autocallable::AutocallableInstrument<'_>,
    autocallable::with_note,
);
