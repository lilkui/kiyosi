# Complete binary and scheduled barrier parity

Type: task
Status: resolved
Blocked by: 01

## Problem

`AnalyticBinaryBarrierEngine` uses one survival-density approximation and
ignores scheduled dates. It does not implement DerivaSharp's complete
cash/asset and one-touch/no-touch/call/put case matrix. Scheduled vanilla
barrier finite differences round observation dates to coarse time layers.

## Work

- Add read-only `observation_interval()` to vanilla and binary barrier values.
  Return zero for continuous monitoring and compute scheduled intervals as
  Actual/365 Fixed from effective through the final observation divided by the
  number of observations.
- Validate schedule membership against the pricing context calendar.
- Implement the pinned DerivaSharp/Haug 28-case analytic binary-barrier matrix,
  terminal payoff, and BGK shift with beta `0.5825971579390107`.
- Restrict at-hit binary settlement to knock-in one-touch cases.
- For asset-at-hit, require payout equal to the barrier settlement level. For
  asset-at-expiry, settle in the underlying and do not scale by payout.
- Use inclusive barrier-hit comparisons and strict call/put strike comparisons.
- Treat a current spot across the effective barrier as already touched;
  otherwise valuation is conditional on no unrepresented prior touch.
- Preserve the BGK approximation for scheduled vanilla barriers and use the
  stored average observation interval rather than `time / count`.
- Place finite-difference scheduled barrier observations on exact event-aware
  grid nodes; do not round or merge contractual events.
- Cover scheduled in/out behavior, at-hit/at-expiry rebates, terminal cases,
  and refinement for analytic and finite-difference barrier engines.
- Replace every touched barrier fixture row with a reviewed pinned reference.

## Acceptance

- All 28 continuous binary reference cases from
  `BinaryBarrierOptionTestData.ValueData` pass at explicit tolerances.
- Scheduled binary cases demonstrably change with observation interval and
  match the pinned BGK convention.
- Invalid at-hit combinations and contradictory asset-at-hit payout/barrier
  terms fail through `std::expected`.
- Expiry settlement covers cash/asset, in/out, null/call/put, and equality
  boundaries.
- Already-touched values short-circuit to their contractual result without
  unstable logarithms.
- Scheduled vanilla analytic and finite-difference cases validate trading days,
  process exact dates, cover rebate timing, and show convergence.
- Focused tests and the full CTest target pass in the Visual Studio Developer
  environment.

## Comments

## Answer

Implemented the pinned 28-case Haug binary-barrier matrix, terminal and already-touched settlement, contractual at-hit validation, shared BGK scheduled intervals, calendar validation, and exact event-aware finite-difference observation nodes. Updated barrier fixtures to pinned DerivaSharp references; focused regressions and all 50 CTest cases pass in the Visual Studio Developer environment.
