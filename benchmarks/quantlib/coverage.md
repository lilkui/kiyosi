# Coverage and exclusions

The default manifest contains 59 scenarios covering 51 required semantic cells
across 20 eligible engines. [suite_coverage.py](suite_coverage.py) is the required
engine/model/product/exercise/payout/activation/settlement inventory, maintained
independently of `cases/`. The report's `coverage.cells` records matching case
IDs and support in each native executable's `--info` registry.

Without `--case`, every cell is required, including with a replacement manifest.
Missing cells produce `missing_coverage` rows. Repeated `--case ID` selects a
focused run (`coverage.mode = focused`); it audits the supplied manifest without
requiring unselected cells. Matching uses actual adapter inputs and implicit
contract semantics, rather than case names. A constant local-volatility surface
cannot satisfy a nonconstant-surface cell.

Unknown IDs or unsupported semantics produce `unsupported` rows; build, version
or loader failures produce `prerequisite_failed` rows. Missing references and
malformed backend responses also fail the run. Every unsuccessful row has a
null ratio.

## Excluded engines

These engines have no verified direct stock QuantLib counterpart for the same
contract, model and algorithm. Similar prices, proxy portfolios and custom
QuantLib product engines do not qualify.

| Engine | Reason |
| --- | --- |
| BjerksundStenslandVanillaEngine | Kiyosi uses the 2002 approximation; QuantLib uses 1993. |
| AnalyticRangeAccrualEngine | QuantLib's rate range accrual has different contract/model semantics. |
| QuadratureBarrierEngine | No matching built-in quadrature engine identified. |
| QuadratureAccumulatorEngine, QuadratureAutocallableEngine | No native direct contract/engine identified. |
| CoxRossRubinsteinDigitalEngine | QuantLib's binomial vanilla engine requires a plain vanilla payoff. |
| FiniteDifferenceDoubleBarrierEngine, FiniteDifferenceDoubleTouchEngine | No matching native BSM/local-volatility FD engine verified. |
| FiniteDifferenceAccumulatorEngine, FiniteDifferenceAutocallableEngine | No native direct contract/engine identified. |
| MonteCarloDigitalEngine | QuantLib's American first-passage digital differs from a European terminal digital. |
| MonteCarloAsianEngine | Continuous averaging differs from QuantLib's discrete-fixing Monte Carlo. |
| MonteCarloAccumulatorEngine, MonteCarloAutocallableEngine | No native direct contract/engine identified. |

## Limits of matched engines

- American quadrature: QuantLib QdFpAmerican iterates an exercise boundary
  rather than integrating backward continuation values.
- FD binary barriers: terminal cash/asset binaries, no-touch at expiry and
  one-touch at hit are matched under BSM and supplied local volatility.
  One-touch at expiry has no verified generic native mapping.
- Analytic double barriers: continuous BSM, zero rebate and strike strictly
  between barriers. Double-touch matches cash payments at expiry only;
  mixed KIKO/KOKI activation and asset double-touch are excluded.
- Monte Carlo barriers: single continuous barriers with zero rebate only.
- Asians: unseasoned continuous averaging from valuation to expiry, flat rates
  and no history. Arithmetic cases exclude near-zero carry and singular Levy
  denominators; see [methodology](methodology.md#digitals-and-asians).
- Barrier/touch contracts: initially untouched continuous monitoring, without
  scheduled monitoring, history or discrete-dividend jumps. Analytic vanilla
  knock-in rebates pay at expiry; knock-out rebates pay at hit. No-touch at hit
  has no meaningful domain contract.

The suite does not cover every payoff, direction, scheme, RNG or moneyness
combination, or explicit Greek requests. Case-specific accuracy gates do not
establish accuracy for every domain input or a universal performance ranking.
