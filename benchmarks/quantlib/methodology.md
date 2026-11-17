# Benchmark methodology

Budgets, numerical profiles and Monte Carlo validation limits are explicit in
`cases/*.json`. Reference provenance and refinement results live in
`references/*.json` and `calibration/*.json`; measured errors and timing results
belong in those artifacts rather than this guide. Ordinary runs use committed
profiles and references without tuning or reference generation.

Both adapters use Actual/365 Fixed and continuously compounded rates. Numerical
methods, mesh domains, interval/node counts and random streams can differ;
each backend must pass the shared accuracy gates before latency is compared.

## Digitals and Asians

QuantLib uses `AnalyticEuropeanEngine` for cash/asset terminal digitals,
`AnalyticContinuousGeometricAveragePriceAsianEngine` for continuous geometric
averages and `ContinuousArithmeticAsianLevyEngine` for continuous arithmetic
averages. Its Turnbull–Wakeman engine uses discrete averaging.

Averaging starts at valuation with no history. The geometric instrument uses
the unseasoned constructor; an explicit start date selects an unsupported
seasoned path. The arithmetic instrument supplies inception and zero current
average. Arithmetic cases require `abs(r-q)`, `abs(r-q+sigma²)` and
`abs(2(r-q)+sigma²)` each at least `1e-6`. The comparison checks the shared
two-moment lognormal approximation, rather than error against the true
arithmetic Asian price. Independent Decimal formulas are refined at 50/80
digits; the budget is `1e-9` with `1e-12` reference uncertainty.

## Barriers and touches

| Contract | Native QuantLib instrument/engine | Settlement |
| --- | --- | --- |
| Single vanilla barrier | `BarrierOption` / `AnalyticBarrierEngine` | KO rebate at hit; KI rebate at expiry |
| Terminal cash/asset binary barrier | `BarrierOption` / `AnalyticBinaryBarrierEngine` | `AmericanExercise(..., payoffAtExpiry=true)` retains terminal strike condition |
| Single touch | `VanillaOption` / `AnalyticDigitalAmericanEngine` | Payoff strike equals barrier; exercise flag selects hit/expiry |
| Single no-touch | `VanillaOption` / `AnalyticDigitalAmericanKOEngine` | Expiry payment on survival |
| Double vanilla barrier | `DoubleBarrierOption` / `AnalyticDoubleBarrierEngine` | Zero rebate; strike inside barriers |
| Double cash touch/no-touch | `DoubleBarrierOption` / `AnalyticDoubleBarrierBinaryEngine` | European exercise; expiry payment |

Continuous asset payment at hit equals the barrier, so a native cash binary
paying that amount represents it. A terminal binary additionally retains its
terminal strike condition; it is distinct from an unconditional touch.

[generate_barrier_references.py](generate_barrier_references.py) integrates
absorbing Brownian log-price densities against cash, asset and vanilla payoffs;
at-hit payments use the first-passage Laplace transform. Decimal calculations
use 90/110 digits, 8/12 images per side for double barriers and a normal-tail
cutoff of 16 standard deviations. Refinement evidence supports the `1e-12`
scenario-specific reference uncertainty and `1e-9` analytic budget.
QuantLib's double-barrier series count is explicit in the profile; its double
binary engine has a fixed 99-term Hui series without a public convergence
control. Independent price gates remain necessary.

## Finite differences

BSM and supplied local-volatility cases cover European/American vanilla,
cash/asset digitals, single vanilla and terminal binary barriers, no-touch at
expiry and one-touch at hit. Native digital payoff types are used directly.
FD touches use a payoff strike equal to the barrier:

| Contract | Native barrier | Binary orientation | Rebate |
| --- | --- | --- | --- |
| Down no-touch | DownOut | Call | 0 |
| Up no-touch | UpOut | Put | 0 |
| Down one-touch at hit | DownOut | Put | Cash payout or barrier for asset |
| Up one-touch at hit | UpOut | Call | Cash payout or barrier for asset |

The opposite binary pays zero on survival; the rebate supplies at-hit payment.
Both adapters reject already touched barriers and unsupported settlements.
Kiyosi uses uniform asset intervals and an explicit upper boundary; QuantLib
uses automatic log-space meshing. Damping and sensitivities differ between
implementations and remain part of the measured price work.

BSM cases have budget `0.003`. European references reuse independent Decimal
formulas; the American put shares the Rust/QuantLib time, space and domain
refinement in [calibration/american.json](calibration/american.json), with `5e-5`
empirical uncertainty. QuantLib up asset no-touch and up cash one-touch need
more time steps to resolve terminal-boundary error; spatial refinement alone
does not fix it. Exact profiles and separate space/time/domain checks are
recorded in `cases/fd_bsm.json` and `calibration/fd_bsm.json`.

Local-volatility cases have budget `0.005`. The supplied surface interpolates
linearly in time and spot, clamps spot endpoints and requires date coverage.
QuantLib transposes the time-row input into `FixedLocalVolSurface` and enables
`localVol=true`. Its `mesher_vol` controls automatic mesh size only; no implied
volatility or Dupire conversion supplies the differential operator's volatility.

Local-volatility references use a separate QuantLib refinement driver, varying
time, space and mesher domain. The asset digital and asset terminal binary need
especially fine spatial grids; the latter uses implicit Euler references to
avoid terminal-jump oscillations. Uncertainty is twice the sum of observed
time/space/domain shifts plus `1e-10`, capped at 10% of the case budget.
These are empirical estimates. Rejected refinements are retained in
`calibration/local_vol_rejected_references.json`; asset digital/binary checks
also refine Rust grids and domains independently.

## Heston

Cases cover an analytic European call and FD European call/American put, with
budgets `1e-7` and `0.005`. These ordinary ATM cases are not a tail/stability
survey. Kiyosi's adaptive Fourier integration uses a deterministic variance
control variate; QuantLib uses Gatheral with adaptive Lobatto. Independent
Lobatto and exp-sinh refinements supply the European reference.

Kiyosi FD uses modified Craig–Sneyd ADI and Ikonen–Toivanen American splitting,
with explicit asset/variance bounds. QuantLib uses Hundsdorfer, native American
payoff projection and automatic domains. Reference probes record those domain
bounds; calibration varies Kiyosi bounds separately at fixed spacing.
American references combine joint-grid and independent-dimension extrapolation
evidence. Their empirical uncertainty must be at most 10% of the case budget.

## Monte Carlo

Both backends use one worker, pseudorandom sampling, no antithetic pairs and no
control variates. Seeds are nonzero 32-bit integers; equal seeds do not imply
equal paths. Validation uses 32 predetermined seeds `1009 + 97*i`, `i=0..31`,
and separate timing seed `1729`. Ensembles run before timing, including with
`--price-only`; limits are empirical finite-ensemble checks, not population
error bounds. Every fresh call regenerates its samples.

European/barrier cases require absolute error plus reference uncertainty at
most `0.10`, RMSE at most `0.05`, absolute signed bias at most `0.025` and
sample standard deviation at most `0.05`. References reuse independent Decimal
BSM and absorbing-density formulas. Path/time refinements accompany seed-level
results in `calibration/monte_carlo.json`.

Kiyosi uses PCG32/Box–Muller and conditional bridge survival; QuantLib uses MT /
inverse-normal draws and samples barrier crossings with a separate MT stream
fixed to seed 5. Both barrier protocols reject primary seed 5 to avoid stream
dependence. The initial seed-1-through-32 collision evidence is retained in
`calibration/monte_carlo_seed_collision.json`; the replacement seed formula
was fixed before observing its outputs. Path counts mean physical paths.

### American least-squares Monte Carlo

The American put uses the shared refined FD reference. Gates require absolute
error plus uncertainty at most `0.15`, RMSE and sample standard deviation at
most `0.075`, and absolute signed bias at most `0.05`.

Kiyosi fits in-the-money pricing paths with basis `1, S/K, (S/K)^2` and
lower-degree fallback. QuantLib uses independent calibration paths, quadratic
monomials plus its built-in payoff basis, and calibration seeds offset by
10000. Both refit on every price request. QuantLib's calibration, variance and
exercise-probability work stays timed. Path, exercise-grid and calibration-path
refinements are recorded in `calibration/american_monte_carlo.json`; path
standard error alone does not bound regression/exercise bias.

## Maintenance

Build companions with `run.py` before native reference generation or calibration.
Choose budgets and validation seeds before selecting profiles. For changed
scenarios, regenerate the relevant references, calibrate profiles, then run
accuracy gates and tests before interpreting timing. Scripts overwrite their
committed artifacts; review the resulting diffs. Refinement can take minutes.

| Cases | Reference generation | Calibration evidence |
| --- | --- | --- |
| `european.json` | `generate_european_references.py` | Decimal refinement in references |
| `digitals_asian.json` | `generate_digitals_asian_references.py` | Precision evidence in references; native evidence in `calibration/digitals_asian.json` and checks in `test_digitals_asian.py` |
| `barriers.json` | `generate_barrier_references.py` | `calibrate_barriers.py` |
| `numerical.json` | `calibrate_numerical.py` | Same script; also refreshes shared `calibration/american.json` |
| `fd_bsm.json` | `generate_fd_references.py` | `calibrate_fd.py` |
| `fd_local_vol.json` | `generate_local_vol_references.py` | `calibrate_local_vol.py` |
| `heston.json` | `generate_heston_references.py` | `calibrate_heston.py` writes `heston_calibration.json` |
| `monte_carlo.json` | `generate_mc_references.py` | `calibrate_monte_carlo.py` |
| `american_monte_carlo.json` | `calibrate_american_monte_carlo.py` reuses shared American FD evidence | Same script |

Run scripts from the repository root, for example
`python3 benchmarks/quantlib/generate_fd_references.py`. Refresh shared American
FD evidence before its dependent FD/CRR/LSM references. Heston generation also
writes `python/tests/quantlib_reference/heston_finite_difference.json`;
its partial probe output is `build/heston-refinements.jsonl`. Some offline
drivers call `quantlib-config` directly, so put the intended installation on
`PATH` as well as configuring the runtime linker.
