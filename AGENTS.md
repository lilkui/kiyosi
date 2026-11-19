# AGENTS.md

Kiyosi is a Rust derivatives pricing library with Python as its primary user-facing interface. A first-class Python API is a core goal. The project is in alpha with no backward-compatibility commitments; update callers directly rather than keeping compatibility shims.

## Repository guidance

- **Domain terms and ADRs**: read [docs/agents/domain.md](docs/agents/domain.md) before exploring.
- **Issues, specs, and wayfinding**: read [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md) for `.scratch/` workflows; for triage, also read [docs/agents/triage-labels.md](docs/agents/triage-labels.md).
- **Benchmarks and reference calibration**: read [benchmarks/quantlib/README.md](benchmarks/quantlib/README.md) for the QuantLib C++ harness and accuracy gates.

## Workspace and Python API

- **Standalone Rust core (`crates/kiyosi`)**: Cargo's `default-members` targets `crates/kiyosi` alone; pass `-p kiyosi-python` or `--workspace` to include the PyO3 bindings and `benchmarks/quantlib/rust`. Keep `crates/kiyosi` independent of Python and PyO3 so Cargo alone can build, test, document, and package it, with the `crates/kiyosi/THIRD_PARTY_NOTICES.txt` symlink intact.
- **Cross-language contract**: Keep domain semantics and validation in `crates/kiyosi`; bindings adapt Python representations, `snake_case` choice strings, and keyword-oriented calling conventions. Interfaces and coverage may differ between Rust and Python, while equivalent operations must agree on defaults, accepted domain inputs and boundaries, validation timing, error categories, and failure-state guarantees.
- **Python API sync**: When adding or changing Python APIs, update all coupled surfaces together:
  1. Rust bindings in `crates/kiyosi-python/src/` (`pricing_classes!` in `registration.rs` for instruments and engines, `signatures.rs` and `_STATE_FIELDS` / `value_methods!` for Rust-derived defaults and `replace`).
  2. Static signatures and overloads in `python/src/kiyosi/_native.pyi` (and typing cases in `python/tests/typing/` when applicable).
  3. Public re-exports and `Literal`/`TypedDict` aliases in `python/src/kiyosi/`.
  4. Stub docstrings: rebuild `_native`, run `tools/sync_python_docstrings.py`, and format `python/src/kiyosi/_native.pyi` with Ruff.

## Validation and errors

- **Boundary conversions**: Route Python inputs through `crates/kiyosi-python/src/conversions.rs`, reporting representation and protocol failures with native Python `TypeError`, `ValueError`, or `OverflowError`. Reject implicit or lossy conversions, including `bool` as number, fractional-to-integer narrowing, inexact non-`float` reals, and out-of-range or overflowing values.
- **Domain validation and structured errors**: Validate domain invariants in `crates/kiyosi` before committing state, and reuse that validation for binding prechecks. Return domain rejections as `kiyosi::Error` variants mapped directly in `crates/kiyosi-python/src/error.rs` to `KiyosiError` subclasses without parsing messages.
- **Error categories**: Report representable but unsupported instrument/model/engine combinations as `UnsupportedOperation` (including mismatched Kiyosi instruments or engines at binding dispatch), absent required prior state as `MissingHistory`, and recoverable operational or cancellation failures through `Error::Io` (`OSError` in Python), documented separately from panics and process termination.

## Immutability, concurrency, and cancellation

- **Immutable values**: Python-exposed objects are immutable (`replace` returns a new validated instance while leaving the original unchanged; collection properties return owned tuples; lazy `ObservationSchedule` iterators retain their owner). Rust values own their data and are `Send + Sync`, while Rust accessors may borrow.
- **Concurrency and GIL**: Preserve the concurrency contract documented in `crates/kiyosi/src/lib.rs` and `python/src/kiyosi/__init__.py`, documenting any type- or operation-specific exceptions. Parallel Monte Carlo uses Kiyosi's dedicated Rayon pool rather than the global pool. Detach the GIL only for pure core work that touches no Python objects or GIL-protected state, keeping short bounded calls inline and running long requests through `cancellation::run` so main-thread signals are polled.
- **Resource bounds and cancellation**: Bound workspace allocations and grid or tree rollback work upfront, and check `CancellationToken` between work units across schedule searches, grid construction, and pricing loops.

## Workflow, testing, and performance

- **Local build-and-test loop**: Rebuild the editable extension (`maturin develop`, adding `--release` for heavy suites or timing) after Rust or binding edits before running `pytest` or `tools/sync_python_docstrings.py`. Pass `--release` to Cargo for compute-heavy Monte Carlo, finite-difference, or convergence suites. Run `python/tests/test_package.py` and `python/tests/test_api_consistency.py` when Python exports, signatures, defaults, `replace`, or docstrings change.
- **Test scope and parity**: Add tests only when they meaningfully extend behavioral or contract coverage. Test shared scenarios in both `crates/kiyosi/tests/` and `python/tests/`, requiring agreement within numerical acceptance criteria.
- **Numerical and performance verification**: Validate numerical results with explicit tolerances and statistical criteria that allow for floating-point rounding and random-sequence variation, using analytic identities, `python/tests/quantlib_reference/` fixtures, and convergence checks as applicable. Preserve pricing accuracy, numerical stability, validation, resource bounds, and safety contracts when optimizing, and verify performance changes with correctness tests and relevant benchmarks.

## Documentation

- Rust doc comments must follow all applicable rules in [RFC 1574](https://raw.githubusercontent.com/rust-lang/rfcs/refs/heads/main/text/1574-more-api-documentation-conventions.md) and the [Rust API Guidelines documentation chapter](https://rust-lang.github.io/api-guidelines/documentation.html).
- Python docstrings must follow NumPy style.
- Keep `README.md` (Simplified Chinese) and `README.en.md` (English) synchronized when user-facing product, model, or algorithm coverage changes.
- All documentation must describe current behavior directly.

## Completion checklist

- **Rust**: Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`. Run `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` when Rust docs change, and check `PYO3_PYTHON=/nonexistent/python cargo check` when core dependencies or build boundaries change.
- **Python**: Run `ruff check python examples/python tools`, `ruff format --check python examples/python tools`, and `ty check --extra-search-path python/src --error unused-ignore-comment python/src python/tests/typing`.
- **Commits**: Use Conventional Commits.
