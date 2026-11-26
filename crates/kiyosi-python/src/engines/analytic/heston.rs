use crate::conversions::{integer_usize, real};
use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use pyo3::{prelude::*, types::PyTuple};

/// Price European calls and puts under Heston by adaptive Fourier integration.
///
/// Parameters
/// ----------
/// absolute_tolerance : float, optional
///     Finite nonnegative tolerance in price units; defaults to 1e-8.
/// relative_tolerance : float, optional
///     Finite nonnegative relative tolerance; defaults to 1e-8.
/// max_evaluations : int, optional
///     Positive request-wide characteristic-function budget; defaults to
///     100,000.
///
/// Raises
/// ------
/// InvalidInputError
///     If tolerances are invalid or both zero, or the budget is zero.
///
/// Notes
/// -----
/// Supports HestonParameters and EuropeanOption only, including zero variance
/// parameters and correlation endpoints. Deterministic rates may be curves.
/// The target price error is absolute_tolerance + relative_tolerance *
/// abs(price). Integration accepts when the estimated quadrature error is
/// at most one quarter of that target. The budget counts Heston and
/// control-variate characteristic-function evaluations. Expiry and deterministic-variance
/// branches need no evaluations. Requested Greeks have status "unsupported";
/// implied-parameter solvers raise UnsupportedOperationError. Pricing releases
/// the GIL for core computation, with independent state for concurrent calls.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct AnalyticHestonEuropeanEngine(pub(crate) kiyosi::AnalyticHestonEuropeanEngine);

engine_methods!(
    /// Return the price.
    ///
    /// Parameters
    /// ----------
    /// instrument : EuropeanOption
    ///     European call or put; valuation may precede effective_time.
    /// context : PricingContext
    ///     Heston market snapshot; initial variance is at valuation. Rate/dividend
    ///     curves must cover valuation through expiry.
    ///
    /// Returns
    /// -------
    /// float
    ///     Price, or intrinsic payoff at the expiry instant.
    ///
    /// Raises
    /// ------
    /// UnsupportedOperationError
    ///     For unsupported instruments or models, including at expiry.
    /// InvalidInputError
    ///     If valuation follows expiry or lies outside curve coverage.
    /// NonConvergenceError
    ///     If the budget or numerical resolution cannot meet the estimated target.
    /// ComputationFailureError
    ///     For invalid numerical intermediates or prices.
    ///
    AnalyticHestonEuropeanEngine, PyRef<'_, EuropeanOption>,
    [
        ///
        /// Notes
        /// -----
        /// Requested Greeks have status "unsupported". Bump controls are
        /// validated but unused; numerical_greeks rejects Heston.
        ///
    ],
    {
        /// Price European calls or puts across strikes with fixed dates and market inputs.
        ///
        /// Parameters
        /// ----------
        /// instrument : EuropeanOption
        ///     Template supplying direction and dates.
        /// context : PricingContext
        ///     Heston market snapshot.
        /// strikes : iterable of float
        ///     Finite positive strikes in the desired output order.
        ///
        /// Returns
        /// -------
        /// tuple of float
        ///     Prices in input order, including duplicates. An empty input
        ///     validates the market and returns an empty tuple.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If applicability, coverage, strike or numerical checks fail.
        /// TypeError, ValueError, OverflowError
        ///     If an input representation is invalid.
        ///
        /// Notes
        /// -----
        /// Dense, evenly spaced log strikes can share a fast Fourier transform.
        /// Other batches share transform samples and retain adaptive convergence
        /// budgets. Pricing releases the GIL and cancellation returns no partial
        /// result.
        #[pyo3(signature = (instrument, context, *, strikes))]
        #[expect(clippy::needless_pass_by_value, reason = "PyO3 owns the extracted inputs before core pricing.")]
        fn price_strikes<'py>(
            &self,
            py: Python<'py>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, EuropeanOption>,
            context: &crate::market::PricingContext,
            #[pyo3(from_py_with = crate::conversions::reals)] strikes: Vec<f64>,
        ) -> PyResult<Bound<'py, PyTuple>> {
            let option = &instrument.0;
            let context = &context.0;
            let values = crate::cancellation::run(py, || self.0.price_strikes(option, context, &strikes))?;
            crate::conversions::python_tuple(py, domain(values)?, Ok)
        }

        #[classattr]
        const _STATE_FIELDS: &'static [&'static str] = &[
            "absolute_tolerance", "relative_tolerance", "max_evaluations",
        ];

        #[new]
        #[pyo3(signature = (*, absolute_tolerance=kiyosi::HestonIntegrationSettings::default().absolute_tolerance, relative_tolerance=kiyosi::HestonIntegrationSettings::default().relative_tolerance, max_evaluations=kiyosi::HestonIntegrationSettings::default().max_evaluations))]
        fn new(
            #[pyo3(from_py_with = real)] absolute_tolerance: f64,
            #[pyo3(from_py_with = real)] relative_tolerance: f64,
            #[pyo3(from_py_with = integer_usize)] max_evaluations: usize,
        ) -> PyResult<Self> {
            Ok(Self(domain(kiyosi::AnalyticHestonEuropeanEngine::new(
                kiyosi::HestonIntegrationSettings {
                    absolute_tolerance,
                    relative_tolerance,
                    max_evaluations,
                },
            ))?))
        }

        /// Return the absolute tolerance in price units.
        #[getter]
        fn absolute_tolerance(&self) -> f64 {
            self.0.settings().absolute_tolerance
        }

        /// Return the dimensionless relative tolerance.
        #[getter]
        fn relative_tolerance(&self) -> f64 {
            self.0.settings().relative_tolerance
        }

        /// Return the total characteristic-function budget per price request.
        #[getter]
        fn max_evaluations(&self) -> usize {
            self.0.settings().max_evaluations
        }
    }
);

engine_methods!(@evaluate AnalyticHestonEuropeanEngine, PyRef<'_, EuropeanOption>, with_contract);
