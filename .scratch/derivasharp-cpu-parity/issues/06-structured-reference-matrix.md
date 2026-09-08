# 06: Complete structured reference and statistical matrix

**What to build:** Direct parity coverage for Phoenix, Snowball, BinarySnowball, TernarySnowball, and Accumulator engines, including every expressible preset, touch state, expiry boundary, deterministic grid, and seeded Monte Carlo budget.

**Blocked by:** 05: Process structured valuation-date and expiry events

**Status:** ready-for-agent

- [ ] Every structured CPU engine family has direct public-boundary reference rows.
- [ ] Touch states, coupon and barrier presets, observation schedules, and expiry handling are represented by reconstructible inputs.
- [ ] Structured finite-difference outputs use the requested grid and reviewed deterministic tolerances.
- [ ] Structured Monte Carlo outputs use fixed seeds, path and step budgets, common random numbers for perturbation checks, and statistical acceptance intervals.
- [ ] Repeatability and state-transition properties remain supplementary rather than substituting for reference comparisons.
