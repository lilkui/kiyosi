# CPU parity conventions

## Context

The CPU parity closure uses translated DerivaSharp reference cases while the
value-oriented C++ API represents schedules and numerical methods directly.
Some reference conventions cannot be reproduced literally without expanding
the public API or adding a second pricing backend.

## Decision

- Scheduled continuous-barrier cases use the engine's documented
  Broadie-Glasserman-Kou barrier shift; exact discrete-monitoring parity is
  deferred until a dedicated discrete barrier solver is in scope.
- Structured products use the supplied `TradingCalendar` for observation and
  accrual dates; fixture tolerances therefore allow the calendar's annual
  trading-day convention instead of forcing Actual/365 on those paths.
- CUDA, accelerator-specific, and source-level DerivaSharp parity remain out
  of scope under ADR 0010.

These differences are recorded in the fixture notes and judged through public
prices, identities, convergence, and seeded statistical tolerances.
