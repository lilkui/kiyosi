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
use crate::risk;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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
        kiyosi::$engine::with_step_count($settings, $step)
    };
    (@barrier $engine:ident, $instrument:ty) => {
        sampling_settings!(@structured $engine, $instrument, with_contract);
    };
    (@structured $engine:ident, $instrument:ty, $with:path) => {
        numerical_greeks_methods!(
            $instrument,
            |binding, py, instrument, context, request| {
                let engine = binding.0;
                let context = &context.0;
                $with!(&instrument, option, Ok(risk::PricingResult(domain(crate::cancellation::run(py, || {
                    engine.price_with_greeks(option, context, request)
                })?)?)))
            },
            [
                /// OSError
                ///     If system entropy or worker creation fails.
                ///
                /// Notes
                /// -----
                /// Barrier engines use the default spot bump for event-availability
                /// checks and continuous at-hit rebate derivatives. Autocallable and
                /// accumulator engines use numerical differences with engine defaults.
                /// At expiry spot Greeks use default terminal-payoff stencils.
                /// Use numerical_greeks for
                /// explicit scalar bump calculations on supported BSM pairs.
            ],
            sampling_settings!(
                $engine,
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
                /// OSError
                ///     If system entropy or worker creation fails.
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
                let engine = self.0;
                let context = &context.0;
                $with!(&instrument, option, domain(crate::cancellation::run(py, || {
                    request.evaluate(&engine, option, context)
                })?))
            }
        }
    };
    ($engine:ident, [$($step:ident)?], $($methods:item),* $(,)?) => {
        #[pymethods]
        impl $engine {
            /// Returns a price with sampling uncertainty.
            ///
            /// Parameters
            /// ----------
            /// instrument : object
            ///     A supported contract. Vanilla statistics require EuropeanOption;
            ///     other engines accept the same contracts as price().
            /// context : PricingContext
            ///     Market snapshot and valuation instant.
            ///
            /// Returns
            /// -------
            /// MonteCarloEstimate
            ///     Mean, optional standard error and asymptotic 95% confidence interval.
            ///     Sobol, insufficient independent samples and odd antithetic counts
            ///     omit uncertainty. Known pseudorandom analytic values have zero error.
            ///     Sampling uncertainty does not measure numerical integration bias.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If pricing or uncertainty validation fails, or the contract is unsupported.
            /// OSError
            ///     If system entropy or worker creation fails.
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
            #[pyo3(signature = (*, path_count=kiyosi::MonteCarloSettings::default().path_count, $($step=kiyosi::$engine::default().$step(),)? seed=None, thread_count=kiyosi::MonteCarloSettings::default().thread_count, sampling=kiyosi::MonteCarloSettings::default().sampling, antithetic=kiyosi::MonteCarloSettings::default().antithetic))]
            fn new(
                #[pyo3(from_py_with = integer)] path_count: i32,
                $(#[pyo3(from_py_with = integer)] $step: i32,)?
                seed: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = integer)] thread_count: i32,
                #[pyo3(from_py_with = sampling)] sampling: kiyosi::MonteCarloSampling,
                #[pyo3(from_py_with = boolean)] antithetic: bool,
            ) -> PyResult<Self> {
                let settings = kiyosi::MonteCarloSettings {
                    path_count,
                    seed: seed.map(|value| integer_index(value)?.extract::<u64>()).transpose()?,
                    thread_count,
                    sampling,
                    antithetic,
                };
                Ok(Self(domain(sampling_settings!(@new $engine, settings, [$($step)?]))?))
            }

            /// Return an engine with a concrete seed, preserving all other controls.
            ///
            /// An existing seed is retained. Save or pickle the returned engine before
            /// pricing to replay its draws within the same build. The original is unchanged.
            /// Entropy failures raise OSError.
            fn with_resolved_seed(&self) -> PyResult<Self> {
                Ok(Self(domain(self.0.with_resolved_seed())?))
            }

            /// Return the exact number of sampled paths.
            #[getter]
            fn path_count(&self) -> i32 {
                self.0.sampling_settings().path_count
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
                self.0.sampling_settings().seed
            }
            /// Return the per-call worker cap; zero selects automatic sizing.
            #[getter]
            fn thread_count(&self) -> i32 {
                self.0.sampling_settings().thread_count
            }
            /// Return the sampling choice string: pseudo_random or sobol.
            #[getter]
            fn sampling(&self) -> &'static str {
                match self.0.sampling_settings().sampling {
                    kiyosi::MonteCarloSampling::PseudoRandom => "pseudo_random",
                    kiyosi::MonteCarloSampling::Sobol => "sobol",
                }
            }
            /// Return whether opposite normal draws are paired.
            #[getter]
            fn antithetic(&self) -> bool {
                self.0.sampling_settings().antithetic
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
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
///     Sobol sampling requires a power of two.
/// step_count : int, optional
///     Exercise intervals, 1 through 10,000; defaults to 50. Validated even when
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
///     If paths are outside 1 through 10,000,000, steps outside 1 through
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// European pricing samples the exact terminal distribution and ignores
/// step_count. Near-strike payoffs preserve small movements even when the
/// terminal spot rounds to the strike. American pricing uses least-squares
/// Monte Carlo (Longstaff-Schwartz) with
/// step_count exercise intervals and requires at least two before expiry.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloVanillaEngine(pub(crate) kiyosi::MonteCarloVanillaEngine);

sampling_settings!(
    MonteCarloVanillaEngine,
    [step_count],
    /// Return native, numerical, mixed, native_with_spot_control, terminal or unsupported.
    ///
    /// Checks model applicability without pricing or sampling. Life and history
    /// validation remain part of pricing. Use numerical_greeks for scalar bumps.
    fn greek_method(
        &self,
        py: pyo3::Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: binomial::VanillaInstrument<'_>,
        context: &crate::market::PricingContext,
    ) -> pyo3::PyResult<&'static str> {
        Ok(self
            .evaluate(py, instrument, context, crate::engines::GreekMethodRequest)?
            .name())
    },
    /// Return shift-control names potentially used by this Greek calculation path.
    ///
    /// Individual requests may use fewer internal default controls. Use
    /// numerical_greeks for configurable scalar differences.
    fn active_greek_controls(
        &self,
        py: pyo3::Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: binomial::VanillaInstrument<'_>,
        context: &crate::market::PricingContext,
    ) -> pyo3::PyResult<Vec<&'static str>> {
        Ok(self
            .evaluate(py, instrument, context, crate::engines::GreekMethodRequest)?
            .active_controls()
            .to_vec())
    },
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
        self.evaluate(py, instrument, context, ScalarRequest::Price)
    },
    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str
    ///     Greek names or "all" to request every measure; duplicates are ignored.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     with time Greeks absent at expiry; other terminal Greeks follow the payoff.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the request is empty, pricing fails or a feasible default bumped valuation fails.
    /// ValueError
    ///     If a Greek name is unknown.
    /// TypeError
    ///     If greeks is not a string or iterable of strings.
    ///
    /// Notes
    /// -----
    /// System entropy and worker-creation failures raise OSError.
    /// Uses the product's default estimation controls. Custom bumps belong to numerical_greeks.
    #[pyo3(signature = (instrument, context, greeks))]
    ///
    /// See Also
    /// --------
    /// numerical_greeks : Uniform numerical bump controls for supported BSM pairs.
    ///     Heston and local-volatility models are unsupported by this helper.
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)]
        instrument: binomial::VanillaInstrument<'_>,
        context: &PricingContext,
        greeks: &Bound<'_, PyAny>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks)?;
        let engine = self.0;
        let context = &context.0;
        let result = binomial::with_vanilla!(
            instrument,
            option,
            domain(crate::cancellation::run(py, || engine
                .price_with_greeks(option, context, request))?)
        )?;
        Ok(risk::PricingResult(result))
    }
);

/// Monte Carlo price with optional sampling uncertainty.
///
/// Notes
/// -----
/// Standard error uses independent pseudorandom observations or even
/// antithetic pair means. The interval is an asymptotic 95% normal confidence interval.
/// Uncertainty is absent for Sobol, insufficient independent samples and odd
/// antithetic counts; known pseudorandom settlements have zero uncertainty.
/// The seed property identifies the actual draws, including for unseeded engines;
/// None means no sampling occurred. Replay requires the same inputs, engine
/// controls and build.
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
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Only likelihood-ratio delta and gamma are returned; other requested Greeks are absent.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloDigitalEngine(pub(crate) kiyosi::MonteCarloDigitalEngine);

numerical_greeks_methods!(
    digital::DigitalInstrument<'_>,
    |binding, py, instrument, context, request| {
        let engine = binding.0;
        let option = instrument.contract();
        let context = &context.0;
        Ok(risk::PricingResult(domain(crate::cancellation::run(py, || {
            engine.price_with_greeks(option, context, request)
        })?)?))
    },
    [
        /// OSError
        ///     If system entropy or worker creation fails.
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use native
        /// likelihood-ratio estimators. Zero volatility uses scalar differences.
        /// At expiry they use default terminal-payoff stencils.
        /// Use numerical_greeks for configurable scalar differences.
    ],
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
            self.evaluate(py, instrument, context, ScalarRequest::Price)
        }
    )
);

/// Flat-rate CPU Monte Carlo pricing for continuous average-price options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Geometric averages and single fixings sample exact distributions.
/// Arithmetic averages use step_count trapezoidal intervals with the first
/// interval integrated conditionally.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloAsianEngine(pub(crate) kiyosi::MonteCarloAsianEngine);

numerical_greeks_methods!(
    asian::AsianInstrument<'_>,
    |binding, py, instrument, context, request| {
        let engine = binding.0;
        let context = &context.0;
        asian::with_asian!(
            instrument,
            option,
            Ok(risk::PricingResult(domain(crate::cancellation::run(py, || {
                engine.price_with_greeks(option, context, request)
            })?)?))
        )
    },
    [
        /// OSError
        ///     If system entropy or worker creation fails.
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use native estimators.
        /// Zero volatility uses scalar differences. At expiry delta and gamma
        /// use default terminal-payoff stencils. Use numerical_greeks for
        /// configurable scalar differences.
    ],
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
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Monitoring uses continuous survival or the original fixing dates.
/// Near-strike payoffs retain movements even when terminal spot rounds to strike.
/// Only delta and gamma are returned.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloBarrierEngine(pub(crate) kiyosi::MonteCarloBarrierEngine);

sampling_settings!(@barrier MonteCarloBarrierEngine, PyRef<'_, barrier::BarrierOption>);

/// Flat-rate CPU Monte Carlo pricing for double-barrier vanilla options.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Monitoring uses continuous survival or the original fixing dates.
/// Near-strike payoffs retain movements even when terminal spot rounds to strike.
/// Only delta and gamma are returned.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct MonteCarloDoubleBarrierEngine(pub(crate) kiyosi::MonteCarloDoubleBarrierEngine);

sampling_settings!(@barrier MonteCarloDoubleBarrierEngine, PyRef<'_, double_barrier::DoubleBarrierOption>);

/// Flat-rate CPU Monte Carlo pricing for snowball and Phoenix notes.
///
/// Parameters
/// ----------
/// path_count : int, optional
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Simulation uses trading days when knock-in matters, or observation
/// dates and expiry otherwise.
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
///     Exact path count, 1 through 10,000,000; defaults to 131,072.
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
/// Calls own their samples and share a library-owned Rayon pool. A worker cap
/// does not reserve workers. Greeks and solves pin a seed per
/// request; solver tolerances do not measure sampling error.
/// Simulation uses the future trading dates. A current knock-out settles
/// before any purchase.
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
        let engine = self.0;
        let context = &context.0;
        binomial::with_vanilla!(
            instrument,
            option,
            domain(crate::cancellation::run(py, || request.evaluate(&engine, option, context))?)
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
        let engine = self.0;
        let option = instrument.contract();
        let context = &context.0;
        domain(crate::cancellation::run(py, || {
            request.evaluate(&engine, option, context)
        })?)
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
        let engine = self.0;
        let context = &context.0;
        asian::with_asian!(
            instrument,
            option,
            domain(crate::cancellation::run(py, || request.evaluate(&engine, option, context))?)
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
