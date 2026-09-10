# 08: Validate geometric and arithmetic Asian options

**What to build:** Supported continuous geometric and arithmetic Asian
contracts have QuantLib-generated comparisons with clear averaging semantics.
Analytic geometric references and approximate Levy arithmetic references are
distinguished, and unsupported seasoned or deferred variants retain their
existing evidence.

**Blocked by:** 02: Validate all ten Greeks and correct numerical scaling.

**Status:** resolved

- [x] Runtime-verify continuous geometric analytic and continuous arithmetic
  Levy engines in the pinned bindings. Match averaging period, running average,
  effective/valuation/expiry dates, payoff, and market conventions exactly.
- [x] Replace matching geometric and arithmetic pricing references and add a
  compact call/put, moneyness, maturity, and relevant averaging-variant matrix.
  Include supported arithmetic running-average cases.
- [x] Label Levy reference values explicitly as approximate and document their
  accuracy limitation. Do not present analytic evaluation of an approximation
  as an exact or convergence-verified value for the arithmetic contract.
- [x] Verify rather than assume support for seasoned geometric or deferred
  averaging contracts. Retain unmatched Asian variants and their original
  provenance, documenting exactly which capabilities are missing.
- [x] Reconstruct every generated contract, market, averaging, and calendar
  convention from fixture inputs and execute actual C++ pricing comparisons.
  Unknown generated cases and unhandled measures fail explicitly.
- [x] Compare native promised sensitivities and exercise the existing numerical
  analytics wrapper for derived Greeks at smooth points. Apply the established
  finite-difference fallback, units, bump stability, and contractual-date rules.
  Explicitly distinguish sensitivities of an approximate reference from those
  of an exact contract value.
- [x] Keep price checks at deliberate averaging or expiry boundaries and
  explicitly explain unavailable Greeks. Do not cross fixing or averaging
  events and call the result a smooth Greek; unexpected instability fails
  generation before replacing fixtures.
- [x] Set explicit absolute price and per-Greek budgets with approximation
  rationale separate from reference numerical uncertainty. Classify
  discrepancies and fix verified Kiyosi defects; no budget is enlarged just
  to make a failing result pass.
- [x] Update documentation and provenance with verified averaging variants,
  approximation limitations, settings, units, and retained exceptions.
  Preserve unrelated structured-product and constructor-validation references.
- [x] Commit generated fixtures, verify byte-identical frozen regeneration,
  and pass reference tests and the full C++ suite without Python during tests.
  Activate the Visual Studio Developer environment before CMake or MSVC on
  Windows.

## Implementation evidence

- `tools/reference-data/asian.py::check_bindings` executes both pinned engines,
  verifies native Greek availability, and checks rejection of past/present/future
  explicit geometric starts and deferred arithmetic starts.
- `Asian QuantLib references reconstruct averaging contracts and approximate Greeks`
  reconstructs 24 generated and four migrated cases, including the SSE calendar.
  Six seasoned arithmetic cases compare all ten wrapper Greeks. Both native
  Asian engines expose price only. Averaging-start and expiry cases check price
  without a time stencil crossing an averaging event.
- `Asian fixtures reject missing boundary declarations and unknown measures`
  checks the strict parser at both averaging-start and expiry boundaries.
  The comparison consumer rejects unknown engines, averaging types, option
  directions, monitoring conventions and calendars.
- `check_generation.py` checks frozen byte identity, source classifications,
  retained unsupported variants, migration independent of old expected prices,
  and atomic rejection of NaN, infinity and unstable Asian Greeks.
- `tools/reference-data/GENERATION.md` records exact averaging semantics,
  approximation limitations, separate stencil uncertainty and error budgets,
  unit conversions, unavailable Greeks and unsupported variants. Levy agreement
  does not establish accuracy against the true arithmetic contract distribution.
- Code review found an expiry provenance mismatch; terminal rows now identify
  `QuantLib.PlainVanillaPayoff`. A red C++ run rejected the old engine label.
  Standards review's optional scenario-order dependency was also removed.
  No production pricing defect or tolerance enlargement was needed.

- Final validation: VS Developer environment activated; `cmake --build --preset windows-debug`
  succeeded; `ctest --preset windows-debug` passed 71/71 tests (195.31 seconds).
  Frozen `generate.py` and `check_generation.py` passed, including byte-identical
  regeneration. All non-Asian fixture lines match the starting commit.
