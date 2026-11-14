use crate::conversions::{boolean, integer};
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

#[derive(FromPyObject)]
pub(crate) enum BarrierInstrument<'py> {
    Single(PyRef<'py, barrier::BarrierOption>),
    Double(PyRef<'py, double_barrier::DoubleBarrierOption>),
}

macro_rules! with_barrier {
    ($instrument:expr, $option:ident, $body:expr) => {{
        match $instrument {
            BarrierInstrument::Single(option) => {
                let $option = &option.0;
                $body
            }
            BarrierInstrument::Double(option) => {
                let $option = &option.0;
                $body
            }
        }
    }};
}

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

pub(crate) fn sampling_name(value: kiyosi::MonteCarloSampling) -> &'static str {
    match value {
        kiyosi::MonteCarloSampling::PseudoRandom => "pseudo_random",
        kiyosi::MonteCarloSampling::Sobol => "sobol",
    }
}

macro_rules! sampling_settings {
    (@class $(#[$summary:meta])* $engine:ident,
     [$($minimum:literal, $step_description:literal)?],
     [$(#[$contracts:meta])*], [$(#[$notes:meta])*]) => {
        $(#[$summary])*
        ///
        /// Parameters
        /// ----------
        /// path_count : int, optional
        ///     Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        ///     sampling requires a power of two.
        $(#[doc = concat!("step_count : int, optional\n    ", $step_description, ", ", stringify!($minimum), " through 10,000; defaults to 50. Validated even\n    when unused; see Notes for product-specific use.")])?
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
        $(#[doc = concat!("    Also raised if step_count is outside ", stringify!($minimum), " through 10,000.")])?
        ///
        /// See Also
        /// --------
        $(#[$contracts])*
        ///
        /// Notes
        /// -----
        /// Fixed seeds reproduce results across thread counts in the same build. Calls
        /// own their samples and share a library-owned Rayon pool. A worker cap does
        /// not reserve workers. Greeks pin a seed per request. Solves require an
        /// explicit seed; call with_resolved_seed() first. Solver tolerances do not
        /// measure sampling error.
        $(#[$notes])*
        ///
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $engine(pub(crate) kiyosi::$engine);
    };
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
    (@structured $engine:ident, $instrument:ty, $with:path) => {
        sampling_settings!(
            $engine, $instrument, [],
            [
                /// OSError
                ///     If system entropy or worker creation fails.
                ///
                /// Notes
                /// -----
                /// Barrier engines use spot_shift for event-availability checks and continuous
                /// at-hit rebate derivatives. Autocallable and accumulator engines use
                /// numerical differences with the request's shifts. At expiry spot Greeks
                /// use terminal-payoff stencils with those shifts. Use numerical_greeks for scalar
                /// bump calculations on supported BSM pairs.
                ///
            ]
        );
        engine_methods!(@evaluate $engine, $instrument, $with);
    };
    ($engine:ident, $instrument:ty, [$($step:ident)?], [$(#[$greeks_doc:meta])*]) => {
        engine_methods!(@price $engine, $instrument, [$(#[$greeks_doc])*], [
            /// OSError
            ///     If system entropy or worker creation fails.
            ///
        ], {
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

            #[classattr]
            const _STATE_FIELDS: &'static [&'static str] = &[
                "path_count", $(stringify!($step),)? "seed", "max_active_workers",
                "sampling", "antithetic",
            ];

            #[new]
            #[pyo3(signature = (*, path_count=kiyosi::MonteCarloSettings::default().path_count, $($step=kiyosi::$engine::default().$step(),)? seed=kiyosi::MonteCarloSettings::default().seed, max_active_workers=kiyosi::MonteCarloSettings::default().max_active_workers, sampling=kiyosi::MonteCarloSettings::default().sampling, antithetic=kiyosi::MonteCarloSettings::default().antithetic))]
            fn new(
                #[pyo3(from_py_with = integer)] path_count: i32,
                $(#[pyo3(from_py_with = integer)] $step: i32,)?
                #[pyo3(from_py_with = crate::conversions::optional_seed)] seed: Option<u64>,
                #[pyo3(from_py_with = integer)] max_active_workers: i32,
                #[pyo3(from_py_with = sampling)] sampling: kiyosi::MonteCarloSampling,
                #[pyo3(from_py_with = boolean)] antithetic: bool,
            ) -> PyResult<Self> {
                let settings = kiyosi::MonteCarloSettings {
                    path_count,
                    seed,
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
            /// unchanged.
            ///
            /// Returns
            /// -------
            /// object
            ///     Engine of the same class with a concrete seed.
            ///
            /// Raises
            /// ------
            /// OSError
            ///     If system entropy is unavailable.
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
                sampling_name(sampling_settings!(@sampling self.0, [$($step)?]).sampling)
            }
            /// Return whether opposite normal draws are paired.
            #[getter]
            fn antithetic(&self) -> bool {
                sampling_settings!(@sampling self.0, [$($step)?]).antithetic
            }
        });
    };
}

sampling_settings!(
    @class
    /// Flat-rate Monte Carlo pricing for European and American vanilla options.
    MonteCarloVanillaEngine,
    [1, "Exercise intervals"],
    [
        /// EuropeanOption : Supported contract.
        /// AmericanOption : Supported contract.
    ],
    [
        /// European pricing samples the exact terminal
        /// distribution and ignores step_count. Near-strike payoffs preserve small
        /// movements even when the terminal spot rounds to the strike. American
        /// pricing uses least-squares Monte Carlo (Longstaff-Schwartz) with step_count
        /// exercise intervals and requires at least two before expiry.
    ]
);

sampling_settings!(
    MonteCarloVanillaEngine, binomial::VanillaInstrument<'_>, [step_count],
    [///
    ]
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

sampling_settings!(
    @class
    /// Flat-rate CPU Monte Carlo pricing for European digitals.
    MonteCarloDigitalEngine,
    [],
    [
        /// CashOrNothingOption : Supported contract.
        /// AssetOrNothingOption : Supported contract.
    ],
    [
        /// Only likelihood-ratio delta and gamma are returned;
        /// other requested Greeks are absent.
    ]
);

sampling_settings!(
    MonteCarloDigitalEngine, digital::DigitalInstrument<'_>, [],
    [
        /// OSError
        ///     If system entropy or worker creation fails.
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use native
        /// likelihood-ratio estimators. Zero volatility uses scalar differences. At
        /// expiry they use terminal-payoff stencils with the request's shifts.
        /// Use numerical_greeks for scalar differences throughout.
        ///
    ]
);

sampling_settings!(
    @class
    /// BSM Monte Carlo pricing for continuous average-price options.
    MonteCarloAsianEngine,
    [2, "Grid size"],
    [
        /// GeometricAveragePriceOption : Supported contract.
        /// ArithmeticAveragePriceOption : Supported contract.
    ],
    [
        /// Geometric averages and single fixings sample exact
        /// distributions. Arithmetic averages use step_count trapezoidal intervals
        /// with the first interval integrated conditionally. Geometric pricing
        /// requires constant rates before expiry; arithmetic pricing accepts
        /// deterministic rate curves.
    ]
);

sampling_settings!(
    MonteCarloAsianEngine, asian::AsianInstrument<'_>, [step_count],
    [
        /// OSError
        ///     If system entropy or worker creation fails.
        ///
        /// Notes
        /// -----
        /// At positive volatility before expiry delta and gamma use native estimators.
        /// Zero volatility uses scalar differences. At expiry delta and gamma use
        /// terminal-payoff stencils with the request's shifts. Use numerical_greeks
        /// for scalar differences throughout.
        ///
    ]
);

sampling_settings!(
    @class
    /// Flat-rate CPU Monte Carlo pricing for single- and double-barrier vanilla options.
    MonteCarloBarrierEngine,
    [],
    [
        /// BarrierOption : Supported single-barrier contract.
        /// DoubleBarrierOption : Supported double-barrier contract.
    ],
    [
        /// Monitoring uses continuous survival or the original
        /// fixing dates. Near-strike payoffs retain movements even when terminal spot
        /// rounds to strike. Only delta and gamma are returned.
    ]
);

sampling_settings!(@structured MonteCarloBarrierEngine, BarrierInstrument<'_>, with_barrier);

sampling_settings!(
    @class
    /// Flat-rate CPU Monte Carlo pricing for snowball and Phoenix notes.
    MonteCarloAutocallableEngine,
    [],
    [
        /// SnowballOption : Supported contract.
        /// BinarySnowballOption : Supported contract.
        /// TernarySnowballOption : Supported contract.
        /// PhoenixOption : Supported contract.
    ],
    [
        /// Simulation uses trading days when knock-in matters,
        /// or observation dates and expiry otherwise.
    ]
);

sampling_settings!(
    @structured MonteCarloAutocallableEngine,
    autocallable::AutocallableInstrument<'_>,
    autocallable::with_note
);

sampling_settings!(
    @class
    /// Flat-rate CPU Monte Carlo pricing for accumulators.
    MonteCarloAccumulatorEngine,
    [],
    [
        /// Accumulator : Supported contract.
    ],
    [
        /// Simulation uses the future trading dates. A current
        /// knock-out settles before any purchase.
    ]
);

sampling_settings!(
    @structured MonteCarloAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
    with_contract
);

engine_methods!(@evaluate MonteCarloVanillaEngine, binomial::VanillaInstrument<'_>, binomial::with_vanilla);

engine_methods!(@evaluate MonteCarloDigitalEngine, digital::DigitalInstrument<'_>, digital::with_digital);

engine_methods!(@evaluate MonteCarloAsianEngine, asian::AsianInstrument<'_>, asian::with_asian);

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
