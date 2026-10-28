# Rust integration acceptance

Ticket: [37 — integrate and switch to Rust](../.scratch/rust-rewrite/issues/37-integrate-and-switch-to-rust.md).
Checks performed on 2026-10-28, starting at `e75bef89a4f7cb9c49344ea774e40ad36dbee7b2`.

## CPU inventory

The inventory was checked against the retained public C++ engine headers under
`include/kiyosi/pricing/engines`, their implementations and tests, Python module
exports, and Rust's public engine implementations. Only CPU combinations are in
scope. Engine names below are public Python names; Rust exports the same engine
types and uses generic `PricingEngine<I>` implementations for supported contracts.

| Product | Supported engines | Rust / Python acceptance suites |
| --- | --- | --- |
| European vanilla | AnalyticVanilla, CoxRossRubinsteinVanilla, QuadratureVanilla, FiniteDifferenceVanilla, MonteCarloVanilla | `european`, `analytics`, `binomial`, `quadrature`, `finite_difference`, `monte_carlo` |
| American vanilla | BjerksundStenslandVanilla, CoxRossRubinsteinVanilla, FiniteDifferenceVanilla, MonteCarloVanilla | `american`, `binomial`, `finite_difference`, `american_monte_carlo` |
| Cash/asset digital | AnalyticDigital, CoxRossRubinsteinDigital, QuadratureDigital, FiniteDifferenceDigital, MonteCarloDigital | `digital`, `binomial`, `quadrature`, `finite_difference`, `digital_monte_carlo` |
| Single-barrier vanilla | AnalyticBarrier, CoxRossRubinsteinBarrier, QuadratureBarrier, FiniteDifferenceBarrier, MonteCarloBarrier | `barrier`, `barrier_binomial`, `barrier_quadrature`, `barrier_finite_difference`, `barrier_monte_carlo` |
| Cash/asset binary barrier; cash/asset single touch/no-touch | AnalyticBinaryBarrier, FiniteDifferenceBinaryBarrier | `binary_barrier`, `binary_barrier_finite_difference` |
| Double barrier | AnalyticDoubleBarrier, FiniteDifferenceDoubleBarrier, MonteCarloDoubleBarrier | `double_barrier`, `double_barrier_finite_difference`, `double_barrier_monte_carlo` |
| Cash double touch/no-touch | FiniteDifferenceDoubleTouch | `double_touch` |
| Continuous geometric Asian | AnalyticGeometricAveragePrice, MonteCarloAsian | `asian`, `asian_monte_carlo` |
| Continuous arithmetic Asian | TurnbullWakemanArithmeticAveragePrice, MonteCarloAsian | `asian`, `asian_monte_carlo` |
| Snowball, binary snowball, ternary snowball, Phoenix | QuadratureAutocallable, FiniteDifferenceAutocallable, MonteCarloAutocallable | `snowball`, `snowball_variants`, `phoenix`, `implied_coupon`, `autocallable_finite_difference`, `autocallable_monte_carlo` |
| Accumulator | QuadratureAccumulator, FiniteDifferenceAccumulator, MonteCarloAccumulator | `accumulator`, `accumulator_finite_difference`, `accumulator_monte_carlo` |
| Range accrual | AnalyticRangeAccrual | `range_accrual` |

Append `Engine` to each engine name. Rust suites are in `rust/core/tests/NAME.rs`;
Python suites are in `rust/python/tests/test_NAME.py`. Python also retains the
migrated FD event, mixed-Greek, Rannacher, scale, accuracy and regression suites.
The eight snowball presets are checked against explicit terms in
`snowball_variants`. No engine/product combination or numerical algorithm was added.

`SupportMatrixTests` in `test_support_matrix.py` supplements the numerical suites:
all 32 exported engines have an accepted product, 61 supported pairings across 22
contract fixtures have independently specified payoffs, expiry Greeks are
unavailable, and expiry implied volatility has category `UNSUPPORTED_OPERATION`.
All other product/engine pairs
are rejected by native Python conversion for price, Greeks and volatility solving.
Rust excludes those unrepresentable pairs by signatures. Representable unsupported
configurations, including node curves, monitoring and settlement restrictions,
are tested as structured domain errors in the respective product suites.

## Shared contracts and numerical evidence

| Contract | Evidence |
| --- | --- |
| Defaults and valid input ranges | Constructors/settings in each product suite; Python engine defaults derive from Rust core defaults |
| Validation timing and stable errors | Invalid settings, life/curve applicability, requests, histories and solve controls in the corresponding Rust/Python suites; `test_package` verifies category numbers against installed stubs |
| Representation boundary | Strict real/integer conversions, bool and fractional rejection, overflow, dates/time zones, iterables and choice strings across Python market, calendar, analytics and product tests |
| Event boundaries and historical state | Before/at/after expiry and observations, strict digital strike, inclusive barriers, prior versus current history, already-paid cashflows, snowball/Phoenix coupon/KI/KO ordering, daily accumulator purchases and range hits |
| Ownership and failure state | Owning results and read-only properties; validated replacements preserve original contracts, markets, curves and histories after success or failure |
| Concurrency | Shared/distinct-instance pricing and solves, GIL release where documented, request-local sampling, fixed-seed exact thread-count reproducibility, and Rust `Send + Sync` contract |
| Price/Greek/solve correctness | Live QuantLib 1.43 references, independent scalar/payoff portfolios, curve and extreme-scale regressions, spatial/time/domain and bump refinement, multi-seed MC/statistical budgets |
| Calendar and data | Exhaustive public SSE queries against committed Rust data, migrated generator tests, live `--check`, and bundled XSHG/Sobol/PCG/inverse-normal notices |

Reference dependencies are required rather than skipped. QuantLib `SavedSettings`
restores the global evaluation date; each suite is serial within its process.
Parallel interpreter runs use separate processes. Arithmetic Asian references
compare the same Levy approximation, not an exact arithmetic-average model.

The first Linux run exposed exact equality in the Rust and Python accumulator MC
gamma checks. The public implementation uses `(up-center)-(center-down)` and the
independent scalar check uses `up-2*center+down`; their Linux results differed by
`1.8e-15`. Both assertions now use an input-scaled machine-epsilon rounding bound.
Pricing code, numerical/statistical budgets and exact seed/thread comparisons
are unchanged. The focused regression passed on Windows and Linux.

## Executed environments and artifacts

Acceptance runs use Windows AMD64 and Ubuntu 26.04.1 x86-64 under WSL2, Rust 1.99.1,
maturin 1.15.0, and QuantLib 1.43. Direct maturin Linux builds produce ordinary
`manylinux_2_35_x86_64` wheels; root PEP 517 sdist rebuilds produce ordinary
`linux_x86_64` wheels. Windows wheels use `win_amd64`. No ABI3 or free-threaded
interpreter is used. The pre-switch installed candidate matrix passed all 444
Python tests on each of the ten platform/interpreter combinations before the
root packaging switch.

All ten ordinary wheels were rebuilt from the root sdist, installed into fresh
environments with the required test extra, imported and tested:

| CPython | Windows AMD64 | Ubuntu 26.04.1 x86-64 / WSL2 |
| --- | --- | --- |
| 3.11.17 | Build/install/import; 444 tests passed | Build/install/import; 444 tests passed |
| 3.12.14 | Build/install/import; 444 tests passed | Build/install/import; 444 tests passed |
| 3.13.15 | Build/install/import; 444 tests passed | Build/install/import; 444 tests passed |
| 3.14.7 | Build/install/import; 444 tests passed | Build/install/import; 444 tests passed |
| 3.15.1 | Build/install/import; 444 tests passed | Build/install/import; 444 tests passed |

Each wheel passed payload, version, Rust classifier, installed stubs/exports,
`py.typed` and license checks. Runtime requirements contain no test-only
dependencies. The sdist contains the workspace, core, binding, data, tests and
reference fixtures and builds without the legacy implementation.

The full 444-test suite also passed on both platforms' CPython 3.15.1 using only
the configured cibuildwheel test-source tree copied from the corrected sdist.
This checks the test resources separately from the development checkout. The
Windows test command was additionally executed through `cmd.exe` to confirm
that the double-quoted discovery glob runs tests rather than silently finding zero.
The required existing `benchmarks/sobol.py` helper is included for its statistics
correctness checks; its performance gates are not run.

The main commands were:

```sh
uv build --sdist --out-dir dist
uv build --wheel --python PATH_TO_INTERPRETER --out-dir wheels dist/kiyosi-0.13.0.tar.gz
uv pip install --python PATH_TO_INTERPRETER 'PATH_TO_WHEEL[test]'
PATH_TO_INTERPRETER -m unittest discover -s rust/python/tests -p "test_*.py"
```

These were repeated with isolated interpreters for every row/platform above.

Standalone core checks passed on Windows and Linux: build/check, all **389** Rust
tests including **74** doctests, warnings-denied Clippy and rustdoc, Cargo package
verification and the separate `tests/rust-consumer` workspace. Core documentation,
packaging and consumer checks use an invalid `PYO3_PYTHON` to disable discovery.
Linux additionally passed `cargo +1.85.0 check --locked` against the declared core
MSRV. That is a build check, not an MSRV full-suite result.

Ruff checks/formatting, Python `ty`, and `generate_sse_calendar.py --check` are part
of acceptance. `delvewheel show` and `auditwheel show` found only Python/system
dependencies, with no CUDA or oneTBB runtime. Third-party notices include the
compiled Rust runtime dependencies, including getrandom and cfg-if.

The modified workflows pass actionlint 1.7.12 with its stale `ubuntu-26.04` label
warning suppressed after checking the [official runner image inventory](https://github.com/actions/runner-images#available-images).
Cibuildwheel's resolved build identifiers contain exactly the five intended
CPython versions on each platform.
The release guard was exercised with a complete ten-wheel inventory, a missing
Windows wheel, an extra ABI3 wheel, and matching/mismatching Cargo release tags;
it accepted only the complete ordinary-wheel matrix and matching tag.

## Review

Standards: **0 unresolved findings**. The Windows discovery quoting defect was
fixed and its command was checked through the Windows shell.

Spec: **0 unresolved findings**. The quoting defect and missing Sobol helper in
sdist/test sources were fixed; copied-source tests passed afterward. Neither axis
found scope creep. Reviews used the staged change against the starting commit.

## Delivery and remaining limits

Root packaging uses maturin, the existing Rust/PyO3 crates and the `kiyosi` name.
Cargo supplies the version. The installed wheel contains typed Python modules
and one Rust extension. Domain defaults remain in Rust. The root test extra
retains required QuantLib 1.43; it is not a user runtime dependency.

CI builds an sdist, builds all five interpreter wheels from it on each platform,
and runs the complete Rust-backed suite against each installation. Linux release
CI uses the manylinux 2.28 image. Publication requires successful CI for the exact
main release commit, ten correctly versioned ordinary wheels and one sdist.

The GitHub-hosted workflow, manylinux 2.28 container build, and actual PyPI release
have not been executed locally. Local WSL Linux results do not claim a separate
bare-metal Linux run. The Windows Rust 1.85 toolchain and full MSRV suites were
not checked. No additional architectures, free-threaded Python or ABI3 are claimed.

Legacy C++/CUDA source, bindings and build files remain for ticket 38; they are
not part of the default runtime. The legacy C++ and legacy Python suites were
not run. Some migrated suites still reuse legacy-located fixtures and independent
reference helpers: ticket 38 must move those before deleting them. There is no
runtime implementation selector, compatibility layer, performance baseline,
speed report, regression gate, crate publication or package publication in this work.
