# Pricing benchmark matrix

Latest report: [Windows CPU results, 2026-09-26](results/2026-09-26-windows.md), with timings, price differences, environment details, and archived raw results.

`kiyosi_benchmarks` includes one fixed Black–Scholes–Merton scenario for every supported C++ instrument and pricing engine pairing. The `matrix/kiyosi/` and `matrix/quantlib/` names share a suffix when both libraries price the same contract. `matrix/kiyosi_only/` marks a pairing without a direct QuantLib C++ engine. The optional CUDA backend adds seven more `kiyosi_only` cases when Kiyosi is built with CUDA.

## Build and run

Install QuantLib C++ with its CMake package, then configure a Release build. CMake adds QuantLib comparisons automatically when it finds the package. QuantLib 1.43 matches the version used by the repository's reference tooling.

On Windows with vcpkg `QuantLib:x64-windows-static`, run these commands from the repository root in a Visual Studio Developer PowerShell. They build and run every registered CPU benchmark, including the QuantLib comparisons. CUDA benchmarks require a separate CUDA-enabled build.

```powershell
$env:VCPKG_ROOT = 'C:/path/to/vcpkg'
cmake --preset windows-release --fresh -DKIYOSI_BUILD_BENCHMARKS=ON -DBUILD_TESTING=OFF -DKIYOSI_BUILD_EXAMPLES=OFF -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded -DCMAKE_TOOLCHAIN_FILE="${env:VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake" -DVCPKG_TARGET_TRIPLET=x64-windows-static
cmake --build --preset windows-release --target kiyosi_benchmarks
./out/build/windows-release/benchmarks/kiyosi_benchmarks.exe --benchmark_repetitions=5 --benchmark_report_aggregates_only=true --benchmark_out=out/build/windows-release/benchmarks.json --benchmark_out_format=json
python benchmarks/compare.py out/build/windows-release/benchmarks.json
```

For a manually installed QuantLib package, replace the toolchain and triplet arguments with `-DQuantLib_DIR=C:/path/to/QuantLib/lib/cmake/QuantLib`. Add `--benchmark_filter=^matrix/` to the benchmark command for a matrix-only run.

On Linux, use `linux-release` and `./out/build/linux-release/benchmarks/kiyosi_benchmarks`, omitting the Windows-specific CMake options. Without a QuantLib C++ package, CMake builds the Kiyosi cases alone. `compare.py` accepts either aggregate or individual Google Benchmark JSON results and prints median times, `QuantLib / Kiyosi` ratios, and warmup price differences. A ratio above 1 means Kiyosi took less CPU time in a paired case.

## Scenario and interpretation

The `analytics/kiyosi/` cases measure accumulator and snowball finite-difference
delta/gamma requests beside price-only baselines using the same contracts and
default 200-by-200 grids. They value at noon on 2025-01-01 to avoid a current
observation discontinuity. A European Monte Carlo implied-volatility case uses
5,000 paths, seed 42, and the default solver settings; the input quote is prepared
before timing. The `volatility` counter records the solved value. Each timed
Greek request retains the complete result so its sensitivities cannot be optimized
away. Run these cases with `--benchmark_filter=^analytics/`; `compare.py` continues
to report only the pricing matrix.

European FD and MC implied-volatility cases use safeguarded Brent interpolation.
For the audit's at-the-money one-year call, the FD solver used 7 price calls
instead of 32; the 5,000-path MC solver used 10 instead of 32. Five-run median
wall times with Release MSVC 19.51 on a Ryzen 9 5900X fell from 13.64 ms to
2.97 ms and from 3.29 ms to 1.01 ms respectively, both recovering volatility
0.20. Bounds, price and
parameter tolerances, and per-solve Monte Carlo seed sharing are retained.

The snowball FD implied-coupon case prepares its quote before timing and records
the solved rate in the `coupon` counter. Built-in MC and FD coupon solvers verify
an interpolated candidate before falling back to bisection, retaining the price
tolerance when FD spline limiting makes the price curve nonlinear.
On 2026-10-11, Release MSVC 19.51 on a Ryzen 9 5900X measured median wall time
falling from 43.069 ms to 4.726 ms (three repetitions, 0.15 s minimum), with
the recovered coupon remaining 0.08.

The snowball MC implied-coupon case uses 20,000 paths and seed 42. The solver
prices both coupon bounds on shared asset paths on CPU and CUDA, then verifies
the interpolated coupon with a separate price. Bounds that require different
knock-in time grids retain separate simulations. On the same CPU, five-run
median timings for the daily-monitoring audit scenario fell from 238.95 ms to
160.47 ms, with the recovered coupon remaining 0.08.

- Common market: valuation on 2025-01-01, expiry on 2026-01-01, spot and vanilla strike 100, flat risk-free rate 4%, dividend yield 1%, volatility 20%, Actual/365 Fixed.
- Instruments and engines are constructed before timing. Each iteration calls Kiyosi `price()` or forces a QuantLib `recalculate()` followed by `NPV()`; plain repeated `NPV()` would report a cached lookup. The warmup price is included as a `price` counter for checking that paired cases value similar contracts.
- CPU cases report CPU time; CUDA cases report wall time, which includes GPU synchronization inside the pricing call.
- CRR trees use 128 steps. Finite difference cases use 80 time and 80 asset steps. Vanilla Monte Carlo uses 5,000 Kiyosi paths and 2,500 antithetic QuantLib sample pairs, with 10 European or 20 American steps and seed 42. Autocallable Monte Carlo uses 2,000 paths and seed 42.
- The 23 QuantLib pairs cover European and American vanilla, cash and asset digital, vanilla and binary barriers, cash and asset one-touch, and continuous geometric and arithmetic Asian contracts. Ten accumulator and autocallable pairings have no direct QuantLib engine. Digital quadrature uses QuantLib `IntegralEngine`; one-touch uses the sum of two QuantLib binary barrier contracts.
- Results compare these configured implementations, not equal amounts of internal work. Kiyosi `price()` and QuantLib `NPV()` both request price only. The Bjerksund–Stensland engines use different published approximations (Kiyosi 2002, QuantLib 1993); finite difference grids, integration methods, and Monte Carlo regression also differ. Inspect price differences before interpreting speed ratios.

Run on an otherwise idle machine, use the same compiler optimization level for both libraries, and keep the QuantLib version and CPU/GPU details with published results.

## CUDA workload selection

Use the default CPU backend for small European Monte Carlo requests. CUDA setup
and synchronization can cost more than the single terminal-price simulation.
Select CUDA explicitly after measuring the intended path count and product; a
daily path or American regression has substantially more work per path.

On 2026-10-10, Release MSVC 19.51 on a Ryzen 9 5900X and RTX 3090 measured these
median wall times for the matrix scenarios (three repetitions, 0.1 s minimum):

| Monte Carlo scenario | CPU | CUDA |
| --- | ---: | ---: |
| European, 5,000 paths | 107 µs | 373 µs |
| American, 5,000 paths | 2,666 µs | 1,001 µs |
| Snowball, 2,000 paths | 10,643 µs | 1,542 µs |

CUDA path outputs share one allocation per pricing call, including reduction
scratch space, and return the mean and invalid-path flag in one host transfer.
This reduced the European CUDA case from 404 µs to 373 µs on this machine;
the remaining setup cost still makes CPU preferable for this small request.
The workspace belongs to the call, so concurrent pricing does not share mutable
buffers. These timings describe this hardware and workload, not a universal
backend crossover.

The analytics cases also include a fixed-cashflow snowball Monte Carlo price and
European FD vanna/zomma requests. The former measures validation and direct
settlement; the latter measure reuse of native delta/gamma at shifted volatility.
The protected snowball MC and FD cases retain stochastic knock-out cashflows but
need no daily knock-in simulation or FD transition layer because both terminal
states pay the same amount. Explicit Euler retains daily stability subdivisions.
On 2026-10-11, Release MSVC 19.51 on a Ryzen 9 5900X measured median wall time
falling from 1.590 ms to 0.460 ms for the protected FD case (three repetitions,
0.15 s minimum). The price changed by 3.7e-8 as redundant time anchors were removed.

European FD charm/color cases measure native delta/gamma differences at shifted
valuation times, using a noon valuation to allow a symmetric time stencil.

The expiry-only snowball FD case measures a single terminal knock-in condition;
no daily transition layer is needed.

The American MC no-dividend analytics case uses a call, a 5% rate, 100,000 paths,
50 steps and seed 42. Nonpositive dividends and nonnegative rates permit exact
terminal sampling with an immediate-exercise floor, eliminating the exercise
grid and regressions. On 2026-10-11, Release MSVC 19.51 on a Ryzen 9 5900X measured
wall time falling from 140.7 ms to 2.8 ms, avoiding a 40 MB path matrix. The seeded
price changes from 10.43075 to 10.45204 because terminal sampling consumes a
different random sequence; both are consistent with the analytic value 10.45058.
American settings validation still applies. Rho stencils crossing zero rates
retain exercise-grid sampling on both sides to avoid an estimator discontinuity.

Analytic barriers evaluate only the Reiner-Rubinstein terms selected by the
contract. On the same compiler and CPU, a million-call probe for a down-and-out
call (spot/strike 100, barrier 80, 4% rate, 1% dividend, 20% volatility, one year)
fell from 325 ns to 229 ns per price (median of five runs). All 48 checked
combinations of barrier type, call/put, strike and rebate retained identical
prices; in-out parity also checks both rebate timings and strikes at the barrier.

American CPU Monte Carlo accumulates one triangle of the symmetric regression
matrix and fills its three mirrored entries once per exercise date. With one
million paths, 50 steps and seed 42 (`bm_monte_carlo_american_cpu`), the same
machine measured 1,476 ms before and 1,436 ms after (median of five runs). The
put price stayed exactly 6.642721968042257; sparse/singular regressions, currency
scaling and odd path counts retain their existing regression checks.

Individual Monte Carlo payoffs use scaled compensated addition directly;
partial sums still use the general merge for CUDA block reduction. A European
call probe (100,000 paths, seed 42, spot/strike 100, 5% rate, 2% dividend,
20% volatility, one year) measured 2.293 ms before and 2.207 ms after on the same
machine (median of five batches of 100 prices). The price remained exactly
9.229877273858007. Tests compare accumulation with partial merging over extreme
scales and cancellation, and exercise CUDA reduction across block boundaries.

The fine-time-grid expiry-only case increases FD time steps from 200 to 1,000
to expose the cost of processing asset nodes on steps without observations.

CRR vanna/zomma/charm/color cases use 1,000 tree steps and a noon valuation.
Shifted native delta/gamma estimates avoid differencing the piecewise-linear
spot-price curve and reduce five or seven tree valuations to three.
On 2026-10-11, Release MSVC 19.51 on a Ryzen 9 5900X measured median wall time
falling from 2.185 to 1.311 ms for vanna, 3.069 to 1.314 ms for zomma,
2.191 to 1.312 ms for charm, and 3.067 to 1.316 ms for color (three repetitions,
0.15 s minimum). Prices were unchanged; C++ and Python checks compare mixed
Greeks against analytic values, including American calls without dividends.

CRR speed also differences shifted native gamma, using three tree valuations
instead of five price valuations; FD speed uses the same completion path. On the
same compiler and CPU, an isolated one-year ATM call probe measured 2.192 ms
before and 1.316 ms after (median of five batches of 50 requests). Default-shift
speed changed from 1.33e-8 to -0.000431050, versus analytic -0.000430993; the base
price was unchanged. C++ and Python checks cover European calls/puts and American
calls without dividends, with a separate check for the price-only fallback.

Analytic binary barriers and touches also evaluate only the formula terms needed
by their payoff branch. On the same machine and compiler, median wall times for
the cash/asset binary matrix cases fell from 166/157 ns to 121/110 ns, and
cash/asset touches from 174/166 ns to 119/113 ns (three repetitions, 0.15 s
minimum). Prices were unchanged; QuantLib references and C++/Python in-out
parity checks cover the formula branches and strikes at the barrier.

The fixed-quantity accumulator case keeps three accumulated units and no future
purchases. Its FD rollback advances only the accumulated-unit layer. On the same
machine and compiler, median wall time fell from 1.548 to 1.439 ms (three
repetitions, 0.15 s minimum), with an unchanged price. C++ and Python checks
preserve price/delta/gamma accrual linearity across all three FD schemes.

Uniform FD grids cache diffusion coefficients until model or scheme parameters
change, as stretched grids already do. On the same machine and compiler, median
wall times fell from 76.94 to 71.12 us for the 80-by-80 European matrix case and
from 2.266 to 2.155 ms for the fine-time-grid expiry-only snowball (three
repetitions, 0.15 s minimum). Prices were unchanged in all six measured FD cases;
the other four improved by 1.3% to 5.5%. An isolated 200-node rollback probe fell
from 2.241 to 2.042 us per step. Tests cover quadratic evolution, parameter/grid
changes, constraints, paired layers, and recovery after failed solves.

The accumulator `zero_carry` cases retain three units without future purchases
and set both rates to zero. The stopped asset is a martingale, so both engines
return the exact value 150 without simulation or rollback. C++ and Python checks
cover positive and negative values, inactive acceleration, spot Greeks, and
required validation; nonzero rates or future accrual retain numerical pricing.
On 2026-10-11, Release MSVC 19.51 on a Ryzen 9 5900X measured median wall
times of 36.8 ns for MC and 47.2 ns for FD (three repetitions, 0.15 s minimum).

The accumulator `inactive_accrual` case keeps three units with nonzero rates,
zero acceleration, and a knock-out below the strike. Surviving paths cannot buy
more units, so FD advances only the accumulated-unit layer. C++ and Python
checks compare price, delta, and gamma with an equivalent zero-daily-quantity
contract for all three schemes, with knock-out equal to or below the strike.

The European `exact_node/fine_space` case uses 10,000 asset steps and one time
step, with spot exactly on a grid node. Interpolation returns the stored value
before allocating or solving spline curvature; off-node interpolation is
unchanged. The audit's isolated same-machine comparison measured 468.6 us before
and 395.7 us after, with identical prices. Tests cover both endpoints, interior
nodes, extreme finite values, neighboring representable spots, and curved
off-node values; Python checks preserve prices at and around a node.

Scheduled knock-in barrier FD advances the vanilla and knock-out layers together,
sharing matrix factors between observations. The scheduled knock-in price case
fell from 0.971 to 0.535 ms on the same compiler and CPU (median of five
repetitions, 0.15 s minimum), with an unchanged price. C++ and Python checks
preserve price, delta, and gamma in-out parity across all three schemes, both
barrier directions, rebates, and expiry observations.

FD factor reuse also retains the factored diagonal in place and skips matrix
assembly after checking the full constraint mask. On the same machine, median
wall times fell from 70.35 to 67.62 us for the European matrix case, 0.432 to
0.406 ms for European speed, and 0.530 to 0.522 ms for the scheduled knock-in
case (five repetitions, 0.15 s minimum). Prices were unchanged. Tests exercise
time-step/model changes, explicit/implicit transitions, constraints at both ends
of the interior, repeated solves, paired layers, and recovery after failures.

Vanilla FD speed reuses spline curvature for the base and shifted price checks.
The `off_node/fine_space/speed` case uses 10,000 asset steps, one time step, and
spot 100.25 on a grid ending at 400. On the same machine, its median wall time
fell from 327.4 to 253.7 us (five repetitions, 0.15 s minimum); the default-grid
speed case stayed within 0.2%. Prices were unchanged. Tests cover repeated spline
lookups, exact nodes, bounded payoff interpolation, and European/American speed
with custom shifts in C++ and Python.

Digital, barrier, accumulator, and autocallable FD speed requests also reuse the
solved layers, avoiding two additional grid solves. The analytics suite includes
speed cases for each family. On 2026-10-11, an isolated 200-by-200 cash-digital
probe (spot 100.25, strike 100, upper boundary 400) measured 1.201 ms before and
0.403 ms after on the same compiler and CPU, with identical price and speed.
C++ and Python checks cover all three schemes, custom shifts, automatic and
explicit domains, event boundaries, expiry, and shifted digital payoff bounds.

Continuous barrier FD relies on the stepper's constraints and edge values instead
of reapplying rebates to every asset node after each step. Scheduled observations
retain their post-step updates. An isolated 200-by-200 comparison on the same
machine measured 0.475 to 0.442 ms for a down-and-out call, with identical prices
across both barrier directions and knock states. A separate 72-scenario check
retained identical price, delta, gamma, and speed for continuous and scheduled
monitoring across all three schemes and both supported rebate timings.

Bjerksund-Stensland caches immutable bivariate quadrature geometry for its fixed
time-split ratio. On the same machine, the American matrix case fell from
3.675 to 3.561 us (median of five repetitions, 0.15 s minimum). All 432 checked
call/put scenarios retained identical prices across eight maturities, three
spots, three volatilities, and three rate/dividend pairs. C++ and Python retain
continuation-price references from one day to ten years.

Continuous barrier FD excludes fixed edge rows from matrix factorization and
back substitution while retaining the full grid for interpolation and validation.
The `continuous_up_and_out/price` analytics case uses spot/strike 100 and barrier
110. On 2026-10-11, Release MSVC 19.51 on a Ryzen 9 5900X measured median wall
time falling from 461 to 262 us (five batches of 500 prices). Price, delta, and
gamma were unchanged; off-node C++ and Python regressions cover price and spot
Greeks across all three schemes. Stepper checks cover changing fixed regions,
internal constraints, all-fixed and single-live-node systems, and failure recovery.
