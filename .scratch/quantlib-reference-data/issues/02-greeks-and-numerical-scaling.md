# 02: Validate all ten Greeks and correct numerical scaling

**What to build:** European reference scenarios independently validate price
and all ten Greeks through the native analytic engine and the existing numerical
analytics wrapper. Smooth sensitivities have measured stability; boundary cases
still validate prices and explicitly explain unavailable sensitivities.

**Blocked by:** 01: Establish reproducible European analytic references.

**Status:** resolved

- [x] Generate delta, gamma, theta, vega, and rho directly from QuantLib where
  supplied. Use finite differences of QuantLib prices for missing first-order
  Greeks and gamma, establishing the fallback needed by later contract families.
- [x] Generate speed, charm, color, vanna, and zomma by central differences of
  QuantLib delta or gamma, using the price-derived equivalents when necessary.
  No expected Greek depends on Kiyosi output.
- [x] Express theta, charm, and color per calendar day; express vega, rho,
  vanna, and zomma per percentage-point change in volatility or rate as
  applicable. Record units and finite-difference shifts in provenance.
- [x] Advance valuation time with whole-day shifts while holding contractual
  dates, spot, and market parameters fixed. Update QuantLib curves consistently
  with valuation date. Smooth time stencils cannot cross expiry, exercise,
  fixing, or barrier events.
- [x] Compare multiple bump sizes to establish Greek stability. Give price and
  each Greek their own explicit absolute tolerance, separating reference
  uncertainty from Kiyosi numerical error budgets and documenting wider budgets.
- [x] Reject unexpected non-finite or unstable values before writing fixtures.
  Deliberate boundary or discontinuity cases record each unavailable measure
  and its reason; the C++ consumer checks these declarations rather than
  silently treating missing output as permission to skip a measure.
- [x] Exercise all ten Greeks on representative smooth European call/put
  scenarios spanning moneyness and maturity. Compare native promised measures
  and exercise the existing numerical-analytics wrapper for derived measures
  without expanding native engine result contracts for testing.
- [x] Correct the missing central-difference factor of two for numerical vega,
  rho, and zomma in their shared implementation. Preserve the already-correct
  vanna scaling and verify the changes through independent fixture comparisons
  that fail with the old scaling.
- [x] Preserve existing European implied-volatility checks when migrating their
  fixture consumers. Do not discard previous coverage to accommodate the new
  measure representation.
- [x] Diagnose other discrepancies before changing data, tolerances, or
  production code; fix verified Kiyosi defects exposed by these comparisons.
- [x] Document Greek availability, units, bump stability, boundary handling,
  and tolerance policy. Regeneration remains byte-identical in the pinned
  environment and refuses invalid output without replacing committed fixtures.
- [x] Commit regenerated fixtures and pass reference tests and the full C++
  suite without invoking Python; activate the Visual Studio Developer
  environment before CMake or MSVC on Windows.

## Implementation evidence

- `QuantLib generated references validate all Greeks and boundary declarations`
  compares all ten Greeks through native and numerical engines on 18 smooth
  call/put scenarios, plus price and seven Greeks on two one-day boundary cases.
  It also recovers implied volatility from all 20 independently generated prices.
- `QuantLib fixture parser rejects missing or invalid Greek declarations` checks
  missing/unknown exclusions, wrong units, non-finite/excessive uncertainty,
  and unknown measure declarations. Existing European implied-volatility tests
  remain intact, including `European reference fixtures compare price, every
  Greek, and implied volatility`.
- `check_generation.py` passes native/price-fallback checks, higher-order
  price-fallback comparisons, exact regeneration against committed bytes,
  retained rows, and atomic rejection of NaN/infinite/unstable Greeks.
- Reference stencils, unit conversions, stability ceilings, separate native and
  numerical error budgets, boundary policy, and measured approximation errors
  are documented in `tools/reference-data/GENERATION.md` and fixture provenance.
- Red/green C++ comparison reproduced doubled numerical vega/rho/zomma before
  the shared denominator fix; vanna remains unchanged. Additional independent
  implied-volatility recovery exposed endpoint stagnation, fixed with a bracket
  progress safeguard. Original implied-volatility failure tests also pass.
- After activating Visual Studio Developer PowerShell, the focused reference
  run passed and `ctest --preset windows-debug --output-on-failure` passed all
  63 tests (5.37 seconds). CMake/CTest did not invoke Python or QuantLib.
- Two-axis code review found no substantive standards/spec issue. Both minor
  review findings (duplicate malformed-case key and positional enum mapping)
  were corrected before the final build and full suite.
