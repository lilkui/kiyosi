use crate::conversions::{check_changes, check_integer, integer};
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

macro_rules! sampling_settings {
    ($engine:ident, $($methods:item),* $(,)?) => {
        #[pymethods]
        impl $engine {
            #[new]
            #[pyo3(signature = (**fields))]
            fn new(py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                check_changes(
                    fields,
                    &[
                        "path_count",
                        "step_count",
                        "seed",
                        "thread_count",
                        "sampling",
                        "antithetic",
                    ],
                )?;
                let mut settings = core::MonteCarloSettings::default();
                if let Some(fields) = fields {
                    if let Some(value) = fields.get_item("path_count")? {
                        settings.path_count = integer(&value)?;
                    }
                    if let Some(value) = fields.get_item("step_count")? {
                        settings.step_count = integer(&value)?;
                    }
                    if let Some(value) = fields.get_item("thread_count")? {
                        settings.thread_count = integer(&value)?;
                    }
                    if let Some(value) = fields.get_item("seed")?.filter(|value| !value.is_none()) {
                        check_integer(&value)?;
                        settings.seed = Some(value.extract::<u64>()?);
                    }
                    if let Some(value) = fields.get_item("sampling")? {
                        settings.sampling = match value.extract::<&str>()? {
                            "pseudo_random" => core::MonteCarloSampling::PseudoRandom,
                            "sobol" => core::MonteCarloSampling::Sobol,
                            _ => return Err(PyValueError::new_err("unknown Monte Carlo sampling")),
                        };
                    }
                    if let Some(value) = fields.get_item("antithetic")? {
                        settings.antithetic = value.cast::<PyBool>()?.extract()?;
                    }
                }
                Ok(Self(domain(py, core::$engine::new(settings))?))
            }

            /// Return the exact number of sampled paths.
            #[getter]
            fn path_count(&self) -> i32 {
                self.0.settings().path_count
            }
            /// Return the configured grid size.
            #[getter]
            fn step_count(&self) -> i32 {
                self.0.settings().step_count
            }
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
/// step_count. American exercise grids use step_count points and require at
/// least three before expiry.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloVanillaEngine(pub(crate) core::MonteCarloVanillaEngine);

sampling_settings!(
    MonteCarloVanillaEngine,
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
/// Terminal sampling ignores step_count. Only native delta and gamma are
/// returned; other requested Greeks remain absent.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloDigitalEngine(pub(crate) core::MonteCarloDigitalEngine);

sampling_settings!(
    MonteCarloDigitalEngine,
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
/// Monitoring uses continuous survival or the original fixing dates;
/// step_count is validated but unused. Only delta and gamma are returned.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloBarrierEngine(pub(crate) core::MonteCarloBarrierEngine);

sampling_settings!(
    MonteCarloBarrierEngine,
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
/// Monitoring uses continuous survival or the original fixing dates;
/// step_count is validated but unused. Only delta and gamma are returned.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloDoubleBarrierEngine(pub(crate) core::MonteCarloDoubleBarrierEngine);

sampling_settings!(
    MonteCarloDoubleBarrierEngine,
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
/// dates and expiry otherwise. step_count is validated but unused.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAutocallableEngine(pub(crate) core::MonteCarloAutocallableEngine);

sampling_settings!(
    MonteCarloAutocallableEngine,
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
///     Exact path count; Sobol sampling requires a power of two.
/// step_count : int, optional
///     Grid size; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None to use system entropy.
/// thread_count : int, optional
///     Worker cap per call: zero is automatic, one is serial.
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws while preserving the exact path count.
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
/// Simulation uses the future trading dates; step_count is validated but
/// unused. A current knock-out settles before any purchase.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct MonteCarloAccumulatorEngine(pub(crate) core::MonteCarloAccumulatorEngine);

sampling_settings!(
    MonteCarloAccumulatorEngine,
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
    /// **settings
    ///     `all_greeks` requests every measure and requires omitting `greeks`.
    ///     Positive absolute bumps are `spot_shift`, `volatility_shift`,
    ///     `rate_shift` and integer `time_shift_days`. Omitted bumps use core
    ///     defaults; analytic engines still validate them.
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
