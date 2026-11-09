use crate::conversions::{boolean, integer, integer_index};
use crate::engines::ScalarRequest;
use crate::engines::binomial;
use crate::error::domain;
use crate::instruments::accumulator;
use crate::instruments::asian;
use crate::instruments::autocallable;
use crate::instruments::barrier;
use crate::instruments::digital;
use crate::instruments::double_barrier;
use crate::market::PricingContext;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Cap the process-wide Monte Carlo pool before its first parallel request.
///
/// Parameters
/// ----------
/// max_workers : int
///     Positive maximum worker count, limited further by available CPUs.
///
/// Raises
/// ------
/// InvalidInputError
///     If max_workers is nonpositive.
/// UnsupportedOperationError
///     If a different cap is supplied after pool initialization.
/// TypeError
///     If max_workers is a bool or does not support integer indexing.
/// OverflowError
///     If max_workers is outside the signed 32-bit integer range.
///
/// Notes
/// -----
/// Creates no threads. The default pool size is available CPU parallelism.
/// Configure each process before submitting parallel pricing work. Repeating
/// the configured cap is valid; failed configuration leaves it unchanged.
/// Engine max_active_workers caps active work per request within this pool.
///
#[pyfunction]
#[pyo3(signature = (*, max_workers))]
pub(crate) fn configure_monte_carlo_pool(
    #[pyo3(from_py_with = integer)] max_workers: i32,
) -> PyResult<()> {
    domain(kiyosi::configure_monte_carlo_pool(max_workers))
}

fn sampling(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::MonteCarloSampling> {
    match value.extract::<&str>()? {
        "pseudo_random" => Ok(kiyosi::MonteCarloSampling::PseudoRandom),
        "sobol" => Ok(kiyosi::MonteCarloSampling::Sobol),
        _ => Err(PyValueError::new_err("unknown Monte Carlo sampling")),
    }
}

macro_rules! sampling_settings {
    (@new $engine:ident, $settings:expr, []) => {
        kiyosi::$engine::new($settings)
    };
    (@new $engine:ident, $settings:expr, [$step:ident]) => {
        kiyosi::$engine::new(kiyosi::MonteCarloGridSettings { sampling: $settings, step_count: $step })
    };
    (@sampling $engine:expr, []) => {
        $engine.sampling_settings()
    };
    (@sampling $engine:expr, [$step:ident]) => {
        $engine.settings().sampling
    };
    (@barrier $engine:ident, $instrument:ty) => {
        sampling_settings!(@structured_impl $engine, $instrument, with_contract);
    };
    (@structured $engine:ident, $instrument:ty, $with:path) => {
        sampling_settings!(@structured_impl $engine, $instrument, $with);
    };
    (@structured_impl $engine:ident, $instrument:ty, $with:path) => {
        numerical_greeks_methods!(
            $instrument,
            [
                /// OSError
                ///     If system entropy or worker creation fails.
                ///
                /// Notes
                /// -----
                /// Barrier engines use spot_shift for event-availability checks and continuous
                /// at-hit rebate derivatives. Autocallable and accumulator engines use
                /// numerical differences with engine defaults. At expiry spot Greeks use
                /// default terminal-payoff stencils. Use numerical_greeks for explicit scalar
                /// bump calculations on supported BSM pairs.
                ///
            ],
            sampling_settings!(
                $engine,
                [],
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
                /// OSError
                ///     If system entropy or worker creation fails.
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
        impl $engine {
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
    ($engine:ident, [$($step:ident)?], $($methods:item),* $(,)?) => {
        #[pymethods]
        impl $engine {
            /// Return the scalar price and the seed used by this request.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     Any contract supported by price(), including American vanilla options.
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            ///
            /// Returns
            /// -------
            /// tuple[float, int or None]
            ///     Price and actual sampling seed; None when no sampling is needed.
            ///     Reprice with replace(seed=seed) and the same inputs and controls to
            ///     replay the result within the same build. The engine is unchanged.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If domain validation, engine applicability or scalar pricing fails.
            /// OSError
            ///     If system entropy or worker creation fails.
            ///
            /// Notes
            /// -----
            /// Does not compute sampling uncertainty. Signal exceptions cancel the request
            /// and propagate without a partial result, as for price().
            ///
            fn price_with_seed(
                &self,
                py: Python<'_>,
                instrument: &Bound<'_, PyAny>,
                context: &PricingContext,
            ) -> PyResult<(f64, Option<u64>)> {
                self.evaluate(py, crate::conversions::instrument(instrument)?, context,
                    crate::engines::SeededPriceRequest)
            }

            /// Return a price with sampling uncertainty.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     A supported contract. Vanilla statistics require EuropeanOption; other
            ///     engines accept the same contracts as price().
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            ///
            /// Returns
            /// -------
            /// MonteCarloEstimate
            ///     Mean, optional standard error and asymptotic 95% confidence interval.
            ///     Sobol, insufficient independent samples and odd antithetic counts omit
            ///     uncertainty. Known pseudorandom analytic values have zero error.
            ///     Sampling uncertainty does not measure numerical integration bias.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If pricing or uncertainty validation fails, or the contract is
            ///     unsupported.
            /// OSError
            ///     If system entropy or worker creation fails.
            ///
            fn price_with_statistics(
                &self,
                py: Python<'_>,
                instrument: &Bound<'_, PyAny>,
                context: &PricingContext,
            ) -> PyResult<MonteCarloEstimate> {
                Ok(MonteCarloEstimate(self.evaluate(
                    py, crate::conversions::instrument(instrument)?, context,
                    crate::engines::StatisticsRequest,
                )?))
            }

            #[new]
            #[pyo3(signature = (*, path_count=kiyosi::MonteCarloSettings::default().path_count, $($step=kiyosi::$engine::default().$step(),)? seed=None, max_active_workers=kiyosi::MonteCarloSettings::default().max_active_workers, sampling=kiyosi::MonteCarloSettings::default().sampling, antithetic=kiyosi::MonteCarloSettings::default().antithetic))]
            fn new(
                #[pyo3(from_py_with = integer)] path_count: i32,
                $(#[pyo3(from_py_with = integer)] $step: i32,)?
                seed: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = integer)] max_active_workers: i32,
                #[pyo3(from_py_with = sampling)] sampling: kiyosi::MonteCarloSampling,
                #[pyo3(from_py_with = boolean)] antithetic: bool,
            ) -> PyResult<Self> {
                let settings = kiyosi::MonteCarloSettings {
                    path_count,
                    seed: seed.map(|value| integer_index(value)?.extract::<u64>()).transpose()?,
                    max_active_workers,
                    sampling,
                    antithetic,
                };
                Ok(Self(domain(sampling_settings!(@new $engine, settings, [$($step)?]))?))
            }

            /// Return an engine with a concrete seed, preserving all other controls.
            ///
            /// An existing seed is retained. Save or pickle the returned engine before
            /// pricing to replay its draws within the same build. The original is
            /// unchanged. Entropy failures raise OSError.
            ///
            fn with_resolved_seed(&self) -> PyResult<Self> {
                Ok(Self(domain(self.0.with_resolved_seed())?))
            }

            /// Return the exact number of sampled paths.
            #[getter]
            fn path_count(&self) -> i32 {
                sampling_settings!(@sampling self.0, [$($step)?]).path_count
            }
            $(
                /// Return the configured grid size.
                #[getter]
                fn $step(&self) -> i32 {
                    self.0.$step()
                }
            )?
            /// Return the unsigned 64-bit seed, or None for system entropy.
            #[getter]
            fn seed(&self) -> Option<u64> {
                sampling_settings!(@sampling self.0, [$($step)?]).seed
            }
            /// Return the per-call active worker cap; zero selects automatic sizing.
            ///
            /// Parallel calls share a CPU-sized pool; this cap does not bound thread
            /// creation.
            ///
            #[getter]
            fn max_active_workers(&self) -> i32 {
                sampling_settings!(@sampling self.0, [$($step)?]).max_active_workers
            }
            /// Return the sampling choice string: pseudo_random or sobol.
            #[getter]
            fn sampling(&self) -> &'static str {
                match sampling_settings!(@sampling self.0, [$($step)?]).sampling {
                    kiyosi::MonteCarloSampling::PseudoRandom => "pseudo_random",
                    kiyosi::MonteCarloSampling::Sobol => "sobol",
                }
            }
            /// Return whether opposite normal draws are paired.
            #[getter]
            fn antithetic(&self) -> bool {
                sampling_settings!(@sampling self.0, [$($step)?]).antithetic
            }
            $($methods)*
        }
    };
}

/// Flat-rate Monte Carlo pricing for European and American vanilla options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// step_count : int, optional
///     Exercise intervals, 1 through 10,000; defaults to 50. Validated even
///     when unused; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, steps outside 1 through
///     10,000, max_active_workers is negative, or Sobol paths are not a power
///     of two.
///
/// See Also
/// --------
/// EuropeanOption : Supported contract.
/// AmericanOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. European pricing samples the exact terminal
/// distribution and ignores step_count. Near-strike payoffs preserve small
/// movements even when the terminal spot rounds to the strike. American
/// pricing uses least-squares Monte Carlo (Longstaff-Schwartz) with step_count
/// exercise intervals and requires at least two before expiry.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloVanillaEngine(pub(crate) kiyosi::MonteCarloVanillaEngine);

numerical_greeks_methods!(
    binomial::VanillaInstrument<'_>,
    [///
    ],
    sampling_settings!(
        MonteCarloVanillaEngine,
        [step_count],
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
        /// Notes
        /// -----
        /// System entropy and worker-creation failures raise OSError.
        ///
        fn price(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)]
            instrument: binomial::VanillaInstrument<'_>,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

/// Monte Carlo price with optional sampling uncertainty.
///
/// Notes
/// -----
/// Standard error uses independent pseudorandom observations or even
/// antithetic pair means. The interval is an asymptotic 95% normal confidence
/// interval. Uncertainty is absent for Sobol, insufficient independent samples
/// and odd antithetic counts; known pseudorandom settlements have zero
/// uncertainty. The seed property identifies the actual draws, including for
/// unseeded engines; None means no sampling occurred. Replay requires the same
/// inputs, engine controls and build.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloEstimate(kiyosi::MonteCarloEstimate);

#[pymethods]
impl MonteCarloEstimate {
    /// Return the sampling seed, or None when no sampling occurred.
    #[getter]
    fn seed(&self) -> Option<u64> {
        self.0.seed()
    }
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
    /// Return availability or the reason sampling uncertainty is absent.
    #[getter]
    fn uncertainty_status(&self) -> &'static str {
        self.0.uncertainty_status().name()
    }
    /// Return the asymptotic 95% confidence interval, or None when unavailable.
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
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, max_active_workers is
///     negative, or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// CashOrNothingOption : Supported contract.
/// AssetOrNothingOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. Only likelihood-ratio delta and gamma are returned;
/// other requested Greeks are absent.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloDigitalEngine(pub(crate) kiyosi::MonteCarloDigitalEngine);

numerical_greeks_methods!(
    digital::DigitalInstrument<'_>,
    [
        /// OSError
        ///     If system entropy or worker creation fails.
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use native
        /// likelihood-ratio estimators. Zero volatility uses scalar differences. At
        /// expiry they use default terminal-payoff stencils. Use numerical_greeks for
        /// configurable scalar differences.
        ///
    ],
    sampling_settings!(
        MonteCarloDigitalEngine,
        [],
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
        /// Notes
        /// -----
        /// System entropy and worker-creation failures raise OSError.
        ///
        fn price(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)]
            instrument: digital::DigitalInstrument<'_>,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

/// Flat-rate CPU Monte Carlo pricing for continuous average-price options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// step_count : int, optional
///     Grid size, 2 through 10,000; defaults to 50. Validated even when
///     unused; see Notes for product-specific use.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, steps outside 2 through
///     10,000, max_active_workers is negative, or Sobol paths are not a power
///     of two.
///
/// See Also
/// --------
/// GeometricAveragePriceOption : Supported contract.
/// ArithmeticAveragePriceOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. Geometric averages and single fixings sample exact
/// distributions. Arithmetic averages use step_count trapezoidal intervals
/// with the first interval integrated conditionally.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloAsianEngine(pub(crate) kiyosi::MonteCarloAsianEngine);

numerical_greeks_methods!(
    asian::AsianInstrument<'_>,
    [
        /// OSError
        ///     If system entropy or worker creation fails.
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use native estimators.
        /// Zero volatility uses scalar differences. At expiry delta and gamma use
        /// default terminal-payoff stencils. Use numerical_greeks for configurable
        /// scalar differences.
        ///
    ],
    sampling_settings!(
        MonteCarloAsianEngine,
        [step_count],
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
        /// Notes
        /// -----
        /// System entropy and worker-creation failures raise OSError.
        ///
        fn price(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)]
            instrument: asian::AsianInstrument<'_>,
            context: &PricingContext,
        ) -> PyResult<f64> {
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

/// Flat-rate CPU Monte Carlo pricing for single-barrier vanilla options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, max_active_workers is
///     negative, or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// BarrierOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. Monitoring uses continuous survival or the original
/// fixing dates. Near-strike payoffs retain movements even when terminal spot
/// rounds to strike. Only delta and gamma are returned.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloBarrierEngine(pub(crate) kiyosi::MonteCarloBarrierEngine);

sampling_settings!(@barrier MonteCarloBarrierEngine, PyRef<'_, barrier::BarrierOption>);

/// Flat-rate CPU Monte Carlo pricing for double-barrier vanilla options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, max_active_workers is
///     negative, or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// DoubleBarrierOption : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. Monitoring uses continuous survival or the original
/// fixing dates. Near-strike payoffs retain movements even when terminal spot
/// rounds to strike. Only delta and gamma are returned.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloDoubleBarrierEngine(pub(crate) kiyosi::MonteCarloDoubleBarrierEngine);

sampling_settings!(@barrier MonteCarloDoubleBarrierEngine, PyRef<'_, double_barrier::DoubleBarrierOption>);

/// Flat-rate CPU Monte Carlo pricing for snowball and Phoenix notes.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, max_active_workers is
///     negative, or Sobol paths are not a power of two.
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
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. Simulation uses trading days when knock-in matters,
/// or observation dates and expiry otherwise.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloAutocallableEngine(pub(crate) kiyosi::MonteCarloAutocallableEngine);

sampling_settings!(
    @structured MonteCarloAutocallableEngine,
    autocallable::AutocallableInstrument<'_>,
    autocallable::with_note
);

/// Flat-rate CPU Monte Carlo pricing for accumulators.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
///     sampling requires a power of two.
/// seed : int or None, optional
///     Unsigned 64-bit seed, or None (default) to use system entropy.
/// max_active_workers : int, optional
///     Nonnegative active worker cap per call: zero (default) is automatic,
///     one is serial. Does not bound thread creation; parallel calls share a
///     worker pool, bounded by configure_monte_carlo_pool().
/// sampling : MonteCarloSampling, optional
///     Sampling method; defaults to pseudo_random.
/// antithetic : bool, optional
///     Pair opposite normal draws; defaults to False. An odd path count leaves
///     one unpaired path.
///
/// Raises
/// ------
/// InvalidInputError
///     If paths are outside 1 through 10,000,000, max_active_workers is
///     negative, or Sobol paths are not a power of two.
///
/// See Also
/// --------
/// Accumulator : Supported contract.
///
/// Notes
/// -----
/// Fixed seeds reproduce results across thread counts in the same build. Calls
/// own their samples and share a library-owned Rayon pool. A worker cap does
/// not reserve workers. Greeks pin a seed per request. Solves require an
/// explicit seed; call with_resolved_seed() first. Solver tolerances do not
/// measure sampling error. Simulation uses the future trading dates. A current
/// knock-out settles before any purchase.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloAccumulatorEngine(pub(crate) kiyosi::MonteCarloAccumulatorEngine);

sampling_settings!(
    @structured MonteCarloAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
    with_contract
);

impl MonteCarloVanillaEngine {
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: binomial::VanillaInstrument<'_>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        binomial::with_vanilla!(
            instrument,
            option,
            crate::engines::evaluate(py, &self.0, option, context, request)
        )
    }
}

impl MonteCarloDigitalEngine {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "retain the extracted Python borrow during evaluation"
    )]
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: digital::DigitalInstrument<'_>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        let option = instrument.contract();
        crate::engines::evaluate(py, &self.0, option, context, request)
    }
}

impl MonteCarloAsianEngine {
    pub(crate) fn evaluate<R: Send>(
        &self,
        py: Python<'_>,
        instrument: asian::AsianInstrument<'_>,
        context: &PricingContext,
        request: impl crate::engines::PricingRequest<R>,
    ) -> PyResult<R> {
        asian::with_asian!(
            instrument,
            option,
            crate::engines::evaluate(py, &self.0, option, context, request)
        )
    }
}

#[pyfunction]
#[pyo3(signature = (*, price, standard_error, uncertainty_status, seed=None))]
pub(crate) fn _monte_carlo_estimate(
    price: &Bound<'_, PyAny>,
    standard_error: Option<&Bound<'_, PyAny>>,
    uncertainty_status: &str,
    seed: Option<&Bound<'_, PyAny>>,
) -> PyResult<MonteCarloEstimate> {
    let status = match uncertainty_status {
        "available" => kiyosi::MonteCarloUncertaintyStatus::Available,
        "unsupported_sampling" => kiyosi::MonteCarloUncertaintyStatus::UnsupportedSampling,
        "insufficient_observations" => {
            kiyosi::MonteCarloUncertaintyStatus::InsufficientObservations
        }
        "incomplete_antithetic_pair" => {
            kiyosi::MonteCarloUncertaintyStatus::IncompleteAntitheticPair
        }
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "unknown uncertainty status",
            ));
        }
    };
    Ok(MonteCarloEstimate(domain(
        kiyosi::MonteCarloEstimate::from_statistics(
            crate::conversions::real(price)?,
            standard_error.map(crate::conversions::real).transpose()?,
            status,
            seed.map(|value| crate::conversions::integer_index(value)?.extract::<u64>())
                .transpose()?,
        ),
    )?))
}
