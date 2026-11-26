use crate::conversions::integer;
use crate::engines::{binomial, expiry_short_pricing};
use crate::error::domain;
use crate::instruments::accumulator;
use crate::instruments::autocallable;
use crate::instruments::barrier;
use crate::instruments::binary_barrier;
use crate::instruments::digital;
use crate::instruments::double_barrier;
use crate::instruments::double_touch;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) mod heston;
pub(crate) mod solution;

fn scheme(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::FiniteDifferenceScheme> {
    match value.extract::<&str>()? {
        "explicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ExplicitEuler),
        "implicit_euler" => Ok(kiyosi::FiniteDifferenceScheme::ImplicitEuler),
        "crank_nicolson" => Ok(kiyosi::FiniteDifferenceScheme::CrankNicolson),
        "rannacher" => Ok(kiyosi::FiniteDifferenceScheme::Rannacher),
        _ => Err(PyValueError::new_err("unknown finite-difference scheme")),
    }
}

pub(crate) fn scheme_name(value: kiyosi::FiniteDifferenceScheme) -> &'static str {
    match value {
        kiyosi::FiniteDifferenceScheme::ExplicitEuler => "explicit_euler",
        kiyosi::FiniteDifferenceScheme::ImplicitEuler => "implicit_euler",
        kiyosi::FiniteDifferenceScheme::CrankNicolson => "crank_nicolson",
        kiyosi::FiniteDifferenceScheme::Rannacher => "rannacher",
    }
}

macro_rules! fd_engine {
    ($(#[$doc:meta])* $name:ident, $instrument:ty $(,)?) => {
        fd_engine!($(#[$doc])* $name, $instrument, with_contract);
    };
    ($(#[$doc:meta])* $name:ident, $instrument:ty, $with:path $(, $short:path)? $(,)?) => {
        /// Finite-difference pricing.
        ///
        /// Parameters
        /// ----------
        /// asset_step_count : int, optional
        ///     Number of spatial steps, at least 3; defaults to 800. Continuous
        ///     barrier grids use all steps between their actual domain edges.
        /// time_step_count : int, optional
        ///     Positive base number of time intervals. The grid includes event dates
        ///     and curve knots.
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
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If asset_step_count is below 3, time_step_count is below 1, the
        ///     estimated grid workspace exceeds 1 GiB, the asset and time step counts
        ///     multiply to more than one billion, or an explicit upper boundary is invalid.
        ///     Pricing checks the work limit after adding event and curve steps and
        ///     damping half-steps. It also checks boundary coverage and explicit-Euler stability.
        ///
        /// Notes
        /// -----
        /// The engine name selects its product family. The vanilla engine accepts
        /// EuropeanOption and AmericanOption. The digital engine accepts
        /// CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
        /// also accepts TouchOption. The autocallable engine accepts all snowball
        /// variants and PhoenixOption. Other engines accept their named product.
        /// All FD product families accept deterministic rate curves and support local-volatility
        /// price, delta and gamma, including American exercise. Other requested Greeks
        /// are None. Autocallable coupon solvers support local volatility; implied
        /// volatility raises UnsupportedOperationError. The local-volatility surface
        /// stays fixed in absolute coordinates; expiry Greeks follow terminal
        /// payoffs and are absent where their stencils cross product events. Vanilla
        /// expiry Greeks use exact supported payoff derivatives, undefined at the
        /// strike. Surface coverage follows remaining stochastic evolution; rate
        /// coverage follows required future discounting. Automatic local-volatility
        /// domains require convergence checks.
        ///
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $name(pub(crate) kiyosi::$name);

        engine_methods!(@price $name, $instrument,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry delta and gamma come from the spatial solution, except zero
                /// BSM volatility uses scalar differences. The vanilla engine can reuse
                /// the same grid for default rate and volatility Greeks on flat-rate BSM
                /// European options. Other BSM Greeks use numerical differences with
                /// request-level shifts. Local volatility provides only
                /// delta and gamma. At expiry vanilla Greeks use exact supported payoff
                /// derivatives; other products use terminal-payoff stencils with the
                /// request's shifts. Use numerical_greeks for scalar differences under BSM.
                ///
            ],
            [],
            {
                /// Prepare an owned fixed-time solution for repeated spot queries.
                ///
                /// Parameters
                /// ----------
                /// instrument : object
                ///     Contract supported by this engine.
                /// context : PricingContext
                ///     Fixed model inputs, valuation time and supplied history.
                /// spot_range : tuple of float
                ///     Two finite positive, strictly increasing bounds. Both endpoints
                ///     are queryable; the context spot need not be inside the interval.
                ///
                /// Returns
                /// -------
                /// FiniteDifferenceSolution
                ///     Owned reusable solution; queries perform no new rollback.
                ///
                /// Raises
                /// ------
                /// KiyosiError
                ///     If applicability, interval, history, coverage or numerical checks fail.
                /// TypeError, ValueError
                ///     If the interval or input representations are invalid.
                ///
                /// Notes
                /// -----
                /// Current observations are evaluated per queried spot. Numerical domains
                /// use log spot and cover the states that need rollback. Active explicit
                /// engine bounds must provide coverage.
                /// Some products prepare several state layers or continuation components.
                /// Accuracy requires grid and truncation convergence checks. Long work
                /// releases the GIL; cancellation returns no partial solution.
                #[pyo3(signature = (instrument, context, *, spot_range))]
                fn solve(
                    &self,
                    py: pyo3::Python<'_>,
                    #[pyo3(from_py_with = crate::conversions::instrument)] instrument: $instrument,
                    context: &crate::market::PricingContext,
                    #[pyo3(from_py_with = solution::spot_range)] spot_range: std::ops::RangeInclusive<f64>,
                ) -> pyo3::PyResult<solution::FiniteDifferenceSolution> {
                    $with!(&instrument, option, solution::solve(py, &self.0, option, context, spot_range))
                }

                #[classattr]
                const _STATE_FIELDS: &'static [&'static str] = &[
                    "asset_step_count", "time_step_count", "scheme", "asset_upper_boundary",
                ];

                #[new]
                #[pyo3(signature = (*, asset_step_count=kiyosi::$name::default().settings().asset_step_count, time_step_count=kiyosi::$name::default().settings().time_step_count, scheme=kiyosi::$name::default().settings().scheme, asset_upper_boundary=kiyosi::$name::default().settings().asset_upper_boundary))]
                fn new(
                    #[pyo3(from_py_with = integer)] asset_step_count: i32,
                    #[pyo3(from_py_with = integer)] time_step_count: i32,
                    #[pyo3(from_py_with = scheme)] scheme: kiyosi::FiniteDifferenceScheme,
                    #[pyo3(from_py_with = crate::conversions::optional_real)] asset_upper_boundary: Option<f64>,
                ) -> PyResult<Self> {
                    Ok(Self(domain(kiyosi::$name::new(kiyosi::FiniteDifferenceSettings {
                        asset_step_count,
                        time_step_count,
                        scheme,
                        asset_upper_boundary,
                    }))?))
                }

                /// Return the configured number of spatial intervals.
                #[getter]
                fn asset_step_count(&self) -> i32 {
                    self.0.settings().asset_step_count
                }

                /// Return the configured base number of time intervals.
                #[getter]
                fn time_step_count(&self) -> i32 {
                    self.0.settings().time_step_count
                }

                /// Return the configured time-marching choice string.
                #[getter]
                fn scheme(&self) -> &'static str {
                    scheme_name(self.0.settings().scheme)
                }

                /// Return the explicit upper spot boundary, or None for automatic selection.
                #[getter]
                fn asset_upper_boundary(&self) -> Option<f64> {
                    self.0.settings().asset_upper_boundary
                }

            }
        );
        engine_methods!(@evaluate $name, $instrument, $with $(, $short)?);
    };
}

fd_engine!(
    /// The rollback work limit counts both layers when future purchases are possible.
    ///
    FiniteDifferenceAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
);
fd_engine!(
    /// The grid uses log spot and an automatically selected positive lower boundary.
    /// Automatic domains with non-explicit schemes concentrate nodes around the strike.
    /// Automatic Rannacher grids for European contracts with constant volatility
    /// shift the terminal strike by the deterministic carry and apply the
    /// corresponding discount after rollback.
    ///
    FiniteDifferenceVanillaEngine,
    binomial::VanillaInstrument<'_>,
    binomial::with_vanilla,
    expiry_short_pricing,
);
fd_engine!(
    /// The grid uses log spot and an automatically selected positive lower boundary.
    /// Automatic domains with non-explicit schemes concentrate nodes around the strike.
    /// Automatic Rannacher grids with constant volatility shift the terminal strike
    /// by the deterministic carry and apply the corresponding discount after rollback.
    ///
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
    /// When daily knock-in is active, implicit schemes subdivide the gaps between
    /// mandatory dates to place the base time intervals. With calendar-time BSM,
    /// the common cash-flow layer can use a separate grid.
    ///
    /// With flat BSM rates and one valuation layer, default first-order market
    /// Greeks can reuse the pricing rollback. Nondefault shifts use repricing.
    FiniteDifferenceAutocallableEngine,
    autocallable::AutocallableInstrument<'_>,
    autocallable::with_note,
);
