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

For packaging acceptance, build an sdist with `uv build --sdist`, rebuild its wheel
using `uv build --wheel path/to/kiyosi-VERSION.tar.gz`, install it in a fresh
environment with the test extra, and run the Python suite above. CI repeats this
for Linux x86-64 and Windows AMD64 with ordinary CPython 3.11–3.15 wheels.

For the layout, dependency boundaries and quality checks, see the
[architecture guide](../docs/architecture.md).
