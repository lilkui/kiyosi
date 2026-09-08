# 04: Enforce explicit finite-difference grid semantics

**What to build:** Finite-difference engines honor the caller-selected explicit Euler grid or reject it as unstable, matching the pinned stability rule and exposing the same behavior on vanilla, digital, and vanilla-barrier public paths.

**Blocked by:** 01: Establish executable reference fixture pipeline

**Status:** resolved

- [x] Hidden time-step refinement is removed from all three affected public engine paths.
- [x] Instability is rejected using the signed-rate reference inequality, including negative-rate cases.
- [x] Equality at the stability boundary is accepted; the first unstable value is rejected.
- [x] Focused tests cover positive, zero, and negative rates for vanilla, digital, and vanilla-barrier engines.
- [x] Deterministic fixtures record results for the requested grid rather than an implicitly refined grid.
