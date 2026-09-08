# DerivaSharp feature-parity assessment

Assessment date: 2026-09-08  
Kiyosi revision: `6e1eac687a9a6a2b253696a6c7eb01672d222fd3`  
DerivaSharp reference revision: `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`

## Conclusion

Kiyosi has an implementation corresponding to every concrete CPU pricing
engine family advertised by the pinned DerivaSharp revision. Feature parity is
not yet demonstrated, however. Completion requires an executable
public-boundary reference comparison for every DerivaSharp-origin CPU family
and a separate variant matrix covering every caller-visible payoff,
settlement, state, and preset branch. Manifest membership or one nominal row
per family is not sufficient evidence.

GPU acceleration, source/API/inheritance compatibility, identical random
streams, DerivaSharp's public `Numerics` types, and Kiyosi-only engines and
instruments such as `CrrEngine` and Bermudan options are excluded from the
parity matrix. They remain independently testable Kiyosi features. Generic
risk measures, scenario grids, implied volatility, and implied coupon solving
remain in scope only where an engine family supports them; parity does not
require manufacturing unsupported analytics. Representation-only API
differences such as `NaN` versus a typed error are excluded.

## Remaining work

### P0: Replace nominal parity evidence

The current manifest cannot establish parity:

- `tools/generate_cpu_parity.ps1` verifies the pinned checkout and rewrites
  metadata, but it never builds or runs DerivaSharp and never extracts an
  expected result. Existing output numbers are therefore retained unchanged.
- Several rows do not reproduce their cited source case. For example,
  `asian-geometric` describes a one-year call with strike 100 and expected
  value 8.1, while the cited DerivaSharp test prices a 91-day put with strike
  85 and expects 4.6923. `asian-arithmetic` likewise differs from the cited
  180-day mid-life cases, whose expected values are 2.056893 and 1.378232.
- Structured finite-difference rows identify `Kiyosi-FD-reviewed` as their
  reference. The accumulator row permits an absolute error of 1500. These are
  convergence smoke checks, not DerivaSharp parity checks.
- Structured Monte Carlo tests check only repeatability and finiteness; they
  do not compare the result with the manifest output. Several other typed
  fixture paths only require `has_value()`.

Generate reference cases from the pinned DerivaSharp executables, store their
complete serialized inputs and outputs, and make the Kiyosi test construct and
price each case from those same inputs through its public boundary. Use fixed
reviewed direct-comparison tolerances for deterministic outputs, including
finite-difference outputs, and separate refinement checks for discretized
engines. CPU Monte Carlo cases must record seed, path, and step budgets, use
common random numbers for perturbed valuations, and apply reviewed statistical
tolerances justified by a confidence or standard-error budget.

Require one executable reference row per DerivaSharp-origin CPU engine family
and a separate variant matrix for every caller-visible branch. This includes
cash-or-nothing and asset-or-nothing digital paths in every engine that accepts
them, the complete barrier case matrix, structured touch states, and all
expressible structured presets. Named Kiyosi convenience factories are not
required. Retain property tests for barrier identities, terminal settlement,
state transitions, and refinement; they complement rather than replace direct
reference comparisons.

Primary sources:

- `tools/generate_cpu_parity.ps1`
- `tests/fixtures/cpu_parity.tsv`
- `tests/parity_fixture_tests.cpp`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.Tests/Asian/GeometricAverageAsianEngineTest.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.Tests/Asian/AsianOptionTestData.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.Tests/Autocallable/`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.MonteCarlo.Tests/`

### P0: Match explicit finite-difference failure behavior

Kiyosi silently increases the requested time-step count for explicit Euler in
the vanilla, digital, and vanilla-barrier engines. DerivaSharp rejects an
unstable explicit grid, and ADR 0010 records the same Kiyosi contract. The
structured engine already rejects instability.

Remove the hidden refinement from all four affected public paths: finite-
difference European, American, digital, and barrier pricing. Match the pinned
DerivaSharp stability inequality exactly, including its signed risk-free-rate
term, and add one focused rejection test per public path plus an accepted
boundary case covering negative-rate behavior. Then regenerate their
deterministic comparison fixtures using the actual caller-selected grid and
retain separate refinement tests.

Primary sources:

- `src/pricing/engines/finite_difference.cpp`
- `src/pricing/engines/digital_fd.cpp`
- `src/pricing/engines/barrier_fd.cpp`
- `src/pricing/engines/structured.cpp`
- `docs/adr/0010-cpu-parity-scope-and-conventions.md`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/BsmFiniteDifferenceEngine.cs`

### P0: Process valuation-date events once

Kiyosi currently treats only dates strictly after valuation as future events.
`src/pricing/engines/structured.cpp` starts simulated trading dates at
`valuation + 1 day`, skips observation dates less than or equal to valuation
in Monte Carlo, and excludes time-zero observations from finite-difference
event anchors. DerivaSharp includes the valuation date in its trading-day grid
and its autocallable engines process an observation on that date immediately.
Its snowball tests explicitly price a knock-out on the valuation date.

Process a valuation-date observation exactly once in structured Monte Carlo and
finite difference, including immediate knock-out, coupon, knock-in, and
accumulator quantity/barrier effects. Replace the current test that expects a
valuation-date event to be ignored with pinned snowball, binary/ternary,
Phoenix, and accumulator cases. Keep dates strictly before valuation excluded,
and process an expiry observation before terminal settlement. This decision
supersedes the removed historical parity spec that excluded dates equal to the
valuation date.

Primary sources:

- `src/pricing/engines/structured.cpp`
- `tests/remaining_tests.cpp`
- `tests/parity_fixture_tests.cpp`
- `E:/AppData/Repos/DerivaSharp/src/DerivaSharp.MonteCarlo/PricingEngines/TradingDayGridBuilder.cs`
- `E:/AppData/Repos/DerivaSharp/src/DerivaSharp.MonteCarlo/PricingEngines/Autocallable/McAutocallableEngine.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.Tests/Autocallable/FdSnowballEngineTest.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.MonteCarlo.Tests/Autocallable/McSnowballEngineTest.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.MonteCarlo.Tests/Accumulator/FdAccumulatorEngineTest.cs`

### P1: Complete structured-product transformations

The generic Kiyosi terms can represent DerivaSharp's structured variants, but
two caller-visible transformations are incomplete:

- Add validated immutable barrier-touch replacement for Phoenix, snowball,
  binary snowball, and ternary snowball instruments. DerivaSharp exposes
  `WithBarrierTouchStatus`; Kiyosi currently requires rebuilding the entire
  value manually.
- Add validated replacement of a snowball's complete knock-out coupon schedule
  and maturity coupon. Kiyosi's `with_coupon_rate(double)` changes only the
  maturity coupon, so its implied-coupon solver cannot preserve schedule
  offsets or optionally align the maturity coupon as DerivaSharp does.

Update implied coupon solving to support DerivaSharp's snowball semantics and
add oracle cases for both alignment modes. Phoenix's scalar coupon replacement
is already sufficient.

DerivaSharp also supplies eight snowball preset factories, three Phoenix
preset factories, and uniform binary/ternary snowball factories. Because API
compatibility is excluded, parity only requires tests proving that Kiyosi's
existing generic factories can express and price each preset. Add named
convenience factories only if those workflows are intended as Kiyosi API.

Primary sources:

- `include/kiyosi/instruments/structured.hpp`
- `include/kiyosi/pricing/analytics.hpp`
- `E:/AppData/Repos/DerivaSharp/src/Instruments/SnowballOption.cs`
- `E:/AppData/Repos/DerivaSharp/src/Instruments/PhoenixOption.cs`
- `E:/AppData/Repos/DerivaSharp/src/Instruments/BinarySnowballOption.cs`
- `E:/AppData/Repos/DerivaSharp/src/Instruments/TernarySnowballOption.cs`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/Autocallable/AutocallableEngineExtensions.cs`

### P1: Align scheduled-barrier state at valuation

The finite-difference barrier path has two state-boundary risks to close with
an oracle case: it short-circuits on the current spot before distinguishing
scheduled from continuous monitoring, and its scheduled event filtering drops
time-zero observations. The analytic path likewise treats a current crossing
as touched without checking observation-date membership. A scheduled barrier
may become touched only on a contractual observation date; a current spot
crossing is not automatically a historical hit on any other date. Apply this
domain rule to both engines while retaining BGK as the analytic approximation
between observations. Add valuation-date and non-observation-date cases for
both paths. This intentionally corrects a boundary weakness in the pinned
DerivaSharp analytic approximation rather than copying it.

Primary sources:

- `src/pricing/engines/barrier_fd.cpp`
- `src/pricing/engines/digital_barrier.cpp`
- `tests/validation_tests.cpp`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/Barrier/FdBarrierEngine.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.Tests/Barrier/FdBarrierEngineTest.cs`

### P1: Port and validate Bjerksund-Stensland 2002

Kiyosi's American approximation is a shorter single-boundary formula with a
1024-step binomial fallback when the two disagree materially. DerivaSharp
implements the Bjerksund-Stensland 2002 two-boundary formulation. The current
Kiyosi fixture covers only one call with a tolerance of 0.1 and does not
reproduce the cited DerivaSharp case, so it cannot show equivalent behavior
across dividends, rates, maturities, calls, and puts.

Because the public engine names Bjerksund-Stensland, implement the correct 2002
two-boundary algorithm directly and remove the algorithm-switching fallback.
Add call and put cases covering early-exercise regimes. Do not reproduce the
pinned DerivaSharp put transformation away from at-the-money inputs: it omits
the transformed strike and is treated as a reference defect. Document that
exception and validate puts against an independent correct reference.

Primary sources:

- `src/pricing/engines/integral.cpp`
- `tests/fixtures/cpu_parity.tsv`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/Vanilla/BjerksundStenslandAmericanEngine.cs`
- `E:/AppData/Repos/DerivaSharp/tests/DerivaSharp.Tests/Vanilla/BjerksundStenslandAmericanEngineTest.cs`

### P1: Validate shared analytics for every engine family

Kiyosi provides `NumericalAnalyticsEngine`, `scenario_grid`, generic implied
volatility, and implied coupon solving, but current tests mostly assert that a
result exists. DerivaSharp promises value, all named Greeks, value/delta/gamma
grids, and implied volatility for every BSM engine, plus implied coupons for
Phoenix and snowball engines. The implementations currently differ in bump
sizes, time-Greek direction and expiry handling, the speed stencil, and grid
semantics, so presence checks cannot demonstrate behavioral parity.

Adopt DerivaSharp's caller-visible numerical conventions for every supported
analytic combination, including bump sizes, time Greeks, speed, and
value/delta/gamma grids. Add representative cross-engine comparisons for the
supported risk measures, scenario grids, implied volatility, and implied
coupon behavior, including validation and expiry boundaries. Do not create
unsupported analytics merely to fill a matrix. Deterministic engines use fixed
reviewed tolerances; Monte Carlo differentiation uses common random numbers
and the fixture's statistical budget. This does not require copying
DerivaSharp's base-class API; the existing adapter remains the Kiyosi boundary.

Primary sources:

- `include/kiyosi/pricing/analytics.hpp`
- `tests/remaining_tests.cpp`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/PricingEngine.cs`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/BsmPricingEngine.cs`
- `E:/AppData/Repos/DerivaSharp/src/PricingEngines/Autocallable/AutocallableEngineExtensions.cs`

## Recommended closure order

1. Make the fixture generator execute the pinned DerivaSharp implementation,
   serialize complete case inputs and outputs, and expose every current
   mismatch.
2. Remove Kiyosi-only rows from parity closure and define the engine-family and
   caller-visible variant matrices.
3. Correct valuation-date event handling, scheduled-barrier state, and the
   exact explicit-grid rejection rule.
4. Implement correct Bjerksund-Stensland 2002 behavior and complete structured
   transformations.
5. Align supported analytics conventions and add direct deterministic and
   variance-controlled Monte Carlo comparisons.
6. Run the full CTest suite on Windows and Linux and remove provisional
   tolerances, self-references, and presence-only checks before declaring CPU
   feature parity complete.

No additional DerivaSharp-origin CPU pricing-engine family is currently
missing. Avoid adding a new abstraction layer or public low-level numeric
utilities for this work; the existing value types, engine classes, analytics
adapter, and TSV fixture format are sufficient.
