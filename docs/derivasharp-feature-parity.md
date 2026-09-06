# DerivaSharp feature-parity backlog

This is the remaining CPU/product-surface work identified by comparing
`kiyosi` with `..\..\DerivaSharp`.  It is a behavioural backlog, not a
source/API-compatibility requirement: Kiyosi's value-oriented C++ boundary and
`std::expected` error model remain the contract.  CUDA acceleration and making
low-level numeric helpers public are intentionally excluded.

## High-priority numerical gaps

### 1. Binary-barrier parity is incomplete

DerivaSharp's `AnalyticBinaryBarrierEngine` implements the full cash/asset
one-touch, no-touch, knock-in and knock-out case matrix, including discrete
observation adjustment (`src/PricingEngines/Digital/AnalyticBinaryBarrierEngine.cs`).
Kiyosi's implementation (`src/pricing/engines/binary_barrier.cpp`) uses a
single survival-density approximation, does not apply the Broadie--Glasserman--
Kou adjustment for scheduled observations, and does not validate or consume
`observation_mode::scheduled` dates.  Add explicit parity cases for every
barrier direction, option-type-null one/no-touch form, payout settlement, and
scheduled monitoring; then implement the missing formulas/adjustment.

### 2. Scheduled monitoring must be covered consistently

The vanilla barrier implementation in `src/pricing/engines/digital_barrier.cpp`
already applies the documented BGK shift, but the binary-barrier path does not
consume scheduled dates (see item 1), and the finite-difference path maps dates
to a coarse time layer.  Add scheduled in/out, rebate-at-hit/expiry, and
convergence fixtures for both analytic and finite-difference barrier engines so
these deliberately approximate conventions remain measurable against
DerivaSharp's `AnalyticBarrierEngine`/`FdBarrierEngine`.

### 3. Structured finite-difference engines are not finite-difference engines

`src/pricing/engines/structured.cpp` implements
`price_finite_difference_structured` as a time-step binomial recursion.  The
`asset_steps` and `finite_difference_scheme` settings are ignored (only
`time_steps` affects the recursion), and it returns price only.  DerivaSharp's
`FdAccumulatorEngine`, `FdSnowballEngine`, `FdBinarySnowballEngine`,
`FdTernarySnowballEngine`, and `FdPhoenixEngine` solve their PDE grids and are
refined in tests (`src/PricingEngines/**/Fd*.cs`).  Implement scheme/grid-aware
CPU solvers or explicitly narrow the supported contract; in either case add
per-product convergence and payoff/settlement parity tests rather than relying
on the current placeholder prices in `tests/fixtures/cpu_parity.tsv`.  Its
`maturity == 0` fast path returns zero, whereas DerivaSharp returns the
contract's terminal payoff; this is an immediate correctness fix even before a
full PDE implementation.

### 4. Structured product state and settlement need product-level parity tests

The Monte Carlo and structured recursion paths in
`src/pricing/engines/structured.cpp` share one simplified state machine.  They
need DerivaSharp-derived tests for coupon accrual, knock-in observation
frequency, pre-touched barrier status, early redemption, loss floors/caps, and
final settlement for accumulator, Phoenix, snowball, binary-snowball, and
ternary-snowball contracts.  In particular, verify the maturity branch and
coupon-date discounting against the corresponding C# engines before claiming
parity.

## Domain and calendar gaps

### 5. Add effective dates to exercise-based options

DerivaSharp's `Option`/`VanillaOption` stores both effective and expiration
dates and pricing validates the valuation date against both
(`src/Instruments/Option.cs`, `src/PricingEngines/PricingEngine.cs`).
Kiyosi's `OptionTerms` in `include/kiyosi/instruments/vanilla.hpp` stores only
strike and expiry.  Add an effective-date term (or an equivalent validated
exercise schedule) and reject valuation before contract effectiveness.  Apply
the same rule consistently to digital and Asian factories.

### 6. Bring schedule factories in line with contractual semantics

DerivaSharp's `Schedule.CreateInterval` starts at `effective + interval` and
`CreateMonthly` starts after the requested lock-up months
(`src/Time/Schedule.cs`).  Kiyosi's `make_fixed_interval_schedule` and
`make_monthly_schedule` in `include/kiyosi/market/calendar.hpp` currently emit
the start date (month offset zero).  They also fail when two adjusted targets
collapse to one trading date, whereas DerivaSharp de-duplicates interval
results.  Fix the first target, de-duplication, and add following-adjustment
tests around weekends/holidays and the expiry boundary.

### 7. Complete SSE calendar data and convention

`include/kiyosi/market/calendar.hpp` has a short 2024--2026 holiday list and
reports 252 annual trading days.  DerivaSharp's `SseCalendar` contains the
published 1991--2030 holiday set and uses `TradingDaysPerYear = 243`
(`src/Time/SseCalendar.cs`).  Port the supported holiday range and 243-day
convention (or document a deliberate data-version choice), then add regression
tests for dates outside 2024--2026 and trading-year fractions.

### 8. Match observation-interval handling

DerivaSharp computes and stores an average observation interval for barrier and
binary-barrier instruments; this drives its discrete BGK correction.  Kiyosi
stores dates but has no interval value and several engines ignore scheduled
dates.  Centralize interval calculation from `TradingCalendar`/Actual-365 and
use it consistently in analytic binary barriers and scheduled vanilla barriers.

## Validation and API hardening

### 9. Prevent invalid structured values from bypassing factories

`Accumulator`, `PhoenixOption`, `SnowballOption`, `BinarySnowballOption`, and
`TernarySnowballOption` have public constructors in
`include/kiyosi/instruments/structured.hpp`; validation is only guaranteed by
the `make_*` helpers.  DerivaSharp constructors enforce argument and schedule
validation.  Make constructors private/protected or validate them at the
boundary so direct construction cannot bypass checks for non-finite terms,
schedule lengths, coupon rates, and barrier state.

### 10. Replace placeholder parity rows with DerivaSharp references

`tests/fixtures/cpu_parity.tsv` names every CPU engine, but many expected values
are rounded placeholders (for example `8.1`, `0.95`, or `7.4`).  Generate
reviewed rows from the DerivaSharp tests/engines for each concrete product and
engine, retaining explicit tolerances and seeded Monte Carlo metadata.  Keep
`tests/parity_fixture_tests.cpp` as the closure gate once the references are
real.

## Explicitly out of scope

- CUDA/TorchSharp or other accelerator implementations and packaging.
- Public exposure of DerivaSharp's internal numeric types (`BrentSolver`,
  quadrature, normal distributions, interpolation, tridiagonal matrix, etc.).
- Source-level C# names, inheritance, or ABI compatibility.
