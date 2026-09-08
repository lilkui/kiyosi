# DerivaSharp feature-parity remaining work

Assessment date: 2026-09-08  
Kiyosi revision: `14042ac`  
DerivaSharp reference: `F:\dev\Repos\DerivaSharp` at revision
`08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`

## Scope

Parity means agreement at Kiyosi's public C++ boundary with the CPU behavior
implemented by the pinned DerivaSharp checkout. It does not require matching
source, inheritance, or language APIs. CUDA acceleration is deferred. Public
low-level `DerivaSharp.Numerics` helpers (for example `BrentSolver`,
`TridiagonalMatrix`, and quadrature/distribution types) are out of scope;
Kiyosi only needs the pricing behavior that consumes them. Kiyosi-only
features such as Bermudan options and `CrrEngine` are not parity blockers.

All DerivaSharp CPU engine families have a Kiyosi counterpart today: vanilla
analytic/integral/binomial/finite-difference/Monte Carlo, digital analytic/
integral/finite-difference, vanilla barriers analytic/finite-difference,
binary barriers, Asian geometric/arithmetic, and structured
Phoenix/snowball/binary-snowball/ternary-snowball/accumulator engines. The
remaining work is evidence and behavior alignment, not adding another engine
family.

## Remaining tasks

### P0 — Replace nominal parity fixtures with executable references

The current manifest and tests do not prove parity. The generator only checks
the pinned revision and rewrites metadata; it does not execute DerivaSharp or
extract outputs (`tools/generate_cpu_parity.ps1`). Several fixture rows do not
reproduce the cited DerivaSharp inputs (notably the geometric and arithmetic
Asian rows), and structured finite-difference rows use a Kiyosi-reviewed
reference. Structured Monte Carlo checks mostly test repeatability/finiteness
instead of comparing to a reference (`tests/fixtures/cpu_parity.tsv`,
`tests/parity_fixture_tests.cpp`).

Implement a reference-runner/exporter against the pinned DerivaSharp tests,
serialize complete inputs, outputs, provenance, and tolerances, and have the
Kiyosi tests construct each case through the public API. Require direct rows
for every CPU engine family and a separate variant matrix covering:

- cash/asset digital branches;
- the complete barrier direction, settlement, rebate, and monitoring matrix;
- structured touch states, expiry handling, and every expressible preset;
- deterministic finite-difference outputs using the requested grid; and
- seeded Monte Carlo outputs with path/step budgets and reviewed statistical
  tolerances (common random numbers for perturbation checks).

Keep property tests (identities, bounds, state transitions, convergence), but
do not treat them as substitutes for direct reference comparisons.

Sources: `tools/generate_cpu_parity.ps1`, `tests/fixtures/cpu_parity.tsv`,
`tests/parity_fixture_tests.cpp`, and the corresponding suites under
`F:\dev\Repos\DerivaSharp\tests`.

### P0 — Match explicit finite-difference failure semantics

Kiyosi silently raises the time-step count for explicit Euler in the vanilla,
digital, and vanilla-barrier paths (`src/pricing/engines/finite_difference.cpp`,
`src/pricing/engines/digital_fd.cpp`, `src/pricing/engines/barrier_fd.cpp`).
DerivaSharp rejects an unstable caller-selected grid in
`F:\dev\Repos\DerivaSharp\src\PricingEngines\BsmFiniteDifferenceEngine.cs`;
the structured path already rejects instability (`src/pricing/engines/structured.cpp`).

Remove hidden refinement from the three affected implementations, reproduce
DerivaSharp's stability inequality including negative-rate behavior, and add
rejection plus accepted-boundary tests for each public path. Regenerate
deterministic fixtures using the actual requested grid.

### P0 — Process valuation-date observations exactly once

Structured Monte Carlo starts trading dates at `valuation + 1 day` and skips
observation dates `<= valuation` (`src/pricing/engines/structured.cpp`,
`trading_dates` and the observation index loop). Structured finite difference
also builds future anchors without a time-zero event. DerivaSharp's trading-day
grid includes valuation date and its autocallable engines process an
observation there (`F:\dev\Repos\DerivaSharp\src\DerivaSharp.MonteCarlo\PricingEngines\TradingDayGridBuilder.cs`,
`F:\dev\Repos\DerivaSharp\src\DerivaSharp.MonteCarlo\PricingEngines\Autocallable\McAutocallableEngine.cs`).

Process a valuation-date observation once for coupon, knock-in, knock-out, and
accumulator quantity/barrier effects; retain exclusion of dates before
valuation and process expiry observations before terminal settlement. Replace
the current ignore-at-valuation expectation in `tests/remaining_tests.cpp`
with pinned Snowball, Binary/Ternary, Phoenix, and Accumulator cases.

### P1 — Complete structured instrument transformations

DerivaSharp exposes immutable `WithBarrierTouchStatus` for Phoenix, Snowball,
BinarySnowball, and TernarySnowball, plus complete Snowball coupon-schedule
replacement (`F:\dev\Repos\DerivaSharp\src\Instruments\PhoenixOption.cs`,
`F:\dev\Repos\DerivaSharp\src\Instruments\SnowballOption.cs`,
`F:\dev\Repos\DerivaSharp\src\Instruments\BinarySnowballOption.cs`, and
`F:\dev\Repos\DerivaSharp\src\Instruments\TernarySnowballOption.cs`).
Kiyosi currently has `with_coupon_rate(double)` only (and no touch-state
replacement) in `include/kiyosi/instruments/structured.hpp`.

Add validated immutable touch-state replacement for all four notes and a
replacement that updates a Snowball's full knock-out coupon schedule together
with maturity coupon. Update `implied_coupon` in
`include/kiyosi/pricing/analytics.hpp` to preserve schedule offsets and test
both DerivaSharp coupon-alignment modes. Named convenience factories are
optional because API compatibility is out of scope; prove existing generic
factories express each preset.

### P1 — Align scheduled-barrier valuation state

`src/pricing/engines/barrier_fd.cpp` short-circuits on the current spot before
distinguishing scheduled monitoring, and `src/pricing/engines/digital_barrier.cpp`
similarly treats a current crossing as touched. Scheduled barriers should only
record a hit on contractual observation dates; a crossing on a non-observation
valuation date is not historical state. Time-zero observations are also
currently filtered out. Add valuation-date and non-observation-date oracle
cases for analytic and finite-difference barriers, retaining the BGK
approximation between observations. This deliberately fixes a boundary issue
instead of copying DerivaSharp's approximation.

### P1 — Implement the named Bjerksund–Stensland 2002 approximation

`src/pricing/engines/integral.cpp` uses a shorter single-boundary approximation
and a binomial fallback when results disagree. DerivaSharp implements the
two-boundary 2002 formulation (`F:\dev\Repos\DerivaSharp\src\PricingEngines\Vanilla\BjerksundStenslandAmericanEngine.cs`).
Replace the algorithm-switching fallback with the 2002 formula and add call and
put cases across dividend/rate and early-exercise regimes. The existing
single fixture is insufficient (`tests/fixtures/cpu_parity.tsv`). Validate puts
against an independent correct reference rather than reproducing DerivaSharp's
known transformed-put defect away from ATM.

### P1 — Validate shared analytics conventions across engines

Kiyosi's adapter (`include/kiyosi/pricing/analytics.hpp`) supplies Greeks,
scenario grids, implied volatility, and implied coupons, but tests generally
assert only presence (`tests/remaining_tests.cpp`). DerivaSharp defines these
through `PricingEngine`/`BsmPricingEngine` and autocallable extensions
(`F:\dev\Repos\DerivaSharp\src\PricingEngines\PricingEngine.cs`,
`F:\dev\Repos\DerivaSharp\src\PricingEngines\BsmPricingEngine.cs`, and
`F:\dev\Repos\DerivaSharp\src\PricingEngines\Autocallable\AutocallableEngineExtensions.cs`).

Add cross-engine behavioral comparisons for supported measures and grids,
including validation and expiry boundaries. Align bump sizes, speed stencil,
time-Greek direction, and grid semantics with the documented DerivaSharp
conventions. Use deterministic tolerances for deterministic engines and common
random numbers plus statistical budgets for Monte Carlo. Do not manufacture
Greeks an engine does not support.

## Closure order

1. Generate executable DerivaSharp references and complete the variant matrix.
2. Remove hidden explicit-grid refinement and close valuation-date events.
3. Fix scheduled-barrier state and structured transformations.
4. Port and validate Bjerksund–Stensland 2002.
5. Align analytics conventions and statistical comparisons.
6. Run Windows/Linux CTest and remove provisional tolerances and presence-only
   assertions before declaring CPU parity complete.
