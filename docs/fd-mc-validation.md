# FD–MC pricing validation

The C++ regression suite compares finite-difference and Monte Carlo prices for
standard, binary, and ternary snowballs, phoenixes, and accumulators. It requires
neither QuantLib nor Python. These are numerical consistency checks, supplemented
by independently calculated settlement and limiting-payoff checks: the structured
engines share payoff rules, so agreement alone is not an independent payoff oracle.

## Running

On Windows, first activate the Visual Studio Developer environment. From the
repository root:

```powershell
cmake --preset windows-release
cmake --build --preset windows-release --target kiyosi_tests
ctest --preset windows-release -R "FD-MC|Finite-difference grids preserve"
```

The default suite includes five stochastic baseline comparisons and fast
known-payoff checks. The extended test is hidden from default Catch2 runs and
excluded from default CTest discovery. Run it explicitly:

```powershell
out/build/windows-release/tests/kiyosi_tests.exe "[extended]" --durations yes
```

To include it in CTest discovery, configure with
`cmake --preset windows-release -DKIYOSI_EXTENDED_VALIDATION=ON`, then run
`ctest --preset windows-release -R "FD-MC extended"`.
Set the option back to `OFF` to restore normal discovery. Linux uses the
`linux-release` preset and the executable without `.exe`.

## Scenarios and error budgets

The baseline uses a 91-day life, spot 100, volatility 25%, rate 4%, and dividend
yield 1%, with the same weekdays calendar and observation dates for both engines.
Autocall levels step from 112 to 108 to 104; snowball coupons also step down.
Extended cases cover spots near knock-in, coupon, and knock-out levels;
expiry-only knock-in at 35% volatility; and historical knock-in together with
nonzero accumulated quantity. Each case runs all five product families.

All budgets are absolute native price units, not percentages of the computed
price. This remains meaningful for negative and near-zero accumulator values.

| Product | FD budget | Maximum MC envelope, default | Maximum MC envelope, extended |
| --- | ---: | ---: | ---: |
| Snowball | 0.003 | 0.004 | 0.002 |
| Binary snowball | 0.0003 | 0.0003 | 0.00015 |
| Ternary snowball | 0.0005 | 0.0004 | 0.0002 |
| Phoenix | 0.008 | 0.008 | 0.004 |
| Accumulator | 2.0 | 8.0 | 4.0 |

Snowballs use principal ratio 1. Phoenix uses principal ratio 1 plus coupons
of `100 * 0.0025 = 0.25` at qualifying observations. Accumulator values are
quantity times underlying-price difference, with daily quantity 1 and
acceleration 2. The budgets deliberately differ by this scale and payoff shape;
they are regression acceptance budgets for these scenarios, not universal
accuracy guarantees for arbitrary contracts or default engine settings.

## Checks and diagnostics

- **FD refinement:** compare coarse, medium, and fine Crank–Nicolson grids.
  Default spatial counts are 300/600/1200 and time counts 240/480/960;
  extended counts are 400/800/1600 for both. The final change must fit the FD
  budget and must not exceed the preceding change, allowing a floor of 10% of
  the FD budget for small nonmonotone changes around discontinuities.
- **Domain truncation:** widen the asset domain from 300 to 450 (default) or
  375 (extended structured cases). Extended accumulators use 128 to 160 to
  resolve the current-day knock-out jump within the engine's 2000-node limit;
  daily knock-out terminates continuation above 110. Preserve fine-grid spacing
  and time steps in every domain comparison. The price change
  must be at most 10% of the FD budget.
- **MC precision:** use 12 fixed, distinct seeds. Each batch has 32,768 paths
  in the default suite, 131,072 in extended structured cases, and 524,288 in
  extended accumulator cases. For batch means `x[b]`, estimate the standard error
  of their overall mean as `sqrt(sum((x[b] - mean)^2) / (12 * 11))`.
  The four-standard-error envelope must fit its fixed MC budget independently
  of whether the engines agree. No retry-until-pass or seed selection occurs.
- **Agreement:** the fine FD price must lie within its FD budget plus the
  measured MC envelope of the MC mean. Its discrepancy must also be no worse
  than the coarse-grid discrepancy, allowing the same sampling envelope.
- **Independent anchors:** explicit expiry payoff tables cover strict knock-in,
  inclusive knock-out/coupon levels, capped losses, historical states, and
  accumulator quantities. Constant-path limits and fixed discounted cashflows
  exercise pre-expiry pricing. Focused regressions cover immediate and future
  accumulator settlement and counting the terminal coupon exactly once.

Failures capture the scenario, grids' prices, domain change, seeds, path count,
batch prices, standard error, discrepancy, and budgets. Catch2's `--success`
option also displays these diagnostics for passing comparisons. Seeded results
are reproducible within a standard-library implementation; normal generators
can differ across platforms. The statistical envelope is a regression guard,
not a simultaneous confidence guarantee or a bound on untested model error.

The suite exposed and now guards two core defects: accumulator knock-out
retaining continuation value, and a rounded FD time-grid endpoint beyond expiry
that replayed terminal coupons or purchases. Python regression cases exercise
the same public pricing behavior through the bindings.
