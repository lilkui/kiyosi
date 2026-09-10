# 03: Validate numerical European engines against analytic references

**What to build:** European binomial, CRR, integral, finite-difference, and Monte
Carlo engines are checked against accurate QuantLib values for the same contract,
using scenarios reconstructed from committed inputs and method-appropriate
error budgets.

**Blocked by:** 02: Validate all ten Greeks and correct numerical scaling.

**Status:** resolved

- [x] Replace every matching pricing reference for these engines with QuantLib
  analytic European values. Generate a compact matrix spanning call/put,
  moneyness, and maturity; preserve any unmatched references and their provenance.
- [x] Reconstruct contract, market, and Kiyosi engine settings from each
  generated case. Every generated row reaches an actual comparison through the
  existing Catch2 executable; unknown cases or measures fail explicitly.
- [x] Compare native promised measures and exercise the numerical-analytics
  wrapper on representative smooth cases using the established Greek stability,
  units, and boundary policy. Unexpected unstable reference Greeks fail generation.
- [x] Preserve convergence comparisons, replacing matching convergence targets
  with accurate QuantLib values. Do not tune QuantLib to reproduce coarse
  Kiyosi numerical settings.
- [x] Record explicit absolute tolerances per measure and engine with an error
  rationale, separate from reference uncertainty. Monte Carlo cases declare
  deterministic seed, paths, steps, and separate comparison budgets; repeatable
  seeds do not imply identical random samples across libraries.
- [x] Diagnose convention mismatches, reference errors, approximation errors,
  and production defects before changing expectations. Fix verified Kiyosi
  defects; do not widen budgets solely to make comparisons pass.
- [x] Update generator documentation and provenance with supported engines,
  settings, coverage, and tolerance rationale. Preserve retained structured
  products, constructor expectations, and other families' references.
- [x] Commit fixtures, verify repeated frozen regeneration is byte-identical,
  and pass the reference tests and full C++ suite without Python during test
  execution. Activate the Visual Studio Developer environment before CMake or
  MSVC on Windows.

## Completion evidence

Implemented 90 generated numerical-engine scenarios and migrated five matching
legacy prices plus three convergence targets. Native promised measures and all
ten wrapper Greeks on representative calls/puts pass independent QuantLib checks.
No production defects were observed and no existing budgets were widened.

- `cmake --build --preset windows-debug`: passed in Visual Studio Developer environment.
- `kiyosi_tests.exe "QuantLib generated*"`: passed (10508 assertions).
- `ctest --preset windows-debug`: 63/63 passed, without Python at test execution.
- Frozen `check_generation.py`: passed repeated byte identity, migration,
  preservation, invalid-profile and reference-stability checks.
- Baseline comparison: exactly five old rows changed, 90 added; all other rows identical.
- Standards and spec reviews: no findings. Details: `.testagent/status.md`.
