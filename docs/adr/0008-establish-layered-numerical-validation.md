# Establish layered numerical validation

The pricing library needs a deterministic validation contract beyond smoke tests and a small set of parity fixtures. Numerical regressions can preserve plausible prices while breaking a risk measure, an identity, a bound, or a discretized engine's refinement behavior.

The test target therefore uses five complementary layers:

- central finite differences cross-check every analytic `PricingResult` field, preserving the public theta, vega, vanna, zomma, and rho units;
- no-arbitrage identities and validation properties cover vanilla, digital, and zero-rebate barrier instruments;
- fixed Haug/Hull Black-Scholes reference cases supplement the existing external parity fixtures;
- binomial and finite-difference prices are checked against analytic values at increasing resolutions;
- all checks remain in the existing Catch2/CTest target with no new runtime dependency.

Reference comparisons use explicit absolute and relative tolerances. Discretized engines are required to improve with refinement and to clear a conservative empirical error-ratio threshold; the suite does not claim a universal formal order for every payoff and boundary treatment.

The textbook reference cases follow the canonical Black-Scholes parameter sets presented in Haug's *The Complete Guide to Option Pricing Formulas* and Hull's *Options, Futures, and Other Derivatives*; dates map their year fractions to the library's Actual/365 Fixed convention.

This keeps the production API unchanged while making numerical parity, validation properties, and convergence visible in the normal build.
