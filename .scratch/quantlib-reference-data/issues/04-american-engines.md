# 04: Validate American engines against converged references

**What to build:** Kiyosi's American pricing engines are compared with
convergence-verified QuantLib contract values, with explicit numerical,
approximation, and Monte Carlo budgets. Bermudan construction remains covered
while its missing Kiyosi pricing capability is made explicit.

**Blocked by:** 02: Validate all ten Greeks and correct numerical scaling.

**Status:** resolved

- [x] Verify American vanilla finite-difference capabilities in the pinned
  Python bindings and represent the same payoff, exercise window, dates, and
  market conventions as Kiyosi. Refine the QuantLib calculation to demonstrate
  reference precision and record the engine and refinement settings.
- [x] Replace matching references for binomial, finite-difference,
  Bjerksund-Stensland, and Monte Carlo American engines. Cover a compact
  call/put, moneyness, maturity, and early-exercise scenario matrix.
- [x] Compare Kiyosi's Bjerksund-Stensland 2002 approximation against converged
  contract values. Do not substitute QuantLib's Bjerksund-Stensland 1993
  approximation as an equivalent implementation reference.
- [x] Reconstruct every generated contract, market, and engine configuration
  from fixture inputs and perform actual C++ comparisons. Unknown generated
  cases or measures fail explicitly.
- [x] Compare native promised Greeks and use the numerical-analytics wrapper
  for derived sensitivities on smooth cases. Apply bump stability and whole-day
  time-shift rules; explicitly explain unavailable Greeks at exercise or expiry
  boundaries while continuing to validate prices.
- [x] Keep per-measure absolute Kiyosi error budgets separate from QuantLib
  refinement uncertainty. Record approximation rationale and deterministic Monte
  Carlo settings with separate statistical comparison tolerances.
- [x] Classify discrepancies before changing fixtures or production code and
  fix verified Kiyosi defects. No tolerance is widened solely to obtain a pass.
- [x] Retain Bermudan construction tests and existing unmatched provenance;
  document that Kiyosi has no Bermudan pricing engine. Do not advertise an old
  Bermudan-labelled price row or construction check as a pricing comparison.
- [x] Document verified bindings, covered exercise variants, refinement,
  tolerance policy, and retained exceptions. Commit generated fixtures, prove
  repeatable frozen regeneration, and pass reference tests and the full C++
  suite without invoking Python. Activate the Visual Studio Developer
  environment before CMake or MSVC on Windows.


## Completion evidence

Implemented 50 generated American rows across ten contracts and five engine
profiles (including the American CRR overload), and migrated four existing
American targets. Other existing rows, including historical Bermudan data,
are byte-identical to the pre-implementation fixture.

| Requirement | Evidence |
| --- | --- |
| Verify American vanilla finite-difference capabilities in the pinned Python bindings | QuantLib 1.41 runtime construction in `american.py`; three-grid refinement and per-measure stability in `reference`; settings documented in `GENERATION.md`. |
| Replace matching references for binomial, finite-difference, Bjerksund-Stensland, and Monte Carlo American engines | `QuantLib generated references validate all Greeks and boundary declarations` reconstructs all 50 new rows and four migrated rows, with native and numerical-wrapper comparisons. |
| Compare Kiyosi's Bjerksund-Stensland 2002 approximation against converged contract values | The same C++ test uses only FD-generated QuantLib targets for BS2002. No QuantLib BS1993 values are generated. |
| Reconstruct every generated contract, market, and engine configuration | The same C++ test dispatches exact instrument/engine pairs and fails unknown contracts, engines, variants and measures. Two retained American convergence sequences also execute against regenerated targets. |
| Compare native promised Greeks and use the numerical-analytics wrapper | The same C++ test compares native price/delta/gamma where promised and all ten wrapper Greeks on ATM call/put cases for every engine. |
| Apply bump stability and whole-day time-shift rules | `check_generation.py` validates refinement and bump errors; the C++ generated test checks explicit expiry/exercise-start exclusions. `American reference fixtures reject unknown measures and exercise boundary omissions` rejects missing or unknown declarations. |
| Keep per-measure absolute Kiyosi error budgets separate from QuantLib refinement uncertainty | Per-measure fields in `american.json` and the TSV, separate MC settings/price budget, and tables/rationale in `GENERATION.md`. |
| Classify discrepancies before changing fixtures or production code and fix verified Kiyosi defects | `American Monte Carlo includes immediate exercise in the exercise window` failed with 49.79607993 versus intrinsic 50 before the time-zero exercise fix, then passed. Sampling probes and non-monotonic reference refinement are documented; no tolerance was widened. |
| Retain Bermudan construction tests and existing unmatched provenance | Existing `Exercise-based options compose shared terms, payoff, and exercise` and API compile-time capability checks remain. Manifest inventory excludes the historical Bermudan row from pricing pairs; documentation explicitly states no Bermudan pricing engine exists. |
| Prove repeatable frozen regeneration | `uv run --project tools/reference-data --frozen python tools/reference-data/check_generation.py` exited 0: three regenerations matched committed bytes, retained data stayed unchanged, and corrupt/non-finite/unstable references were rejected atomically. |
| Pass reference tests and the full C++ suite without invoking Python | After Visual Studio Developer activation, build succeeded and `ctest --preset windows-debug --output-on-failure` passed 65/65 tests in 48.83 seconds. CMake/CTest only consumed committed TSV data. |

Standards review and spec review each found the same instrument/engine dispatch
mistake in the initial test extension. Shared typed dispatch corrected it, and
both independent focused rechecks reported no remaining findings. The full
C++ suite passed after the correction.
