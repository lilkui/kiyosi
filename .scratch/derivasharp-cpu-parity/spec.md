Status: ready-for-agent

## Problem Statement

Kiyosi advertises CPU pricing-engine families corresponding to the pinned DerivaSharp implementation, but the current parity evidence is incomplete. The fixture generator checks revision metadata without executing the external reference, several Asian and structured fixtures do not reproduce their cited inputs, and many tests verify only that a result exists or is repeatable. Important caller-visible behaviors also differ: explicit finite-difference grids are silently refined, valuation-date observations are skipped, scheduled barriers can treat non-observation crossings as historical touch, structured instruments lack several immutable transformations, the American engine is not the named Bjerksund–Stensland 2002 formulation, and shared analytics conventions are not behaviorally verified.

## Solution

Establish behavioral parity at Kiyosi's public C++ boundary with CPU behavior from the pinned DerivaSharp revision. A developer-side exporter will execute the pinned reference tests and check in complete language-neutral reference fixtures. Kiyosi's public API tests will construct every case and compare values and supported risk measures using engine-appropriate tolerances, while focused behavioral tests close the finite-difference, observation, barrier, transformation, American approximation, and analytics gaps.

## User Stories

1. As a pricing-library maintainer, I want every CPU engine family represented by an executable reference fixture, so that parity claims are based on observed behavior rather than metadata.
2. As a pricing-library maintainer, I want each fixture to contain reconstructible instrument terms, market assumptions, engine settings, outputs, provenance, and tolerances, so that a failing comparison can be reproduced and audited.
3. As a developer on Windows, I want one exporter command to run the pinned DerivaSharp reference and fail on a revision mismatch, so that fixture regeneration is deliberate and traceable.
4. As a developer on Linux, I want parity tests to consume checked-in language-neutral fixtures without the external .NET checkout, so that the normal C++ test suite remains reproducible.
5. As a user of a digital pricing engine, I want cash and asset settlement branches compared against reference results, so that both public payoff forms are trustworthy.
6. As a user of a barrier pricing engine, I want every direction, in/out condition, settlement timing, rebate, and monitoring mode covered, so that the barrier case matrix is explicit.
7. As a user of a structured product engine, I want touch states, expiry handling, and every expressible preset compared with direct references, so that state transitions are not hidden behind generic smoke tests.
8. As a Monte Carlo user, I want seeded reference outputs recorded with path and step budgets and statistical tolerances, so that stochastic parity is meaningful rather than limited to finiteness or repeatability.
9. As a finite-difference user, I want my requested explicit Euler grid either evaluated as requested or rejected as unstable, so that the engine never silently changes my numerical contract.
10. As a finite-difference user, I want the stability rule to match the reference for positive, zero, and negative rates, so that boundary validation is predictable.
11. As a finite-difference user, I want an exactly stable boundary accepted and an unstable boundary rejected, so that strict inequality behavior is preserved.
12. As a structured-product user, I want an observation on the valuation date applied once, so that coupons, knock-in, knock-out, and accumulator effects reflect the contractual state.
13. As a structured-product user, I want dates before valuation excluded and expiry observations processed before terminal settlement, so that event ordering is unambiguous.
14. As a structured-product user, I want valuation-date processing to be idempotent across repeated valuations, so that an observation is never replayed.
15. As a scheduled-barrier user, I want a crossing on a non-observation valuation date ignored as historical touch, so that scheduled monitoring follows the contract rather than spot prechecks.
16. As a scheduled-barrier user, I want a contractual valuation-date observation recognized exactly once, so that time-zero state is consistent with the observation schedule.
17. As a scheduled-barrier user, I want the BGK approximation retained between observation dates, so that the established scheduled-monitoring valuation remains usable.
18. As a Phoenix, Snowball, BinarySnowball, or TernarySnowball user, I want an immutable touch-status replacement, so that I can value alternate historical states without mutating the original instrument.
19. As a Snowball user, I want to replace the complete knock-out coupon schedule and maturity coupon together, so that schedule offsets and maturity behavior remain aligned.
20. As an instrument user, I want replacements to return validated results and leave the source instrument unchanged, so that invalid terms cannot enter valuation and existing values remain safe.
21. As an American-option user, I want the named two-boundary Bjerksund–Stensland 2002 approximation, so that the engine's advertised method matches its behavior.
22. As an American-call user, I want coverage across dividend, rate, and early-exercise regimes, so that approximation quality is not inferred from one fixture.
23. As an American-put user, I want values checked against an independent correct reference, so that a known transformed-put defect is not copied into Kiyosi.
24. As a risk-measure user, I want supported Greeks, implied values, and scenario grids to follow documented reference conventions, so that results are comparable across engines.
25. As a risk-measure user, I want bump sizes, speed stencils, theta direction, and grid semantics aligned, so that sensitivities do not vary by accidental implementation detail.
26. As a risk-measure user, I want unsupported measures represented as absent or failed rather than fabricated, so that a missing capability is visible.
27. As a maintainer, I want deterministic engines checked with deterministic tolerances and Monte Carlo checked with common random numbers and statistical budgets, so that acceptance criteria match numerical uncertainty.
28. As a release maintainer, I want Windows and Linux CTest runs to exercise the same checked-in references, so that CPU parity is portable before it is declared complete.
29. As a maintainer, I want provisional tolerances and presence-only assertions removed, so that the suite cannot pass while silently skipping parity.

## Implementation Decisions

- Use the existing public-boundary parity harness as the highest test seam. Extend its fixture parser and comparison helpers rather than creating a second test architecture.
- Add one developer-side reference exporter that invokes the pinned DerivaSharp tests, serializes complete inputs and outputs, records provenance and tolerances, and refuses to run against any other revision. The exporter is not a Kiyosi runtime or Linux test dependency.
- Store language-neutral, versioned fixtures that distinguish analytic, deterministic discretized, and statistical reference results. Statistical rows include seed, path count, step count, and reviewed acceptance budgets.
- Require direct comparison rows for every CPU engine family and a separate variant matrix for digital settlement, the complete barrier case matrix, structured touch and expiry states, requested finite-difference grids, and seeded Monte Carlo cases. Retain identity, bound, state-transition, and convergence properties as supplementary tests.
- Remove hidden explicit-grid refinement from vanilla, digital, and vanilla-barrier finite-difference engines. Apply the reference stability condition `max_dt * (volatility² * max_price² / price_step² + risk_free_rate) > 1` as a rejection rule; preserve signed rates and accept equality.
- Centralize structured event scheduling so a contractual valuation-date observation is applied once before terminal settlement, dates before valuation are excluded, expiry observations run before terminal settlement, and later valuation calls do not replay events.
- Add immutable, validated touch-status replacements to Phoenix, Snowball, BinarySnowball, and TernarySnowball. Add an immutable Snowball replacement for the full knock-out coupon schedule plus maturity coupon. Return `result<T>` and reuse existing factory validation; do not add named convenience factories when generic factories already express the presets.
- Make scheduled barrier state depend on contractual observation dates. Ignore non-observation valuation-date crossings as historical touch, recognize contractual time-zero observations once, and retain BGK approximation between observations. Record this intentional difference from the reference boundary behavior.
- Replace the single-boundary and binomial-fallback American implementation with the two-boundary Bjerksund–Stensland 2002 approximation. Validate calls against pinned outputs and puts against an independent correct offline reference; do not add the independent reference library as a runtime dependency.
- Codify shared analytics conventions in fixture metadata and behavioral tests before changing implementations. Align bump sizes, speed stencil, theta direction, and scenario-grid semantics where a comparable reference exists; preserve explicit absence for unsupported measures.
- Treat checked-in reference fixtures, valuation-date observation semantics, and deliberate parity exceptions as durable decisions, as recorded in the accepted ADRs. Keep API naming and tolerance details in executable tests and fixture metadata.

## Testing Decisions

- Test caller-visible behavior through public instruments, market assumptions, pricing engines, and result accessors. Do not assert private algorithms, helper call order, or source-level similarity.
- Extend the existing Catch2 parity suite to construct every fixture case through the public API and compare each declared output within its declared tolerance. A fixture with incomplete inputs, outputs, provenance, or tolerance metadata must fail parsing or validation.
- Add focused public API cases for explicit-grid rejection and accepted equality boundaries across vanilla, digital, and vanilla-barrier paths, including negative-rate behavior.
- Add pinned structured cases where valuation equals an observation and where valuation equals expiry, covering Snowball, BinarySnowball, TernarySnowball, Phoenix, and Accumulator effects and ensuring one-time processing.
- Add scheduled-barrier oracle cases for contractual valuation-date observations and non-observation valuation dates in analytic and finite-difference engines.
- Add transformation tests for all four touch-status replacements and Snowball schedule replacement, covering immutability, valid alternatives, invalid enum values, schedule length/order, and non-finite rates.
- Add Bjerksund–Stensland call and put cases across dividend, rate, and exercise regimes, with independent put references and no binomial fallback acceptance path.
- Add cross-engine analytics comparisons for supported measures, implied values, validation boundaries, expiry boundaries, and scenario grids. Verify unsupported measures remain absent.
- Use deterministic tolerances for analytic and deterministic discretized engines. Use seeded Monte Carlo, common random numbers for perturbation checks, explicit path/step budgets, and reviewed statistical intervals.
- Correct or replace the known placeholder Asian and structured fixtures, remove metadata-only validation rows, and eliminate presence-only parity assertions.
- Run the same CTest suite on Windows and Linux; parity is not complete until both platforms pass without provisional tolerances.

## Out of Scope

- CUDA acceleration or GPU parity.
- Source, inheritance, ABI, or language-API compatibility with DerivaSharp.
- Public low-level DerivaSharp numerics helpers such as solvers, matrix types, quadrature, and distribution types except where their pricing behavior is consumed.
- Kiyosi-only features such as Bermudan options and `CrrEngine` as parity blockers.
- Adding QuantLib or another independent American-option reference library to Kiyosi's runtime or public dependencies.
- Named convenience factories when existing generic factories can express the required structured presets.
- Replacing property tests with fixture tests; both remain, with direct references serving as the parity gate.

## Further Notes

- The authoritative external behavior is DerivaSharp revision `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`; the Kiyosi assessment revision is `14042ac`.
- The exporter is intentionally developer-side because the normal CMake/C++ test build must not depend on the external checkout or .NET toolchain.
- Scheduled-barrier contractual state and independent American-put validation are intentional parity exceptions, not accidental drift.
- Closure order is: executable references and variant matrix; explicit-grid and valuation-date fixes; scheduled barriers and transformations; Bjerksund–Stensland 2002; shared analytics; then cross-platform CTest and removal of provisional assertions.
