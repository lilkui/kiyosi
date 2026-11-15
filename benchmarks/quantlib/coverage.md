# Counterpart coverage and exclusions

The default manifest has 59 scenarios covering 51 required semantic cells across
20 eligible Kiyosi engines. The independently maintained
[suite_coverage.py](suite_coverage.py) declares each required engine, family,
model, product, exercise, payout, activation and settlement selector. It does
not derive requirements from scenario IDs or the current contents of `cases/`.
The report's `coverage.cells` lists the selector, matching scenario IDs, and
support in **each** native executable's `--info` family registry.

| Eligible engine | Required model/product cells |
| --- | --- |
| AnalyticEuropeanEngine | BSM European vanilla |
| AnalyticDigitalEngine | BSM European cash/asset digital |
| AnalyticGeometricAveragePriceEngine | BSM continuous geometric average-price |
| TurnbullWakemanArithmeticAveragePriceEngine | BSM continuous arithmetic average-price, equivalent Levy two-moment approximation |
| AnalyticBarrierEngine | BSM continuous single barrier vanilla |
| AnalyticBinaryBarrierEngine | BSM cash/asset terminal binary barrier; cash/asset one-touch at hit and expiry; cash/asset no-touch at expiry |
| AnalyticDoubleBarrierEngine | BSM continuous double barrier vanilla |
| AnalyticDoubleTouchEngine | BSM cash double-touch and double-no-touch at expiry |
| AnalyticHestonEuropeanEngine | Heston European vanilla |
| QuadratureVanillaEngine | BSM European vanilla |
| QuadratureDigitalEngine | BSM European cash/asset digital |
| CoxRossRubinsteinVanillaEngine | BSM European/American vanilla |
| CoxRossRubinsteinBarrierEngine | BSM continuous single barrier vanilla |
| FiniteDifferenceVanillaEngine | BSM and supplied nonconstant local volatility, each European/American vanilla |
| FiniteDifferenceDigitalEngine | BSM and supplied nonconstant local volatility, each cash/asset digital |
| FiniteDifferenceBarrierEngine | BSM and supplied nonconstant local volatility, each continuous single barrier vanilla |
| FiniteDifferenceBinaryBarrierEngine | BSM and supplied nonconstant local volatility, each cash/asset terminal binary barrier, cash/asset no-touch at expiry and one-touch at hit |
| FiniteDifferenceHestonVanillaEngine | Heston European/American vanilla |
| MonteCarloVanillaEngine | BSM European vanilla and American least-squares Monte Carlo |
| MonteCarloBarrierEngine | BSM continuous single barrier vanilla |

With no `--case`, the runner requires every cell, including when `--cases` supplies
a replacement manifest. Deleting all representatives of a required cell creates
an explicit `missing_coverage` row and an unsuccessful run. Repeated `--case ID`
selects a focused run; the report records `coverage.mode = focused` and still
lists the full manifest audit without requiring unselected cells. Matching uses
actual adapter inputs and implicit contract semantics, rather than ID spelling:
for example, analytic touch `hit` and FD touch `at_hit` both mean payment at hit.
A constant supplied local-volatility surface cannot satisfy the nonconstant cell.

Unknown IDs and unsupported engine/model/product semantics have `unsupported`
rows. Both native registries must support each requested family. Missing
references and malformed/incomplete backend responses have failed rows; build,
version or loader prerequisites produce `prerequisite_failed` rows for every
otherwise runnable request. Other requested rows continue after per-case
failures. JSON numbers cannot use booleans, nonfinite values or fractional native
integer controls. Every unsuccessful row has a null ratio. Accuracy and empirical
precision failures prevent timing; a rejection during recorded batches retains
the raw data and suppresses the ratio.

## Excluded engines

These 14 engine types have no verified direct stock QuantLib counterpart in the
approved contract/model/algorithm intersection. Similar prices alone do not
qualify; no proxy portfolio or custom QuantLib product engine fills these gaps.

| Engine | Reason |
| --- | --- |
| BjerksundStenslandVanillaEngine | Kiyosi uses the 2002 approximation, QuantLib the different 1993 approximation |
| AnalyticRangeAccrualEngine | QuantLib's rate range accrual has different contract/model semantics |
| QuadratureBarrierEngine | No matching built-in quadrature engine identified |
| QuadratureAccumulatorEngine | No native direct contract/engine identified |
| QuadratureAutocallableEngine | No native direct contract/engine identified |
| CoxRossRubinsteinDigitalEngine | Stock QuantLib binomial vanilla engine requires a plain vanilla payoff |
| FiniteDifferenceDoubleBarrierEngine | No matching native BSM/local-volatility FD engine identified |
| FiniteDifferenceDoubleTouchEngine | No matching native FD engine verified |
| FiniteDifferenceAccumulatorEngine | No native direct contract/engine identified |
| FiniteDifferenceAutocallableEngine | No native direct contract/engine identified |
| MonteCarloDigitalEngine | QuantLib's American first-passage digital differs from a European terminal digital |
| MonteCarloAsianEngine | Continuous Kiyosi averaging differs from QuantLib's discrete-fixing Monte Carlo |
| MonteCarloAccumulatorEngine | No native direct contract/engine identified |
| MonteCarloAutocallableEngine | No native direct contract/engine identified |

## Unmatched subsets of eligible engines

- American quadrature: QuantLib QdFpAmerican iterates an exercise boundary rather
  than integrating backward continuation values.
- FD binary barrier one-touch at expiry, for BSM and local volatility: no verified
  generic native mapping. No-touch is matched only at expiry; no-touch at hit has
  no meaningful domain contract.
- Analytic double-touch at hit: only cash expiry payments are matched.
- Monte Carlo double barriers: the matched native engine handles single barriers.
- Asian averages: ordinary inception, flat rates and unseasoned contracts only.
  Forward starts, seasoning and general deterministic curves lack a verified
  mapping. Arithmetic profiles avoid singular Levy carry denominators and
  near-zero carry, as documented in [digitals_asian.md](digitals_asian.md).
- Analytic double barriers: untouched continuous BSM, zero rebate and strike
  strictly between barriers. Nonzero rebates and outside strikes are excluded.
- Barrier/touch contracts: untouched continuous monitoring, without scheduled
  monitoring/history or discrete-dividend jumps. Analytic vanilla knock-in
  rebates pay at expiry; knock-out rebates pay at hit. FD mappings cover only
  the explicitly listed models and settlements.

Full payoff/direction/activation/scheme/RNG/moneyness cross-products and explicit
Greek requests are outside this small scenario suite. Numerical budgets and
reference uncertainties are case-specific; passing this suite does not establish
accuracy for every domain input or a universal performance ranking.
