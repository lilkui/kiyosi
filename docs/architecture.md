# Repository layout and responsibilities

Kiyosi uses a Cargo workspace for Rust and a separate Python source directory for
the maturin mixed project. The repository root owns version metadata, build
configuration and CI; Python distributions and Cargo packages share that version.

```text
.
├── Cargo.toml / Cargo.lock       # Workspace metadata and Rust dependency lock
├── pyproject.toml               # Python metadata, maturin and wheel test config
├── crates/
│   ├── kiyosi-core/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs           # Public Rust exports and concurrency contract
│   │   │   ├── instruments/     # Contract terms, history, payoff and validation
│   │   │   ├── engines/         # Implementations grouped by numerical method
│   │   │   │   ├── analytic/    # Closed forms and analytic approximations
│   │   │   │   ├── binomial/
│   │   │   │   ├── finite_difference/
│   │   │   │   ├── monte_carlo/
│   │   │   │   └── quadrature/
│   │   │   ├── risk/            # Pricing interface, Greeks and inverse solvers
│   │   │   ├── market/          # BSM parameters, curves and pricing context
│   │   │   ├── time/            # Dates, calendars, schedules and holiday data
│   │   │   └── error.rs         # Structured errors shared by the core
│   │   └── tests/               # Rust public API integration tests
│   └── kiyosi-python/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs           # PyO3 extension entry point
│           ├── registration.rs  # Extension exports
│           ├── instruments/     # Python contract representations and factories
│           ├── engines/         # Analytic and numerical engine adapters
│           ├── risk/            # Python requests, results and solver dispatch
│           ├── market.rs        # Python parameters, curves and pricing context
│           ├── calendar.rs      # Python calendars and factories
│           ├── schedule.rs      # Python schedules and owned iterators
│           ├── conversions.rs   # Native Python representation checks
│           └── error.rs         # Python exception mapping
├── python/
│   └── kiyosi/                  # Python imports, choice aliases and typing
├── tests/
│   ├── python/                  # Installed Python package and tool tests
│   │   ├── quantlib_reference/  # Independent oracles and reference data
│   │   └── support/             # Shared Python test support
│   └── rust-consumer/           # Independent downstream Cargo consumer
├── tools/                       # Maintainer tools, including calendar generation
└── docs/                        # Architecture, data provenance and agent guides
```

## Dependency boundaries

`python/kiyosi` → `kiyosi._native` (`crates/kiyosi-python`) → `kiyosi-core`.
The core has no Python or PyO3 dependency. Workspace default commands select only
the core; the Python extension is selected explicitly or through maturin. Each
Rust crate keeps its own license and third-party notices for Cargo packaging.

The core owns domain defaults, validation, error categories, numerical algorithms
and concurrency guarantees. `lib.rs` re-exports the supported Rust interface;
implementation modules remain private. Rust callers continue to import types such
as `kiyosi_core::BarrierOption` and `kiyosi_core::MonteCarloAsianEngine`.

| Core module | Responsibility and dependencies |
| --- | --- |
| `instruments/` | Contract construction, replacement, history, monitoring and payoff rules; uses market/time values and the pricing interface, never concrete engines |
| `engines/` | Prices validated contracts using a selected method; owns method settings, workspaces, sampling and numerical helpers |
| `risk/` | Defines `PricingEngine`, requests/results, numerical Greeks and implied volatility/coupon solvers; works through the pricing interface |
| `market/` | Owns deterministic curves, BSM parameters and pricing context |
| `time/` | Owns dates, timestamps, calendars, schedules and generated SSE holiday data |
| `error.rs` | Stable structured domain rejections and recoverable operational failures |

Product definitions and analytic engines live in separate files. Asian Monte Carlo
belongs to `engines/monte_carlo/asian.rs`, alongside the other Monte Carlo engines.
Autocallable presets and shared event-state rules stay with the contract in
`instruments/autocallable/`: all pricing methods must apply those rules consistently.
Engine families may reuse existing analytic and Gaussian numerical helpers;
extract another module only when it clarifies an actual shared responsibility.

The binding crate mirrors the contract/engine/risk split. It adapts Python
representations and exceptions while domain validation remains in the core.
`lib.rs` delegates export registration to `registration.rs`; adapters use explicit
imports rather than inheriting a root prelude. Product adapters contain contract
classes and factories, while engine adapters own pricing entry points and GIL
release. Inverse-solver dispatch lives in `risk/`, rather than in product classes.
`conversions.rs` handles numeric, date/time, choice and keyword representation
checks; `error.rs` maps core errors to Python exceptions.

Binding helpers remain crate-private. Moving adapters does not change Python class
identities or import paths. Python modules expose `kiyosi.instruments`,
`kiyosi.market` and `kiyosi.pricing`; `_native.pyi` and `py.typed` ship alongside
them. Keep those modules as imports and typing, with business logic in Rust.

## Layout assessment and refactoring decisions

The workspace, two-crate dependency direction, separate `python/` source tree,
installed-package tests and Cargo-only core delivery already fit this project.
Keep those seams: more crates would add manifests and publication coordination
without creating a needed independent delivery unit.

The internal layout previously mixed contract definitions with analytic pricing,
placed Asian Monte Carlo under the Asian product, scattered quadrature/binomial
binding engines across product modules, and exposed common imports implicitly
through the extension root. The layout above separates those responsibilities and
makes imports show their dependencies. The registration module now owns extension
assembly, and inverse solvers no longer share files with contract representations.

Rust integration tests remain in the core crate for standalone Cargo packaging;
Python tests and independent QuantLib references remain outside the importable
package. Do not merge these suites merely to make their directories symmetrical.
No new runtime dependencies or public namespaces are needed for this refactoring.

## Ownership and concurrency

Public values own their data. Rust permits concurrent reads and pricing on shared
or distinct instances; mutation requires exclusive access. Python values expose
read-only properties, and replacement returns a new value. Schedule iterators keep
their Python owner alive. Custom Rust calendar predicates must uphold the core's
determinism contract, and custom pricing engines define their own operational and
thread-safety contracts; consult the core rustdoc for details.

Binding operations that release the GIL do so around core calculations with owned
Rust inputs. Representation checks and Python object access run with the GIL.
Short analytic calls may keep it. Calls are synchronous, without cancellation or
core signal polling; Python handles signals when control returns to Python.

## Development and delivery

From the repository root, core checks require Cargo alone:

```sh
cargo fmt --all --check
cargo clippy -p kiyosi-core --all-targets --locked -- -D warnings
cargo test --release --locked
cargo doc --no-deps --locked
cargo package -p kiyosi-core --locked
cargo run --release --locked --manifest-path tests/rust-consumer/Cargo.toml
```

With a supported Python interpreter available, also check the binding crate and
Python sources:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
uv run --no-project --with ruff -- ruff check python tests/python tools/generate_sse_calendar.py
uv run --no-project --with ruff -- ruff format --check python tests/python tools/generate_sse_calendar.py
uv run --no-project --with ty -- ty check python
```

The [test guide](../tests/README.md) describes installed-package tests and the
sdist-to-wheel acceptance check. Python tests stay outside the importable package
and exercise the installed extension. Their support code and QuantLib fixtures
travel with the source distribution. CI's wheel tests copy only the inputs listed
in `tool.cibuildwheel.test-sources`; update that list and maturin's sdist includes
when adding a test dependency outside the suite.

Calendar tools update committed Rust data; see
[calendar provenance and regeneration](sse-calendar.md).

The layout uses Cargo's [virtual workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html)
and maturin's [separate Python source directory](https://www.maturin.rs/project_layout.html).
