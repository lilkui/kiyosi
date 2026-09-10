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

`scenarios.json` declares inputs and Kiyosi per-measure absolute tolerances, never
expected prices or Greeks. Edit it to add a scenario. `generate.py` computes every owned
price and Greek afresh using QuantLib alone, validates the full candidate manifest, then
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
QuantLib. The test `QuantLib generated references validate all Greeks and boundary declarations`
reconstructs every owned row and rejects unknown contracts, engines, variants,
or measures instead of skipping them.

## Supported contracts and tolerance policy

The smooth matrix covers European vanilla calls and puts at spot 80/100/120,
strike 100, and maturities 30/365/730 calendar days. Volatility is 30%, rate 4%,
and dividend yield 1%. Two additional ATM call/put cases expire in one day.
All use Actual/365 Fixed, continuously compounded flat curves, no date rolls,
and exercise/settlement at expiry. Effective date precedes valuation by seven
days so central time stencils stay inside the contract lifetime.

QuantLib supplies price, delta, gamma, theta, vega, and rho directly. Missing
native Greeks fall back to central differences of QuantLib prices (gamma uses
the second difference). Only a `not provided` error permits fallback; other
pricing failures abort. Speed, charm, color, vanna, and zomma use central
QuantLib delta/gamma differences, recursively using prices if necessary.
The generator never reads Kiyosi results. `check_generation` explicitly checks
price-derived higher Greeks against native-delta/gamma-derived values.

| Measure | Unit | Reference stencil |
| --- | --- | --- |
| Price | currency | native NPV |
| Delta / gamma | price per spot / spot squared | native; price fallback |
| Speed | price per spot cubed | central gamma vs spot |
| Theta | price per calendar day | native annual theta / 365; price fallback |
| Charm / color | delta / gamma per calendar day | central delta / gamma vs valuation date |
| Vega / rho | price per percentage point of volatility / rate | native / 100; price fallback |
| Vanna / zomma | delta / gamma per volatility percentage point | central delta / gamma vs volatility, divided by 100 |

Each reference stencil is evaluated at two bump sizes: spot 0.01 and 0.02,
volatility/rate 0.0001 and 0.0002, and time one and two **whole calendar days**.
Within two days of expiry, spot bumps shrink to 0.001 and 0.002 because the
one-day ATM gamma has much greater curvature. Contract dates, spot, volatility,
rate, and dividend remain fixed for time differences; valuation and all curve
reference dates move together. Time stencils reject touching expiry or preceding
effective date. European vanilla has no intermediate exercise, fixing, or barrier
events; later families must add their event boundaries before reusing this path.

One-day cases still check price and seven non-time Greeks. Theta, charm, and
color each declare `unavailable_<measure>=whole-day stability stencil touches expiry`.
This is reference unavailability, not a claim that native theta is undefined.
The numerical wrapper still computes its existing one-sided boundary time
outputs, but they are not compared to a smooth reference. C++ validates the
exact exclusions and reasons, rejects unknown/missing declarations and measures,
and checks every available native and wrapper output. Native result contracts
remain unchanged. Original European Greek and implied-volatility checks remain;
all 20 generated prices also drive independent implied-volatility recovery.

## Stability and tolerances

Each row records units, shifts, sources, `uncertainty_<measure>`,
`stability_limit_<measure>`, and `numerical_tolerance_<measure>`. Uncertainty is
the maximum absolute discrepancy from the selected reference at the two bumps;
for native first-order Greeks and gamma it includes price-fallback differences.
This measures stencil sensitivity, not a rigorous error bound. Price uncertainty
is zero under repeated deterministic NPV; its 1e-10 budget covers roundoff.
Non-finite estimates are rejected individually before computing the maximum.
`STABILITY` in the generator fixes independent ceilings, so an unstable reference
cannot silently enlarge its tolerance. Every output has an explicit native
absolute error budget in the TSV tolerances column; `scenarios.json` declares
both native and wrapper budgets. Comparisons add measured reference uncertainty
to the selected Kiyosi budget, keeping the two quantities separate.

Typical native budgets are 1e-10 for price and direct Greeks, 1e-8 for speed and
vanna, 1e-9 for zomma, 4e-6 for charm and 5e-7 for color. Day differences have
larger truncation error than continuous-time native formulas. The wrapper uses
its public default shifts (spot 0.01, volatility/rate 0.0001, time one day);
price differentiation needs budgets of 1e-6 for delta/vega/rho, 1e-7 for
gamma/speed/vanna/zomma/charm, 1e-8 for color, and 3e-5 for theta.
At one-day ATM, spot-step truncation gives roughly 8.6e-7 gamma error and
2.1e-7 speed error, so those two rows explicitly use 1e-6 and 5e-7 budgets,
and 2e-7 for the nested volatility difference in zomma. These remain far below
the old factor-of-two scaling errors. Vanna retains its correct scaling.

Before changing data, code, or tolerance, classify a discrepancy as an
input/convention mismatch, reference error, expected approximation error, or
Kiyosi defect. The independent comparisons reproduced doubled numerical vega,
rho, and zomma. Correcting their shared central denominators fixes all callers.
They also exposed implied-volatility secant stagnation for the 30-day OTM call:
near-zero vega kept steps near a bracket endpoint. The solver now bisects when
a proposed step cannot remove at least 10% of the bracket.

## Migration ledger

| Family / variant | Current status |
| --- | --- |
| European analytic vanilla price and all ten Greeks | 18 smooth independent QuantLib cases plus two one-day boundary cases |
| Original `european-analytic` and `european-analytic-put` | Retained unchanged, including their original Greek and implied-volatility checks; retained alongside independent Greek comparisons |
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
reference. Other contract families remain assigned to subsequent tickets.
