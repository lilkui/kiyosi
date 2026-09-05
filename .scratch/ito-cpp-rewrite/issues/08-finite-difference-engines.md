Type: task
Status: ready-for-agent
Blocked by: 03, 05

# Finite-difference European and American engines

## Goal

Implement the first finite-difference engines for European and American vanilla options using the standard-library numerical core.

## Depends on

Dates, calendars, context, and parity fixture harness.

## Acceptance criteria

- Grid dimensions, scheme selection, and numerical parameters are validated.
- European and American boundary/terminal conditions are explicit and tested.
- The engine returns the shared result contract and uses method-specific tolerances.
- European finite-difference values cross-check analytic values over representative grids.
- American values cross-check the binomial engine where convergence permits.

## Test

Cover grid validation, expiry, boundary conditions, scheme behavior, convergence, and cross-engine parity fixtures.
