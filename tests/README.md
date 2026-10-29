# Tests

The default delivery uses the Rust core and its Python bindings. Keep the installed
package in an isolated environment for artifact verification.

From the repository root:

```sh
cargo test --release --locked
python -m pip install '.[test]'
python -m unittest discover -s tests/python -p 'test_*.py'
```

`cargo test` uses only the core. The Python suite exercises the installed Rust
extension, including the generator tests. Independent references and shared
fixtures live alongside the suite in `tests/python/quantlib_reference` and
`tests/python/support`.

In an extracted Python sdist, use `cargo test -p kiyosi-core --release --locked`:
maturin removes workspace `default-members` when preparing that archive. Selecting
the core explicitly keeps build, test and documentation checks independent of Python.

The test extra pins QuantLib 1.43. It is required: missing QuantLib fails discovery
rather than skipping price validation. QuantLib is unnecessary for runtime use,
or for building, testing, documenting and packaging the standalone Rust core.

Independent price/Greek, scalar-bump and implied-volatility references and explicit
scenarios live in `tests/python/quantlib_reference`. Structured references use
basic-payoff portfolios. American references refine 800, 1600 and 3200 point
grids and check bump stability. Arithmetic Asian comparisons validate the same
Levy approximation, not the exact arithmetic-average value. Numerical acceptance
also covers event/history boundaries, grid/domain and shift convergence, multi-seed
Monte Carlo error, Sobol, antithetic paths and thread-count reproducibility.

The references use Actual/365 Fixed, continuously compounded BSM parameters,
`WeekendsOnly` matched to `weekdays_calendar`, and unrolled contract dates.
Reference uncertainty is checked independently of engine error budgets. QuantLib
changes a process-global evaluation date: tests restore settings with
`SavedSettings`. Run comparisons serially within a process, or isolate parallel
runs in separate processes.

Rust and Python public API tests cover equivalent defaults, validation timing,
stable errors, failed replacement state, event/history behavior, ownership and
concurrency. `test_support_matrix.py` checks terminal prices and Greek availability
for every CPU engine, unidentifiable volatility at expiry, and native rejection
of unsupported product/engine pairs.

Keep shared public-contract scenarios in both languages: defaults, accepted inputs,
event boundaries, validation order, error categories, failure state, ownership and
concurrency exercise different API surfaces. Python tests also own native protocol
and exception conversion, GIL release, installed exports and live QuantLib checks.
Similar names or calls alone do not establish redundancy.

Keep exhaustive numerical regressions in the Cargo-only core when Python repeats
the same inputs and oracle without adding binding coverage. The following core
tests own the numerical checks removed from Python; the corresponding Python
modules retain API scenarios and independent price/Greek comparisons:

| Core integration test | Numerical coverage |
| --- | --- |
| `accumulator.rs::spatial_and_domain_convergence_preserve_legacy_acceptance_targets` | The 16-scenario space/domain refinement matrix |
| `accumulator.rs::discounted_low_tail_preserves_large_finite_quantities` | Large discounted tail quantities and linear scaling |
| `american.rs::american_price_preserves_2002_regression` | Fixed call/put approximation prices |
| `american.rs::american_numerical_stability_regressions` | Small volatility, currency scaling and extreme tails |
| `american_monte_carlo.rs::sampling_combinations_preserve_threads_seeds_and_exact_odd_paths` | Sampling, seed, thread and odd-path matrix |
| `autocallable_finite_difference.rs::binary_domains_use_actual_levels_in_small_price_units` | Automatic and explicit domains at small price scales |
| `autocallable_monte_carlo.rs::discounted_cashflows_preserve_representable_values_beyond_discount_range` | Extreme discounts and cancelling cashflows |
| `barrier_quadrature.rs::continuous_quadrature_matches_analytic_for_all_payoffs_and_rebates` | Analytic price matrix across directions, barriers, rebates and rates |
| `double_barrier_monte_carlo.rs::pseudo_random_prices_and_likelihood_greeks_match_across_seeds` | Pseudo-random price/Greek matrix across seeds |
| `monte_carlo.rs::scaled_statistics_preserve_finite_prices_when_sums_squares_or_discounts_overflow` | Price and standard-error scaling at extreme amounts and discounts |
| `quadrature.rs::terminal_quadrature_matches_vanilla_cash_and_asset_references` | Fixed vanilla and digital call/put prices |
| `quadrature.rs::high_variance_negative_rates_and_small_widths_match_analytic_limits` | High variance, negative carry and tiny integration widths |
| `quadrature.rs::scaled_tails_and_underflowed_discounts_preserve_representable_payments` | Decimal tail references and extreme discounts |

Retain numerical tests with additional evidence even when they overlap: Python's
Monte Carlo statistical test also checks standardized errors and inverse-square-root
error scaling, and its sparse American-path test checks an interior optimal exercise
date. Independent references and distinct acceptance criteria are not duplicates.

For packaging acceptance, build an sdist with `uv build --sdist`, rebuild its wheel
using `uv build --wheel path/to/kiyosi-VERSION.tar.gz`, install it in a fresh
environment with the test extra, and run the Python suite above. CI repeats this
for Linux x86-64 and Windows AMD64 with ordinary CPython 3.11–3.15 wheels.

For the layout, dependency boundaries and quality checks, see the
[architecture guide](../docs/architecture.md).
