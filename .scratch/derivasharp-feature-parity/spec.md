# DerivaSharp CPU feature parity

Status: accepted

## Objective

Close Kiyosi's remaining CPU behavioral gaps against DerivaSharp revision
`08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2` while preserving Kiyosi's
value-oriented C++23 API and `std::expected` error model. Source names,
inheritance, ABI, and random-number streams are not compatibility targets.

Delivery proceeds in five independently verifiable vertical slices. Each slice
replaces any parity rows it touches with reviewed reference results; the final
slice audits and closes the complete fixture set.

## Scope

- Explicit effective dates across exercise-based, digital, Asian, vanilla
  barrier, and binary-barrier instruments.
- DerivaSharp-compatible schedule factories and SSE calendar data.
- The complete analytic binary-barrier case matrix and consistent scheduled
  barrier handling.
- Product-level structured settlement behavior for accumulator, Phoenix,
  snowball, binary snowball, and ternary snowball instruments.
- Genuine scheme-aware and grid-aware CPU finite-difference engines for every
  advertised structured instrument.
- Executable, provenance-bearing reference fixtures for every concrete CPU
  instrument/pricing-engine pair.

CUDA, accelerators, public numerical helpers, source/API compatibility, and an
exact discrete-monitoring barrier solver remain out of scope.

## Contract decisions

### Instrument life

All affected factories require explicit `effective, expiry` arguments; old
expiry-only and valuation-date overloads are removed. Effective and expiry
dates form an inclusive ordered instrument life. The effective date need not
be a trading day. Every pricing engine rejects valuation before effectiveness
or after expiry.

`OptionTerms` and `AsianOptionTerms` store and expose the effective date.
Asian averaging starts within the inclusive instrument life. Vanilla and
binary barrier instruments also store and expose effective dates.

### Schedules and calendars

Fixed-interval schedules start at `effective + interval`, adjust following
without passing expiry, de-duplicate targets that collapse onto one trading
date, and return an empty schedule when no target fits. Monthly schedules take
a positive `lock_up_months`, begin at `effective + lock_up_months`, continue
monthly, and return empty when the lock-up exceeds the term. Following
adjustment searches through expiry; the old `following_bound` option is
removed.

SSE calendar behavior is pinned to DerivaSharp's 1991--2030 holiday table and
243 trading days per year. Outside the holiday table's range, weekdays remain
trading days and weekends remain closed; the supported holiday-data range is
documented rather than enforced as an error boundary.

Observation ordering and life bounds are validated at construction. Trading
day membership is validated during pricing against the supplied
`TradingCalendar`.

### Scheduled barriers

Scheduled monitoring uses the documented Broadie--Glasserman--Kou shifted
continuous-barrier approximation. Exact discrete monitoring requires a future
dedicated pricing engine.

Both barrier instrument types expose `observation_interval()`. Continuous
monitoring reports zero. Scheduled monitoring reports Actual/365 Fixed from
effective date through the final observation, divided by observation count.
The same value drives analytic vanilla and binary-barrier BGK adjustment.

Mid-life barrier valuation is conditional on no prior touch that is not
represented by the instrument. A current spot already across the effective
barrier is treated as touched and short-circuits to the contractual knock-in or
knock-out result.

### Binary barriers

The analytic engine implements the 28 DerivaSharp/Haug cases spanning cash or
asset settlement, four barrier directions, one-touch/no-touch/call/put strike
conditions, and settlement timing. Terminal settlement is evaluated directly
at expiry.

At-hit settlement is legal only for knock-in one-touch instruments. Cash
settlement pays the configured payout. Asset-at-hit requires the configured
payout to equal the barrier settlement level. Asset-at-expiry pays the
underlying itself; its payout field does not scale the settlement.

Barrier hit comparisons are inclusive. Call/put strike conditions are strict,
matching DerivaSharp terminal behavior.

### Structured instruments

All five concrete structured constructors become private; `make_*` factories
are the only construction path. Immutable coupon-copy operations return
`result<T>` and validate through those factories. Numeric defects return
`invalid_parameter`; invalid or unordered life/schedule dates return
`invalid_schedule`.

Coupon, maturity-coupon, and minimal-coupon rates may be any finite signed
value. Principal, barriers, schedules, and other existing non-negative terms
retain their contractual restrictions.

Structured valuations are principal-inclusive according to `principal_ratio`.
An already up-touched autocallable is worth zero. A down-touch persists as
known knock-in state. Knock-in uses strict `<`; knock-out uses `>=`.

Each observation event is applied once. Dates before or equal to valuation are
treated as resolved and are not replayed, except that valuation at expiry
processes an expiry observation before terminal settlement. Final knock-out
therefore takes precedence over the non-knock-out maturity branch.

Phoenix observation coupons are fixed cash amounts equal to
`initial_price * coupon_rate`. Snowball, binary-snowball, and ternary-snowball
event coupons are annualized rates accrued Actual/365 Fixed from effective
date through the event. Discounting uses Actual/365 Fixed from valuation to
payment. Calendars select observation dates; they do not define monetary day
count.

Structured Monte Carlo continues to simulate every future trading day. Its
public settings remain path count and optional seed; any fixture step count is
derived provenance, not an engine control.

### Structured finite differences

Every advertised structured finite-difference engine becomes a genuine PDE
engine and returns price only. The implementation reuses Kiyosi's existing BSM
operator and tridiagonal solve, adding small product-specific terminal and
observation-transition helpers rather than a generic product state-machine
framework.

`asset_steps`, `time_steps`, `scheme`, and `upper_boundary` all affect the
computation. Time steps specify minimum resolution. Mandatory trading and
observation dates are exact grid nodes and may add nodes; this event-aware
refinement is contractual, not a hidden stability adjustment. An explicit
Euler request whose resulting maximum time step is unstable fails through
`std::expected` rather than silently changing resolution or scheme.

Structured grids use the supplied trading calendar to place daily state and
observation transitions, while elapsed PDE time, coupon accrual, and
discounting use Actual/365 Fixed. An explicit upper boundary must exceed spot,
initial price, relevant strikes, and barrier levels. The default is derived
from those same product levels.

At zero maturity, the expiry observation is processed first and the resulting
terminal settlement is returned. Past observation dates are never clamped to
or replayed on a future layer. All three finite-difference schemes must show
per-product refinement behavior at stable settings.

### Reference fixtures

`tests/fixtures/cpu_parity.tsv` remains the human-readable manifest. Typed,
family-specific C++ tests execute every concrete instrument/pricing-engine pair
and the required payoff, state, monitoring, settlement, and convergence
variants. Manifest shape alone is not closure.

Each checked-in reference records:

- DerivaSharp revision
  `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`;
- the source test or data symbol;
- its calendar/day-count convention;
- whether it is analytic, discretized, or statistical;
- explicit absolute or statistical tolerance.

A checked-in developer script outside the C++ build extracts or regenerates
reference data. CMake, the installed library, and normal C++ tests do not
depend on the sibling DerivaSharp checkout or a C# runtime.

Monte Carlo parity requires deterministic repeatability for a fixed Kiyosi
seed plus agreement with the pinned DerivaSharp reference mean within a
reviewed fixed tolerance. Cross-language random-number-stream identity is not
required.

## Delivery map

1. [Effective dates, schedules, and calendar](issues/01-effective-dates-schedules-calendar.md)
2. [Binary and scheduled barrier parity](issues/02-binary-and-scheduled-barriers.md)
3. [Structured state, settlement, and validation](issues/03-structured-state-settlement-validation.md)
4. [Structured finite-difference engines](issues/04-structured-finite-difference-engines.md)
5. [Executable reference-fixture closure](issues/05-executable-reference-fixtures.md)

The first slice is unblocked. Barrier and structured-state work can proceed in
parallel after it. Structured finite differences depend on the structured
settlement contract; final fixture closure depends on all implementation
slices.

## Verification

Use the Visual Studio Developer environment before invoking MSVC or CMake on
Windows. Each slice must configure, build, and run the focused Catch2 tests,
then the complete CTest target. No slice may retain placeholder expected
values for an engine it changes.
