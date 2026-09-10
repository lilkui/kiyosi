# 06: Validate continuous barriers and rebate settlement

**What to build:** Continuous barrier options and their rebate settlement
variants are compared against independent QuantLib contract values through
Kiyosi's analytic and finite-difference engines. Scheduled monitoring is
identified honestly wherever an independent matching reference is unavailable.

**Blocked by:** 02: Validate all ten Greeks and correct numerical scaling.

**Status:** resolved

- [x] Verify actual pinned binding support for the required payoff, barrier,
  exercise, monitoring, and rebate settlement combinations. Prefer analytic
  QuantLib values; demonstrate numerical convergence where an analytic mapping
  is unavailable.
- [x] Use exact combinations of QuantLib-priced contracts when required to
  match rebate timing. Document the contractual decomposition and its pricing
  engines; do not substitute a superficially similar settlement convention.
- [x] Replace every representable existing pricing reference and expand with a
  compact matrix spanning call/put, moneyness, maturity, up/down, in/out, and
  applicable hit/expiry rebate settlement variants.
- [x] Reconstruct complete contract terms, market inputs, and Kiyosi engine
  settings from each fixture. Exercise every generated row through analytic
  or finite-difference C++ comparisons as applicable; unknown cases or measures
  fail explicitly.
- [x] Compare native promised measures and numerical-wrapper Greeks at smooth
  points, using stable reference bumps and the established units. Include
  appropriate already-hit or other boundary cases, validate prices, and
  explicitly explain unavailable Greeks without crossing barrier or expiry
  events in a purportedly smooth stencil.
- [x] Preserve scheduled-barrier references with their existing provenance
  where QuantLib cannot independently represent the observation schedule.
  Applying Kiyosi's BGK adjustment before QuantLib continuous pricing must not
  be labelled an independent discrete-monitoring reference.
- [x] Record reference engines, conventions, decomposition, refinement or bump
  settings, and separate per-measure absolute tolerances. Diagnose mismatches
  before changing code or data, fix verified Kiyosi defects, and justify
  approximation budgets independently of failures.
- [x] Document supported variants and retained exceptions; preserve constructor
  expectations and unrelated references. Commit fixtures and confirm
  byte-identical frozen regeneration and passing reference/full C++ suites
  without Python during tests. Activate the Visual Studio Developer environment
  before CMake or MSVC on Windows.

## Implementation evidence

| Requirement | Evidence |
| --- | --- |
| Verify actual pinned binding support for the required payoff, barrier, exercise, monitoring, and rebate settlement combinations. | `tools/reference-data/barrier.py::check_bindings` executes supported contracts, verifies native Greek absence and analytic engine payoff/exercise restrictions in QuantLib 1.41; `GENERATION.md` records the schedule limitation. |
| Use exact combinations of QuantLib-priced contracts when required to match rebate timing. | `barrier.py::price` uses zero-rebate KO plus a discount-engine bond minus the KI rebate spread for deferred hit payment. `check_bindings` independently checks both timings/directions against `AnalyticDigitalAmericanEngine` cash one-touches. |
| Replace every representable existing pricing reference and expand with a compact matrix spanning call/put, moneyness, maturity, up/down, in/out, and applicable hit/expiry rebate settlement variants. | 18 migrated continuous rows and 120 generated rows. `QuantLib continuous barrier portfolios validate prices and numerical Greeks` executes every row, including both engines and all six direction/in-out/timing combinations for call and put. |
| Reconstruct complete contract terms, market inputs, and Kiyosi engine settings from each fixture. | The same C++ test reconstructs contracts, contexts, grids and wrapper shifts, rejects unknown kinds/engines/measures, and executes all legacy refinement sequences. |
| Compare native promised measures and numerical-wrapper Greeks at smooth points, using stable reference bumps and the established units. | The same C++ test asserts native price-only output and checks all ten Greeks on 48 analytic and 12 FD smooth contracts; all already-hit and one-day rows validate prices. `QuantLib fixture parser rejects missing or invalid Greek declarations` now includes a barrier boundary row. |
| Preserve scheduled-barrier references with their existing provenance where QuantLib cannot independently represent the observation schedule. | Exact comparison with baseline `ab052d35fd13224bb070f31f48d723b54383cc6f` verified all original rows except the 18 continuous rows unchanged, including scheduled, binary, Asian and constructor rows. Frozen regeneration also checks retention. |
| Record reference engines, conventions, decomposition, refinement or bump settings, and separate per-measure absolute tolerances. | Fixture metadata and `tools/reference-data/GENERATION.md` document these. `Continuous barrier finite differences enforce the absorbing boundary during each solve` failed by 0.151 before the Dirichlet correction and passes the original 0.05 budget with decreasing errors at 800/1600/3200 grids afterward. |
| Document supported variants and retained exceptions; preserve constructor expectations and unrelated references. | `GENERATION.md` continuous-barrier section; unchanged constructor fixtures; `Barrier validation fixtures exercise public constructors` and existing scheduled tests pass. |

Validation:

- `uv run --frozen --project tools/reference-data python -u tools/reference-data/check_generation.py`
  completed with `Generation checks passed`, including two byte-identical
  regenerations, binding probes, poisoned legacy targets and failure atomicity.
- After activating Visual Studio Developer PowerShell, the focused barrier
  reference test passed all 120 matrix rows, 18 migrated rows, wrapper measures
  and legacy convergence checks. The absorbing-boundary regression and parser
  mutation tests also passed.
- `ctest --preset windows-debug --output-on-failure` passed **68/68 tests**
  (140.78 seconds), using committed fixtures without Python or QuantLib.
- Two-axis review against the baseline reported **Standards: 0 findings** and
  **Spec: 0 findings**.
