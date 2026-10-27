# Rust European analytic candidate

Ticket 01 adds an isolated candidate for European call/put prices with flat BSM
parameters. The root `pyproject.toml`, CMake delivery, and legacy Python package
remain usable. The candidate also installs as `kiyosi`: use separate virtual
environments or processes. There is no runtime implementation selector.

The workspace version in `Cargo.toml` supplies both crates and Python's
`__version__` and wheel metadata. It currently matches the legacy version.
The workspace defaults to the core; ordinary `cargo build`, `cargo test`, and
`cargo doc` never build PyO3 or discover Python.

## Rust API

Requires Rust 1.85 or newer. A downstream crate depends on
`kiyosi-core = { path = "/path/to/kiyosi/rust/core" }`.

```rust
use kiyosi_core::{AnalyticVanillaEngine, BlackScholesMertonParameters, Date,
                  EuropeanOption, OptionType, PricingContext};

fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let expiry = Date::new(2026, 1, 1)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    let context = PricingContext::new(model, 100.0, start.into())?;
    let price = AnalyticVanillaEngine.price(&option, &context)?;
    assert!((price - 9.227005508154036).abs() < 1e-12);
    Ok(())
}
```

`Date::new` validates Gregorian components and the existing core year range
(-32767 through 32767). `Timestamp::new` adds validated intraday components with
microsecond resolution. Time to expiry uses Actual/365 Fixed. A context is valid
independently of a contract: pricing checks that valuation lies between the
effective date at midnight and expiry at midnight, inclusive. At expiry the price
is intrinsic value; even one microsecond after expiry is rejected.

Strike, spot, and volatility must be finite and strictly positive; both rates
must be finite and may be negative. Constructors validate before returning a
value. Accessors borrow or copy from owned data. `PricingContext::new` uses
`TradingCalendar::default()` (weekdays, annualization 252); the Python default is
derived from the same core value. This slice exposes only that calendar identity
and annualization property. Calendar operations, other calendars, term structures,
Greeks, implied volatility, and other instruments/engines arrive in later tickets.
The engine accepts only `EuropeanOption`: unsupported instrument types are
excluded by Rust's signature and Python's native argument conversion.

## Python candidate installation and usage

Use a dedicated environment with CPython 3.11–3.15 and a Rust toolchain:

```sh
python -m venv .venv/candidate
.venv/candidate/bin/python -m pip install './rust/python[test]'
.venv/candidate/bin/python -m unittest discover -s rust/python/tests -p 'test_*.py'
```

On Windows use `.venv\candidate\Scripts\python.exe` in place of the POSIX paths.
The [PyO3 guide](https://pyo3.rs/v0.30.1/) and
[maturin layout guide](https://www.maturin.rs/project_layout.html) describe the
binding and packaging tools. Wheels use each CPython version's ordinary ABI;
ABI3 and free-threaded builds are outside this migration's delivery contract.

```python
from datetime import date
from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

option = EuropeanOption(
    option_type="call", strike=100,
    effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
)
model = BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
)
context = PricingContext(
    model_parameters=model, spot_price=100, valuation_time=date(2025, 1, 1),
)
assert abs(AnalyticVanillaEngine().price(option, context) - 9.227005508154036) < 1e-12
```

Constructors require keyword arguments. Choices are `"call"` and `"put"`.
Numeric fields accept `numbers.Real` (including integers) and reject booleans,
strings and complex values. Conversion errors propagate, including integer to
float overflow. Contract dates accept `datetime.date`, excluding datetimes.
Valuation accepts dates (midnight UTC) or timezone-aware datetimes normalized to
UTC. Naive datetimes are rejected. Date subclasses use stored components and
built-in UTC conversion. UTC normalization outside Python's year 1–9999 range
raises its native conversion error. No floating-point epoch conversion is used.
All exposed properties are read-only; returned model parameters, calendar and
date/time values are owned and remain valid after their source is released.
The installed package includes `py.typed` and native API stubs.

## Errors and concurrency

| Failure | Rust | Python |
| --- | --- | --- |
| Invalid domain input or non-finite price | `Err(Error { category, message })` | `KiyosiError.category`, an `ErrorCategory` member |
| Wrong representation, protocol or signature | Excluded by types, or checked date constructor | Native `TypeError`, `ValueError`, `OverflowError`; user conversion exceptions propagate |
| Allocation exhaustion | Native allocator behavior, normally process abort | Python allocations may raise `MemoryError`; Rust allocator exhaustion may terminate the process |
| Internal bug/panic | Native panic behavior | PyO3 `PanicException` if unwinding reaches the binding; aborts still terminate the process |

Categories implemented here are `INVALID_STRIKE` (2), `INVALID_VOLATILITY` (3),
`INVALID_RISK_FREE_RATE` (4), `INVALID_DIVIDEND_YIELD` (5), `INVALID_SPOT_PRICE` (6),
`INVALID_DATE` (7), `INVALID_TIME_RANGE` (8), and `INVALID_RESULT` (9).
These identifiers and numbers match the legacy categories. Messages are diagnostic
text and must not be parsed. Resource failures and panics are not domain rejections.

The crate-level rustdoc states the default concurrency contract: all current
values are `Send + Sync`, distinct and shared instances permit concurrent reads
and pricing, and Rust mutation requires exclusive access. Python wrappers are
immutable. The short scalar analytic formula retains the GIL, as in the legacy
binding. Calls are synchronous; there is no cancellation API or core signal
polling. Python signal handling (including `KeyboardInterrupt` on the main thread)
waits until control returns to Python and is not mapped to `KiyosiError`.

## Checks

From the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cargo doc --no-deps
cargo package -p kiyosi-core
PYO3_PYTHON=/nonexistent/python cargo run --manifest-path tests/rust-consumer/Cargo.toml
ruff check rust/python
ruff format --check rust/python
ty check rust/python/python
maturin build --manifest-path rust/python/Cargo.toml --interpreter .venv/candidate/bin/python
maturin sdist --manifest-path rust/python/Cargo.toml
```

Install the built wheel into the candidate environment before running its tests.
Also rebuild a wheel from the sdist in a temporary directory to check that the
workspace and core path dependency are included. `cargo package --allow-dirty`
is useful while developing; it still verifies the packaged core independently.
For core-only build/test/doc/package checks, setting `PYO3_PYTHON` to a nonexistent
path demonstrates that Python discovery is not involved. The separate consumer
has its own workspace and uses only the public crate API.

Install the unchanged root `.[test]` into `.venv/legacy` separately, then run:

```sh
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p test_european.py
.venv/legacy/bin/python -m unittest discover -s tests/python -p 'test_*.py'
```

The shared European suite covers the same domain and representation cases in
both processes. It reuses all 20 existing European scenarios and the independent
live QuantLib 1.43 oracle, restoring QuantLib settings after each scenario.
Additional Rust and Python cases retain high-precision tiny-volatility/tail
regressions, extreme scales, validation timing, expiry boundaries, owning values,
and shared/distinct-instance concurrency. Candidate-only tests check installed
metadata, exports, category values and stub members. Tests require QuantLib;
missing reference dependencies fail discovery.

Windows and all CPython versions require separate build/test runs; a local Linux
run does not establish that delivery matrix. No release or package publication
is part of ticket 01.
