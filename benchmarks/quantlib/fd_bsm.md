# BSM finite differences

`cases/fd_bsm.json` covers European and American vanilla, cash and asset digital,
continuous single barrier, cash and asset terminal binary barrier, cash and asset
no-touch at expiry, and cash and asset one-touch at hit. Barriers are initially
untouched; effective time equals valuation. Double barriers, double touches and
one-touch at expiry have no verified generic native FD counterpart and are
excluded. There are no custom QuantLib payoffs or proxy portfolios.

Both native adapters use the same contracts and BSM model. Digital payoffs use
`CashOrNothingPayoff` / `AssetOrNothingPayoff` inside native vanilla or barrier
instruments. Touches use a positive payoff strike equal to the barrier:

| Contract | Native barrier | Binary payoff orientation | Rebate |
| --- | --- | --- | --- |
| Down no-touch | DownOut | Call | 0 |
| Up no-touch | UpOut | Put | 0 |
| Down one-touch at hit | DownOut | Put | cash payout or barrier for asset |
| Up one-touch at hit | UpOut | Call | cash payout or barrier for asset |

A surviving down path ends above the barrier; an up survivor ends below it.
The opposite binary is zero on survival, so the rebate supplies the at-hit
payment. Continuous asset settlement at hit equals the barrier. The adapters
reject already touched barriers and the unsupported settlement combinations.
The tests compare the actual cash/asset terminal and touch prices with
independent absorbing-density and first-passage formulas.

The common absolute budget **0.003** was declared before selecting grids. It
is substantially tighter than the existing broad FD regression budgets. Rust
uses 800 uniform asset intervals, 400 time intervals, explicit upper boundary
400 and Rannacher damping. QuantLib uses 800 log-space mesher nodes,
Crank–Nicolson, two damping steps and its automatic domain. Node counts and
Rannacher/damping arithmetic differ. Native QuantLib FD also retrieves
sensitivities; its additional API work remains in each timed call.

For the up asset no-touch, QuantLib uses 16000 time steps. Its initial 400-step
error was about 0.0696; time doubling reduces this first-order terminal-boundary
error, whereas spatial refinement alone does not fix it. The up cash one-touch
uses 1600 steps (initial 400-step error about 0.00589). The selected asset
no-touch error is about 0.00177 and cash hit error about 0.00148. These measured
failures and refinements are retained in calibration evidence, rather than
relaxing their budgets. All other selected grids use 400 time steps.

`generate_fd_references.py` reuses independent standard-library Decimal BSM,
absorbing log-density and first-passage formulas at 90/110 digit precision.
Their declared absolute uncertainty is 1e-12. The American put reuses
`calibration/american.json`: QuantLib 1.44 space refinement through 3200 nodes,
time through 25600, and independent Rust space/time/domain refinement through
6400/25600 with domain enlargement at fixed spacing. First-order time
extrapolates are 6.660684545893456 (QuantLib) and 6.660682089388095 (Rust).
The 5e-5 numerical uncertainty conservatively covers the observed differences;
it is an empirical allowance, not a mathematical error bound. This shared
reference also supports American CRR and Monte Carlo validation.

`calibrate_fd.py` records actual prices and errors with selected profiles,
independently doubled space and time, and Rust upper-bound enlargement from
400 to 600 at fixed spacing. QuantLib's public native engines expose no explicit
boundary control; automatic meshing is disclosed and damping sensitivity is
recorded. Discontinuous payoffs and knock-in subtraction can show nonmonotone
mesh errors. Every selected profile is gated against its independent reference;
matching work counts or agreement between the two prices is insufficient.

Run from the repository root:

```sh
python3 benchmarks/quantlib/generate_fd_references.py
python3 benchmarks/quantlib/run.py --cases benchmarks/quantlib/cases/fd_bsm.json --references benchmarks/quantlib/references/fd_bsm.json --price-only
python3 benchmarks/quantlib/calibrate_fd.py
python3 benchmarks/quantlib/run.py --cases benchmarks/quantlib/cases/fd_bsm.json --references benchmarks/quantlib/references/fd_bsm.json --report /tmp/fd-bsm.json
KIYOSI_QUANTLIB_LIVE=1 python3 -m unittest benchmarks.quantlib.test_fd
```

Reference generation and calibration run outside ordinary timing. The reusable
Rust `fd::prepare_product` and C++ `fd_product` accept a constructed context or
process so the supplied local-volatility adapter uses the same contract logic.

Linux development validation with installed QuantLib 1.44 completed all 11 cases
with 15 paired samples in 59.07 seconds including builds. The machine also ran
other calibration work, so this establishes protocol/report operation; the
complete-suite validation supplies the clean performance measurement. Raw report:
`/tmp/fd-bsm-15pairs.json` (local artifact, not a versioned performance claim).
