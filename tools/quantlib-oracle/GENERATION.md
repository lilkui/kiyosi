# Reference generation

From the repository root:

```sh
uv run --frozen --project tools/quantlib-oracle python tools/quantlib-oracle/generate.py
uv run --frozen --project tools/quantlib-oracle python tools/quantlib-oracle/check_generation.py
```

QuantLib 1.43 is pinned in the existing uv environment. Generation computes each contract reference once, then attaches the target engine settings, native and numerical error budgets, and wrapper coverage selection. All rows remain in `tests/fixtures/pricing_reference.tsv`; the C++ tests consume it without Python or QuantLib.

`oracle.py` provides shared QuantLib calculations and fixture encoding. Product modules supply contract construction, stencil settings and unavailable-measure declarations explicitly. QuantLib references use `WeekendsOnly`; Kiyosi validation uses `weekdays_calendar`. Contract dates are not rolled. Each row records `calendar=weekends_only`. `generate.py` assembles and validates the manifest, then replaces it atomically. None of these modules uses Kiyosi prices or production defaults to calculate reference values.

The seven TSV columns are `case_id`, `instrument`, `engine`, `variant`, `inputs`, `outputs`, and `tolerances`. `engine` names the tested engine; `source_symbol` identifies the QuantLib reference engine. Monte Carlo seed, paths, and steps are recorded once in `inputs`; the price comparison budget is in `tolerances`. These describe the tested engine, not a statistical reference estimator.

Reference stability limits are independent of engine error budgets. Analytic references compare native Greeks with price differences and two bump sizes; American references also compare refined finite-difference grids. The recorded uncertainty is empirical, not a rigorous bound. Arithmetic Asian references measure agreement with the same Levy approximation, not error against the true arithmetic-average value.

Scenario JSON and product row builders own explicit test configurations. Native budgets cover analytic roundoff, quadrature, grid error, approximation error or sampling error as applicable. Numerical budgets additionally allow finite-stencil truncation and interpolation error. The `wrapper` field selects scenarios exercising Kiyosi's numerical sensitivities; it does not affect reference calculation. Changing a budget or coverage selection must not change the underlying reference or its stability metadata.
