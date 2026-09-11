# Pricing engine layout

Public engine headers live under `include/kiyosi/pricing/engines/`, grouped
by instrument family and then pricing method:

- `vanilla/`: analytic, binomial, finite-difference, integral,
  Bjerksund–Stensland, and Monte Carlo engines.
- `asian/`: average-option engines.
- `digital/`: analytic, finite-difference, and integral engines.
- `barrier/`: analytic, finite-difference, and binary-barrier engines.
- `structured/`: finite-difference and Monte Carlo engines.
- `settings/`: finite-difference settings shared across instrument families.

Implementation files mirror this layout in `src/pricing/engines/`.
Shared implementation helpers remain private in `src/pricing/detail/`.
Instrument definitions remain in `include/kiyosi/instruments/`.

Include a specific engine through its family and method, for example
`<kiyosi/pricing/engines/digital/analytic.hpp>`, or include
`<kiyosi/kiyosi.hpp>` for the complete public API. The former flat engine
header paths have been removed; engine type names and pricing behavior
are unchanged.

## Tests

`tests/CMakeLists.txt` registers the sources for the single `kiyosi_tests`
executable. CTest discovers individual Catch2 test cases as before.

- `tests/api/`: compile-time public header and engine interface checks.
- `tests/core/`: library version checks.
- `tests/instruments/`: contract construction and validation.
- `tests/market/`: pricing contexts, calendars, dates, and schedules.
- `tests/pricing/engines/`: engine tests grouped by instrument family and
  pricing method, matching the production layout.
- `tests/pricing/`: numerical comparisons across engines and integration
  checks spanning multiple instrument families.
- `tests/reference/`: fixture-driven pricing and reference-fixture validation.
- `tests/support/`: shared test helpers and reference-fixture parsing.
- `tests/fixtures/`: pinned TSV reference data.

Add new test sources explicitly to `tests/CMakeLists.txt`. Run the suite
with the existing CMake build and CTest presets; on Windows, activate the
Visual Studio Developer environment first.
