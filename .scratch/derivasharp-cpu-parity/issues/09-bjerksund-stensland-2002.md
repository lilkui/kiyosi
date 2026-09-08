# 09: Implement Bjerksund–Stensland 2002 parity

**What to build:** The named two-boundary Bjerksund–Stensland 2002 American approximation with direct call references and independent correct put references across meaningful market and exercise regimes.

**Blocked by:** 01: Establish executable reference fixture pipeline

**Status:** ready-for-agent

- [ ] The American integral engine uses the two-boundary 2002 formulation.
- [ ] The algorithm-switching binomial fallback is removed.
- [ ] Call cases cover dividend, rate, and early-exercise regimes and compare against pinned references.
- [ ] Put cases compare against an independent correct offline reference rather than the known transformed-put defect.
- [ ] Regression tests cover ATM and away-from-ATM calls and puts with reviewed tolerances.
