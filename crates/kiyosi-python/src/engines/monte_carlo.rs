use crate::conversions::{boolean, integer};
use crate::engines::{binomial, expiry_short_pricing};
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
///     If a different cap is supplied after pool initialization or the
///     calling process forked after the parent's first parallel request.
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
/// Use multiprocessing's spawn start method if the parent has already run
/// parallel Monte Carlo. Serial engines support pricing in a forked child.
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
        ///     Pricing with the shared sampler rejects more than one billion path
        ///     steps, measured as the path count times the number of draws per path.
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
        /// Parallel pricing raises UnsupportedOperationError in a child forked after
        /// the parent's first parallel request. Use spawn or max_active_workers=1.
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
    (@structured $engine:ident, $instrument:ty, $with:path $(, {$($extra:item)*})?) => {
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
            ] $(, {$($extra)*})?
        );
        engine_methods!(@evaluate $engine, $instrument, $with);
    };
    ($engine:ident, $instrument:ty, [$($step:ident)?], [$(#[$greeks_doc:meta])*] $(, {$($extra:item)*})?) => {
        engine_methods!(@price $engine, $instrument, [$(#[$greeks_doc])*], [
            /// OSError
            ///     If system entropy or worker creation fails.
            ///
        ], {
            $($($extra)*)?

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
            /// An existing seed is retained. Reuse the returned engine to replay its
            /// draws within the same build. The original is unchanged.
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
        /// Large grids use checkpoints and regenerate path windows. The exercise
        /// workspace is capped at 1 GiB per request. Each rollback allows one
        /// billion simulated path steps and exercise updates combined, counting
        /// replayed steps and the exercise updates of each sensitivity scenario.
        /// Concurrent requests own separate workspaces. A failed workspace
        /// reservation raises ComputationFailureError if the allocator reports it.
    ]
);

sampling_settings!(
    MonteCarloVanillaEngine, binomial::VanillaInstrument<'_>, [step_count],
    [///
    ],
    {
        /// Fit an American exercise rule and price it on separate paths.
        ///
        /// Parameters
        /// ----------
        /// instrument : AmericanOption
        ///     American call or put.
        /// context : PricingContext
        ///     Flat-rate calendar-time Black–Scholes–Merton snapshot.
        /// training_path_count : int
        ///     Number of paths used to fit the exercise rule. Sobol requires a
        ///     power of two. The engine's path_count controls valuation paths.
        ///
        /// Returns
        /// -------
        /// float
        ///     The estimated value of the fitted exercise strategy.
        ///
        /// Notes
        /// -----
        /// The engine seed determines the separate training and valuation streams.
        /// Accuracy depends on both path counts and the number of exercise dates.
        /// Cancellation propagates without a partial price.
        #[pyo3(signature = (instrument, context, *, training_path_count))]
        #[expect(clippy::needless_pass_by_value, reason = "PyO3 owns the extracted instrument.")]
        fn price_american_out_of_sample(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<'_, crate::instruments::american::AmericanOption>,
            context: &PricingContext,
            #[pyo3(from_py_with = integer)] training_path_count: i32,
        ) -> PyResult<f64> {
            let option = &instrument.0;
            let context = &context.0;
            domain(crate::cancellation::run(py, || {
                self.0.price_american_out_of_sample(option, context, training_path_count)
            })?)
        }
    }
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
        /// fixing dates. Dense scheduled knock-ins can integrate the terminal
        /// payoff conditional on the first hit. Near-strike payoffs retain movements even when terminal spot
        /// rounds to strike. Only delta and gamma are returned.
        /// Scheduled transition and sampling workspace is capped at 1 GiB per request.
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
        /// Scheduled transition and sampling workspace is capped at 1 GiB per request.
    ]
);

sampling_settings!(
    @structured MonteCarloAutocallableEngine,
    autocallable::AutocallableInstrument<'_>,
    autocallable::with_note,
    {
    /// Price a batch of notes of one product type using one market snapshot.
    ///
    /// Parameters
    /// ----------
    /// instruments : iterable of SnowballOption, BinarySnowballOption, TernarySnowballOption, or PhoenixOption
    ///     Notes of the same concrete type. Compatible observation grids share
    ///     simulated paths within the batch workspace.
    /// context : PricingContext
    ///     Shared market snapshot and valuation instant.
    /// shared_grid : bool, optional
    ///     Share paths between notes with the same daily knock-in grid even
    ///     when cash-flow dates differ. Defaults to False. With True, the
    ///     seeded batch can differ from individually seeded scalar prices.
    ///
    /// Returns
    /// -------
    /// tuple of float
    ///     Prices in input order. All simulation in the batch uses one seed.
    ///     Configure a seed or call with_resolved_seed() to replay the prices.
    ///
    /// Raises
    /// ------
    /// UnsupportedOperationError
    ///     If the instruments have different types or the market is unsupported.
    /// KiyosiError
    ///     If scalar pricing validation or computation fails.
    /// OSError
    ///     If interrupted or worker creation or entropy retrieval fails.
    #[pyo3(signature = (instruments, context, *, shared_grid = false))]
    fn price_many<'py>(
        &self,
        py: Python<'py>,
        instruments: &Bound<'py, PyAny>,
        context: &PricingContext,
        #[pyo3(from_py_with = boolean)] shared_grid: bool,
    ) -> PyResult<Bound<'py, pyo3::types::PyTuple>> {
        use autocallable::AutocallableInstrument;
        let instruments = instruments.try_iter()?.enumerate().map(|(index, instrument)| {
            if index % 1024 == 0 {
                py.check_signals()?;
            }
            crate::conversions::instrument::<AutocallableInstrument<'py>>(&instrument?)
        }).collect::<PyResult<Vec<_>>>()?;
        macro_rules! price {
            ($variant:ident) => {{
                let notes = instruments
                    .iter()
                    .map(|instrument| {
                        if let AutocallableInstrument::$variant(note) = instrument {
                            Ok(&note.0)
                        } else {
                            Err(kiyosi::Error::UnsupportedOperation(
                                "Monte Carlo price_many requires one autocallable product type"
                                    .into(),
                            ))
                        }
                    })
                    .collect::<kiyosi::Result<Vec<_>>>();
                let notes = crate::error::domain(notes)?;
                crate::error::domain(crate::cancellation::run(py, || {
                    if shared_grid {
                        self.0.price_many_on_shared_grid(&notes, &context.0)
                    } else {
                        self.0.price_many(&notes, &context.0)
                    }
                })?)?
            }};
        }
        let prices = match instruments.first() {
            Some(AutocallableInstrument::Snowball(_)) => price!(Snowball),
            Some(AutocallableInstrument::Binary(_)) => price!(Binary),
            Some(AutocallableInstrument::Ternary(_)) => price!(Ternary),
            Some(AutocallableInstrument::Phoenix(_)) => price!(Phoenix),
            None => Vec::new(),
        };
        pyo3::types::PyTuple::new(py, prices)
    }
    }
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
        /// Simulation uses future observation dates and expiry. When an explicit
        /// knock-out schedule groups multiple fixed purchase dates, only knock-out
        /// dates and expiry require simulation. A current knock-out settles before
        /// any purchase.
        /// Scheduled transition and sampling workspace is capped at 1 GiB per request.
    ]
);

sampling_settings!(
    @structured MonteCarloAccumulatorEngine,
    PyRef<'_, accumulator::Accumulator>,
    with_contract
);

engine_methods!(@evaluate MonteCarloVanillaEngine, binomial::VanillaInstrument<'_>, binomial::with_vanilla, expiry_short_pricing);

engine_methods!(@evaluate MonteCarloDigitalEngine, digital::DigitalInstrument<'_>, digital::with_digital, expiry_short_pricing);

engine_methods!(@evaluate MonteCarloAsianEngine, asian::AsianInstrument<'_>, asian::with_asian);
