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
European, American, and digital bindings for every scenario. Run from a fresh checkout to verify initial
environment creation. Subsequent runs use the same frozen environment.

`scenarios.json` declares inputs and Kiyosi per-measure absolute tolerances, never
expected prices or Greeks. Edit it to add a scenario. `generate.py` computes every owned
price and Greek afresh using QuantLib alone, validates the full candidate manifest, then
atomically replaces `tests/fixtures/pricing_reference.tsv` in the same directory.
Validation or pricing failures leave the old file intact. The standalone check
exercises repeatability, retained rows, recomputation, missing inputs, duplicate
IDs, invalid dates/markets/tolerances, and malformed outputs.

The TSV retains its ten columns. Generated IDs start with `ql-` and carry
`owner=QuantLib`; both markers must agree. Unmatched existing rows stay in their original
order with their original values and provenance. Five matching numerical European
four American, and four digital rows retain their IDs and comparison settings but now use
QuantLib prices and provenance. Generated rows follow in case-ID
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
| European numerical engines | 90 independent analytic targets across five engines; five original prices and three convergence targets migrated |
| American vanilla | Independent refined FD references; see the American section |
| European cash/asset digitals | Independent analytic prices and Greeks across analytic, integral, and FD engines; four original pricing rows migrated |
| Continuous barriers and binary barriers | Original references retained; migration must match rebate/hit/expiry settlement exactly |
| Scheduled barriers | Retained: repeating Kiyosi's BGK adjustment would not independently price discrete monitoring |
| Continuous geometric/arithmetic Asians | Original references retained; migration pending; seasoned geometric and deferred arithmetic variants need separate capability verification; Levy is approximate |
| Accumulator, Phoenix, binary/standard/ternary snowballs | Retained: no matching built-in QuantLib contract identified |
| Bermudan | Retained construction/capability checks; Kiyosi has no pricing engine |
| Constructor validation and other-family convergence/simulation checks | Retained with original provenance; not claimed as QuantLib comparisons |

All unmatched rows retain their original `source_revision` and
`source_symbol`. The five migrated numerical rows use `reference_provider=QuantLib`
and QuantLib source provenance while keeping their legacy IDs and price-only schema. Their `reference_kind=analytic` alone does **not** imply QuantLib
ownership. No retained row in this slice is silently promoted to an independent
reference. Other contract families remain assigned to subsequent tickets.


## Numerical European engines (issue 03)

`numerical_engines.json` declares five engine profiles, all settings, absolute
budgets, and wrapper shifts. The 18 smooth scenarios are crossed with each profile
(90 rows). The analytic engine remains the QuantLib oracle regardless of Kiyosi
resolution; no QuantLib tree, PDE grid, or random sampling is used. Existing
one-day analytic boundary checks remain unchanged; numerical profiles use only
smooth maturities, with time stencils inside the effective/expiry dates.

| Kiyosi engine | Settings | Native measures | Native absolute budgets |
| --- | --- | --- | --- |
| Binomial / CRR | 800 steps | price, delta, gamma | 0.04, 0.003, 0.0003 |
| Integral | fixed implementation: Simpson, 1024 panels, normal bounds +/-10 | price | 1e-6 |
| Finite difference | 400 asset / 800 time steps, Crank-Nicolson, fixed upper asset boundary 400 | price, delta, gamma | 0.03, 0.003, 0.0003 |
| Monte Carlo | seed 42, 200000 paths, 2 grid points (one exact GBM increment), antithetic pairs | price | 0.5 |

Tree price budgets allow the O(1/N) payoff-kink error, and delta/gamma budgets
allow first/second-level tree estimates. The PDE grid has unit asset spacing;
fixed upper boundary keeps it identical across spot shifts. Its budgets allow
spatial interpolation and second-difference error, especially at 30-day maturity.
Integral quadrature starts at the exercise threshold, so it integrates a smooth
payoff branch; 1e-6 allows Simpson truncation and tail loss over this matrix.
MC budgets allow sampling error, conservatively several standard errors even at
the longest maturity; antithetic pairing reduces variance. They are regression
budgets, not confidence guarantees. Seeds repeat within Kiyosi; they do not imply
identical random samples across libraries or standard-library implementations.
MC seed, paths, steps and price budget appear in the dedicated TSV column and
are cross-checked against the profile attributes.

All ten wrapper Greeks are compared for each engine on the ATM one-year call
and put. Integral uses the established default shifts and analytic-wrapper
budgets, with 1e-6 price tolerance for quadrature. Tree, PDE, and MC use spot
shift 4, volatility shift 0.01, rate shift 0.001 and one calendar day. Spot 4
spans multiple tree/grid cells and avoids differentiating a locally linear
price with a tiny stencil; MC reuses the seed across bumps. Volatility 0.01
reduces amplification of grid/sampling noise in mixed derivatives. These are
Kiyosi shifts only: reference stability still uses the original independent
small QuantLib stencils and unchanged ceilings.

| Wrapper measure | Tree / CRR / PDE absolute budget | MC absolute budget |
| --- | --- | --- |
| price | engine native budget | 0.5 |
| delta | 0.003 | 0.01 |
| gamma | 0.002 | 0.002 |
| speed | 0.0002 | 0.0002 |
| theta | 0.0003 | 0.001 |
| charm | 0.00003 | 0.00005 |
| color | 0.00001 | 0.00002 |
| vega | 0.005 | 0.015 |
| vanna | 0.001 | 0.001 |
| zomma | 0.0003 | 0.0003 |
| rho | 0.005 | 0.01 |

Second and third differences amplify lattice and sampling errors; the larger
spot/volatility stencils also introduce truncation error. Higher-Greek budgets
are therefore coarser than the integral checks, which retain the tight all-Greek
scaling regression. All budgets are explicit in each generated row; reference
uncertainty remains separate and never changes in response to Kiyosi errors.
The full reference vector is retained for every row; all native promised
measures are checked, and `wrapper=true` selects the representative all-Greek
comparisons. Unsupported native Greeks must remain absent.

Migrated IDs: `european-binomial`, `crr-vanilla`, `european-integral`,
`european-fd`, `european-mc`. Their old targets included rounded values 13.15
and 10.2; these were reference approximations, not production defects. Their
existing comparison budgets are retained (0.05 tree/CRR, 0.1 integral/PDE,
0.5 MC). The legacy PDE remains explicit Euler with 200/4000 steps; legacy MC
retains seed 42, 20000 paths and 252 grid points. All five now reconstruct their
contracts, markets and settings from the fixture in the generated-reference test.
The tree, CRR and PDE convergence sequences retain resolutions 50/100/200,
compare decreasing final versus initial error, and enforce their original 0.1
final budget against the recomputed analytic target. No production defect was
observed in this slice; no tolerance was widened to obtain a passing result.

## American vanilla references

`american.json` declares ten contracts and five engine profiles: binomial,
the American overload of CRR, finite difference, Bjerksund–Stensland 2002,
and Longstaff–Schwartz Monte Carlo. The 50 generated rows reconstruct the
contract, market, shifts, and engine settings in the C++ fixture test. The
four older `american-binomial`, `american-fd`, `american-bs`, and `american-mc`
rows also have independently recomputed targets and complete reconstruction
inputs, including the two retained Kiyosi convergence sequences.

Verified in the frozen QuantLib-Python 1.18 / QuantLib 1.41 wheel:
`AmericanExercise(earliestDate, latestDate, payoffAtExpiry=False)` and
`FdBlackScholesVanillaEngine(process, tGrid, xGrid, dampingSteps, schemeDesc)`
are callable. The engine supplies NPV, delta, and gamma. Curves and volatility
use the same Actual/365 Fixed, continuous rates/dividends and valuation date
as European references. Effective date is the start of the exercise window;
exercise is allowed through expiry and payment is at exercise. The supported
Kiyosi valuation interval is effective through expiry; forward-start pricing
before effective is not supported. These cases contain no discrete dividends.

The matrix spans calls and puts at spot 80/100/120, maturities 30/365/730 days,
a dividend-paying long-dated call, a long-dated ITM put, deep immediate-exercise
call and put, an ITM one-day put, and a call valued at exercise-window start.
Reference price/delta/gamma are native FD results. All other Greeks use central
differences of FD price/delta/gamma; delta and gamma also have price-derived
cross-checks. No Bjerksund engine is called in Python: QuantLib's 1993 formula
is not an implementation reference for Kiyosi's 2002 approximation.

Reference refinement uses 800x800, 1600x1600, 3200x3200 time-by-space grids,
Douglas stepping, two damping steps, local volatility disabled. Each row
records both adjacent-grid discrepancies per measure. `refinement_*` is the
last discrepancy; `coarse_refinement_*` is the earlier one. The generation
check requires decreasing price discrepancy (with a 1e-9 roundoff floor),
or confirms the price on an additional 6400x6400 grid within its already
recorded uncertainty. The 30-day OTM put has non-monotonic differences
2.34e-7 then 3.17e-7; the additional grid differs by 2.50e-7, confirming
the existing 3.17e-7 uncertainty without changing the reference or budgets.
The final maximum price discrepancy over the matrix is 0.000725. An initial
200x400/400x800 trial differed by 0.00599 on the long call, so the reference
was refined before any Kiyosi comparisons. This is empirical precision, not
a rigorous error bound.

Reference bumps are spot 0.5/1, volatility 0.002/0.004, rate 0.001/0.002, and
time 1/2 whole calendar days, with contractual dates and market levels fixed.
`bump_error_*` records the difference between these estimates (or native
delta/gamma versus their price stencils). `uncertainty_*` is the maximum of
that discrepancy and final grid discrepancy, separate from all Kiyosi budgets.
Units remain price/day for theta, delta/day for charm, gamma/day for color,
and per percentage point for vega/vanna/zomma/rho.

| Measure | Independent reference stability ceiling |
| --- | --- |
| price | 0.001 |
| delta | 0.0002 |
| gamma | 0.00005 |
| speed | 0.00002 |
| theta | 0.00003 |
| charm | 0.00002 |
| color | 0.000002 |
| vega | 0.001 |
| vanna | 0.00003 |
| zomma | 0.00001 |
| rho | 0.001 |

At one-day expiry proximity, theta/charm/color are declared unavailable:
`whole-day stability stencil touches expiry`. At effective date they use
`whole-day stability stencil precedes exercise window`. The parser and actual
C++ comparisons enforce these exact declarations and still validate price
and all supplied native measures. Immediate exercise scenarios lie well inside
the exercise region, where the payoff is locally linear; their spatial Greeks
are stable and remain checked. No free-boundary kink is claimed to be smooth.

| Kiyosi engine | Settings | Native budgets (price/delta/gamma) |
| --- | --- | --- |
| Binomial / CRR | 800 steps | 0.04 / 0.003 / 0.0003 |
| FD | 400 asset, 800 time, Crank–Nicolson, upper=400 | 0.03 / 0.003 / 0.0003 |
| Bjerksund–Stensland 2002 | fixed approximation | 0.15 / unavailable / unavailable |
| MC | seed 42, 40000 paths, 50 points, antithetic, quadratic LSM | 0.7 / unavailable / unavailable |

Tree and FD budgets allow payoff/grid interpolation and early-exercise time
discretization. The BS2002 budget allows suboptimal exercise-boundary
approximation, without interpreting its error as reference uncertainty.
MC budgets allow sampling, regression policy and exercise-grid errors; they
are absolute regression budgets, not statistical confidence guarantees.
Seed/path/grid settings and the statistical price budget are repeated in the
MC TSV column and cross-checked in C++. Seed equality does not promise identical
samples across libraries or C++ standard-library implementations.

All ten wrapper Greeks are checked on the ATM one-year call and put for every
engine. Tree/CRR/FD use spot 4, volatility 0.01, rate 0.001, time 1 day. BS2002
uses spot 0.5, volatility 0.002, rate 0.001, time 1 day. MC uses spot 8,
volatility 0.02, rate 0.01, time 2 days, with common random numbers across bumps.

| Wrapper measure | Tree / CRR / FD | BS2002 | MC |
| --- | --- | --- | --- |
| price | native budget | 0.15 | 0.7 |
| delta | 0.003 | 0.015 | 0.03 |
| gamma | 0.002 | 0.002 | 0.004 |
| speed | 0.0002 | 0.0003 | 0.0008 |
| theta | 0.001 | 0.001 | 0.005 |
| charm | 0.0001 | 0.0001 | 0.001 |
| color | 0.00003 | 0.00002 | 0.0003 |
| vega | 0.01 | 0.02 | 0.05 |
| vanna | 0.002 | 0.002 | 0.004 |
| zomma | 0.0005 | 0.0005 | 0.002 |
| rho | 0.02 | 0.04 | 0.08 |

The wider mixed/time budgets account for exercise-decision changes under
bumps. MC differentiates a sampled fitted exercise policy, so its third and
mixed derivatives are intentionally coarse. A diagnostic at spot bumps 4/8
and path counts 40000/80000 isolated this noise: call gamma at bump 4 was
0.00772/0.00941, versus 0.01093/0.01104 at bump 8 and reference 0.01276;
color fell from 0.000665/0.000491 to 0.000116/0.000145 (reference 0.0000189).
The larger bump reduces policy noise while adding truncation error. The
initial budgets were retained; no tolerance was enlarged to obtain a pass.

Discrepancies were classified before changing targets or production code:
the old American put price 10.225098 was a stale reference (the converged
contract price is 10.53913614); BS2002 differences are approximation error;
the MC Greek discrepancies above are regression/sampling noise. A verified
production defect omitted exercise at valuation in MC: a deep put returned
49.79607993 when immediate exercise pays 50. The public-engine regression
failed before the fix and passed after comparing continuation with intrinsic
value at time zero. The fix preserves the simulation and regression algorithm.

Bermudan construction and API capability checks remain in the C++ suite.
Kiyosi has **no Bermudan pricing engine**. The old `bermudan-binomial` row is
retained byte-for-byte with its original provenance only as historical data;
its European-labelled source and price do not constitute a Bermudan pricing
comparison. The manifest inventory excludes it from instrument/engine pricing
pairs. All other unmatched rows retain their original bytes and provenance.

Run the same frozen regeneration and `check_generation.py` commands above.
They validate byte identity, retained rows, corrupted-reference rejection and
atomic replacement for both families. CMake and the ordinary C++ tests only
read committed TSV data and never invoke Python or QuantLib.


## European digital references (issue 05)

`digital.json` declares the common market, spots 80/100/120, maturities
30/365/730 days, cash payout 10, and all Kiyosi engine profiles. `digital.py`
crosses both payoff kinds and call/put directions with that matrix, adding
one-day ATM contracts for each kind/direction: 40 contracts, 120 generated
rows. Every row is reconstructed and priced in C++; unknown instruments,
engines, payoff kinds, variants, and measures fail the comparison.

The frozen QuantLib-Python 1.18 / QuantLib 1.41 bindings support
`CashOrNothingPayoff(direction, strike, cashPayoff)` and
`AssetOrNothingPayoff(direction, strike)` in `VanillaOption` with
`EuropeanExercise` and `AnalyticEuropeanEngine`. Spot, strike, payout,
continuous rate/dividend yield, volatility, Actual/365 Fixed dates, and
expiry settlement match Kiyosi. Cash pays the specified amount; asset pays
the terminal underlying. Both use strictly positive signed intrinsic value
and pay zero exactly at strike. There are no date rolls or interim events.

QuantLib supplies price/delta/gamma/theta/vega/rho natively for both payoffs.
The existing price fallback and higher-Greek finite differences are retained
and checked at both bump sizes. Reference units and spot/volatility/rate/time
bumps are those in the common table above. Every row records its own measured
uncertainty. Digital reference stability ceilings are calibrated from
QuantLib alone, before Kiyosi comparisons:

| Measure | Reference stability ceiling |
| --- | --- |
| price | 1e-10 |
| delta / speed / vega / rho | 1e-5 |
| gamma / vanna / zomma | 1e-6 |
| theta | 3e-4 |
| charm | 2e-4 |
| color | 1e-5 |

Asset digitals have greater curvature than vanilla options: maximum whole-day
bump discrepancies were 2.732e-4 theta, 1.617e-4 charm, and 7.623e-6 color.
The largest speed discrepancy was 6.257e-6 on the one-day asset call. These
are empirical reference uncertainties, not Kiyosi error allowances or rigorous
bounds. Unexpected non-finite or above-ceiling sensitivities abort before
fixture replacement. The generation check injects bad digital Greeks to verify
this, checks price-derived higher Greeks, and verifies byte-identical frozen
regeneration and preservation of unrelated rows.

One-day cases still check price and native delta/gamma, and carry reference
values for all seven non-time Greeks. Theta/charm/color explicitly declare
`whole-day stability stencil touches expiry`; the parser enforces that reason.
No numerical wrapper is run on these cases. All smooth analytic/integral rows
check all ten wrapper Greeks; FD does so on the one-year ATM call and put of
each payoff kind. No time stencil reaching expiry is evidence of smoothness.
For exact expiry, QuantLib's option NPV reports an expired contract, so it is
not a settlement oracle. `check_generation.py` instead verifies its payoff
bindings at spot 99/100/101 for both directions and payoff kinds. The C++ test
`Digital expiry settlement uses strict strikes without smooth Greeks` checks
those same exact cash flows through all three engines and requires every
Greek to be absent. In particular, every ATM settlement is zero.

| Kiyosi engine | Settings | Native price / delta / gamma budgets |
| --- | --- | --- |
| AnalyticDigitalEngine | fixed formula | 1e-8 / 1e-9 / 1e-9 |
| IntegralDigitalEngine | Simpson, 2048 panels, normal bounds +/-12, split at strike | 1e-8 / unavailable / unavailable |
| FiniteDifferenceDigitalEngine | Crank-Nicolson, 1602 asset / 1600 time steps, upper=400; one-day cases use 9602 asset steps | 0.03 / 0.003 / 0.003 |

Analytic budgets allow floating-point evaluation; the integral price budget
allows smooth-branch Simpson truncation and tail loss. FD budgets allow cell
averaging, spatial/time discretization, and interpolation. The upper boundary
stays fixed under spot bumps. The strike lies between grid nodes on the main
matrix; the migrated convergence sequence additionally exercises aligned and
unaligned strikes. The one-day asset delta error at 1602 steps was 0.08069;
resolution is increased for those cases rather than relaxing the 0.003 budget.

| Wrapper measure | Analytic / integral budget | FD budget |
| --- | --- | --- |
| price | 1e-8 | 0.03 |
| delta | 1e-5 | 0.003 |
| gamma | 1e-6 | 0.003 |
| speed | 1e-6 | 0.0003 |
| theta | 0.0001 | 0.001 |
| charm | 1e-6 | 0.0001 |
| color | 1e-7 | 0.00003 |
| vega | 1e-5 | 0.01 |
| vanna | 1e-6 | 0.002 |
| zomma | 1e-6 | 0.0005 |
| rho | 1e-5 | 0.02 |

Analytic/integral wrapper shifts are spot 0.01, volatility/rate 0.0001 and
one day. Their higher-derivative budgets allow roundoff amplification from
price differences and truncation relative to native reference derivatives.
FD uses spot 2, volatility 0.002, rate 0.001 and one day: the spatial stencil
spans multiple cells instead of differentiating locally linear interpolation;
its budgets allow the resulting truncation and amplified discretization error.
Reference uncertainty is added separately in Greek comparisons and never
absorbed into these Kiyosi budgets.

Discrepancies exposed two production problems. Integral quadrature selected
the ITM interval but re-tested the strict payoff at the strike endpoint,
occasionally replacing its one-sided limit with zero due to rounding. This
lost about 0.078 in asset digital value and produced enormous spurious bumped
Greeks. The integrand now uses the smooth ITM branch throughout the selected
interval; exact-expiry strict settlement is unchanged. FD sampled the terminal
jump at grid nodes and returned Greeks at the lower node rather than at spot.
Cell-average terminal payoffs remove strike-alignment bias, and interpolation
of node Greeks evaluates them at spot. The new independent comparisons failed
before these fixes. All original Kiyosi error budgets were retained.

Migrated rows: `cash-digital-analytic`, `asset-digital-analytic`,
`digital-integral`, `digital-fd`. All have complete fixture inputs and freshly
computed QuantLib prices. The FD put's old 4.0 target was stale: the independent
value is 4.9955171365497844. Its grid sequence 50/100/200 and original final
0.3 convergence budget remain, now against that target. Constructor validation
and other families' references/provenance remain unchanged. All requested
European cash/asset combinations are supported. American/Bermudan digital
payoffs have no applicable Kiyosi pricing engine and are not claimed as
validated pricing variants; their constructor/capability checks are retained.

## Continuous vanilla barriers (issue 06)

`barrier.py` generates 120 rows (60 contracts through each of the analytic and
finite-difference engines), and migrates all 18 continuous vanilla-barrier
pricing rows. The compact matrix crosses call/put, up/down, in/out, and all
valid rebate timings with three paired spot/maturity points (80/30 days,
100/365 days, 120/730 days). Strike is 100; barriers are 60 and 140. It also
includes already-hit spots 50/150 and a one-day, spot-110 boundary. Each row
records the complete contract, market, monitoring, engine/grid, shifts,
reference units, uncertainty and independent per-measure absolute budgets.
The existing at-hit knock-in constructor rejection remains unchanged.

### Pinned binding and exact settlement mapping

`barrier.check_bindings()` executes against QuantLib-Python 1.18 / QuantLib
1.41. `BarrierOption` accepts a barrier type, level, rebate, payoff and exercise;
it exposes neither a rebate-payment flag nor an observation schedule.
`AnalyticBarrierEngine` prices European plain-vanilla barriers. Actual probes
reject cash-or-nothing payoffs (`non-plain payoff given`) and American exercise
(`only european style option are supported`). Its delta, gamma, theta, vega
and rho calls report `not provided`. All required live continuous contracts
have an analytic mapping, so no discretized QuantLib substitute is needed.

Let `KI(R)` and `KO(R)` denote QuantLib barriers with cash rebate R and the
same strike, direction, barrier, expiry and market. QuantLib pays a knock-in
rebate at expiry on paths that never touch, and a knock-out rebate at the
first touch. Therefore:

- Kiyosi knock-in, rebate at expiry: `KI(R)` directly.
- Knock-out, rebate at hit: `KO(R)` directly.
- Knock-out, rebate at expiry: `KO(0) + Bond(R,T) - (KI(R) - KI(0))`.

All barrier legs use `AnalyticBarrierEngine`; `Bond(R,T)` is a zero-coupon
bond with face R, zero settlement days, NullCalendar and unadjusted maturity,
priced by `DiscountingBondEngine` on the same continuous flat risk-free curve.
The bracketed portfolio pays R at expiry only on no-touch paths, so its
subtraction leaves precisely the deferred hit payment, without changing its
payment date. A second executable check compares both up/down rebate legs
and both payment timings against cash one-touch `VanillaOption` contracts
priced by `AnalyticDigitalAmericanEngine`, with American exercise beginning
at valuation and `payoffAtExpiry` set to match settlement.

QuantLib's live barrier engine rejects an already-crossed barrier. In that
state the contract has already reduced to a European vanilla (knock-in,
`AnalyticEuropeanEngine`), immediate `SimpleCashFlow` (knock-out at hit), or
zero-coupon bond (knock-out at expiry). These state reductions supply the
boundary prices, without asking QuantLib to price an invalid live barrier.

### Greeks and approximation budgets

Both Kiyosi native barrier engines promise only price, which the consumer
asserts explicitly. References for all ten Greeks use central differences of
QuantLib portfolio prices, including nested price-derived gamma for speed,
color and zomma. Spot steps 0.03/0.06 balance third-derivative cancellation
against truncation; volatility/rate steps are 0.0001/0.0002 and valuation
steps are 1/2 whole calendar days. Contract dates stay fixed and all curves
are rebuilt at the shifted valuation date. A full speed stencil stays at
least three spot bumps away from a change in hit state. Already-hit points
are strictly beyond the barrier; no stencil crosses it. One-day rows omit
only theta/charm/color, explicitly declaring that the stability stencil
would touch expiry. Their prices remain checked; the full wrapper is disabled
because it always computes time Greeks. Its other reference Greeks are
retained for completeness, not claimed as native comparisons.

Reference stability ceilings are price 1e-10, delta 1e-5, gamma 1e-6, speed
1e-6, theta 1e-4, charm 1e-5, color 3e-6, vega/rho/vanna 1e-6, zomma 1e-7.
These limits concern reference bump consistency, not Kiyosi error. The largest
observed two-bump discrepancies are 1.79e-6 delta, 3.02e-7 gamma, 3.76e-7
speed, 2.02e-5 theta, 8.03e-7 charm, 2.39e-6 color, 1.20e-7 vega,
3.44e-7 vanna, 4.07e-8 zomma and 1.11e-7 rho. The larger nested-derivative
limits allow finite-stencil truncation; 0.001 spot bumps exhibited roundoff
amplification and 0.1 bumps increased truncation, so neither was adopted.

| Measure | Analytic wrapper budget | FD wrapper budget |
| --- | --- | --- |
| price | 1e-8 | 0.05 |
| delta | 1e-6 | 0.005 |
| gamma | 1e-7 | 0.002 |
| speed | 2e-7 | 0.0005 |
| theta | 0.0001 | 0.001 |
| charm | 0.00001 | 0.0002 |
| color | 0.000003 | 0.00005 |
| vega | 0.000001 | 0.005 |
| vanna | 0.0000001 | 0.001 |
| zomma | 0.00000002 | 0.0002 |
| rho | 0.000001 | 0.005 |

Analytic wrapper shifts are spot 0.01, volatility/rate 0.0001, one day.
It runs on all 48 smooth contracts. FD uses a fixed upper boundary of 400,
1600 asset/time steps and Crank-Nicolson; barriers, strikes and base spots
align with the 0.25 grid. Its spot bump of 2 spans eight cells to avoid
locally linear interpolation; volatility/rate bumps are 0.002/0.001 and
time is one day. All 12 one-year spot-100 combinations run through this
wrapper. Budgets allow spatial/time discretization, interpolation and the
larger finite-stencil truncation; they are fixed separately from the measured
reference uncertainty, which is added only in Greek comparisons. There is
no native delta/gamma promise to expand. The price budget is 0.05 currency
units on a strike-100 contract; higher-derivative budgets permit differentiation
of that approximate surface rather than asserting analytic precision.

### Diagnosed FD defect and retained exceptions

The initial independent comparison of `barrier-fd` missed its original 0.05
price budget by 0.291. At an aligned 1600/1600 grid, the old solver still missed
by 0.151: alignment alone did not explain it. It solved across knocked-out
nodes before overwriting them, permitting diffusion through an absorbing
boundary within each implicit step. Continuous knocked-out nodes now impose
Dirichlet equations during the solve, including domain endpoints. Scheduled
observation handling is unchanged. A dedicated regression asserts the original
0.05 budget and decreasing errors at aligned 800/1600/3200 refinements.

The legacy `barrier-fd` row now uses the aligned 1600/1600 grid and upper
boundary 400; its recorded asset refinements are 400/800/1600. The old 1000
asset grid placed barrier 95 between cells and retained a 0.120 barrier-location
error even after the solver fix. Its 0.05 price and 0.1 final refinement budgets
were not widened. Other legacy FD settings and budgets remain as before.
The consumer executes every legacy convergence sequence and final bound.

`barrier-scheduled-monitoring` remains byte-for-byte unchanged with its
DerivaSharp provenance. QuantLib's continuous `BarrierOption` has no schedule
argument. Applying Kiyosi's BGK shift and then pricing continuously is not an
independent discrete-monitoring reference. Binary-barrier, Asian, constructor
and unrelated pricing references are outside this migration and retained.
Frozen regeneration and failure-atomicity checks run offline tooling only;
CMake/CTest use committed TSV values and never invoke Python or QuantLib.
