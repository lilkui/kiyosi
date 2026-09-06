# Define CPU parity scope and conventions

## Decision

Kiyosi covers every concrete DerivaSharp CPU instrument and pricing engine in
the single-underlying Black–Scholes–Merton domain. Each vertical slice has
translated parity fixtures, validation checks, and explicit numerical
tolerances. The production core is standard-library-only; Monte Carlo uses
CPU standard-library random generation. The project targets Windows and Linux
with C++23 and CMake, and uses `kiyosi` for the library and public namespace.
CTest runs the Catch2 test target and parity fixtures use the documented
line-oriented TSV format.

Parity follows these conventions:

- scheduled continuous-barrier cases use the engine's documented
  Broadie–Glasserman–Kou barrier shift; exact discrete-monitoring parity
  requires a dedicated discrete barrier solver;
- structured products use their supplied `TradingCalendar` for observation
  and accrual dates, so their tolerances follow the calendar's annual trading
  day convention rather than forcing Actual/365;
- CUDA, accelerator-specific implementations, and source-level DerivaSharp
  compatibility are out of scope.

## Consequences

Integral, Asian, accumulator, and autocallable products are supported CPU
families. Numerical parity is judged through public prices, validation
properties, convergence, and seeded statistical tolerances. Additional
accelerators or discrete-monitoring methods can be added without changing the
value model.
