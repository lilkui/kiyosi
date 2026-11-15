# Analytic digitals and continuous Asians

`cases/digitals_asian.json` adds six fixed BSM scenarios: cash-call and asset-put
terminal digitals, geometric call/put, and arithmetic call/put. Each case records
its native QuantLib counterpart. Digital payoffs use `AnalyticEuropeanEngine`;
geometric averaging uses `AnalyticContinuousGeometricAveragePriceAsianEngine`;
arithmetic averaging uses `ContinuousArithmeticAsianLevyEngine`. QuantLib's
`TurnbullWakemanAsianEngine` is a discrete-average engine and does not represent
these contracts.

All cases share Actual/365 Fixed, continuous rates, positive spot, strike,
volatility and maturity, and curve reference dates equal to valuation. Asian
averaging is continuous from valuation to expiry with no history. Both native
adapters reject a different `averaging_start`. QuantLib's geometric option uses
the unseasoned constructor, whose omitted start means evaluation time; providing
an explicit date selects a seasoned path that its geometric engine rejects.
The arithmetic option supplies the inception date and a zero current-average
quote; elapsed averaging is zero so this quote contributes nothing.

The arithmetic benchmark requires `abs(r-q)`, `abs(r-q+sigma²)` and
`abs(2(r-q)+sigma²)` each at least `1e-6`. These conservative benchmark guards
exclude near-zero carry branching and singular literal Levy denominators. The
library itself supports a broader contract set; this benchmark compares only
the verified shared subset. Its first two moments and lognormal fit agree with
Kiyosi's Turnbull–Wakeman approximation. This comparison measures agreement on
that approximation rather than its error against the true arithmetic Asian
price.

`generate_digitals_asian_references.py` independently evaluates digital Gaussian
formulas, continuous geometric lognormal moments, and arithmetic first/second
moments using Python Decimal at 50 and 80 digits. The shared absolute budget is
`1e-9`; reference uncertainty is conservatively `1e-12`. Reference refinement
and native errors are recorded under `references/` and `calibration/`.
The live QuantLib 1.44 calibration has maximum absolute error `7.43e-12` and
Kiyosi `3.42e-14`, each before the reference uncertainty allowance. Analytic
engines have no numerical controls, so both profiles are explicitly empty.
Native QuantLib additional Greek work remains inside fresh public pricing.

Run only these cases, retaining the full report with raw samples and metadata:

```sh
python3 benchmarks/quantlib/run.py \
  --cases benchmarks/quantlib/cases/digitals_asian.json \
  --references benchmarks/quantlib/references/digitals_asian.json \
  --report /tmp/digitals-asian.json
python3 benchmarks/quantlib/generate_digitals_asian_references.py
KIYOSI_QUANTLIB_LIVE=1 python3 -m unittest discover \
  -s benchmarks/quantlib -p test_digitals_asian.py
```

The committed calibration completed all six cases with 15 pairs per case in
15.20 seconds including builds on the recorded machine. Timings reflect that
execution's machine load; they are evidence of execution, not universal speed
claims. The opt-in live tests require the installed C++ QuantLib prerequisite
and verify native prices, subset rejections, and failed accuracy suppressing
both timing and ratios. The ordinary test command runs Rust checks without
requiring QuantLib.
