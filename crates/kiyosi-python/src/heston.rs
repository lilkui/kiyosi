use crate::conversions::real;
use crate::error::domain;
use crate::market::{rate, rate_properties};
use pyo3::prelude::*;

/// Heston stochastic-volatility parameters under the risk-neutral measure.
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
///     Volatility-of-volatility parameter xi in dv = kappa * (theta - v) dt +
///     xi * sqrt(v) dW.
/// correlation : float
///     Correlation between the asset and variance Brownian motions in [-1, 1],
///     including endpoints.
///
/// Raises
/// ------
/// InvalidInputError
///     For non-finite inputs, negative variance parameters or invalid
///     correlation.
///
/// Notes
/// -----
/// Zero parameters and violations of the Feller condition (2 * kappa * theta
/// >= xi**2) are accepted. AnalyticHestonEuropeanEngine prices European
/// options; FiniteDifferenceHestonVanillaEngine prices European and American
/// options. Requested Greeks have status "unsupported"; implied-parameter solvers
/// raise UnsupportedOperationError. Numeric conversion rejects booleans.
/// Replacement validates in Rust before returning a new owned model; the original
/// is unchanged on failure.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct HestonParameters(pub(crate) kiyosi::HestonParameters);

rate_properties!(
    HestonParameters,
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
            kiyosi::HestonParameterInputs {
                risk_free_rate: rate(risk_free_rate)?,
                dividend_yield: rate(dividend_yield)?,
                initial_variance: real(initial_variance)?,
                long_run_variance: real(long_run_variance)?,
                mean_reversion_speed: real(mean_reversion_speed)?,
                vol_of_vol: real(vol_of_vol)?,
                correlation: real(correlation)?,
            },
        ))?))
    },
    /// Return instantaneous variance at valuation.
    #[getter]
    fn initial_variance(&self) -> f64 {
        self.0.initial_variance()
    },
    /// Return the long-run variance level.
    #[getter]
    fn long_run_variance(&self) -> f64 {
        self.0.long_run_variance()
    },
    /// Return mean-reversion speed per year.
    #[getter]
    fn mean_reversion_speed(&self) -> f64 {
        self.0.mean_reversion_speed()
    },
    /// Return the volatility-of-volatility parameter xi.
    #[getter]
    fn vol_of_vol(&self) -> f64 {
        self.0.vol_of_vol()
    },
    /// Return the correlation between the asset and variance Brownian motions.
    #[getter]
    fn correlation(&self) -> f64 {
        self.0.correlation()
    },
);
