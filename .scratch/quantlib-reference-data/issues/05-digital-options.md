# 05: Validate cash and asset digital options

**What to build:** European cash-or-nothing and asset-or-nothing digital
contracts have independent QuantLib references exercised by Kiyosi's applicable
analytic, integral, and finite-difference engines, including explicit treatment
of payoff discontinuities.

**Blocked by:** 02: Validate all ten Greeks and correct numerical scaling.

**Status:** resolved

- [x] Verify the pinned bindings support the required digital payoffs with the
  analytic European engine. Match cash amounts, strike conditions, call/put
  direction, expiry, and BSM conventions exactly.
- [x] Replace all matching digital pricing references and add a compact matrix
  covering both payoff kinds, call/put, moneyness, and maturity. Document and
  retain unsupported combinations with their previous provenance.
- [x] Reconstruct every generated case and applicable Kiyosi engine settings
  from fixture inputs. Exercise all generated rows through actual C++ pricing
  comparisons, failing unknown cases or unhandled measures.
- [x] Compare native promised Greeks and the existing numerical-analytics
  wrapper on smooth cases. Use direct QuantLib sensitivities where available
  and the established finite-difference fallback otherwise, checking multiple
  bump sizes and preserving per-day/per-percentage-point units.
- [x] Include deliberate boundary or discontinuity cases whose prices remain
  checked and whose unavailable Greeks have explicit reasons. Do not use time
  or spot stencils crossing a discontinuity as evidence of smooth sensitivity.
  Unexpected instability fails generation before fixture replacement.
- [x] Use separate explicit absolute price and Greek tolerances with numerical
  error rationales; keep reference uncertainty distinct from Kiyosi budgets.
  Classify discrepancies and fix verified Kiyosi defects rather than widening
  tolerances merely to pass.
- [x] Update provenance and documentation with verified mappings, supported
  variants, Greek availability, and retained exceptions. Preserve other
  families' references and constructor-validation expectations.
- [x] Commit generated fixtures, confirm byte-identical frozen regeneration,
  and pass reference tests and the full existing C++ suite without Python
  during tests. Activate the Visual Studio Developer environment before CMake
  or MSVC on Windows.


## Implementation evidence

- `digital.json` and `digital.py` define 40 independently valued contracts and
  120 generated rows. QuantLib-Python 1.18 / QuantLib 1.41 supplies both cash and
  asset payoff bindings with the analytic European engine. The matrix covers
  call/put, spot 80/100/120, 30/365/730-day maturity, plus four one-day ATM cases.
- `QuantLib generated references validate all Greeks and boundary declarations`
  reconstructs every generated row and four migrated digital rows, including
  all FD settings. It compares price and every native promised Greek; all ten
  wrapper Greeks are checked on 36 smooth contracts per analytic/integral
  engine and four representative FD contracts. Unknown contracts, engines,
  payoff kinds, variants and unhandled measures fail.
- `QuantLib fixture parser rejects missing or invalid Greek declarations`
  now exercises both cash and asset digital boundary rows. One-day cases
  declare unavailable theta/charm/color with exact expiry-stencil reasons;
  price and native delta/gamma remain checked, and no wrapper is run there.
- `Digital expiry settlement uses strict strikes without smooth Greeks`
  checks cash and asset calls/puts at spot 99/100/101 through all three engines,
  requiring exact settlement and absent Greeks. The same exact values are
  verified against QuantLib payoff bindings in `check_generation.py`.
- `check_generation.py` checks direct versus price-fallback higher Greeks,
  measured stability at multiple bumps, atomic rejection of non-finite and
  unstable digital Greeks, recomputation of old digital targets, retained rows,
  and byte-identical frozen regeneration. The existing finite-difference
  reference path preserves calendar-day and percentage-point Greek units.
- `tools/reference-data/GENERATION.md` records verified payoff/engine mappings,
  separate price/Greek budgets and reference uncertainties, all numerical
  settings, boundary handling, supported variants and unsupported
  American/Bermudan digital pricing. Constructor/capability tests are retained.
- The reference comparison first failed with 394 assertions. Integral
  quadrature incorrectly included a zero endpoint at the strike instead of
  the smooth branch's one-sided limit. FD sampled the terminal jump and
  reported Greeks at the lower grid node. The fixes use the smooth integrand,
  cell-average terminal payoffs, and interpolated node Greeks. The remaining
  one-day FD error was resolved with a finer grid, without widening Kiyosi
  budgets. The old digital FD put target 4.0 was stale; its independent value
  is 4.9955171365497844. The original 50/100/200 convergence sequence and 0.3
  final tolerance are preserved and pass.
- A direct fixture diff against starting commit `20ed887` confirms exactly
  four migrated rows (`cash-digital-analytic`, `asset-digital-analytic`,
  `digital-integral`, `digital-fd`) and 120 added rows. Every other original
  row is unchanged, including its provenance and validation expectations.
- Validation completed successfully:
  - Frozen `uv run --project tools/reference-data --frozen tools/reference-data/check_generation.py`.
  - Focused `kiyosi_tests.exe 'QuantLib*,Digital expiry*,Pricing reference manifest*' --reporter compact`:
    37,726 assertions in seven test cases.
  - Visual Studio Developer environment activated before build and CTest;
    `ctest --preset windows-debug --output-on-failure`: all 66 tests passed,
    72.92 seconds. C++ tests consume committed TSV data without Python.
- Independent standards and spec review of the implementation against `20ed887`
  found no substantive issues.
