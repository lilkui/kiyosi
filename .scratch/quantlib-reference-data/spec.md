# QuantLib reference data

Status: Decisions agreed; implementation design ready for final confirmation

## Settled requirements

- Generate reference data with QuantLib's Python bindings.
- Run generation as a standalone process.
- Have the C++ test project validate against pre-generated data.
- Use `uv` to manage the Python interpreter and install `QuantLib-Python`.
- Commit pre-generated fixtures and regenerate them manually; ordinary builds
  and tests only consume the committed data.
- Compare prices and directly available Greeks; calculate higher-order Greeks
  with finite differences of QuantLib results.
- Cover all Kiyosi instruments supported by QuantLib.
- Replace existing reference data with QuantLib-generated data where possible.
  Retain existing cases that have no matching QuantLib reference.
- Compare approximate Kiyosi engines with accurate values for the same
  contract, using method-appropriate tolerances rather than matching coarse
  QuantLib numerical settings.
- Expand the existing scenarios with a compact matrix covering moneyness,
  maturity, call/put direction, and relevant contract variants.
- Diagnose and fix verified Kiyosi defects exposed by independent references.
- Require stable Greeks for smooth scenarios. Explicitly identify unavailable
  Greeks at boundaries or discontinuities while still validating prices.
  Use finite differences for missing first-order Greeks as well.
- Defer Bermudan pricing. Retain its construction tests and document the
  missing pricing capability.

## Repository findings

- Tests already consume a ten-column TSV fixture through
  `tests/support/reference_fixture.hpp`.
- The existing fixture contains 84 cases spanning pricing, convergence,
  simulation, and constructor validation.
- European analytic cases reconstruct their inputs from the fixture;
  most other families select named cases and hard-code inputs in the tests.
- Kiyosi reports time Greeks per calendar day and volatility/rate Greeks
  per percentage point; generated comparisons must match those units.
- PyPI's `QuantLib-Python` depends on the separate `QuantLib` distribution;
  reproducibility requires recording the resolved binding version too.
- The shared numerical-analytics implementation omits the central-difference
  factor of two for vega, rho, and zomma. The analytic European formulas use
  the correct scaling; vanna in the numerical implementation is also correct.
- QuantLib prices Bermudan options, but Kiyosi has no Bermudan pricing engine.
  `tests/api/header_tests.cpp` explicitly rejects that pricing capability.

## QuantLib coverage investigation

| Kiyosi family | Candidate QuantLib reference | Qualification |
| --- | --- | --- |
| European vanilla | Analytic European | Direct BSM mapping |
| American vanilla | Converged vanilla finite difference | QuantLib's Bjerksund approximation is 1993; Kiyosi's is 2002 |
| Bermudan vanilla | Vanilla finite difference or binomial | Kiyosi pricing capability is missing |
| European cash/asset digital | Analytic European with digital payoff | Direct mapping |
| Barrier | Analytic barrier | Standard continuous monitoring; some rebate timings need exact contract decomposition |
| Binary barrier | Analytic binary barrier or American digital | Engine and exercise representation depend on hit/expiry settlement and strike condition |
| Continuous geometric Asian | Analytic continuous geometric Asian | Current unseasoned cases match; seasoned averaging is not directly supported |
| Continuous arithmetic Asian | Continuous arithmetic Asian Levy | Running average supported; deferred averaging requires another reference |
| Accumulator, Phoenix, three snowball families | No matching built-in contract identified | Retain existing reference cases |

Scheduled barriers use a BGK barrier adjustment in Kiyosi. Applying the same
adjustment before calling a QuantLib continuous-barrier engine would validate
that approximation, not independently price the discrete-monitoring contract.

Exposure was checked against upstream sources; the chosen pinned wheel still
needs runtime verification. Sources:
[options](https://github.com/lballabio/QuantLib-SWIG/blob/master/SWIG/options.i),
[Asians](https://github.com/lballabio/QuantLib-SWIG/blob/master/SWIG/asianoptions.i),
[barriers](https://github.com/lballabio/QuantLib-SWIG/blob/master/SWIG/barrieroptions.i),
[binary exercise constraints](https://github.com/lballabio/QuantLib/blob/master/ql/pricingengines/barrier/analyticbinarybarrierengine.cpp),
[Bjerksund version](https://github.com/lballabio/QuantLib/blob/master/ql/pricingengines/vanilla/bjerksundstenslandengine.hpp).

## Implementation design

The following details make the accepted decisions concrete for final review.

### Coverage boundary

- Replace every existing pricing reference for which the pinned QuantLib
  bindings can represent the same payoff, exercise, averaging, monitoring,
  and settlement conventions. Exact combinations of QuantLib-priced contracts
  are acceptable when required to represent a payoff or rebate timing.
- Retain scheduled-barrier references where QuantLib cannot independently
  represent the observation schedule. Do not label a repeated Kiyosi BGK
  adjustment as an independent discrete-monitoring reference.
- Retain unmatched Asian variants, structured-product references, and Kiyosi
  constructor-validation expectations with their existing provenance.
- Verify actual Python binding capabilities during implementation. Document
  unsupported variants explicitly instead of silently omitting them.
- Prefer an exact analytic QuantLib value, otherwise a converged numerical
  value. A QuantLib approximation, such as the arithmetic Asian Levy engine,
  must be labeled as approximate and have an explicit accuracy limitation;
  it must not be presented as an exact or convergence-verified contract value.

### Standalone regeneration

- A standalone `tools/reference-data/` uv project owns the generator,
  Python version pin, dependency declarations, and lockfile.
- Pin an exact managed Python version and lock both `QuantLib-Python` and its
  resolved `QuantLib` dependency. Regeneration uses the frozen lockfile.
- Keep the existing TSV format and fixture location. Record the QuantLib
  version, pricing engine, conventions, numerical settings, and finite-
  difference bumps in generated provenance.
- Fix valuation dates, seeds where needed, ordering, numeric formatting,
  encoding, and newlines. Do not emit wall-clock timestamps.
- Regeneration depends on declared scenarios and QuantLib results, never on
  Kiyosi output. Retained non-QuantLib cases keep their distinct provenance.
- Define scenario inputs separately from computed expected outputs. The
  generator may preserve unmatched rows in the existing manifest, but must
  recompute all QuantLib-owned outputs from those declared inputs.
- Validate generated content before replacing committed fixture data.
- Provide a documented manual command from the repository root, using
  `uv run --project tools/reference-data --frozen` to run the generator.
- Byte-identical regeneration is required within the pinned environment.
  Cross-platform floating-point identity is not presumed.

### Greeks and tolerances

- Match Actual/365 Fixed and continuously compounded BSM rates/dividends.
  Hold contractual dates, spot, and market parameters fixed when advancing
  valuation time; update QuantLib curves consistently with valuation date.
- Use QuantLib's directly supplied first-order Greeks and gamma where
  available. Obtain speed/charm/color/vanna/zomma by central differences of
  QuantLib delta/gamma; derive missing delta/gamma from QuantLib prices.
- Convert annual time sensitivities to per-calendar-day units and volatility/
  rate derivatives to per-percentage-point units. Record finite-difference
  shifts and units in provenance.
- Compare multiple bump sizes to verify smooth-case Greek stability. Use
  whole-day time shifts with a smooth-point stencil; do not cross expiry,
  exercise, fixing, or barrier events and call the result a smooth Greek.
- Unexpected non-finite or unstable results fail generation before writing.
  Deliberate boundary cases explicitly record unavailable measures and the
  reason; absence must not become an unnoticed test skip.
- Use separate tolerances for price and each Greek. Establish numerical
  reference precision through refinement and bump stability. Set Kiyosi
  engine error budgets separately from reference-generation uncertainty.
- Preserve the existing explicit absolute-tolerance representation. Record
  the rationale for wider numerical or approximation tolerances. Do not
  enlarge them solely to make a failing Kiyosi result pass.
- Diagnose discrepancies as input/convention mismatch, reference numerical
  error, expected approximation error, or Kiyosi defect before changing data
  or production code.

### C++ validation

- Every generated row must be exercised by a C++ test reconstructed from its
  inputs. Reuse the existing parser and Catch2 executable.
- Compare the measures each native engine promises. Exercise the existing
  numerical-analytics wrapper for derived Greeks without expanding native
  engine result contracts merely for testing.
- Keep separate tolerances and deterministic settings for Kiyosi Monte Carlo
  comparisons; a seed ensures repeatability, not agreement between libraries.
- Keep Python and QuantLib out of CMake and ordinary C++ test execution.
- Correct the confirmed numerical vega/rho/zomma scaling defects at the
  shared implementation and verify them against the independent fixtures.
  Preserve the already-correct vanna scaling.

## Acceptance checks

1. A fresh uv-managed environment can regenerate fixtures with the frozen
   lockfile; a second run produces identical output in that environment.
2. Every generated case has complete inputs, honest provenance, explicit
   tolerances, and either finite expected measures or explained boundary
   exceptions. Unmatched retained cases remain identifiable.
3. Each generated case reaches an actual pricing comparison in the C++ test
   executable. An unknown or unhandled generated case fails the test.
4. Representative smooth cases validate all ten Greeks, including the shared
   numerical-analytics vega/rho/zomma corrections. Boundary cases validate
   prices without asserting nonexistent derivatives.
5. Reference tests and the full existing C++ suite pass against committed
   fixtures without invoking Python. On Windows, activate the Visual Studio
   Developer environment before using CMake or MSVC.
6. The generator documentation lists the regeneration command, pinned tool
   versions, supported contract variants, retained reference exceptions,
   Greek units, and tolerance policy.
