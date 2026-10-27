# Rust European pricing and analytics candidate

Tickets 01–05 provide an isolated candidate for European vanilla and digital prices and analytics with
flat BSM rates or configured risk-free and dividend term structures, trading
calendars and observation schedules. The root
`pyproject.toml`, CMake delivery, and legacy Python package remain usable. The
candidate also installs as `kiyosi`: use separate virtual environments or
processes. There is no runtime implementation selector.

The workspace version in `Cargo.toml` supplies both crates and Python's
`__version__` and wheel metadata. It currently matches the legacy version.
The workspace defaults to the core; ordinary `cargo build`, `cargo test`, and
`cargo doc` never build PyO3 or discover Python.

## Rust API

Requires Rust 1.85 or newer. A downstream crate depends on
`kiyosi-core = { path = "/path/to/kiyosi/rust/core" }`.

```rust
use kiyosi_core::{
    AnalyticVanillaEngine, BlackScholesMertonParameters, Date, EuropeanOption, OptionType,
    PricingContext,
};

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
derived from the same core value. Other exotic instruments and engines arrive in later tickets.
The vanilla engine accepts only `EuropeanOption`: unsupported instrument types are
excluded by Rust's signature and Python's native argument conversion.

## Greeks and implied volatility

Rust `AnalyticVanillaEngine::price_with_greeks` accepts a `GreeksRequest` and
`NumericalShiftSettings`. `GreeksRequest::new([Greek::Delta, Greek::Vega])`
selects measures, ignoring duplicates; `GreeksRequest::all()` selects all ten.
Empty requests return `InvalidParameter` before pricing. Every `PricingResult`
owns a finite price and optional finite Greeks. `get` returns `Option<f64>`,
`has` reports availability, and `require` returns `InvalidResult` for unavailable
measures. `values()` borrows immutable storage for the result's lifetime.
Unrequested or undefined measures are absent, including every Greek at expiry.
A zero derivative is an available value.

The analytic engine computes all ten Greeks directly. Delta, gamma and speed
are first, second and third spot derivatives. Vega, vanna and zomma are changes
in price, delta and gamma per volatility percentage point (0.01); rho is price
change per interest-rate percentage point, shifting the full risk-free curve in
parallel. Theta, charm and color are changes in price, delta and gamma per
calendar day of forward valuation time. Curve references and nodes stay fixed;
time derivatives use right-hand instantaneous rates at knots. Requested
non-finite derivatives fail the operation; unrequested derivatives cannot spoil
an otherwise finite result. Tail density weights are applied in log space.

The core exposes `numerical_greeks` for scalar implementations of
`EuropeanPricingEngine`. Absolute bump defaults are spot 0.01, volatility
0.0001, rate 0.0001 and time one calendar day. All must be positive (and numeric
bumps finite), even when the analytic engine does not use them. Numerical
stencils use representable symmetric spacing; collapsed, asymmetric or
nonpositive stencils leave affected measures absent. Time stencils stay within
contract life, curve coverage and neighboring knots, using one-sided differences
at boundaries. Feasible bumped pricing failures fail the whole operation.
Finite bumps introduce truncation and rounding error; compare step sizes,
especially near expiry. Python exposes the same shift validation contract;
the vanilla analytic engine always returns analytic derivatives.

```rust
use kiyosi_core::{AnalyticVanillaEngine, BlackScholesMertonParameters, Date,
    EuropeanOption, OptionType, PricingContext, Greek, GreeksRequest,
    NumericalShiftSettings, ImpliedVolatilitySettings, implied_volatility};
fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    let result = AnalyticVanillaEngine.price_with_greeks(&option, &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]), NumericalShiftSettings::default())?;
    assert!(result.require(Greek::Delta)? > 0.0);
    let sigma = implied_volatility(&AnalyticVanillaEngine, &option, &context,
        result.price(), ImpliedVolatilitySettings::default())?;
    assert!((sigma - 0.2).abs() < 1e-8);
    Ok(())
}
```

`implied_volatility` reuses the scalar engine boundary and safeguarded Brent
interpolation with bisection fallback. Defaults are bounds `[0.0001, 4.0]`,
absolute price tolerance `1e-8`, absolute volatility-interval tolerance `1e-8`
and 100 iterations, excluding endpoint evaluations. Either tolerance can stop
iteration. Bounds must be finite, positive and ordered, tolerances finite and
positive, and the iteration limit positive. Observed prices must be finite;
finite unattainable quotes, including negative prices, return
`UnbracketedVolatility`. Settings and quote validation precede endpoint pricing;
pricing validates the contract and curve horizon before expiry returns
`UnsupportedOperation` because volatility is unidentifiable. Endpoint matches
return their bounds. An exhausted solve returns `SolverNonConvergence`;
non-finite scalar prices or residuals return `SolverNonFinite`. Engine domain
failures propagate, including `InvalidResult` from analytic pricing. Analysis
and solving preserve the original market and engine on success and failure.

Python exports the `Greek` Literal choice alias and owning `PricingResult` at
package level. Results have read-only Greek properties and `require(name)`.
Request one lowercase name, an iterable of names, or `all_greeks=True`,
exclusively. Missing requests, wrong protocols and conflicting forms raise
native `TypeError`; unknown names raise `ValueError`; empty iterables reach core
validation. Numeric controls use the strict real-number protocol below.
`time_shift_days` and `max_iterations` use the integer index protocol, excluding
booleans and floats, with native `OverflowError` outside signed 32-bit range.
Representation errors precede domain validation. Defaults come from Rust.

```python
from kiyosi.pricing import AnalyticVanillaEngine, implied_volatility

engine = AnalyticVanillaEngine()
result = engine.price_with_greeks(option, context, ["delta", "vega"])
assert result.gamma is None
sigma = implied_volatility(engine, option, context, result.price)
assert abs(sigma - context.model_parameters.volatility) < 1e-8
```

## Digital contracts and analytic pricing

Rust `DigitalOption::cash_or_nothing(direction, strike, payout, start, end)` and
`DigitalOption::asset_or_nothing(direction, strike, start, end)` own validated
European contracts. Cash payout is required, finite and strictly positive; invalid
payout returns `InvalidParameter` before strike and date validation. Strike and
life dates follow the vanilla contract rules. `payout()` returns `Some(amount)`
for cash and `None` for asset settlement. Both settle only **strictly** in the
money: a call needs spot greater than strike, a put needs spot less than strike,
and equality at expiry pays zero. Expiry is midnight UTC; all Greeks are absent
at expiry, while implied volatility returns `UnsupportedOperation`.

`AnalyticDigitalEngine` accepts only `DigitalOption`. It computes price, delta
and gamma analytically, applying discounts, settlement amounts and normal-tail
weights in log space when necessary. Non-finite prices or requested derivatives
return `InvalidResult`; unrequested derivatives cannot spoil finite results.
Other requested Greeks use the shared finite-difference completion and the same
Rust `NumericalShiftSettings` defaults as vanilla numerical analysis. Speed
differences native gamma; vanna/zomma and charm/color use native delta/gamma when
both stencil legs provide the measure. If either lacks it, both legs use scalar
differences, including time bumps that reach expiry. A speed stencil needs only positive symmetric ±h spots when native
gamma is available. Pure scalar `numerical_greeks` still requires ±2h for speed.
Collapsed or infeasible stencils leave affected measures absent. Bumped pricing
errors fail the operation. Finite-difference accuracy depends on the bump and
distance to expiry; the one-day QuantLib scenarios validate price and native
delta/gamma under the existing acceptance criteria.

`EuropeanContract` exposes inclusive life dates without changing payoff semantics.
`EuropeanPricingEngine<I>` selects supported instrument types, defaulting to
`EuropeanOption`; the shared `numerical_greeks` and `implied_volatility` also
accept digital contracts. Digital prices can be non-monotone in volatility.
The solver retains the caller's bracket, may select one of several roots, and
returns `UnbracketedVolatility` when the endpoint residuals have the same sign,
even if the quote is attainable elsewhere. No root search or new algorithm is added.

```rust
use kiyosi_core::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
    DigitalOption, DigitalOptionChanges, OptionType, PricingContext,
    Greek, GreeksRequest, ImpliedVolatilitySettings, implied_volatility};

fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
        start, Date::new(2026, 1, 1)?)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    let result = AnalyticDigitalEngine.price_with_greeks(&option, &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]), Default::default())?;
    assert!((result.price() - 4.945810910532236).abs() < 1e-12);
    let sigma = implied_volatility(&AnalyticDigitalEngine, &option, &context,
        result.price(), ImpliedVolatilitySettings {
            lower_bound: 0.1, upper_bound: 0.22, ..Default::default()
        })?;
    assert!((sigma - 0.2).abs() < 1e-7);
    let updated = option.replace(DigitalOptionChanges {
        payout: Some(7.0), ..Default::default()
    })?;
    assert_eq!(updated.payout(), Some(7.0));
    assert_eq!(option.payout(), Some(10.0));
    Ok(())
}
```

Python exports `CashOrNothingOption` and `AssetOrNothingOption` from
`kiyosi.instruments`, and `AnalyticDigitalEngine` from `kiyosi.pricing`.
Constructors retain the existing keyword-only arguments; cash also requires
`payout`. Numeric and date representations use the strict protocols described
below. Analytic price and Greek calls retain the GIL; implied-volatility solves
release it using owned Rust inputs.

```python
from datetime import date
from kiyosi.instruments import CashOrNothingOption, AssetOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticDigitalEngine, implied_volatility

terms = dict(option_type="call", strike=100,
             effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
cash = CashOrNothingOption(**terms, payout=10)
asset = AssetOrNothingOption(**terms)
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = AnalyticDigitalEngine()
result = engine.price_with_greeks(cash, context, ["delta", "vega"])
assert abs(result.price - 4.945810910532236) < 1e-12
assert abs(engine.price(asset, context) - 58.6851146134764) < 1e-12
sigma = implied_volatility(engine, cash, context, result.price,
                           lower_bound=0.1, upper_bound=0.22)
assert abs(sigma - 0.2) < 1e-7
assert cash.replace(payout=7).payout == 7
assert cash.payout == 10
```

Rust `DigitalOption::replace(DigitalOptionChanges)` and Python `.replace(**fields)`
return owning validated contracts; failure leaves originals intact. Omitted fields
retain their values. Settlement kind cannot change. Rust rejects a cash payout
replacement on asset settlement with `UnsupportedOperation`; Python's asset
contract has no payout parameter and rejects it with native `TypeError`.
Digital contract replacement is a candidate interface; the legacy digital API
provides construction only. Python properties are read-only and equality compares
contract values. Vanilla/digital engine mismatches use Rust type checking or
Python's native argument-conversion errors.

## Term structures and replacement

`RateTermStructure::flat(rate, reference)` creates a finite continuous annual
rate. `None` covers the full supported timestamp range; `Some(reference)` sets an
inclusive lower bound with no upper bound. `RateTermStructure::new(reference,
nodes, factors)` requires equally sized, nonempty arrays, strictly increasing
nodes after the reference, and positive finite factors. An implicit factor of one
at the reference anchors log-linear interpolation. Factors may increase when
rates are negative. Node curves have inclusive coverage from reference through
last node and never extrapolate.

`factor(start, end)` and `log_factor(start, end)` require ordered, covered
intervals. Equal endpoints return one and zero. Underflow of a factor to zero is
allowed; non-finite query results return `InvalidResult`. `forward_rate(time)`
uses the right-hand interval at knots and the left-hand interval at the last node.
`shifted(shift)` adds a finite parallel continuous-rate spread and returns an owning
curve with the same coverage. `nodes()` borrows stable storage for the curve's
lifetime; `factors()` returns an owned vector including shifts. As in the legacy
API, shifted factors can underflow or overflow even when forward rates remain
finite; interval queries still validate their numerical results.

Both model rate arguments accept `f64` or `RateTermStructure` independently.
`risk_free_curve()` and `dividend_curve()` borrow the configured curves.
The Rust scalar accessors `risk_free_rate()` and `dividend_yield()` now return
`Result<f64>`: a node curve returns `UnsupportedOperation`, even when its nodes
describe a constant rate. Python keeps its existing scalar-or-curve properties;
explicit flat curves return a scalar from these properties and an owning curve
from the `*_curve` properties.

```rust
use kiyosi_core::{
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, Date, PricingContext,
    PricingContextChanges, RateTermStructure,
};
fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let curve = RateTermStructure::new(start, vec![end], vec![(-0.05_f64).exp()])?;
    let model = BlackScholesMertonParameters::new(curve, 0.02, 0.2)?;
    let bumped = model.replace(BlackScholesMertonParameterChanges {
        risk_free_rate: Some(model.risk_free_curve().shifted(0.01)?.into()),
        ..Default::default()
    })?;
    let context = PricingContext::new(model, 100.0, start)?;
    let updated = context.replace(PricingContextChanges {
        model_parameters: Some(bumped),
        spot_price: Some(105.0),
        ..Default::default()
    })?;
    assert_eq!(context.spot_price(), 100.0);
    assert_eq!(updated.spot_price(), 105.0);
    Ok(())
}
```

`Rate::Flat` and `Rate::Curve` support mixed rate replacements; `From<f64>` and
`From<RateTermStructure>` let callers use `.into()`. Absent change fields retain
their values. Model replacement reuses constructor validation; context replacement
reuses context validation, including spot and valuation coverage in both curves.
All replacements return new owned values, and failures leave originals intact.
A model need not have overlapping curve coverage; context creation requires a
covered valuation. Pricing separately validates contract life and coverage through
expiry in both curves, including dated flats. Unresolvable knots after conversion
to floating-point pricing time return `InvalidResult`; timestamp queries still
preserve microsecond intervals by subtracting timestamps before conversion.
The analytic engine consumes both integrated curve rates over valuation to expiry.

## Calendars and schedules

`TradingCalendar::all_days()`, `::weekdays()` (also `::default()`) and `::sse()`
use annualization bases 365, 252 and 243 respectively. `is_trading_day` queries
a validated `Date`. `trading_days_between` counts the half-open interval
`[start, end)`; `trading_year_fraction` divides that count by the calendar's basis.
Equal endpoints return zero; reversed intervals return `InvalidTimeRange`.
`adjust(date, BusinessDayConvention::Following / Preceding)` includes the nominal
date and returns `InvalidDate` if no trading date exists before the supported
date boundary. Contracts themselves never adjust dates implicitly.

`TradingCalendar::custom(base, closed, open, basis)` owns its base and date vectors,
sorts and deduplicates overrides, and rejects overlapping open/closed dates with
`InvalidCalendar`. Weekend opening overrides are allowed. `None` inherits the
base's annualization basis; overrides never recalculate it. Nonpositive bases
are rejected at construction. Clones retain owning lifetimes and immutable data.
`TradingCalendar::new(predicate, basis)` retains Rust-only custom predicates:
closures must be `Send + Sync + 'static`, deterministic throughout each operation,
and independent of query order or count. User predicate panics remain native
panics. Built-ins compare by kind; predicate calendars compare by shared predicate
identity. `PricingContext::calendar()` now borrows; clone it to retain an owning value.

`ObservationSchedule::fixed_interval(start, end, interval_days, &calendar)`
generates `start + n * interval_days` for positive `n`.
`::monthly(start, end, first_observation_month, &calendar)` generates monthly
candidates from the positive first-month offset, anchoring to the start day and
clamping to each target month's last day. Both exclude start, include end as an
upper bound, use following adjustment, merge duplicate adjusted dates, and stop
before crossing end. End need not appear; empty schedules are valid. Reversed
life is checked before nonpositive intervals/offsets (`InvalidSchedule`).
`::new(dates, start, end, &calendar)` first validates life, then strict ordering
and inclusive bounds (`InvalidSchedule`), then trading-day status (`InvalidDate`).
`dates()` borrows the schedule's immutable storage for its lifetime; newly built
schedules own their data. Rust fixed intervals accept `i64` calendar days;
monthly offsets and annualization bases use `i32`.

```rust
use kiyosi_core::{Date, ObservationSchedule, TradingCalendar};
fn main() -> kiyosi_core::Result<()> {
    let calendar = TradingCalendar::default();
    let schedule = ObservationSchedule::fixed_interval(
        Date::new(2025, 1, 3)?, Date::new(2025, 1, 7)?, 1, &calendar,
    )?;
    assert_eq!(schedule.dates(), &[Date::new(2025, 1, 6)?, Date::new(2025, 1, 7)?]);
    Ok(())
}
```

Python exports the same calendar factories as `all_days_calendar()`,
`weekdays_calendar()`, `sse_calendar()` and keyword-only `custom_calendar(base=...,
closed_dates=(), open_dates=(), trading_days_per_year=None)`. Overrides accept
date iterables and are copied; `None` is valid only for the optional basis.
No Python predicate callbacks are exposed. Adjustment choices are the strings
`"following"` and `"preceding"`. Dates exclude datetimes, using the same stored
component conversion as contract dates. Basis, interval and month arguments accept
`numbers.Integral` with the integer index protocol, excluding bool, floats and strings, with native
`OverflowError` outside the signed 32-bit range. Representation errors occur
before core domain validation. Result conversion beyond Python's year 1–9999
range preserves Python's native date error.

```python
from datetime import date
from kiyosi.market import custom_calendar, fixed_interval_schedule, weekdays_calendar

calendar = custom_calendar(
    base=weekdays_calendar(), open_dates=[date(2026, 10, 10)],
)
schedule = fixed_interval_schedule(
    start=date(2026, 10, 9), end=date(2026, 10, 11), interval_days=1,
    calendar=calendar,
)
assert schedule.dates == [date(2026, 10, 10)]
assert calendar.trading_days_per_year == 252
```

`monthly_schedule` takes named `start`, `end`, `first_observation_month`, and an
optional calendar. Both Python builders default to Rust's default calendar.
Schedules expose read-only owning `.dates` copies, `len`, integer/negative
indexing, equality and independent lazy iterators retaining the schedule owner.
Invalid indices raise `IndexError`. Python has no direct schedule constructor.
See [SSE calendar maintenance](sse-calendar.md) for forecast limitations,
upstream sources, generator commands and licensing.

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

Python exposes the same curve operations and supports date/datetime iterables for
nodes and `numbers.Real` iterables for factors. Iterables are consumed and copied;
returned node/factor lists and curves own their data. Dates mean midnight UTC;
datetime normalization uses the same strict rules as valuation time. For example:

```python
from datetime import date
from kiyosi.market import RateTermStructure

curve = RateTermStructure(
    reference_time=date(2025, 1, 1),
    times=[date(2025, 6, 30), date(2026, 1, 1)],
    factors=[0.99, 0.95],
)
curved_model = model.replace(risk_free_rate=curve)
curved_context = context.replace(model_parameters=curved_model, spot_price=105)
assert context.spot_price == 100
price = AnalyticVanillaEngine().price(option, curved_context)
```

Python `.replace()` methods accept only named fields from their constructors.
Omitted fields retain their values; explicit `None` is rejected. Numeric booleans,
strings, overflow and invalid datetime representations use native conversion
exceptions before core domain validation. All properties remain read-only.

## Errors and concurrency

| Failure | Rust | Python |
| --- | --- | --- |
| Invalid domain input or non-finite price | `Err(Error { category, message })` | `KiyosiError.category`, an `ErrorCategory` member |
| Wrong representation, protocol or signature | Excluded by types, or checked date constructor | Native `TypeError`, `ValueError`, `OverflowError`; user conversion exceptions propagate |
| Allocation exhaustion | Native allocator behavior, normally process abort | Python allocations may raise `MemoryError`; Rust allocator exhaustion may terminate the process |
| Internal bug/panic | Native panic behavior | PyO3 `PanicException` if unwinding reaches the binding; aborts still terminate the process |

Categories implemented here are `INVALID_STRIKE` (2), `INVALID_VOLATILITY` (3),
`INVALID_RISK_FREE_RATE` (4), `INVALID_DIVIDEND_YIELD` (5), `INVALID_SPOT_PRICE` (6),
`INVALID_DATE` (7), `INVALID_TIME_RANGE` (8), `INVALID_RESULT` (9),
`INVALID_SCHEDULE` (10), `INVALID_CALENDAR` (11), `INVALID_PARAMETER` (12),
`UNBRACKETED_VOLATILITY` (13), `SOLVER_NON_CONVERGENCE` (14),
`SOLVER_NON_FINITE` (15), and `UNSUPPORTED_OPERATION` (19).
Malformed curves and non-finite flat rates or
shifts use `INVALID_PARAMETER`; scalar model rate validation retains its
field-specific categories.
These identifiers and numbers match the legacy categories. Messages are diagnostic
text and must not be parsed. Resource failures and panics are not domain rejections.

The crate-level rustdoc states the default concurrency contract: all current
values are `Send + Sync`, distinct and shared instances permit concurrent reads
and pricing, and Rust mutation requires exclusive access. Python wrappers are
immutable. Short analytic prices and Greek formulas retain the GIL, as in the
legacy binding. Solves release it around core work with owned Rust inputs and
no Python callbacks or Python state. User-supplied Rust engines must uphold
their own concurrency contract. Calls are synchronous; there is no cancellation API or core signal
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
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_european.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_market.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_calendars.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_analytics.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_digital.py'
.venv/legacy/bin/python -m unittest discover -s tests/python -p 'test_*.py'
```

The shared market suite migrates the applicable legacy curve and replacement
contracts, adds independent live QuantLib curve pricing and parity, and checks
coverage timing, representation errors, ownership and concurrent pricing. The
Rust public API tests cover equivalent market scenarios. Other product/engine
cases remain in the legacy suite.

The shared analytics suite checks all 20 European scenarios against live QuantLib
Greeks and scalar price differences at two spot bump sizes, using existing error
budgets and independently checked oracle uncertainty. It covers volatility
recovery, mixed requests, availability, validation order, strict representations,
expiry, curve knots, ownership and concurrent solves. Rust public API tests cover
equivalent scenarios plus reusable numerical analysis, collapsed and infeasible
stencils, feasible bump failures, non-finite solver evaluations and
high-precision extreme-scale derivative regressions.

The shared digital suite covers all 40 existing digital QuantLib scenarios with
the existing native and numerical error budgets, and scalar-price differences at
two spot bumps away from expiry. It migrates strict terminal strike boundaries,
valuation timing, construction, request/shift validation, solver errors, curve
coverage and knots, concurrent reads, large-scale speed, tiny-volatility gamma,
normal-tail delta and discount/settlement overflow and underflow regressions.
Rust public API tests exercise equivalent contracts and reusable scalar numerical
analysis. Candidate-only digital tests verify owning replacement and failure state;
the legacy implementation has no digital contract replacement method.

The shared calendar suite reuses existing custom-calendar contracts and checks
counting, adjustment, schedule endpoints, month clamping, large offsets,
conversion errors, ownership, iteration and concurrent reads. It checks every
date in the SSE data range against the generated Rust table in both isolated
implementations. Generator tests cover the upstream forecast boundary and Rust
rendering. Calendar data and its attribution ship in the standalone Cargo crate
and candidate Python artifacts; Python is needed only to regenerate the data.

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
is part of these tickets.
