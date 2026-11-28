use crate::conversions::integer;
use crate::engines::binomial;
use crate::error::domain;
use kiyosi::engines::VanillaBatchEngine;
use pyo3::{prelude::*, types::PyTuple};

/// Finite-difference prices for European and American options under Heston.
///
/// Parameters
/// ----------
/// asset_step_count : int, optional
///     Asset intervals, at least 3; read the property for the Rust-defined
///     default.
/// variance_step_count : int, optional
///     Variance intervals, at least 3; read the property for the default.
/// time_step_count : int, optional
///     Base time intervals, at least 1; rate/dividend knots are added.
/// asset_upper_boundary : float or None, optional
///     Finite positive bound, or automatic selection. Must exceed spot and
///     strike when a numerical grid is needed.
/// variance_upper_boundary : float or None, optional
///     Finite positive bound, or automatic selection. Must exceed initial
///     variance when a variance grid is needed.
///
/// Raises
/// ------
/// InvalidInputError
///     For invalid counts or bounds. Context-dependent coverage is checked at
///     pricing.
///
/// Notes
/// -----
/// The asset grid uses log spot and an automatically selected positive lower boundary.
/// Uses the modified Craig-Sneyd (MCS) alternating direction implicit (ADI)
/// scheme on nonuniform grids, with Ikonen-Toivanen splitting for American
/// exercise. Supports HestonParameters and EuropeanOption/AmericanOption,
/// including violations of the Feller condition, zero parameters, correlation
/// endpoints and deterministic rate curves. Pricing rejects grids whose estimated
/// workspace exceeds 1 GiB or whose spatial grid point count times the number
/// of time intervals, including curve knots, exceeds one billion. American
/// pricing also limits estimated exercise-boundary work to 25 million curve-knot
/// visits. Check spatial
/// and time refinement and domain enlargement
/// separately. Non-finite or materially negative results raise
/// ComputationFailureError. Requested Greeks have status "unsupported";
/// implied-parameter solvers raise UnsupportedOperationError. Inputs and settings
/// are immutable; core pricing releases the GIL and permits concurrent calls
/// with independent state.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct FiniteDifferenceHestonVanillaEngine(
    pub(crate) kiyosi::FiniteDifferenceHestonVanillaEngine,
);

engine_methods!(
    /// Return the price.
    ///
    /// Parameters
    /// ----------
    /// instrument : EuropeanOption or AmericanOption
    ///     European or American call or put. European valuation may precede
    ///     effective_time; American valuation must be within contract life.
    /// context : PricingContext
    ///     Heston market snapshot, with initial variance at valuation. Rate/dividend
    ///     curves must cover valuation through expiry.
    ///
    /// Returns
    /// -------
    /// float
    ///     Price, or intrinsic value at the expiry instant.
    ///
    /// Raises
    /// ------
    /// UnsupportedOperationError
    ///     For other models or instruments, including at expiry.
    /// InvalidInputError
    ///     Outside permitted valuation bounds or curve coverage, or for
    ///     insufficient explicit bounds.
    /// ComputationFailureError
    ///     For unrepresentable grids or invalid numerical results.
    ///
    FiniteDifferenceHestonVanillaEngine, binomial::VanillaInstrument<'_>,
    [
        ///
        /// Notes
        /// -----
        /// Requested Greeks have status "unsupported". Bump controls are
        /// validated but unused; numerical_greeks rejects Heston.
        ///
    ],
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
        /// cover the interval; active explicit engine bounds must provide coverage.
        /// Some products prepare several state layers or continuation components.
        /// Accuracy requires grid and truncation convergence checks. Long work
        /// releases the GIL; cancellation returns no partial solution.
        #[pyo3(signature = (instrument, context, *, spot_range))]
        #[expect(clippy::needless_pass_by_value, reason = "PyO3 extracts the instrument enum by value.")]
        fn solve(
            &self,
            py: pyo3::Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: binomial::VanillaInstrument<'_>,
            context: &crate::market::PricingContext,
            #[pyo3(from_py_with = super::solution::spot_range)] spot_range: std::ops::RangeInclusive<f64>,
        ) -> pyo3::PyResult<super::solution::FiniteDifferenceSolution> {
            binomial::with_vanilla!(&instrument, option, super::solution::solve(py, &self.0, option, context, spot_range))
        }

        /// Price European or American vanilla options across strikes.
        ///
        /// Parameters
        /// ----------
        /// instrument : EuropeanOption or AmericanOption
        ///     Template supplying the option type, exercise style and dates.
        /// context : PricingContext
        ///     Fixed Heston market and valuation time.
        /// strikes : iterable of float
        ///     Finite positive strikes. Empty input is valid.
        ///
        /// Returns
        /// -------
        /// tuple of float
        ///     Prices in strike order, preserving duplicates.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If applicability, contract, strike, coverage or numerical checks fail.
        /// TypeError, ValueError, OverflowError
        ///     If an input representation is invalid.
        ///
        /// Notes
        /// -----
        /// The automatic asset domain shares one rollback over the full
        /// spot-to-strike range. Compatible strikes with an explicit asset bound share
        /// an absolute grid. European batches can reuse one set of payoff weights.
        /// Check mesh convergence over the full range. Pricing releases the GIL
        /// and cancellation returns no partial result. The request caps combined
        /// rollback work at one billion grid updates and bounds strike inputs
        /// by the batch workspace limit.
        #[pyo3(signature = (instrument, context, *, strikes))]
        #[expect(clippy::needless_pass_by_value, reason = "PyO3 owns the extracted inputs before core pricing.")]
        fn price_strikes<'py>(
            &self,
            py: Python<'py>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: binomial::VanillaInstrument<'_>,
            context: &crate::market::PricingContext,
            #[pyo3(from_py_with = crate::conversions::finite_difference_strikes)] strikes: Vec<f64>,
        ) -> PyResult<Bound<'py, PyTuple>> {
            let context = &context.0;
            let values = binomial::with_vanilla!(&instrument, option,
                crate::cancellation::run(py, || self.0.price_strikes(option, context, &strikes))
            )?;
            crate::conversions::python_tuple(py, domain(values)?, Ok)
        }

        /// Price a vanilla strike and expiry surface.
        ///
        /// Parameters
        /// ----------
        /// instrument : EuropeanOption or AmericanOption
        ///     Template supplying the payoff direction and effective time.
        /// context : PricingContext
        ///     Fixed Heston market and valuation time.
        /// expiries : iterable of datetime
        ///     Expiries at or after valuation and the effective time. American
        ///     valuation must be at or after the effective time.
        /// strikes : iterable of float
        ///     Finite positive strikes.
        ///
        /// Returns
        /// -------
        /// tuple of tuple of float
        ///     Rows in expiry order, with prices in strike order and duplicates preserved.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If applicability, input, workspace or numerical checks fail.
        /// TypeError, ValueError, OverflowError
        ///     If a Python input representation is invalid.
        ///
        /// Notes
        /// -----
        /// European surfaces share a forward grid spanning the longest maturity.
        /// Compatible American surfaces share rollback across tenors or factored
        /// operators in expiry tiles. Check mesh and domain convergence across
        /// the surface. The combined rollback work limit applies across the
        /// request. Cancellation returns no partial result.
        #[pyo3(signature = (instrument, context, *, expiries, strikes))]
        fn price_surface<'py>(
            &self,
            py: Python<'py>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: binomial::VanillaInstrument<'_>,
            context: &crate::market::PricingContext,
            expiries: &Bound<'_, PyAny>,
            strikes: &Bound<'_, PyAny>,
        ) -> PyResult<Bound<'py, PyTuple>> {
            let (expiries, strikes) = crate::conversions::surface_inputs(expiries, strikes)?;
            let context = &context.0;
            let rows = binomial::with_vanilla!(instrument, option, crate::cancellation::run(py, || {
                self.0.price_surface(option, context, &expiries, &strikes)
            }))?;
            crate::conversions::python_tuple(py, domain(rows)?, |row| {
                crate::conversions::python_tuple(py, row, Ok)
            })
        }

        #[classattr]
        const _STATE_FIELDS: &'static [&'static str] = &[
            "asset_step_count", "variance_step_count", "time_step_count",
            "asset_upper_boundary", "variance_upper_boundary",
        ];

        #[new]
        #[pyo3(signature = (*, asset_step_count=kiyosi::engines::HestonFiniteDifferenceSettings::default().asset_step_count, variance_step_count=kiyosi::engines::HestonFiniteDifferenceSettings::default().variance_step_count, time_step_count=kiyosi::engines::HestonFiniteDifferenceSettings::default().time_step_count, asset_upper_boundary=kiyosi::engines::HestonFiniteDifferenceSettings::default().asset_upper_boundary, variance_upper_boundary=kiyosi::engines::HestonFiniteDifferenceSettings::default().variance_upper_boundary))]
        fn new(
            #[pyo3(from_py_with = integer)] asset_step_count: i32,
            #[pyo3(from_py_with = integer)] variance_step_count: i32,
            #[pyo3(from_py_with = integer)] time_step_count: i32,
            #[pyo3(from_py_with = crate::conversions::optional_real)] asset_upper_boundary: Option<f64>,
            #[pyo3(from_py_with = crate::conversions::optional_real)] variance_upper_boundary: Option<f64>,
        ) -> PyResult<Self> {
            Ok(Self(domain(
                kiyosi::FiniteDifferenceHestonVanillaEngine::new(
                    kiyosi::engines::HestonFiniteDifferenceSettings {
                        asset_step_count,
                        variance_step_count,
                        time_step_count,
                        asset_upper_boundary,
                        variance_upper_boundary,
                    },
                ),
            )?))
        }

        /// Return the configured number of asset intervals.
        #[getter]
        fn asset_step_count(&self) -> i32 {
            self.0.settings().asset_step_count
        }

        /// Return the configured number of variance intervals.
        #[getter]
        fn variance_step_count(&self) -> i32 {
            self.0.settings().variance_step_count
        }

        /// Return the base time intervals before inserting curve knots.
        #[getter]
        fn time_step_count(&self) -> i32 {
            self.0.settings().time_step_count
        }

        /// Return the explicit asset upper bound, or None.
        #[getter]
        fn asset_upper_boundary(&self) -> Option<f64> {
            self.0.settings().asset_upper_boundary
        }

        /// Return the explicit variance upper bound, or None.
        #[getter]
        fn variance_upper_boundary(&self) -> Option<f64> {
            self.0.settings().variance_upper_boundary
        }
    }
);

engine_methods!(@evaluate FiniteDifferenceHestonVanillaEngine, binomial::VanillaInstrument<'_>, binomial::with_vanilla);
