# 10: Align shared analytics conventions

**What to build:** Cross-engine behavioral parity for supported Greeks, implied values, scenario grids, validation, and expiry boundaries using the documented reference conventions and explicit absence for unsupported measures.

**Blocked by:** 02: Close vanilla, digital, and Asian parity matrix; 03: Close barrier and binary-barrier case matrix; 06: Complete structured reference and statistical matrix; 09: Implement Bjerksund–Stensland 2002 parity

**Status:** ready-for-agent

- [ ] Supported risk measures compare across applicable deterministic engines with deterministic tolerances.
- [ ] Monte Carlo perturbation checks use common random numbers and documented statistical budgets.
- [ ] Bump sizes, speed stencil, theta direction, and scenario-grid semantics match the documented conventions.
- [ ] Implied volatility and implied coupon validation and expiry boundaries are behaviorally tested.
- [ ] Unsupported Greeks and measures remain explicitly absent or failed and are never synthesized.
