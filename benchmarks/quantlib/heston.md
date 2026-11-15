The three Heston cases cover analytic European call pricing and FD European call
and American put pricing. Shared inputs are S=K=100, r=.05, q=.02, v0=theta=.04,
kappa=1.5, xi=.3, rho=-.7, valuation 2025-01-01 and expiry 2026-01-01.
Both libraries use Actual/365 Fixed and continuously compounded flat rates.

The budgets were declared before profile calibration: `1e-7` absolute price
units for Fourier integration and `.005` for FD. These are ordinary ATM cases,
not a broad stability or tail test. The fixed backend profiles and current
accuracy gates are in [cases/heston.json](cases/heston.json). A finer grid does
not inherently make the comparison fair; each implementation must satisfy the
same reference and uncertainty budget before timing.

Kiyosi uses adaptive Fourier integration with a deterministic variance control
variate, a semi-infinite frequency transform and bounded unresolved tail;
QuantLib uses Gatheral with adaptive Lobatto. Their committed tolerances are
explicit, with maximum characteristic-function/evaluation budgets. Independent
Lobatto and exp-sinh calculations at `1e-10` and `1e-12` provide the European
reference. The quadrature tolerance does not claim equal evaluation counts.

FD uses different numerical methods. Kiyosi has fixed modified Craig–Sneyd
ADI theta=1/3, no configurable damping, and Ikonen–Toivanen American splitting.
It counts grid intervals and has explicit upper asset/variance bounds 400/.6.
QuantLib uses Hundsdorfer, damping=0, native American payoff projection, and
counts mesher nodes. Its public engine does not expose truncation-domain
controls. The offline reference probe records the actual asset and variance
extrema from `getSolverDesc(1.0)` after each price, including their changes under
independent grid refinements. These domains are not equal to Kiyosi's. Kiyosi
upper asset and variance bounds are separately doubled with corresponding grid
refinement to check truncation sensitivity. No custom QuantLib product engine
or multiple-strike cache is involved.

[references/heston.json](references/heston.json) contains native
QuantLib **1.44** evidence: independent integration and separate FD dimensions,
and American joint 400/800/1600 grids with first-order exercise-time
extrapolation. American reference uncertainty includes consecutive joint-grid
extrapolate disagreement and disagreement with an independently extrapolated
sum of time/asset/variance corrections. It is an empirical convergence estimate,
not a mathematical bound. Every ordinary run includes that uncertainty in its
accuracy check and requires uncertainty at most one tenth of the case budget.
The American joint extrapolate is 6.450555006874128; its uncertainty is
0.00012831737323626413. The independent dimension extrapolate is
6.450541435750119, differing by about 1.36e-05.
The correctness fixture records QuantLib 1.44 joint grids and uses the
joint-grid extrapolate disagreement as its reference uncertainty.

Offline regeneration, after building the companions with `run.py`:

```sh
python3 benchmarks/quantlib/generate_heston_references.py
python3 benchmarks/quantlib/calibrate_heston.py
```

The first command builds a standalone native reference probe, flushes each
completed observation to `build/heston-refinements.jsonl`, and writes versioned
references only after all refinements finish. The second records current
committed prices and independent integration/grid/domain checks in
[heston_calibration.json](heston_calibration.json). Both may take minutes and
are excluded from ordinary timing.

The native benchmark calls public price APIs on preconstructed inputs.
QuantLib FD inherently retrieves additional sensitivities; Kiyosi Heston
pricing does not calculate Greeks. All actual numerical work and allocations
inside those calls remain timed. The report retains the methods, explicit
profiles, reference evidence and 15 paired native timing batches. A coarse
profile failing the budget produces an unsuccessful accuracy report and
suppresses the speed ratio; the native integration test exercises that path.

All 24 committed/refinement calibration checks pass. Errors including reference
uncertainty for the committed profiles are:

| Case | Kiyosi | QuantLib | Budget |
| --- | ---: | ---: | ---: |
| Analytic call | 1.01e-11 | 1.01e-11 | 1e-7 |
| FD European call | .000489 | .000905 | .005 |
| FD American put | .001249 | .003671 | .005 |

Focused development timing validates the native sampling/report protocol;
concurrent development workloads may affect those measurements. Use an idle
machine for performance interpretation.
