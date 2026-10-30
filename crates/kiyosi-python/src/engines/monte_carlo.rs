use crate::conversions::{check_integer, integer};
use crate::engines::binomial;
use crate::error::domain;
use crate::instruments::accumulator;
use crate::instruments::asian;
use crate::instruments::autocallable;
use crate::instruments::barrier;
use crate::instruments::digital;
use crate::instruments::double_barrier;
use crate::instruments::vanilla::EuropeanOption;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict};

fn boolean(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    value.cast::<PyBool>()?.extract()
}

fn sampling(value: &Bound<'_, PyAny>) -> PyResult<core::MonteCarloSampling> {
    match value.extract::<&str>()? {
        "pseudo_random" => Ok(core::MonteCarloSampling::PseudoRandom),
        "sobol" => Ok(core::MonteCarloSampling::Sobol),
        _ => Err(PyValueError::new_err("unknown Monte Carlo sampling")),
    }
}

macro_rules! sampling_settings {
    ($engine:ident, [$($step:ident)?], $($methods:item),* $(,)?) => {
        #[pymethods]
        impl $engine {
            #[new]
            #[pyo3(signature = (*, path_count=core::MonteCarloSettings::default().path_count, $($step=core::MonteCarloSettings::default().$step,)? seed=None, thread_count=core::MonteCarloSettings::default().thread_count, sampling=core::MonteCarloSettings::default().sampling, antithetic=core::MonteCarloSettings::default().antithetic))]
            fn new(
                py: Python<'_>,
                #[pyo3(from_py_with = integer)] path_count: i32,
                $(#[pyo3(from_py_with = integer)] $step: i32,)?
                seed: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = integer)] thread_count: i32,
                #[pyo3(from_py_with = sampling)] sampling: core::MonteCarloSampling,
                #[pyo3(from_py_with = boolean)] antithetic: bool,
            ) -> PyResult<Self> {
                let settings = core::MonteCarloSettings {
                    path_count,
                    seed: seed.map(|value| {
                        check_integer(value)?;
                        value.extract::<u64>()
                    }).transpose()?,
                    thread_count,
                    sampling,
                    antithetic,
                    ..Default::default()
                };
                $(let settings = core::MonteCarloSettings { $step, ..settings };)?
                Ok(Self(domain(py, core::$engine::new(settings))?))
            }

            /// Return the exact number of sampled paths.
            #[getter]
            fn path_count(&self) -> i32 {
                self.0.settings().path_count
            }
            $(
                /// Return the configured grid size.
                #[getter]
                fn $step(&self) -> i32 {
                    self.0.settings().$step
                }
            )?
            /// Return the unsigned 64-bit seed, or None for system entropy.
            #[getter]
            fn seed(&self) -> Option<u64> {
                self.0.settings().seed
            }
            /// Return the per-call worker cap; zero selects automatic sizing.
            #[getter]
            fn thread_count(&self) -> i32 {
                self.0.settings().thread_count
            }
            /// Return the sampling choice string: pseudo_random or sobol.
            #[getter]
            fn sampling(&self) -> &'static str {
                match self.0.settings().sampling {
                    core::MonteCarloSampling::PseudoRandom => "pseudo_random",
                    core::MonteCarloSampling::Sobol => "sobol",
                }
            }
            /// Return whether opposite normal draws are paired.
            #[getter]
            fn antithetic(&self) -> bool {
                self.0.settings().antithetic
            }
            $($methods)*
        }
    };
}

/// Flat-rate CPU Monte Carlo pricing for European and American vanilla
/// options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size, 2 through 10,000; defaults to 50. Validated even when
///     unused; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, steps outside 2 through
///     10,000, thread_count is negative, or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// EuropeanOption : Supported contract.
/// AmericanOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// European pricing samples the exact terminal distribution and ignores
/// step_count. Near-strike payoffs preserve small movements even when the
/// terminal spot rounds to the strike. American exercise grids use step_count
/// points and require at least three before expiry.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloVanillaEngine(pub(crate) core::MonteCarloVanillaEngine);

sampling_settings!(
    MonteCarloVanillaEngine,
    [step_count],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: binomial::VanillaInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        match instrument {
            binomial::VanillaInstrument::European(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
            binomial::VanillaInstrument::American(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
        }
    },
    /// Return the price and estimated sampling uncertainty.
    ///
    /// Parameters
    /// ----------
    /// instrument : EuropeanOption
    ///     European vanilla contract; American statistics are not exposed.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    ///
    /// Returns
    /// -------
    /// MonteCarloEstimate
    ///     Mean, optional standard error and asymptotic 95% confidence interval.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If domain validation or numerical pricing fails.
    /// OSError
    ///     If system entropy or worker creation fails.
    fn price_with_statistics(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            EuropeanOption,
        >,
        context: &PricingContext,
    ) -> PyResult<MonteCarloEstimate> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(MonteCarloEstimate(domain(
            py,
            py.detach(|| engine.price_with_statistics(&option, &context)),
        )?))
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float or None, optional
    ///     Positive finite absolute spot bump. European default: 0.01 spot units.
    ///     American default: 8% of spot; None also selects this American default.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    /// For American vanilla options, omitted or None `spot_shift` uses 8% of
    /// spot; other bumps use the shared core defaults.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: binomial::VanillaInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let engine = self.0;
        let context = context.0.clone();
        let result = match instrument {
            binomial::VanillaInstrument::European(option) => {
                let shifts = risk::shifts(settings)?;
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
            binomial::VanillaInstrument::American(option) => {
                let default = core::NumericalShiftSettings {
                    spot_shift: core::MonteCarloVanillaEngine::default_american_spot_shift(
                        &context,
                    ),
                    ..Default::default()
                };
                let fields = settings
                    .map(Bound::copy)
                    .transpose()?
                    .unwrap_or_else(|| PyDict::new(py));
                if fields
                    .get_item("spot_shift")?
                    .is_some_and(|value| value.is_none())
                {
                    fields.del_item("spot_shift")?;
                }
                let shifts = risk::shifts_with_defaults(Some(&fields), default)?;
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
        }?;
        Ok(risk::PricingResult(result))
    }
);

/// Monte Carlo price with optional sampling uncertainty.
///
/// Notes
/// -----
/// Standard error uses independent pseudorandom observations or even
/// antithetic pair means. The interval is an asymptotic 95% normal interval.
/// Uncertainty is absent for Sobol, insufficient independent samples and odd
/// antithetic counts; known pseudorandom settlements have zero uncertainty.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloEstimate(core::MonteCarloEstimate);

#[pymethods]
impl MonteCarloEstimate {
    /// Return the finite price in contract payment units.
    #[getter]
    fn price(&self) -> f64 {
        self.0.price()
    }
    /// Return sampling standard error, or None when unavailable.
    #[getter]
    fn standard_error(&self) -> Option<f64> {
        self.0.standard_error()
    }
    /// Return the asymptotic 95% interval, or None when unavailable.
    #[getter]
    fn confidence_interval(&self) -> Option<(f64, f64)> {
        self.0.confidence_interval()
    }
}

/// Flat-rate CPU Monte Carlo pricing for European digitals.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, thread_count is negative,
///     or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// CashOrNothingOption : Supported contract.
/// AssetOrNothingOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Only native delta and gamma are returned; other requested Greeks remain absent.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloDigitalEngine(pub(crate) core::MonteCarloDigitalEngine);

sampling_settings!(
    MonteCarloDigitalEngine,
    [],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: digital::DigitalInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.contract().clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: digital::DigitalInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let option = instrument.contract().clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);

/// Flat-rate CPU Monte Carlo pricing for continuous average-price options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size, 2 through 10,000; defaults to 50. Validated even when
///     unused; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, steps outside 2 through
///     10,000, thread_count is negative, or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// GeometricAveragePriceOption : Supported contract.
/// ArithmeticAveragePriceOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Geometric averages and single fixings sample exact distributions.
/// Arithmetic averages use step_count trapezoidal intervals with the first
/// interval integrated conditionally.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAsianEngine(pub(crate) core::MonteCarloAsianEngine);

sampling_settings!(
    MonteCarloAsianEngine,
    [step_count],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: asian::AsianInstrument<
            '_,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        match instrument {
            asian::AsianInstrument::Geometric(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = option.0.clone();
                domain(py, py.detach(|| engine.price(&option, &context)))
            }
        }
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: asian::AsianInstrument<
            '_,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let context = context.0.clone();
        let result = match instrument {
            asian::AsianInstrument::Geometric(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
            asian::AsianInstrument::Arithmetic(option) => {
                let option = option.0.clone();
                domain(
                    py,
                    py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
                )
            }
        }?;
        Ok(risk::PricingResult(result))
    }
);

/// Flat-rate CPU Monte Carlo pricing for single-barrier vanilla options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, thread_count is negative,
///     or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// BarrierOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Monitoring uses continuous survival or the original fixing dates.
/// Only delta and gamma are returned.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloBarrierEngine(pub(crate) core::MonteCarloBarrierEngine);

sampling_settings!(
    MonteCarloBarrierEngine,
    [],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            barrier::BarrierOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            barrier::BarrierOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);

/// Flat-rate CPU Monte Carlo pricing for double-barrier vanilla options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, thread_count is negative,
///     or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// DoubleBarrierOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Monitoring uses continuous survival or the original fixing dates.
/// Only delta and gamma are returned.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloDoubleBarrierEngine(pub(crate) core::MonteCarloDoubleBarrierEngine);

sampling_settings!(
    MonteCarloDoubleBarrierEngine,
    [],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            double_barrier::DoubleBarrierOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            double_barrier::DoubleBarrierOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);

/// Flat-rate CPU Monte Carlo pricing for snowball and Phoenix notes.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, thread_count is negative,
///     or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// SnowballOption : Supported contract.
/// BinarySnowballOption : Supported contract.
/// TernarySnowballOption : Supported contract.
/// PhoenixOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Simulation uses trading days when knock-in matters, or observation
/// dates and expiry otherwise.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAutocallableEngine(pub(crate) core::MonteCarloAutocallableEngine);

sampling_settings!(
    MonteCarloAutocallableEngine,
    [],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let context = context.0.clone();
        autocallable::with_note!(
            instrument,
            note,
            domain(py, py.detach(|| engine.price(&note, &context)))
        )
    },
    /// Return the price and estimated sampling uncertainty.
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
    /// MonteCarloEstimate
    ///     Mean, optional standard error and asymptotic 95% confidence interval.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If domain validation or numerical pricing fails.
    /// OSError
    ///     If system entropy or worker creation fails.
    fn price_with_statistics(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
    ) -> PyResult<MonteCarloEstimate> {
        let engine = self.0;
        let context = context.0.clone();
        autocallable::with_note!(
            instrument,
            note,
            Ok(MonteCarloEstimate(domain(
                py,
                py.detach(|| engine.price_with_statistics(&note, &context))
            )?))
        )
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &Bound<'_, PyAny>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let context = context.0.clone();
        autocallable::with_note!(
            instrument,
            note,
            Ok(risk::PricingResult(domain(
                py,
                py.detach(|| engine.price_with_greeks(&note, &context, request, shifts))
            )?))
        )
    }
);

/// Flat-rate CPU Monte Carlo pricing for accumulators.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 100,000.
///     Sobol sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// thread_count : int, optional
///     Nonnegative worker cap per call: zero (default) is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count
///     leaves one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, thread_count is negative,
///     or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// Accumulator : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build.
/// Each call owns its workers and samples. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Simulation uses the future trading dates. A current knock-out settles
/// before any purchase.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAccumulatorEngine(pub(crate) core::MonteCarloAccumulatorEngine);

sampling_settings!(
    MonteCarloAccumulatorEngine,
    [],
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
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            accumulator::Accumulator,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        domain(py, py.detach(|| engine.price(&option, &context)))
    },
    /// Return the price and estimated sampling uncertainty.
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
    /// MonteCarloEstimate
    ///     Mean, optional standard error and asymptotic 95% confidence interval.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If domain validation or numerical pricing fails.
    /// OSError
    ///     If system entropy or worker creation fails.
    fn price_with_statistics(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            accumulator::Accumulator,
        >,
        context: &PricingContext,
    ) -> PyResult<MonteCarloEstimate> {
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(MonteCarloEstimate(domain(
            py,
            py.detach(|| engine.price_with_statistics(&option, &context)),
        )?))
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            accumulator::Accumulator,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        let engine = self.0;
        let option = instrument.0.clone();
        let context = context.0.clone();
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| engine.price_with_greeks(&option, &context, request, shifts)),
        )?))
    }
);
