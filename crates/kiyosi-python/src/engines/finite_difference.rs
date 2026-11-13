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
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) mod heston;

fn scheme(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::FiniteDifferenceScheme> {
    match value.extract::<&str>()? {
        "explicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ExplicitEuler),
        "implicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ImplicitEuler),
        "crank_nicolson" => Ok(kiyosi::FiniteDifferenceScheme::CrankNicolson),
        "rannacher" => Ok(kiyosi::FiniteDifferenceScheme::Rannacher),
        _ => Err(PyValueError::new_err("unknown finite-difference scheme")),
    }
}

macro_rules! fd_engine {
    ($name:ident, $instrument:ty $(,)?) => {
        fd_engine!($name, $instrument, with_contract);
    };
    ($name:ident, $instrument:ty, $with:path $(,)?) => {
        /// Finite-difference pricing.
        ///
        /// Parameters
        /// ----------
        /// asset_step_count : int, optional
        ///     Number of spatial steps, at least 3; defaults to 800. Continuous
        ///     barrier grids use all steps between their actual domain edges.
        /// time_step_count : int, optional
        ///     Positive base number of time intervals; event dates and curve knots are
        ///     added.
        #[doc = concat!("    Read ``", stringify!($name), "().time_step_count`` for the engine's default.")]
        /// scheme : FiniteDifferenceScheme, optional
        ///     Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        ///     "crank_nicolson" uses standard Crank-Nicolson without time-step
        ///     damping.
        /// asset_upper_boundary : float or None, optional
        ///     Finite positive upper spot boundary, or None (default) for the
        ///     automatic domain. Must exceed spot, strike and barriers at pricing.
        ///     Continuous up and double barriers fix their own upper grid edge; this
        ///     bound is inactive for that grid. Local-volatility knock-in vanilla
        ///     components and scheduled monitoring use the full domain.
        /// spot_shift : float or None, optional
        ///     Positive absolute spot bump for native Greek availability and terminal
        ///     stencils. None selects the automatic bump; prices and grids are
        ///     unchanged.
        /// volatility_shift : float or None, optional
        ///     Positive decimal volatility bump, or None (default) for the smaller of
        ///     0.0001 and half the current volatility. Native spatial delta and gamma
        ///     retain their estimator.
        /// rate_shift : float, optional
        ///     Positive decimal rate bump; defaults to 0.0001.
        /// time_shift : datetime.timedelta or None, optional
        ///     Positive whole-microsecond time bump, or None for automatic selection.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If asset_step_count is below 3, time_step_count is below 1, or an
        ///     explicit upper boundary or Greek bump is invalid. Boundary coverage and
        ///     explicit-Euler stability are checked during pricing.
        ///
        /// Notes
        /// -----
        /// The engine name selects its product family. The vanilla engine accepts
        /// EuropeanOption and AmericanOption. The digital engine accepts
        /// CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
        /// also accepts TouchOption. The autocallable engine accepts all snowball
        /// variants and PhoenixOption. Other engines accept their named product.
        /// Digital pricing requires constant rates; other families accept
        /// deterministic rate curves. All FD product families support local-volatility
        /// price, delta and gamma, including American exercise. Other requested Greeks
        /// are None. Autocallable coupon solvers support local volatility; implied
        /// volatility raises UnsupportedOperationError. Sigma
        /// uses fixed absolute surface coordinates; expiry Greeks follow terminal
        /// payoffs and are absent where their stencils cross product events. Vanilla
        /// expiry Greeks use exact supported payoff derivatives, undefined at the
        /// strike. Surface coverage follows remaining stochastic evolution; rate
        /// coverage follows required future discounting. Automatic local-volatility
        /// domains require convergence checks.
        ///
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        numerical_greeks_methods!(
            $instrument,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry delta and gamma come from the spatial solution, except zero
                /// BSM volatility uses scalar differences. Other BSM Greeks use numerical
                /// differences with the configured shifts. Local volatility provides only
                /// delta and gamma. At expiry vanilla Greeks use exact supported payoff
                /// derivatives; other products use default terminal-payoff stencils. Use
                /// numerical_greeks for configurable scalar differences under BSM.
                ///
            ],
            engine_methods!(
                $name,
                #[new]
                #[expect(clippy::too_many_arguments, reason = "explicit Python grid and Greek controls")]
                #[pyo3(signature = (*, asset_step_count=kiyosi::$name::default().settings().asset_step_count, time_step_count=kiyosi::$name::default().settings().time_step_count, scheme=kiyosi::$name::default().settings().scheme, asset_upper_boundary=None, spot_shift=None, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift=None))]
                fn new(
                    #[pyo3(from_py_with = integer)] asset_step_count: i32,
                    #[pyo3(from_py_with = integer)] time_step_count: i32,
                    #[pyo3(from_py_with = scheme)] scheme: kiyosi::FiniteDifferenceScheme,
                    asset_upper_boundary: Option<&Bound<'_, PyAny>>,
                    spot_shift: Option<&Bound<'_, PyAny>>,
                    #[pyo3(from_py_with = crate::conversions::optional_real)] volatility_shift: Option<f64>,
                    #[pyo3(from_py_with = real)] rate_shift: f64,
                    time_shift: Option<&Bound<'_, PyAny>>,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(kiyosi::FiniteDifferenceSettings {
                        asset_step_count,
                        time_step_count,
                        scheme,
                        asset_upper_boundary: asset_upper_boundary.map(real).transpose()?,
                        greek_shifts: kiyosi::NumericalShiftSettings {
                            spot_shift: spot_shift.map(real).transpose()?,
                            volatility_shift,
                            rate_shift,
                            time_shift: time_shift.map(crate::conversions::duration).transpose()?,
                        },
                    }))?))
                },
                /// Return the configured number of spatial intervals.
                #[getter]
                fn asset_step_count(&self) -> i32 {
                    self.0.settings().asset_step_count
                },
                /// Return the configured base number of time intervals before event insertion.
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
                        kiyosi::FiniteDifferenceScheme::Rannacher => "rannacher",
                    }
                },
                /// Return the explicit upper spot boundary, or None for automatic selection.
                #[getter]
                fn asset_upper_boundary(&self) -> Option<f64> {
                    self.0.settings().asset_upper_boundary
                },
                /// Return the native Greek spot bump, or None for automatic selection.
                #[getter]
                fn spot_shift(&self) -> Option<f64> {
                    self.0.settings().greek_shifts.spot_shift
                },
                /// Return the numerical volatility bump, or None for automatic selection.
                #[getter]
                fn volatility_shift(&self) -> Option<f64> {
                    self.0.settings().greek_shifts.volatility_shift
                },
                /// Return the numerical parallel rate bump in decimal units.
                #[getter]
                fn rate_shift(&self) -> f64 {
                    self.0.settings().greek_shifts.rate_shift
                },
                /// Return the explicit time bump, or None for automatic selection.
                #[getter]
                fn time_shift<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, pyo3::types::PyDelta>>> {
                    self.0.settings().greek_shifts.time_shift.map(|value| crate::conversions::python_duration(py, value)).transpose()
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
