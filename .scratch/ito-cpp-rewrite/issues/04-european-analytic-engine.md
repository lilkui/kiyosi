Type: task
Status: ready-for-agent
Blocked by: 02, 03

# European analytic pricing vertical slice

## Goal

Implement the first public pricing seam for European calls and puts: explicit analytic engine, value and deterministic Greeks, implied-volatility solving, and result/error propagation.

## Depends on

Domain values and validated factories; dates, calendars, and context.

## Acceptance criteria

- European call and put prices follow the BSM convention for positive time and at expiry.
- The engine returns all agreed deterministic Greeks with documented units.
- Implied volatility uses a validated bracket and reports convergence failure through `std::expected`.
- Public calls expose engine selection explicitly and do not require a registry or base class.
- Numerical edge cases are handled without NaN-as-error signaling.

## Test

Use reviewed analytic reference values, put-call parity, expiry behavior, call/put symmetry, Greek identities where applicable, and solver success/failure cases.
