# Parity fixtures

`tests/fixtures/european_bsm.tsv` is a language-neutral, tab-separated set of
reviewed European option reference results. The values were generated once
from the DerivaSharp C# implementation (the parity oracle), then checked in
so C++ tests do not need the oracle or a network connection.

Each data row contains the instrument (`option_type`, `strike`, `expiry`),
market assumptions (`spot`, rates, dividend yield, volatility, and valuation
date), the observed price used for implied volatility, and an expected value
and absolute tolerance for every output. `theta`, `charm`, and `color` are per
calendar day using Actual/365 Fixed. `vega`, `vanna`, and `zomma` are per
percentage-point volatility move; `rho` is per percentage-point rate move.
Other Greeks use one-unit changes in their named underlying quantity. Prices
and rates follow the continuously compounded Black-Scholes-Merton convention.

The public `date` boundary is midnight-anchored; `timestamp` and
`PricingContext::valuation_time()` preserve intraday valuation moments. Shared
`year_fraction` uses Actual/365 Fixed, while `TradingCalendar::trading_year_fraction`
uses its validated annual trading-day count for calendar-aware calculations.

Lines beginning with `#` and blank lines are ignored. The test-only parser
rejects missing columns, invalid dates/numbers, unknown option types, and
negative tolerances with a row number and field name. Fixture comparison is
absolute and inclusive at the stated tolerance.

`tests/fixtures/cpu_parity.tsv` is the broader CPU parity manifest. Its
language-neutral rows name the instrument, engine, and variant, then carry
semicolon-separated `key=value` inputs, outputs, and per-output tolerances.
Every row also carries `source_revision`, `source_symbol`, `convention`, and
`reference_kind` in `inputs`; the parser requires the pinned DerivaSharp
revision `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2` and an explicit tolerance
for each numeric output.
Validation expectations use `category|message`; convergence metadata uses
`parameter|resolution1,resolution2|reference|tolerance`; seeded Monte Carlo
metadata uses `seed|paths|steps|tolerance`. A dash means that metadata does
not apply. The Catch2 manifest test validates every row and includes the case,
instrument, and engine in assertion context, so failures remain actionable
without a live DerivaSharp build.

The closure tests require one fixture for each concrete CPU engine and include
public payoff, in/out, convergence, and seeded Monte Carlo properties. Schedule
and settlement convention differences are recorded in ADR 0010; CUDA parity
remains out of scope there.

## Regeneration and review

From the Kiyosi checkout, run the developer-only script with PowerShell:

```powershell
pwsh -NoProfile -File tools/generate_cpu_parity.ps1 `
  -DerivaSharpRoot F:\dev\Repos\DerivaSharp
```

The script verifies the sibling checkout is exactly at the pinned revision,
checks the referenced DerivaSharp source symbols, and rewrites the checked-in
manifest deterministically. It is not included in CMake, CTest, packaging, or
the installed library. Review generated numeric changes against the pinned
source test/data symbol before committing; Monte Carlo means retain their fixed
seed, path count, derived trading-step provenance, and reviewed statistical
tolerance.
