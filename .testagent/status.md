# Issue 03 verification

## Standards

No findings from the independent standards review.

## Spec

No findings from the independent spec review. Required generation, C++ validation,
and preservation checks completed after review.

## Test quality

Applied assertion-quality and test-gap-analysis to the changed reference consumer
and standalone generation check. Assertions include independent approximate values,
exact native availability, negative declaration checks, convergence ordering,
collection coverage, exception rejection, and byte-preservation side effects.
Neither changed test is assertion-free or presence-only. Repeated-byte comparisons
verify reproducibility; they do not substitute for independent pricing comparisons.

Public outcomes mapped: prices across five engines/call-put/moneyness/maturity;
native delta/gamma on tree/PDE; all wrapper Greeks on one-year ATM calls/puts;
three convergence sequences; MC repeatability retained in the existing seeded-path
suite; unknown dispatch and malformed Greek declarations fail. Generation corruption
checks reject unknown profiles/settings, fractional steps, NaN shifts, missing budgets,
non-finite outputs and unstable reference Greeks, preserving the previous fixture.
The new coverage guard failed before generation (one engine instead of six), then
passed with the full matrix. Existing factor-of-two Greek scaling errors would fail
the tight integral/analytic wrapper comparisons. Coarse tree/MC higher-Greek budgets
remain approximation checks, not precision guarantees for arbitrary scenarios.
No production mutations were executed; the behavioral-gap review is static.

## Requirement evidence

| Requirement | Evidence |
| --- | --- |
| Replace every matching pricing reference for these engines with QuantLib analytic European values. Generate a compact matrix spanning call/put, moneyness, and maturity; preserve any unmatched references and their provenance. | `QuantLib generated references validate all Greeks and boundary declarations`; baseline comparison proves only five legacy rows changed and 90 added. |
| Reconstruct contract, market, and Kiyosi engine settings from each generated case. Every generated row reaches an actual comparison through the existing Catch2 executable; unknown cases or measures fail explicitly. | `QuantLib generated references validate all Greeks and boundary declarations` reconstructs all 115 QuantLib-derived cases and rejects unknown engines/measures. |
| Compare native promised measures and exercise the numerical-analytics wrapper on representative smooth cases using the established Greek stability, units, and boundary policy. Unexpected unstable reference Greeks fail generation. | `QuantLib generated references validate all Greeks and boundary declarations` checks native contracts and 10 wrapper Greeks per engine; `check_generation` rejects unstable estimates. |
| Preserve convergence comparisons, replacing matching convergence targets with accurate QuantLib values. Do not tune QuantLib to reproduce coarse Kiyosi numerical settings. | `QuantLib generated references validate all Greeks and boundary declarations` runs all three 50/100/200 convergence sequences with QuantLib targets. |
| Record explicit absolute tolerances per measure and engine with an error rationale, separate from reference uncertainty. Monte Carlo cases declare deterministic seed, paths, steps, and separate comparison budgets; repeatable seeds do not imply identical random samples across libraries. | `numerical_engines.json` and `GENERATION.md`; `QuantLib generated references validate all Greeks and boundary declarations` cross-checks MC metadata and uses separate uncertainty. |
| Diagnose convention mismatches, reference errors, approximation errors, and production defects before changing expectations. Fix verified Kiyosi defects; do not widen budgets solely to make comparisons pass. | `GENERATION.md` records rounded legacy references and approximation rationale; no production defects found or budgets widened. |
| Update generator documentation and provenance with supported engines, settings, coverage, and tolerance rationale. Preserve retained structured products, constructor expectations, and other families' references. | `GENERATION.md`; baseline preservation verification confirms all unmatched original rows, including analytic Greeks, remain identical. |
| Commit fixtures, verify repeated frozen regeneration is byte-identical, and pass the reference tests and full C++ suite without Python during test execution. Activate the Visual Studio Developer environment before CMake or MSVC on Windows. | `check_generation` passed; `cmake --build --preset windows-debug` passed; `ctest --preset windows-debug` passed 63/63 without Python. |
