# 07: Validate binary barrier contract variants

**What to build:** Binary barrier prices and smooth Greeks are independently
checked against QuantLib contracts with matching cash/asset payoff, strike
condition, barrier direction, and hit/expiry settlement. Unsupported combinations
remain visible with their existing reference provenance.

**Blocked by:** 02: Validate all ten Greeks and correct numerical scaling.

**Status:** resolved

- [x] Runtime-verify the pinned bindings' analytic binary barrier and American
  digital representations, including exercise and settlement restrictions.
  Select a mapping for each representable contract; use exact QuantLib-priced
  decompositions if needed and document their equivalence.
- [x] Replace every existing binary barrier pricing reference with a matching
  QuantLib reference where supported. Cover a compact matrix of cash/asset,
  call/put or absent strike condition where applicable, up/down, in/out,
  hit/expiry settlement, moneyness, and maturity.
- [x] Reconstruct all contract, market, observation, and settlement terms from
  fixture inputs and compare every generated row through the existing C++
  executable. Unknown or unhandled generated cases and measures fail.
- [x] Validate native promised measures and derived Greeks through the existing
  numerical-analytics wrapper at smooth points. Apply direct QuantLib Greeks
  or stable finite-difference fallbacks with documented shifts and units.
- [x] Validate prices at deliberate barrier or payoff boundaries and explicitly
  record unavailable sensitivities and reasons. Unexpected instability or
  non-finite references fail generation before replacing committed fixtures.
- [x] Preserve unmatched scheduled-monitoring and other unsupported variants
  with original provenance. Repeating the Kiyosi BGK adjustment does not count
  as independent validation of discrete monitoring. Retain constructor checks.
- [x] Record mapping, engines, settings, reference precision, and separate
  absolute price/Greek tolerances. Classify discrepancies, fix verified Kiyosi
  defects, and do not widen tolerances solely to obtain a pass.
- [x] Document supported and unsupported combinations, commit generated
  fixtures, confirm byte-identical frozen regeneration, and pass reference
  tests and the full C++ suite without invoking Python. Activate the Visual
  Studio Developer environment before CMake or MSVC on Windows.

## Implementation evidence

- All 30 existing continuous binary-barrier price rows now use QuantLib;
  268 generated rows cover the valid cash/asset, strike/no-strike, direction,
  knock-in/out and settlement combinations, including 100 price-only boundary
  rows and all ten Greeks at 140 smooth points.
- Runtime binding probes establish deferred American binary exercise,
  immediate/deferred American digital support, delayed-window restrictions,
  native Greek availability by mapping, and exact one-touch decompositions.
- The expanded strike/barrier-ordering matrix reproduced a verified asset
  up-and-in put defect (52.6017295715558 versus 32.9401813202996). Correcting
  the shared reflected term from a3 to a4 fixes price and derived Greeks.
  Nested-zomma roundoff was resolved with a 0.02 spot bump; comparison
  tolerances and reference stability ceilings were not widened.
- `tools/reference-data/GENERATION.md` records mappings, payoff boundaries,
  all shifts, units, precision, budgets, measured stability, and retained
  unsupported scheduled monitoring and constructor cases.

| Requirement | Evidence |
| --- | --- |
| Every migrated/generated contract and native/derived measures | `QuantLib binary barrier contracts validate prices and smooth Greeks` |
| Exact unavailable declarations | `Binary boundary declarations reject missing or invented sensitivities`; `QuantLib fixture parser rejects missing or invalid Greek declarations` |
| Terminal strict strikes and inclusive hits retained | `Binary barrier expiry uses inclusive hits and strict strikes` plus generated terminal rows |
| Unsupported scheduled checks retained | `Scheduled binary barriers validate calendars and use the stored BGK interval` |
| Constructor checks retained | `Binary barriers expose observation intervals and reject invalid at-hit terms` |
| Frozen bytes, independent mappings, rejection before replacement | `uv run --frozen --project tools/reference-data python tools/reference-data/check_generation.py` passed |
| Focused comparisons and malformed declarations | `out/build/windows-debug/tests/kiyosi_tests.exe 'QuantLib binary barrier*,Binary boundary declarations*,QuantLib fixture parser*'` passed, 14,248 assertions |
| Full C++ suite, without Python invocation | `ctest --preset windows-debug --output-on-failure` passed all 69 tests after Visual Studio Developer environment activation |

## Review

### Standards

No documented-standard breaches or actionable complexity findings, including
re-review of the native-Greek follow-up.

### Spec

The initial review identified available American-digital and European Greeks
being bypassed. The generator now uses them directly, checks price fallbacks,
and records the correct sources. Re-review found no remaining spec or
correctness findings.
