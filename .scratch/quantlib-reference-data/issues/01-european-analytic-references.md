# 01: Establish reproducible European analytic references

**What to build:** A developer can manually regenerate independent European
call/put price references in a pinned uv environment, then run the existing C++
tests against the committed fixtures without Python or QuantLib installed.
This first slice establishes the generation and consumption path with real
European scenarios; subsequent tickets extend it to Greeks and other families.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] Create the standalone uv generator project specified by the feature
  design. Pin an exact uv-managed Python version and commit dependency
  declarations and a frozen lockfile covering both `QuantLib-Python` and its
  resolved `QuantLib` distribution. Verify the required European bindings at
  runtime in that environment.
- [x] Declare scenario inputs separately from expected outputs. Generate prices
  from QuantLib's analytic European engine using Actual/365 Fixed and continuously
  compounded BSM rates and dividends. Never derive expected outputs from Kiyosi.
- [x] Include a compact call/put matrix spanning moneyness and maturity. Each
  generated case carries complete contract and market inputs, including fixed
  valuation and contractual dates.
- [x] Preserve the existing ten-column TSV format and fixture location. Identify
  QuantLib-owned cases explicitly and recompute their outputs from declared
  inputs on every regeneration. Existing cases awaiting migration remain
  distinguishable from generated cases.
- [x] Record resolved binding versions, reference engine, conventions, relevant
  numerical settings, reference classification, and explicit absolute price
  tolerances in provenance. Keep reference uncertainty separate from the
  tolerance allowed for the Kiyosi engine.
- [x] Reuse the existing parser and Catch2 executable to reconstruct every
  generated case from its inputs and perform an actual pricing comparison.
  Unknown or unhandled generated cases and measures fail explicitly; adding a
  generated row cannot silently bypass comparison.
- [x] Validate complete inputs, unique case identifiers, finite expected prices,
  and matching non-negative finite tolerances before replacing committed data.
  A failed generation leaves the previous fixture intact.
- [x] Fix ordering, numeric formatting, encoding, newlines, dates, and any seeds;
  omit wall-clock timestamps. A fresh managed environment regenerates with the
  frozen lockfile, and a second run is byte-identical in that environment.
  Cross-platform floating-point identity is not required.
- [x] Preserve existing checks and unmatched references with their original
  provenance, including structured products and constructor-validation
  expectations. Track family migrations and unsupported variants in the
  generator documentation so retained cases cannot be mistaken for independent
  QuantLib comparisons.
- [x] Document the repository-root manual regeneration command required by the
  spec, pinned tool versions, current supported variants, retained exceptions,
  and tolerance policy. Keep Python and QuantLib out of CMake and ordinary test
  execution.
- [x] Classify discrepancies as input/convention mismatch, reference error,
  expected approximation error, or Kiyosi defect before changing data or code.
  Fix verified defects exposed by this slice; do not widen tolerances just to
  obtain a pass.
- [x] Commit the generated fixtures and pass the reference tests and full
  existing C++ suite without invoking Python. On Windows, activate the Visual
  Studio Developer environment before using CMake or MSVC.

## Implementation evidence

- Added `tools/reference-data/` with uv 0.11.2, managed CPython 3.13.12,
  QuantLib-Python 1.18, QuantLib 1.41, and a frozen lockfile.
- Added 18 independent call/put prices; retained all 84 original rows and their
  provenance/checks. See `tools/reference-data/GENERATION.md` for the migration
  ledger, regeneration command, and tolerance policy.
- Fresh managed environment generation succeeded. `check_generation.py` passed
  repeatability, recomputation, preservation, invalid-input and output checks.
- `kiyosi_tests.exe '*reference*'`: 12 cases, 2037 assertions passed.
- After activating Visual Studio Developer PowerShell, `ctest --preset
  windows-debug`: all 62 tests passed without invoking Python.
- No pricing discrepancy was exposed; no production fix or tolerance widening.
