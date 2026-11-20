# AGENTS.md

Kiyosi is a Rust derivatives pricing library with a first-class Python API as its primary interface. Public APIs are unfrozen: refactor freely for a cleaner design and update all callers in place.

## Guides

- **Domain terms & ADRs**: before exploring, read [docs/agents/domain.md](docs/agents/domain.md).
- **Issues, specs & wayfinding (`.scratch/`)**: read [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md); when triaging, also read [docs/agents/triage-labels.md](docs/agents/triage-labels.md).
- **QuantLib benchmarks & calibration**: read [benchmarks/quantlib/README.md](benchmarks/quantlib/README.md).

## Workspace and Python API

- **Standalone core (`crates/kiyosi`)**: Cargo's `default-members` targets `crates/kiyosi` alone; pass `-p kiyosi-python` or `--workspace` to include the PyO3 bindings and `benchmarks/quantlib/rust`. Keep `crates/kiyosi` pure Rust (free of Python and PyO3) so Cargo alone builds, tests, documents, and packages it, preserving the `crates/kiyosi/THIRD_PARTY_NOTICES.txt` symlink.
- **Cross-language contract**: `crates/kiyosi` owns domain semantics, defaults, and validation; `crates/kiyosi-python` adapts Python types, `snake_case` choice strings, and keyword-oriented signatures. API shape and coverage may differ, while equivalent operations match on defaults, accepted inputs and boundaries, validation timing, error categories, and failure-state guarantees.
- **Python API sync**: Update all coupled surfaces together and verify parity:
  1. **Bindings** (`crates/kiyosi-python/src/`): `pricing_classes!` in `registration.rs` for instruments and engines; `value_methods!` (`_STATE_FIELDS` and `replace`) in `lib.rs` and `signatures.rs` for Rust-derived defaults.
  2. **Typing & exports**: static signatures and overloads in `python/src/kiyosi/_native.pyi` (plus `python/tests/typing/` cases) and public re-exports, `Literal`, and `TypedDict` aliases in `python/src/kiyosi/`.
  3. **Docstrings & parity**: run `maturin develop`, `tools/sync_python_docstrings.py`, and `ruff format python/src/kiyosi/_native.pyi`, then pass `pytest python/tests/test_package.py python/tests/test_api_consistency.py`.

## Validation and errors

- **Boundary conversions**: Route Python inputs through `crates/kiyosi-python/src/conversions.rs`. Raise native `TypeError`, `ValueError`, or `OverflowError` for representation and protocol failures, and reject implicit or lossy coercions (`bool` as number, fractional-to-integer narrowing, inexact non-`float` reals, out-of-range values).
- **Domain invariants**: Validate in `crates/kiyosi` before constructing or replacing state, reusing core validation for binding prechecks, and match `kiyosi::Error` variants directly onto `KiyosiError` subclasses in `crates/kiyosi-python/src/error.rs`.
- **Error categories**: Use `UnsupportedOperation` for unsupported instrument/model/engine combinations (including mismatched Kiyosi types at binding dispatch), `MissingHistory` for absent required prior state, and `Error::Io` (`OSError` in Python) for recoverable operational or cancellation failures (documented separately from panics and process termination).

## Immutability, concurrency, and cancellation

- **Immutable values**: Python objects are immutable (`replace` returns a new validated instance; collection properties return owned tuples; lazy `ObservationSchedule` iterators retain their owner). Rust values own their data and are `Send + Sync`; Rust accessors may borrow.
- **Concurrency and GIL**: Uphold the concurrency contract in `crates/kiyosi/src/lib.rs` and `python/src/kiyosi/__init__.py`, documenting any type- or operation-specific exceptions. Install parallel Monte Carlo work onto Kiyosi's dedicated Rayon pool. Detach the GIL only for pure core work free of Python objects and GIL state: keep short bounded calls inline and run long requests through `cancellation::run`.
- **Resource bounds and cancellation**: Bound workspace allocations and grid/tree rollback work upfront, and poll `CancellationToken` between work units across schedule searches, grid construction, and pricing loops.

## Testing and performance

- **Build loop**: Run `maturin develop` after Rust or binding edits before `pytest`; pass `--release` to `cargo` and `maturin` for timing and compute-heavy suites (Monte Carlo, finite difference, convergence).
- **Scope and parity**: Add tests only for new behavioral or contract coverage. Mirror shared scenarios in `crates/kiyosi/tests/` and `python/tests/` within numerical acceptance criteria.
- **Numerical and performance checks**: Assert numerical outputs with explicit tolerances and statistical bounds grounded in analytic identities, `python/tests/quantlib_reference/` fixtures, or convergence checks. Verify optimizations with `--release` benchmarks (`benchmarks/quantlib/` where covered) alongside correctness tests.

## Documentation

- **Rust doc comments**: follow every applicable rule in [RFC 1574](https://raw.githubusercontent.com/rust-lang/rfcs/refs/heads/main/text/1574-more-api-documentation-conventions.md) and the [Rust API Guidelines documentation chapter](https://rust-lang.github.io/api-guidelines/documentation.html).
- **Python docstrings**: use NumPy style.
- Describe current behavior directly in the present tense.

## Completion checks

- **Rust**: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`; add `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` when Rust docs change, and `PYO3_PYTHON=/nonexistent/python cargo check` when core dependencies or build boundaries change.
- **Python**: `ruff check python examples/python tools`, `ruff format --check python examples/python tools`, and `ty check --extra-search-path python/src --error unused-ignore-comment python/src python/tests/typing`.
- **Commits**: Conventional Commits.
