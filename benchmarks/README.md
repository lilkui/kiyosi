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

Python analytic vanilla `price_with_greeks` retains the GIL for its short native
formulas. On 2026-10-11, CPython 3.14 with Release MSVC 19.51 on a Ryzen 9 5900X,
20,000 selected-delta calls per thread took 7.212 ms before and 6.446 ms after on
one thread, and 45.088 ms before and 25.811 ms after on four threads (median of
five repetitions). Each repetition used one shared engine, European call and
context from the common market below. Numerical engines still release the GIL.

Analytic digital `price_with_greeks` also retains the GIL. On 2026-10-12,
CPython 3.13 with Release MSVC 19.51 on a Ryzen 7 3700X measured 80,000
selected-delta calls taking 56.67 ms before and 49.62 ms after with one worker,
and 111.08 ms before and 50.49 ms after with four workers (fixed total work,
median of five repetitions). Workers shared a cash-or-nothing call with payout
10 and the common market below. These short requests benefit from avoiding
GIL transitions; the result does not imply parallel native execution.

Event-free FD time grids construct their ordered nodes and canonical interval
widths directly. Release MSVC 19.51 on the same CPU measured 0.332 us versus
1.631 us through the equivalent event-merging path at 200 steps, and 314.138 us
versus 1,331.19 us at 100,000 steps. These are grid construction times, not full
pricing times: maturity 1.37, 100 constructions per sample, median of five samples.
Both paths produced exactly equal node and width vectors. Event-bearing grids
retain event merging and deduplication.

FD factor caching retains three exact step-width/parameter combinations so
calendar splits can reuse a matrix beyond the immediately previous slot. The
`accumulator/fd/calendar_daily/split_grid/price` and `calendar_weekdays/split_grid/price`
analytics cases use the default 200-by-200 grid, spot 100.25, and the common market
below. On 2026-10-12, Release MSVC 19.51 on a Ryzen 7 3700X measured median wall
time falling from 2.462 ms to 2.269 ms for all days and from 2.071 ms to 1.912 ms
for weekdays (five repetitions, 0.15 s minimum). Prices were exactly unchanged
across these and eight existing FD cases, including accumulator delta/gamma,
snowball and scheduled knock-in barriers. Constraint masks still invalidate
changed matrices; the extra factor storage is allocated only when needed.

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

Calendar-driven FD intervals compute widths from timestamp durations when both
endpoints are calendar events. This preserves event times and tiny intervals while
allowing equal day counts to reuse the same matrix factors. The analytics suite
includes `accumulator/fd/calendar_daily/price`. An isolated 366-day, 80-node,
all-days-calendar probe with one uniform time step measured 430 to 386 us for an
accumulator and 436 to 397 us for a snowball on the same machine. Price and spot
Greeks across all three schemes changed only at rounding scale (largest price
difference 6e-11). Grid tests cover intraday valuation, mixed uniform/event steps,
duplicate calendar dates and a one-microsecond final interval.

CPU European Monte Carlo Greek requests generate Gaussian samples once and cache
terminal growth factors across consecutive spot bumps. They retain the generic
scalar stencils, payoff summation, validation and seed behavior. At 100,000 paths
and seed 42, a one-year noon-valuation probe on the same machine measured delta/gamma
falling from 6.67 to 3.55 ms and all ten Greeks from 41.94 to 20.08 ms (median of
five batches). Scalar price stayed around 2.2 ms; prices and all Greek values were
bit-for-bit identical. The analytics suite includes `european/mc/delta_gamma` and
`european/mc/all`. C++ and Python checks compare scalar stencils across call/put,
odd/even path counts, seeds, custom shifts, life boundaries and concurrent calls.
The per-request cache uses about 12 bytes per requested path (1.2 MB at 100,000
paths); CUDA retains its existing execution path.

European Monte Carlo implied-volatility solves reuse the same per-request cache
across trial volatilities, retaining scalar summation and the generic solver's
validation, seeding and bracketing behavior. A 2026-10-11 one-year ATM call probe
with seed 42 measured 1.01 to 0.51 ms at 5,000 paths, 21.56 to 11.86 ms at 100,000
paths and 215.90 to 120.18 ms at 1,000,000 paths (median of five runs, same Release
MSVC compiler and CPU). Solved volatilities were bit-for-bit identical. The
analytics suite includes `european/mc/implied_volatility`; C++ and Python
regressions cover calls/puts, seeds, odd/even path counts, custom bounds, expiry,
errors and concurrent solves. The cache stays local to each call and uses the
same linear storage as European Greek requests.

CPU accumulator Monte Carlo evaluates spot bumps together, sharing each Gaussian
draw and growth factor while retaining separate accrual and knock-out states.
On 2026-10-11, a one-year, all-days-calendar, noon-valuation probe with spot 100.25,
strike 100, knock-out 115 and seed 42 measured delta/gamma falling from 329.6 to
142.6 ms at 20,000 paths and from 1659.0 to 713.8 ms at 100,000 paths (median of
five runs, same Release MSVC compiler and CPU). Scalar prices stayed at about
110 and 550 ms, respectively. Price and Greeks were bit-for-bit identical to
separate seeded scalar valuations. Additional storage is independent of path
count; scalar pricing and CUDA retain their existing simulation paths. The
analytics suite includes `accumulator/mc/price` and `accumulator/mc/delta_gamma`.
C++ and Python checks cover odd/even path counts, seeds, early knock-out,
different accrual branches, unavailable event-boundary Greeks and expiry;
C++ also checks failures, concurrent calls and unseeded settings.

CPU autocallable Monte Carlo spot Greeks likewise share draws and growth factors,
with independent coupon, knock-in and knock-out states for each spot bump. A
2026-10-11 one-year, all-days-calendar, noon-valuation snowball probe with seed 42
measured delta/gamma falling from 336 to 148 ms at 20,000 paths and from 1669 to
735 ms at 100,000 paths (median of five runs, same Release MSVC compiler and CPU).
Price and spot Greeks were bit-for-bit identical to separate seeded valuations.
Storage is independent of path count. The analytics suite includes
`snowball/mc/price` and `snowball/mc/delta_gamma`. C++ and Python regressions cover
all four note types, daily/expiry knock-in, different knock-out dates, odd/even
path counts and seeds; C++ also checks expiry, event boundaries, errors and
concurrent requests. CUDA retains its existing execution path.

Continuous up-barrier FD boundaries return the rebate directly instead of
computing a vanilla upper-boundary value that is immediately replaced. A
2026-10-11 Release MSVC comparison using spot 100.25, strike 100 and barrier 110
measured 114.6 to 104.9 us for an 80-by-200 grid, 265.0 to 252.7 us for 200-by-200,
and 553.4 to 510.4 us for 80-by-1000 (median of seven batches of 300 prices).
All 144 checked scenarios retained identical price, delta, gamma and speed
across three schemes, both option and barrier directions, knock-in/out,
continuous/scheduled monitoring, zero/nonzero rebates and valid rebate timings.
C++ and Python regressions retain put price and spot-Greek references for both
rebate timings, including the nonzero lower boundary.

CPU American Monte Carlo Greek requests reuse their path buffer and cache growth
factors across spot bumps while fitting each exercise regression independently.
A 2026-10-11 Release MSVC probe with a one-year ATM put, rate 5%, dividend yield
2%, volatility 20%, seed 42 and 50 steps measured delta/gamma falling from
430.8 to 315.6 ms at 100,000 paths and from 4333.5 to 3174.5 ms at 1,000,000
paths (median of three runs). Scalar pricing stayed near 145 and 1450 ms.
The cache adds 8 bytes per rounded-even path per time interval (39.2 MB at
100,000 paths and 50 steps), only for requests using spot stencils. All storage
is local to the request; exercise regressions remain independent. C++ and Python regression
checks retain exact seeded scalar stencils, including mixed Greeks, odd/even
path counts, different seeds, failures and concurrent calls. The analytics
suite includes `american/mc/price` and `american/mc/delta_gamma`.

Python scalar prices for the six short closed-form engines keep the GIL, except
for scheduled barrier contracts; numerical prices, Greek requests (except
analytic vanilla) and implied-value solves release it. A
2026-10-11 CPython 3.14 Release MSVC probe priced a one-year ATM European call
(rate 5%, dividend yield 2%, volatility 20%) 100,000 times through the public
`AnalyticVanillaEngine.price` binding. Median elapsed time over three batches
fell from 18.46 to 15.72 ms with one worker and from 42.27 to 16.18 ms with four
workers dividing the same total work. The unchanged scalar price was checked
in every batch. This avoids GIL handoffs costing more than the formula itself;
the package docstring lists the engines covered by this policy.

Scheduled `AnalyticBarrierEngine` and `AnalyticBinaryBarrierEngine` scalar prices
release the GIL while validating and pricing their observation schedules. A
2026-10-11 CPython 3.14 Release MSVC probe on a Ryzen 9 5900X priced a ten-year
down-and-out call with 2,609 weekday observations, spot/strike 100, barrier 80,
rate 4%, dividend yield 1% and volatility 20%. For 20,000 total calls, median
elapsed time over five batches changed from 746.7 to 741.5 ms with one worker
and from 736.8 to 306.3 ms with four workers. Prices remained identical.
Continuous scalar prices retain the GIL to avoid handoff overhead for their
short formulas. Run `python benchmarks/python_barrier_threads.py` with the
Release Python package installed to repeat both workloads; the script checks
every price. Python regressions cover shared engine, instrument and context
access, calendar errors and recovery for vanilla barriers, binary barriers and
touch options.

American CPU Monte Carlo caches growth only for feasible spot stencils. At
100,000 paths and 50 steps, requests for delta with a 200-unit spot shift or
vanna with a 0.3 volatility shift return an unavailable Greek. A 2026-10-11
Release MSVC allocation probe measured their total allocations falling from
80.0 MB to 40.8 MB, matching scalar pricing, with identical prices. At one
million paths this avoids 392 MB of unused growth storage. The analytics suite
includes `american/mc/unavailable_delta` and `american/mc/unavailable_vanna`;
C++ and Python regressions preserve prices and unavailable results for calls
and puts.

CUDA accumulator and autocallable spot stencils now share each path's random
draws and growth factors, retaining independent settlement states and the scalar
payoff reductions. On 2026-10-11, a one-year all-days-calendar noon-valuation
probe on an RTX 3090 with seed 42 and one million paths measured accumulator
delta/gamma falling from 234.6 to 102.2 ms and snowball from 226.1 to 91.4 ms
(median of five calls, same Release MSVC compiler and GPU). Scalar prices stayed
near 79 and 77 ms. Prices and checked Greeks were bit-for-bit identical to
separate seeded scalar valuations. C++ checks cover batches of one to five
spots, CUDA block boundaries, different settlement dates and failure recovery;
Python covers all four note types and both knock-in monitoring modes. Existing
checks retain mixed Greeks, extreme discounts and concurrent requests.
The analytics suite includes accumulator and snowball `mc_cuda/price` and
`mc_cuda/delta_gamma` cases at 250,000 paths. Payoff buffers use 24 bytes per
path for delta/gamma and up to 40 for speed, plus reduction and step buffers;
all storage remains local to the request.

Analytic vanilla delta/rho-only requests skip density and log-weight calculations
used by other Greeks. A 2026-10-11 Release MSVC probe through the public C++
`price_with_greeks` API, using a one-year ATM call, rate 5%, dividend yield 2%
and volatility 20%, measured delta falling from 145.1 to 111.0 ns and rho from
151.2 to 118.3 ns (median of seven batches of one million calls). C++ and Python
regressions preserve exact prices and selected values against full analytic
results for calls and puts, deep tails, tiny/large volatility and expiry;
unrequested Greeks remain unavailable. The analytics suite includes
`european/analytic/delta` and `european/analytic/rho`.

CPU terminal-only Monte Carlo batches 256 normal draws on the stack before
evaluating their antithetic payoffs, preserving the seeded draw and reduction
order. A 2026-10-11 Release MSVC probe on a Ryzen 9 5900X, with a one-year call,
spot 100.25, strike 100, rate 5%, dividend yield 2%, volatility 20% and seed 42,
measured one million paths falling from 23.86 to 19.60 ms (median of seven
calls). Call and put prices remained bit-for-bit identical through ten million
paths; C++ and Python Greek regressions cover partial batches and odd path
counts. `bm_monte_carlo_european_cpu` measures the million-path scalar case.

Finite-difference steps retain two matrix factorizations, including their
constraint masks, so repeated weekday and weekend widths avoid refactoring.
Entries are allocated lazily and failed solves invalidate the selected entry.
A 2026-10-11 Release MSVC probe on a Ryzen 9 5900X rolled two layers through
261 weekday-calendar intervals from 2025-01-06 to 2026-01-06. At 80 asset steps,
median rollback time fell from 283.1 to 260.3 us; at 200, from 703.4 to 666.8 us
(five batches of 100 rollbacks, one pinned CPU core). Both complete layers
remained identical. The second entry costs about 40 bytes per interior node.
`accumulator/fd/calendar_weekdays/price` covers the calendar pricing workload;
C++ and Python regressions retain constraint, failure and calendar results.

CUDA American Monte Carlo spot stencils now cache growth factors in the same
allocation as the path buffer, fitting every exercise regression independently.
Scalar pricing and unavailable spot stencils avoid this cache. It adds 8 bytes
per rounded-even path per time interval (392 MB at one million paths and 50
steps); all storage is request-local. A 2026-10-11 Release MSVC/CUDA probe on an
RTX 3090 under Windows WDDM, with a one-year ATM put, noon valuation, rate 5%,
dividend yield 2%, volatility 20%, seed 42 and 50 steps, measured full Greek
requests falling from 63.94 to 52.88 ms at 100,000 paths and from 428.24 to
372.23 ms at one million paths (median of five calls). Price and all ten Greeks
remained bit-for-bit identical. At one million paths, delta/gamma stayed near
77–79 ms: allocation costs absorb much of the simulation saving for that
smaller request. C++ and Python checks cover cache keys, CUDA block boundaries,
mixed Greeks, unavailable stencils, failures and concurrent calls. Analytics
cases include `american/mc_cuda/price`, `delta_gamma`, `speed`, `all` and
`unavailable_delta` at 100,000 paths.

Unavailable scalar speed requests check the complete four-point spot stencil
before evaluating bumps. American requests also omit the growth cache in this
case. `american/mc/unavailable_speed` and its CUDA counterpart use a 60-unit
shift at spot 100. A 2026-10-11 Release run at 100,000 paths and 50 steps measured
148.1 ms versus 147.9 ms for CPU scalar pricing, and 6.22 ms versus 6.27 ms on
CUDA (median of three benchmark repetitions). Native gamma-derived speed keeps
its smaller stencil; unavailable speed does not suppress feasible delta/gamma.

American Monte Carlo implied volatility retains path and cash-flow buffers for
one solve, and reuses its CUDA workspace. Each trial regenerates growth factors
and refits exercise regressions. A 2026-10-11 Release MSVC allocation probe for
a one-year ATM put, noon valuation, rate 5%, dividend yield 2%, volatility 20%,
100,000 paths, 50 steps and seed 42 reduced cumulative allocation from
1,264,802,418 to 40,800,078 bytes (31 large path matrices to one). The solved
volatility was bit-for-bit identical at 0.2000129745263024.
`american/mc/implied_volatility` tracks the solve workload; caches are never
shared between requests or stored in the public engine.

Effective-date charm and color requests retain their private native delta/gamma
dependencies until time differences are complete. The `european/{crr,fd}/effective_date`
analytics cases cover this one-sided stencil. A 2026-10-12 Release MSVC probe
with a one-year ATM call, rate 4%, dividend yield 1% and volatility 30% measured
default FD charm falling from 2.40 to 0.97 ms and color from 2.89 to 0.95 ms;
1,000-step CRR charm fell from 2.71 to 1.07 ms and color from 3.26 to 1.08 ms
(median of seven batches of 100 requests). CRR color now matches the joint
color/gamma request at 0.00001897446, close to analytic 0.00001892253, rather
than the scalar-stencil value 0.00027356906. Returned fields remain selective.

CPU accumulator rho batches base/up/down rate paths with shared random draws
and independent drift, discount and knock-out state. The `accumulator/mc/rho`
analytics case tracks this workload. A 2026-10-12 Release MSVC probe with
20,000 paths, seed 42, one-year maturity, noon valuation, spot/strike 100,
knock-out 115, rate 4%, dividend yield 1% and volatility 20% measured 352.8 ms
before and 182.8 ms after (median of seven requests). Prices and rho remain
bit-for-bit equal to independent scalar valuations, including zero-carry fixed
positions whose base value settles immediately while rate bumps still simulate.

Unconstrained FD steps omit per-node constraint-mask comparisons and writes;
cached factors distinguish constrained and unconstrained solves. Barrier
observation mode is selected before marching, and scheduled steps use the
unconstrained path. The `european/fd/price` analytics case tracks the default
grid alongside existing accumulator, snowball and barrier cases. A 2026-10-12
Release MSVC probe over 200 time steps measured 200-node unconstrained solves
falling from 467.5 to 442.6 us and 2,000-node solves from 4,646.7 to 4,228.2 us
(median of seven batches of 100 solves), with bit-for-bit identical prices.
Seven-repetition benchmark medians measured accumulator FD at 1.846 versus
1.729 ms, snowball FD at 1.885 versus 1.777 ms, scheduled knock-in at 636 versus
624 us, and continuous barrier at 494 versus 493 us. Their prices were unchanged;
tests also check transitions between pinned, unconstrained and paired layers.

Scheduled analytic barrier, binary barrier and touch Greeks validate the
calendar once per request, while each shifted price retains its other domain
checks. The `barrier/analytic/scheduled_dense` cases compare the request with
independent scalar stencils over 2,608 weekday observations in 2025–2035.
A 2026-10-11 Release MSVC run measured 38.3 us versus 454.1 us (median of five
repetitions), with identical prices and Greeks. Calendar queries fell from
33,904 to 2,608; subsequent calls validate again, including after a rejection.

CPU European Monte Carlo requests without feasible Greek stencils stream the
scalar price without retaining samples. The `european/mc/unavailable_delta`,
`unavailable_speed` and `unavailable_vanna` cases track these requests.
A 2026-10-11 Release MSVC allocation probe for a one-year ATM put, noon
valuation, rate 5%, dividend yield 2%, volatility 20%, one million paths and
seed 42 reduced unavailable-delta allocation from 12,000,078 bytes to zero.
Its price remained exactly 6.3236944868234461; both scalar and Greek requests
took about 20 ms. Requests with feasible stencils retain their shared samples.

CUDA accumulator and autocallable spot batches queue all payoff reductions and
host transfers before one readback wait. Error exits still wait for queued
accesses to caller-owned inputs and host results. A 2026-10-11 Release CUDA 13.4
probe on an RTX 3090 used a reused workspace, one simulation step, 250,000 paths,
seed 42 and five spots. Median wall time fell from 497–498 us to 475–478 us
(seven batches of 50 calls per run, two runs), with every price bit-for-bit
equal to its scalar valuation. These timings depend on hardware and workload.

Structured Monte Carlo delta/gamma requests keep their three-spot batch when
speed's five-spot stencil is unavailable. The `accumulator/mc` and `snowball/mc`
`delta_gamma_unavailable_speed` cases track this combination. A 2026-10-12
Release MSVC probe on a Ryzen 7 3700X used a one-year daily schedule, noon
valuation, spot 100, spot shift 60, knock-out level 1,000, 20,000 paths and
seed 42. Median wall time over seven runs fell from 614 to 324 ms for the
accumulator and 606 to 404 ms for the snowball, with identical prices and
available Greeks. Timing depends on path termination and workload.

Accumulator CPU spot-plus-rho requests reuse the completed spot batch's base
price; the rate batch then simulates only the two shifted trajectories. The
`accumulator/mc/delta_gamma_rho` case tracks this request. With the same
2026-10-12 daily-schedule probe and default spot/rate shifts, median time fell
from 697 to 576 ms. Regression checks compare all results against independent
seeded prices, including early knock-outs, unavailable spot stencils and a
fixed base position whose rate bumps still require simulation.

Autocallable Monte Carlo and FD Greek requests validate immutable contract
terms and the full observation calendar once per request. MC implied-volatility
trials reuse the solver's contract preflight. Each shifted valuation still
checks its time, history, settings and trading expiry; later requests validate
afresh. The `snowball/{mc,fd}/scheduled_dense` cases track price and vega/rho
on a ten-year schedule near expiry. A 2026-10-12 Release MSVC probe with 3,653
daily observations, valuation twelve hours before expiry, one MC path (seed 42)
or an 80-by-1 FD grid reduced calendar queries for vega/rho from 21,924 to
3,659 (MC) and 18,270 to 3,658 (FD). Median time over seven batches of 50
requests fell from 528 to 73 us and 486 to 130 us respectively. Independent
valuations, concurrent calls and calendar rejection/recovery checks preserve
prices, Greeks and error categories.
