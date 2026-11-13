use crate::conversions::{integer_index, real};
use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

fn evaluation_count(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    integer_index(value)?.extract()
}

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
///     10000.
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
/// abs(price). Error estimates are not a universal accuracy guarantee. The
/// budget covers Heston and control-variate characteristic functions across
/// all integrals and tail checks. Expiry and deterministic-variance branches
/// need no evaluations. Requested Greeks have status "unsupported";
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
        #[new]
        #[pyo3(signature = (*, absolute_tolerance=kiyosi::HestonIntegrationSettings::default().absolute_tolerance, relative_tolerance=kiyosi::HestonIntegrationSettings::default().relative_tolerance, max_evaluations=kiyosi::HestonIntegrationSettings::default().max_evaluations))]
        fn new(
            #[pyo3(from_py_with = real)] absolute_tolerance: f64,
            #[pyo3(from_py_with = real)] relative_tolerance: f64,
            #[pyo3(from_py_with = evaluation_count)] max_evaluations: usize,
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

impl AnalyticHestonEuropeanEngine {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "retain the Python borrow during core evaluation"
    )]
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: PyRef<'_, EuropeanOption>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let option = &instrument.0;
        crate::engines::evaluate(py, &self.0, option, context, request)
    }
}
