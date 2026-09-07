# Implement structured finite-difference engines

Type: task
Status: resolved
Blocked by: 03

## Problem

`price_finite_difference_structured` is a binomial recursion. It ignores
`asset_steps` and the selected scheme, rounds observations to layers, and
returns zero at maturity. The advertised engine names and settings therefore
do not describe the computation performed.

## Work

- Replace the recursion with genuine BSM finite-difference solvers for
  accumulator, Phoenix, snowball, binary snowball, and ternary snowball.
- Reuse the existing finite-difference operator and tridiagonal solve. Add
  product-specific terminal conditions and observation transitions through
  small helpers, not a general PDE framework.
- Honor `asset_steps`, `time_steps`, `scheme`, and `upper_boundary`.
- Interpret `time_steps` as minimum resolution. Build an event-aware time grid
  with every future trading and observation date as an exact node; add
  subdivisions where needed to meet the requested maximum step size.
- For explicit Euler, validate the maximum resulting step and reject unstable
  requests rather than altering scheme or stability-driven resolution.
- Use the supplied trading calendar for daily transition dates and Actual/365
  Fixed for grid time, coupon accrual, and discounting.
- Derive a default upper asset boundary from spot, initial price, strikes, and
  barriers. Reject an explicit boundary that does not exceed every relevant
  product level.
- Exclude resolved past events and process an expiry observation before the
  terminal branch.
- Implement product behavior from issue 03, including two-state knock-in
  valuation where required and accumulator accrual/knock-out behavior.
- Return price only.
- Replace all structured finite-difference fixture rows with reviewed pinned
  references.

## Acceptance

- Changing `asset_steps`, `time_steps`, or scheme changes the numerical solve;
  no setting remains decorative.
- All three schemes run at stable settings and return an error for an unstable
  explicit request.
- Every product has refinement checks for every scheme against a pinned or
  independently validated reference.
- Observation dates are exact grid nodes; distinct contractual dates are never
  rounded into one transition.
- Maturity valuation returns contractual terminal settlement, with a final
  observation applied first when present.
- Past observations do not appear on future grid layers.
- Upper-bound validation covers all relevant product levels.
- Focused tests and the full CTest target pass in the Visual Studio Developer
  environment.

## Comments

## Answer

Structured finite-difference pricing now uses event-aware BSM PDE grids with
explicit, implicit, and Crank–Nicolson schemes, exact future trading and
observation nodes, Actual/365 timing, product-specific terminal/state
transitions, upper-bound validation, and explicit-Euler stability rejection.
All five engines have public refinement coverage and reviewed fixture values.
