# Native QuantLib comparison

On Linux, from the repository root:

```sh
python3 benchmarks/quantlib/run.py
```

The Python standard-library coordinator builds an optimized Cargo release example
and a separate optimized C++ executable, checks prices, then prints per-case
latency and writes `benchmarks/quantlib/report.json`. The complete default covers
59 scenarios, 51 required semantic cells and 20 eligible engines; see the
[coverage and exclusion inventory](coverage.md) for all 14 excluded engines and
unmatched subsets. Cargo-only library builds,
tests, docs, and packaging do not require QuantLib or Python.

Install Rust/Cargo, Python 3.12+, a C++17 compiler, Boost headers, and **QuantLib
C++ 1.44** first. The suite never downloads or builds QuantLib. The Python
QuantLib package alone does not supply this prerequisite. Use an optimized
installed QuantLib library: for a separately obtained 1.44 source release, a
normal installation uses `./configure CXXFLAGS='-O3 -DNDEBUG' --prefix=PREFIX`,
`make -jN`, and `make install`. Choose PREFIX and N for your machine. Do not
benchmark a debug or sanitizer QuantLib build against release Rust.

`quantlib-config --version` must report 1.44, and its `--cflags` / `--libs` must
locate that installation. For a nonstandard prefix:

```sh
QUANTLIB_CONFIG=/opt/quantlib-1.44/bin/quantlib-config \
LD_LIBRARY_PATH=/opt/quantlib-1.44/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} \
python3 benchmarks/quantlib/run.py
```

`--quantlib-config`, `--cxx` (`CXX`), and `--cxxflags` (`CXXFLAGS`) also control
build discovery. No pkg-config entry is required. Compiler/linker and loader
failures are reported with their diagnostics. The JSON records config and
header versions, the runtime loader's resolved shared-library paths, and actual
build commands. QuantLib provides no runtime library-version API; that field
and unavailable installed-library build flags are explicitly identified. The
runner resolves loader symlinks and rejects a version mismatch identifiable in
the real library filename. A filename is version evidence, not a runtime API
or proof of the installed library's optimization flags.
Ensure the loader path refers to the same optimized 1.44 installation.

Select stable case IDs with repeatable `--case`:

```sh
python3 benchmarks/quantlib/run.py --case european-bsm-atm-call --report /tmp/bsm.json
python3 benchmarks/quantlib/run.py --no-build --price-only
python3 -m unittest discover -s benchmarks/quantlib -p 'test_*.py'
```

`--no-build` uses the existing companions. `--price-only` checks prices without
collecting timing samples and suppresses ratios. It supports numerical profile
calibration without repeated timing runs. Each binary also accepts `--info` or
scalar `key=value` parameters; `iterations=1` is useful for profile refinements.
Without `--case`, a supplied `--cases` manifest must still cover every required
cell. Select the IDs explicitly for a fragment-only development run.
Unknown IDs, missing references/families, failed native calls, incompatible
versions, and failed accuracy checks make the run unsuccessful. Diagnostic
JSON retains results available before the failure. Ratios appear only when
both current prices meet the same error budget.

This measures **native price-request latency**, with contract, context, model,
and engine construction outside timing. Each Rust iteration invokes the public
engine `price` method with compiler barriers. Each QuantLib iteration invokes
`recalculate()` followed by `NPV()`, forcing fresh engine work. Validation and
allocations performed by these calls remain inside timing. QuantLib's analytic
European engine inherently computes additional Greeks; the ratio compares the
actual native APIs' work. Explicit Greek requests, Python binding overhead,
market updates, and parallel scaling are outside scope.

Each native process warms the price path with three calls before monotonic
batch timing. The runner separately adapts each backend's batch size to at least
20 ms by default (`--target-ms`), then records 15 pairs, alternating the first
backend. Backends run serially with one worker. The report retains every recorded
sample and adaptation batch, without overhead subtraction or outlier removal.
Median and interquartile range use per-call batch latencies, with inclusive
linearly interpolated quartiles. `Q/K` is QuantLib median divided by Kiyosi
median; values above one mean lower Kiyosi latency. The spread helps interpret
machine noise; small differences do not establish a statistically significant
winner. There is no aggregate score. Idle the machine for useful measurements.
The JSON's `wall_seconds` records the local runtime including any builds.

The committed European scenarios align valuation/expiry dates, Actual/365 Fixed
time, continuous rates, and flat BSM volatility. The absolute budget is `1e-9`
price units for each case. Independently evaluated Decimal BSM formulas at 50
and 80 digits agree to better than `1e-48`; references conservatively declare
`1e-13` uncertainty. Both libraries must satisfy
`abs(price - reference) + reference_uncertainty <= error_budget`. References,
provenance, precision refinement, and native calibration evidence are versioned.
Regeneration is an explicit offline step:

```sh
python3 benchmarks/quantlib/generate_european_references.py
```

Scenario and reference fragments under `cases/` and `references/` use schema
version 1 and unique IDs. The runner resolves shared `inputs` and explicit
per-backend `profiles` into scalar native parameters. No numerical work settings
are needed by the analytic baseline; these profiles are explicitly empty.
Additional engine families use separate native adapters and JSON fragments.
The normal run does no reference generation or profile search.

The `numerical.json` fragment covers European vanilla, cash digital and asset
digital quadrature, European and American vanilla CRR, and continuous single
barrier CRR. The representative digital inputs include moneyness away from the
integration grid's midpoint, exposing QuantLib's discontinuity error. Absolute
budgets are `1e-5` for vanilla quadrature, `0.02` for cash digital quadrature,
`0.2` for asset digital quadrature, `0.005` for vanilla CRR and `0.02` for barrier
CRR. Each budget applies to both libraries. The larger digital budgets accommodate
the fixed QuantLib integration rule; they do not describe Kiyosi's accuracy limit.

Kiyosi quadrature uses 512 Simpson panels with extent 8 standard deviations;
the recorded vanilla time count of 1 is ignored for European exercise. QuantLib
IntegralEngine uses its inherent 5,000 trapezoid intervals and exposes no
integration controls. Offline evidence varies Kiyosi panels through 2,048 and
extent independently through 12; both libraries are checked against independent
50/80-digit Decimal closed forms. The barrier reference uses the analytic
down-and-out reflection formula, rather than treating a tree-to-tree price match
as accuracy evidence.

Both vanilla CRR profiles use 512 steps; barrier profiles use 2,048. QuantLib's
barrier engine is explicitly instantiated with `DiscretizedBarrierOption` to
exclude Derman–Kani enhancement, and `maxTimeSteps == timeSteps` disables
Boyle–Lau adjustment. These are the actual tree counts. Refinements include
odd/even pairs from 255/256 through 4,095/4,096, and 8,191/8,192 for barriers.
Finite-step transition probabilities differ between libraries; the recorded
errors are measured separately against a common reference. Barrier convergence
oscillates with grid alignment, so a small consecutive-price difference is not
used as an error estimate.

The American ATM put has genuine early exercise value. Its reference is a
first-order time extrapolation of native QuantLib 1.44 Crank–Nicolson FD prices,
independently checked against Kiyosi Rannacher FD. Separate spatial and temporal
refinements and Rust domain checks are retained in `calibration/american.json`.
The extrapolated values differ by `2.46e-6`; the declared `5e-5` numerical
uncertainty conservatively covers observed time, space and domain changes.
QuantLib's FD mesher chooses its domain internally; its public engine has no
domain-bound control. This is numerical convergence evidence, not a rigorous
error bound.

Regenerate the numerical references and calibration outside measured batches
after building the native companions:

```sh
python3 benchmarks/quantlib/run.py --price-only --case european-bsm-atm-call
python3 benchmarks/quantlib/calibrate_numerical.py
python3 benchmarks/quantlib/run.py --no-build --case crr-bsm-american-put
```

The offline script also compiles the two independent FD reference probes. Normal
runs only read the committed references and profiles. American quadrature,
quadrature barriers and structured products, and CRR digitals remain excluded:
there is no matching stock QuantLib algorithm in the approved counterpart set.
QuantLib's fixed-point American quadrature and proxy portfolios are not used.

The [digital and continuous Asian cases](digitals_asian.md) document their native
counterparts, inception-averaging constraints, independent formulas, and live
QuantLib 1.44 calibration.

Analytic barrier and touch coverage, exact payment mappings, exclusions and
reference/series calibration are documented in [barriers.md](barriers.md).

[European and continuous-barrier Monte Carlo](monte_carlo.md) uses a live fixed
32-seed error/dispersion gate before timing. RNG identities, independent sampling
restrictions, physical path/sample counts, and path/time refinements are recorded
with the profiles. Reproduce offline evidence with
`python3 benchmarks/quantlib/calibrate_monte_carlo.py` after building the native
companions; select `--case mc-european-call` for focused
normal runs. QuantLib barrier primary seed 5 is rejected because it couples the
Gaussian and hardcoded auxiliary crossing RNG streams.

[American least-squares Monte Carlo](american_monte_carlo.md) uses the shared
refined American FD reference and the live 32-seed gate. Pricing paths, exercise
intervals, and QuantLib's additional calibration paths are refined outside
timing. Each timed price includes continuation fitting and simulation; QuantLib
recalibrates on each call. Reproduce evidence with
`python3 benchmarks/quantlib/calibrate_american_monte_carlo.py`; select
`--case mc-american-put` for a focused normal run.

The `fd_bsm.json` fragment covers all verified BSM finite-difference product
families, including American exercise and native cash/asset terminal and touch
payoffs. Its shared 0.003 accuracy budget, explicit grids, independent references,
mesh differences and offline calibration are documented in [fd_bsm.md](fd_bsm.md).

The [Heston cases](heston.md) cover analytic European and FD European/American
pricing, with independent Fourier and numerical refinement evidence, explicit
method/domain differences, and freshly generated QuantLib 1.44 provenance.

The `fd_local_vol.json` fragment extends those eleven FD cases to an actually
supplied nonconstant local-volatility surface. Shared interpolation, external
QuantLib local-volatility wiring, explicit native profiles, independent offline
reference refinements and the 0.005 budget are documented in
[fd_local_vol.md](fd_local_vol.md).


The JSON retains `coverage` selectors and registry support, all resolved inputs
and per-backend `profiles`, references with uncertainty and provenance, current
`accuracy` checks, MC `seed_ensemble` samples and empirical statistics, native
warmups, adaptive batches, 15 raw timing samples per backend, sample order,
medians/quartiles/IQR, status and ratio. Top-level fields record UTC, wall time,
source revision/dirty state, build commands and executable SHA-256 hashes,
toolchains, library metadata/linkage, CPU and OS. With `--no-build`, recorded
build metadata is used only if both executable paths and hashes still match;
otherwise its unavailability is explicit. Reports and local binaries are local
artifacts, not committed reference data.

Reference/profile regeneration is offline work, separate from normal runs.
The `generate_*_references.py` and `calibrate_*.py` scripts and each family document
explain their prerequisites and evidence files. Some reference generators require
additional numerical/Python oracle dependencies; the normal coordinator uses
only the standard library. Build once with a focused command above, then run the
specific family generator/calibrator. Preserve independent reference refinements,
uncertainty, all predetermined MC seeds and calibration/path/time refinements.
Declare the common price budget before choosing work profiles; recalibrate both
backends and rerun coverage/accuracy checks when scenarios or profiles change.
Do not regenerate references as part of collecting timing samples.

Settings deliberately differ when different work is needed to meet the same
budget. QuantLib analytic vanilla and FD engines inherently retrieve additional
sensitivities. FD domain/mesh choices, quadrature rules, finite-step CRR
probabilities, Heston integration/FD controls, MC barrier crossing estimators and
American regression/calibration behavior differ between public APIs; the linked
family documents specify these differences. LSM fit work and QuantLib calibration
paths remain inside every price call. The ratio measures these actual API
requests under calibrated budgets. One-worker, serial batches do not measure
parallel throughput. Linux is the only supported and validated platform.


A complete default run on the AMD Ryzen 9 5900X, Linux WSL2 x86_64 machine took
**505.31 seconds (8 min 25 s)** including builds, with installed native QuantLib
1.44 (`/usr/local/lib/libQuantLib.so.1.44.0`). All 59 scenarios and 51 required
cells passed their accuracy/precision gates, retaining 1,770 native timing
samples and 320 Monte Carlo validation-seed samples. The run used the default
20 ms target, 15 alternating pairs and one worker per backend, with other agent
native workloads idle. This is an observed runtime on one machine, not a time
limit or portable performance guarantee; WSL2 scheduling and ordinary machine
noise still apply.

The measured pricing source was clean revision `de6da7b9`. The full local raw
artifact is `/tmp/kiyosi-benchmark-full-report.json` (SHA-256
`b092e7a251af8d2ee0039783181c399e557cebf788c97df9d3e8c0516b74e929`), with the
complete table in `/tmp/kiyosi-benchmark-full-terminal.txt`. These local artifacts
are outside worktrees so worktree cleanup preserves them. Rerun the command to
create a report for your machine; no overall speed score is calculated.
