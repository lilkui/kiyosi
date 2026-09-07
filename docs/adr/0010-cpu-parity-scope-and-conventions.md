# Define CPU parity scope and conventions

## Decision

Kiyosi covers every concrete DerivaSharp CPU instrument and pricing engine in
the single-underlying Black–Scholes–Merton domain. Each vertical slice has
translated parity fixtures, validation checks, and explicit numerical
tolerances. The production core is standard-library-only; Monte Carlo uses
CPU standard-library random generation. The project targets Windows and Linux
with C++23 and CMake, and uses `kiyosi` for the library and public namespace.
Behavioral parity is pinned to DerivaSharp revision
`08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`; source, API, inheritance, and
random-number-stream compatibility are not required.
CTest runs the Catch2 test target and parity fixtures use the documented
line-oriented TSV format.

Parity follows these conventions:

- scheduled continuous-barrier cases use the engine's documented
  Broadie–Glasserman–Kou barrier shift; exact discrete-monitoring parity
  requires a dedicated discrete barrier solver;
- exercise-based options, Asian options, and barrier options carry explicit
  effective and expiry dates, and valuation is admissible on their inclusive
  life interval;
- trading calendars select contractual observation dates, while average
  observation intervals, coupon accrual, and discounting use Actual/365 Fixed;
- structured product valuations include the supplied principal ratio, apply
  each observation event once, and process an expiry observation before
  terminal settlement;
- finite-difference engine settings identify the actual asset grid, time grid,
  and scheme used; unstable explicit grids fail rather than being silently
  refined;
- deterministic and discretized reference fixtures record their DerivaSharp
  source and revision; Monte Carlo parity uses reviewed statistical tolerances
  without requiring identical cross-language random streams;
- CUDA, accelerator-specific implementations, and source-level DerivaSharp
  compatibility are out of scope.

## Consequences

Integral, Asian, accumulator, and autocallable products are supported CPU
families. Numerical parity is judged through public prices, validation
properties, convergence, and seeded statistical tolerances. Additional
accelerators or discrete-monitoring methods can be added without changing the
value model.
