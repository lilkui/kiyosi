use crate::conversions::{real, supplied};
use crate::error::domain;
use crate::market::{RateTermStructure, rate};
use pyo3::prelude::*;

/// Immutable Heston stochastic-volatility parameters under the risk-neutral measure.
///
/// Parameters
/// ----------
/// risk_free_rate, dividend_yield : float or RateTermStructure
///     Continuously compounded annual rates or owned deterministic curves.
/// initial_variance : float
///     Variance at valuation; 20% initial volatility means 0.04 variance.
/// long_run_variance : float
///     Long-run variance level theta.
/// mean_reversion_speed : float
///     Mean-reversion coefficient kappa, with time measured in years.
/// vol_of_vol : float
///     Volatility-of-volatility parameter xi in
///     dv = kappa * (theta - v) dt + xi * sqrt(v) dW.
/// correlation : float
///     Correlation between the asset and variance Brownian motions in [-1, 1], including endpoints.
///
/// Raises
/// ------
/// InvalidInputError
///     For non-finite inputs, negative variance parameters or invalid correlation.
///
/// Notes
/// -----
/// Zero parameters and violations of the Feller condition
/// (2 * kappa * theta >= xi**2) are accepted. AnalyticHestonEuropeanEngine
/// prices European options; FiniteDifferenceHestonVanillaEngine prices European
/// and American options. Requested Greeks remain absent; implied-parameter solvers are unsupported.
/// Numeric conversion rejects booleans. Replacement validates in Rust before
/// returning a new owned model; the original is unchanged on failure.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct HestonParameters(pub(crate) kiyosi::HestonParameters);

#[pymethods]
impl HestonParameters {
    #[new]
    #[pyo3(signature = (*, risk_free_rate, dividend_yield, initial_variance, long_run_variance, mean_reversion_speed, vol_of_vol, correlation))]
    fn new(
        risk_free_rate: &Bound<'_, PyAny>,
        dividend_yield: &Bound<'_, PyAny>,
        initial_variance: &Bound<'_, PyAny>,
        long_run_variance: &Bound<'_, PyAny>,
        mean_reversion_speed: &Bound<'_, PyAny>,
        vol_of_vol: &Bound<'_, PyAny>,
        correlation: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::HestonParameters::new(
            rate(risk_free_rate)?,
            rate(dividend_yield)?,
            real(initial_variance)?,
            real(long_run_variance)?,
            real(mean_reversion_speed)?,
            real(vol_of_vol)?,
            real(correlation)?,
        ))?))
    }

    /// Return a validated new model; omitted fields or Ellipsis retain their values.
    ///
    /// Parameters
    /// ----------
    /// risk_free_rate, dividend_yield : float or RateTermStructure, optional
    ///     Replacement continuously compounded rates or owned curves.
    /// initial_variance, long_run_variance, mean_reversion_speed, vol_of_vol : float, optional
    ///     Replacement finite nonnegative variance-process parameters.
    /// correlation : float, optional
    ///     Replacement finite correlation in [-1, 1].
    ///
    /// Returns
    /// -------
    /// HestonParameters
    ///     New owned value; the original is unchanged on success and failure.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If replacement parameters fail model validation.
    /// TypeError
    ///     If a supplied value is None, bool or another unsupported representation.
    #[pyo3(signature = (*, risk_free_rate=crate::conversions::OMITTED, dividend_yield=crate::conversions::OMITTED, initial_variance=crate::conversions::OMITTED, long_run_variance=crate::conversions::OMITTED, mean_reversion_speed=crate::conversions::OMITTED, vol_of_vol=crate::conversions::OMITTED, correlation=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = supplied)] risk_free_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] dividend_yield: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] initial_variance: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] long_run_variance: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] mean_reversion_speed: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] vol_of_vol: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] correlation: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self(domain(self.0.replace(
            kiyosi::HestonParameterChanges {
                risk_free_rate: risk_free_rate.map(rate).transpose()?,
                dividend_yield: dividend_yield.map(rate).transpose()?,
                initial_variance: initial_variance.map(real).transpose()?,
                long_run_variance: long_run_variance.map(real).transpose()?,
                mean_reversion_speed: mean_reversion_speed.map(real).transpose()?,
                vol_of_vol: vol_of_vol.map(real).transpose()?,
                correlation: correlation.map(real).transpose()?,
            },
        ))?))
    }
    /// Returns the scalar rate for dated or undated explicitly flat curves.
    ///
    /// Raises UnsupportedOperationError for node-based curves; use risk_free_curve
    /// to preserve coverage when passing rates to constructors or replacements.
    #[getter]
    fn risk_free_rate(&self) -> PyResult<f64> {
        domain(self.0.risk_free_rate())
    }
    /// Returns the scalar yield for dated or undated explicitly flat curves.
    ///
    /// Raises UnsupportedOperationError for node-based curves; use dividend_curve
    /// to preserve coverage when passing yields to constructors or replacements.
    #[getter]
    fn dividend_yield(&self) -> PyResult<f64> {
        domain(self.0.dividend_yield())
    }
    /// Return an owned copy of the risk-free curve.
    #[getter]
    fn risk_free_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.risk_free_curve().clone())
    }
    /// Return an owned copy of the dividend curve.
    #[getter]
    fn dividend_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.dividend_curve().clone())
    }
    /// Return instantaneous variance at valuation.
    #[getter]
    fn initial_variance(&self) -> f64 {
        self.0.initial_variance()
    }
    /// Return the long-run variance level.
    #[getter]
    fn long_run_variance(&self) -> f64 {
        self.0.long_run_variance()
    }
    /// Return mean-reversion speed per year.
    #[getter]
    fn mean_reversion_speed(&self) -> f64 {
        self.0.mean_reversion_speed()
    }
    /// Return the volatility-of-volatility parameter xi.
    #[getter]
    fn vol_of_vol(&self) -> f64 {
        self.0.vol_of_vol()
    }
    /// Return the correlation between the asset and variance Brownian motions.
    #[getter]
    fn correlation(&self) -> f64 {
        self.0.correlation()
    }
}
