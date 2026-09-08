# 08: Align scheduled-barrier historical state

**What to build:** Scheduled barrier valuation that derives touch state from contractual observation dates, distinguishes valuation-date observations from non-observation crossings, and retains the established BGK approximation between observations.

**Blocked by:** 03: Close barrier and binary-barrier case matrix

**Status:** ready-for-agent

- [ ] A non-observation valuation-date spot crossing does not create historical touch for scheduled barriers.
- [ ] A contractual valuation-date observation creates touch exactly once when its condition is met.
- [ ] Analytic and finite-difference scheduled barrier engines agree on the public historical-state contract.
- [ ] Time-zero observation cases are no longer silently filtered out.
- [ ] Between observations, scheduled valuation retains the BGK approximation and its documented tolerance behavior.
