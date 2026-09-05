Type: task
Status: ready-for-agent
Blocked by: 03, 05

# Binomial American pricing

## Goal

Implement an explicit binomial engine for American calls and puts as the first early-exercise numerical cross-check.

## Depends on

Dates, calendars, context, and parity fixture harness.

## Acceptance criteria

- American option values support early exercise under the BSM assumptions.
- Step count and numerical parameters are validated.
- The engine uses the shared result/error boundary and documents which Greeks are analytic or numerical.
- European limiting cases can be compared against the analytic engine.

## Test

Cover early-exercise cases, European-limit convergence, invalid step counts, expiry, call/put symmetry, and fixture tolerance behavior.
