# QuantLib reference generation

From the repository root, with uv **0.11.2** installed:

```sh
uv run --project tools/reference-data --frozen tools/reference-data/generate.py
uv run --project tools/reference-data --frozen tools/reference-data/check_generation.py
```

uv downloads managed CPython **3.13.12** and installs **QuantLib-Python 1.18**
and its **QuantLib 1.41** binding distribution from the committed `uv.lock`.
The project requires that uv version and managed Python; no system Python is
needed. The runtime checks both distribution versions and executes the required
European bindings for every scenario. Run from a fresh checkout to verify initial
environment creation. Subsequent runs use the same frozen environment.

`scenarios.json` declares inputs and Kiyosi absolute price tolerances, never
expected prices. Edit it to add a scenario. `generate.py` computes every owned
price afresh using QuantLib alone, validates the full candidate manifest, then
atomically replaces `tests/fixtures/pricing_reference.tsv` in the same directory.
Validation or pricing failures leave the old file intact. The standalone check
exercises repeatability, retained rows, recomputation, missing inputs, duplicate
IDs, invalid dates/markets/tolerances, and malformed outputs.

The TSV retains its ten columns. Generated IDs start with `ql-` and carry
`owner=QuantLib`; both markers must agree. Existing rows stay in their original
order with their original values and provenance. Generated rows follow in case-ID
order, with sorted attributes, 17-significant-digit numbers, UTF-8 without BOM,
LF newlines, fixed dates, and no timestamps or random numbers. A second run is
byte-identical in the pinned environment; cross-platform floating-point identity
is not required. Ordinary CMake builds and Catch2 tests do not invoke Python or
QuantLib. The test `QuantLib generated reference rows all compare European prices`
reconstructs every owned row and rejects unknown contracts, engines, variants,
or measures instead of skipping them.

## Supported contracts and tolerance policy

This slice covers European vanilla call/put **prices** via
`QuantLib.AnalyticEuropeanEngine`: spot 80/100/120, strike 100, maturities
30/365/730 calendar days, volatility 30%, rate 4%, dividend yield 1%.
All cases declare effective, valuation, and expiry dates. Exercise and settlement
are at expiry; no date adjustment or business-day roll applies. Flat BSM curves
use continuously compounded rates/dividends and Actual/365 Fixed; volatility
uses the same day count. There is no numerical grid, seed, or finite-difference
bump. Kiyosi reconstructs the dates at midnight with its all-days calendar.

References are independent analytic double-precision results, with floating-point
roundoff uncertainty (not a measured convergence bound). This classification and
uncertainty are recorded separately from the **1e-10 absolute price** allowance
for the Kiyosi analytic engine. The allowance accommodates double-precision
arithmetic and normal-CDF evaluation differences at these roughly 100-unit
contract scales; it is not an approximation budget. Outputs and tolerances must
have identical keys and finite values, with non-negative tolerances. Provenance
also records the explicit price tolerance and both resolved binding versions.

Before changing data, code, or tolerance, classify a discrepancy as an
input/convention mismatch, reference error, expected approximation error, or
Kiyosi defect. Fix verified defects; never widen a tolerance merely to pass.
The initial matrix passed at the declared tolerance without production changes.

## Migration ledger

| Family / variant | Current status |
| --- | --- |
| European analytic vanilla prices | 18 independent QuantLib cases generated here |
| Original `european-analytic` and `european-analytic-put` | Retained unchanged, including their original Greek and implied-volatility checks; migration awaits the Greek slice |
| European numerical engines and American vanilla | Original references retained; later slice will compare against independent analytic or converged references |
| European cash/asset digitals | Original references retained; analytic migration pending |
| Continuous barriers and binary barriers | Original references retained; migration must match rebate/hit/expiry settlement exactly |
| Scheduled barriers | Retained: repeating Kiyosi's BGK adjustment would not independently price discrete monitoring |
| Continuous geometric/arithmetic Asians | Original references retained; migration pending; seasoned geometric and deferred arithmetic variants need separate capability verification; Levy is approximate |
| Accumulator, Phoenix, binary/standard/ternary snowballs | Retained: no matching built-in QuantLib contract identified |
| Bermudan | Retained construction/capability checks; Kiyosi has no pricing engine |
| Constructor validation, convergence and simulation checks | Retained with original provenance; not claimed as QuantLib comparisons |

All rows without QuantLib ownership retain their original `source_revision` and
`source_symbol`. Their `reference_kind=analytic` alone does **not** imply QuantLib
ownership. No retained row in this slice is silently promoted to an independent
reference. Greeks, their unit conversions and finite-difference stability checks
belong to subsequent tickets.
