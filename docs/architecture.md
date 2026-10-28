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
│   │   ├── src/                 # Domain types, validation and pricing engines
│   │   └── tests/               # Rust public API integration tests
│   └── kiyosi-python/
│       ├── Cargo.toml
│       └── src/                 # PyO3 representation and calling adapters
├── python/
│   └── kiyosi/                  # Python imports, choice aliases and typing
├── tests/
│   ├── python/                  # Installed Python package and tool tests
│   │   ├── quantlib_reference/  # Independent oracles and reference data
│   │   └── support/             # Shared Python test support
│   └── rust-consumer/           # Independent downstream Cargo consumer
├── benchmarks/                  # Explicit performance measurements
├── tools/                       # Maintainer tools, including calendar generation
└── docs/                        # Architecture, data provenance and agent guides
```

## Dependency boundaries

`python/kiyosi` → `kiyosi._native` (`crates/kiyosi-python`) → `kiyosi-core`.
The core has no Python or PyO3 dependency. Workspace default commands select only
the core; the Python extension is selected explicitly or through maturin. Each
Rust crate keeps its own license and third-party notices for Cargo packaging.

The core owns domain defaults, validation, error categories, numerical algorithms
and concurrency guarantees. Its private modules follow products and numerical
methods, with the supported Rust API re-exported from `src/lib.rs`. Engine
submodules hold method-specific implementations; public API integration tests
remain beside the core crate so Cargo can test and package it independently.

The binding crate adapts Python representations and exceptions. Its `lib.rs`
declares modules and registers the extension API. Shared boundary code lives in:

| Module | Responsibility |
| --- | --- |
| `conversions.rs` | Numeric, date/time, choice and keyword representation checks |
| `error.rs` | Map core domain and operational errors to Python exceptions |
| `market.rs` | Term structures, BSM parameters and pricing contexts |
| `calendar.rs` | Trading calendars and calendar factories |
| `schedule.rs` | Observation schedules, factories and owned iterators |
| `vanilla.rs` | European options and analytic vanilla pricing |

Other binding modules adapt their named products or numerical engines. Binding
helpers remain crate-private; moving code between modules does not change Python
class identities or import paths. Python modules expose the public namespaces
`kiyosi.instruments`, `kiyosi.market` and `kiyosi.pricing`. `_native.pyi` and
`py.typed` ship alongside them. Keep business logic in the core rather than adding
a second implementation in those Python modules.

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

Benchmarks run explicitly rather than as correctness gates. The Sobol benchmark
also supplies existing accuracy checks, so it is included in source distributions
and wheel test inputs. Calendar tools update committed Rust data; see
[calendar provenance and regeneration](sse-calendar.md).

The layout uses Cargo's [virtual workspace](https://doc.rust-lang.org/cargo/reference/workspaces.html)
and maturin's [separate Python source directory](https://www.maturin.rs/project_layout.html).
