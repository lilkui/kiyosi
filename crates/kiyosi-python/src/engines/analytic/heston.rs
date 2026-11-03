use crate::conversions::{check_integer, real};
use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;

fn evaluation_count(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    check_integer(value)?;
    value.extract()
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
///     Positive request-wide characteristic-function budget; defaults to 10000.
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
/// The target price error is absolute_tolerance + relative_tolerance * abs(price).
/// Error estimates are not a universal accuracy guarantee. The budget covers
/// Heston and control-variate characteristic functions across all integrals and
/// tail checks. Expiry and deterministic-variance branches need no evaluations.
/// Requested Greeks remain None; implied-parameter solvers raise UnsupportedOperationError. Pricing releases
/// the GIL for core computation, with independent state for concurrent calls.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticHestonEuropeanEngine(pub(crate) kiyosi::AnalyticHestonEuropeanEngine);

#[pymethods]
impl AnalyticHestonEuropeanEngine {
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

    /// Return the price without changing the instrument or context.
    ///
    /// Parameters
    /// ----------
    /// instrument : EuropeanOption
    ///     European call or put within its inclusive contract life.
    /// context : PricingContext
    ///     Heston market snapshot; initial variance is at valuation.
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
    ///     Outside contract life or curve coverage.
    /// NonConvergenceError
    ///     If the budget or numerical resolution cannot meet the estimated target.
    /// ComputationFailureError
    ///     For invalid numerical intermediates or prices.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        self.evaluate(py, instrument, context, ScalarRequest::Price)
    }

    /// Return the Heston price with all requested sensitivities absent.
    ///
    /// Parameters
    /// ----------
    /// instrument : EuropeanOption
    ///     European option.
    /// context : PricingContext
    ///     Market snapshot.
    /// greeks : str or iterable of str
    ///     Required Greek request; representation and names are checked first.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price with every requested Greek set to None.
    ///
    /// Raises
    /// ------
    /// UnsupportedOperationError
    ///     If the model is unsupported; Heston currently provides no sensitivities.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts an owned borrow guard"
    )]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
        greeks: &Bound<'_, PyAny>,
    ) -> PyResult<risk::PricingResult> {
        let engine = self.0;
        let option = &instrument.0;
        let context = &context.0;
        let request = risk::greeks_request(greeks)?;
        Ok(risk::PricingResult(domain(py.detach(|| {
            engine.price_with_greeks(option, context, request, Default::default())
        }))?))
    }
}

impl AnalyticHestonEuropeanEngine {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "retain the Python borrow during core evaluation"
    )]
    pub(crate) fn evaluate(
        &self,
        py: Python<'_>,
        instrument: PyRef<'_, EuropeanOption>,
        context: &PricingContext,
        request: ScalarRequest,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = &instrument.0;
        let context = &context.0;
        domain(py.detach(|| request.evaluate(&engine, option, context)))
    }
}
