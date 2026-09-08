# 02: Close vanilla, digital, and Asian parity matrix

**What to build:** Direct public-boundary parity coverage for vanilla analytic, integral, binomial, finite-difference, and Monte Carlo engines; digital cash and asset branches; and geometric and arithmetic Asian engines using faithfully reconstructed reference inputs.

**Blocked by:** 01: Establish executable reference fixture pipeline

**Status:** ready-for-human

- [x] Every in-scope vanilla, digital, and Asian engine has at least one executable reference comparison.
- [x] Digital cash and asset settlement branches are constructed through the public API and compared to reference outputs.
- [x] Geometric and arithmetic Asian fixtures reproduce the pinned reference terms, dates, realized observations, and market assumptions.
- [x] Deterministic outputs use reviewed deterministic tolerances and seeded Monte Carlo outputs include their budgets and statistical tolerances.
- [x] Existing identity, bound, and convergence properties remain supplementary to direct comparisons.
