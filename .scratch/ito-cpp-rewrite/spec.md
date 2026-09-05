# Modern C++23 derivatives-pricing library

Status: ready-for-agent

## Problem Statement

DerivaSharp currently provides derivatives valuation as a .NET 10 library. Native C++ consumers cannot use its pricing domain without adopting the .NET runtime and C# API shape. The rewrite also needs a deliberate financial contract: numerical parity must be measured, while the C++ domain model is free to improve ownership, validation, and composition.

## Solution

Build `ito`, a C++23 derivatives-pricing library in staged releases. The first release focuses on a single-underlying Black–Scholes–Merton domain with flat volatility and continuously compounded rates, explicit schedules, and deterministic pricing. It exposes immutable value-oriented instruments, explicit pricing algorithms, validated factories returning `std::expected`, standard day-based dates, owning custom calendars, and compiled CMake targets.

The first vertical slice prices European calls and puts, returns the complete deterministic risk result, solves implied volatility, and proves the public seam with language-neutral parity fixtures generated once from the C# implementation. Phase 1 then adds digital and barrier analytic engines, binomial American pricing, and finite-difference European/American pricing. Later phases may add integral, Asian, structured-product, Monte Carlo, and CUDA engines.

## User Stories

1. As a quantitative researcher, I want to construct a validated European call or put, so that invalid contractual terms cannot enter pricing.
2. As a quantitative researcher, I want to represent an option as an immutable value, so that pricing inputs can be copied and shared safely.
3. As a quantitative researcher, I want to specify the underlying price, volatility, rates, dividends, valuation date, and trading calendar explicitly, so that a valuation is reproducible.
4. As a quantitative researcher, I want to price a European option with an explicit analytic engine, so that the reference valuation path is obvious at the call site.
5. As a quantitative researcher, I want value, delta, gamma, speed, theta, charm, color, vega, vanna, zomma, and rho, so that the first release covers the established deterministic risk contract.
6. As a quantitative researcher, I want Greek units documented as calendar-day or percentage-point sensitivities where applicable, so that results are not misinterpreted.
7. As a quantitative researcher, I want to solve implied volatility from an observed option price, so that market quotes can be compared with model assumptions.
8. As a library consumer, I want invalid instrument and valuation inputs returned through `std::expected`, so that I can handle failures without exception-driven control flow.
9. As a library consumer, I want stable machine-readable error categories with human-readable messages, so that applications can both branch on failures and display useful diagnostics.
10. As a library consumer, I want to use `std::chrono::sys_days`, so that date arithmetic and comparisons compose with the standard library.
11. As a library consumer, I want to provide an owning custom trading calendar, so that market-specific observation rules work without dangling references.
12. As a library consumer, I want the initial domain to remain single-underlying BSM, so that the API stays small and its assumptions stay visible.
13. As a library consumer, I want to select explicit analytic, binomial, or finite-difference algorithms, so that engine choice is visible and testable rather than hidden behind a registry.
14. As a library consumer, I want the deterministic engines to share one pricing-result contract, so that replacing an engine does not change how results are consumed.
15. As a library consumer, I want analytic European, digital, and barrier pricing, so that common closed-form reference values are available.
16. As a library consumer, I want binomial American pricing, so that early-exercise behavior has a numerical cross-check.
17. As a library consumer, I want finite-difference European and American pricing, so that the first phase validates a reusable numerical engine boundary.
18. As a maintainer, I want parity fixtures in a language-neutral CSV/TSV format, so that the C++ tests do not require a live .NET build.
19. As a maintainer, I want fixtures generated once from the C# reference implementation, so that the source of each expected result is auditable.
20. As a maintainer, I want per-engine and per-output tolerances, so that numerical methods are judged against realistic error profiles.
21. As a maintainer, I want parity conflicts resolved using written financial conventions and recorded decisions, so that accidental C# behavior is not silently canonized.
22. As a C++ integrator, I want a static library by default and an optional shared library, so that I can choose a deployment model without duplicating the implementation.
23. As a C++ integrator, I want exported CMake targets, so that the library can be consumed through normal target-based builds.
24. As a C++ integrator, I want Windows x64 and Linux x64 builds, so that the first release covers the agreed native platforms.
25. As a contributor, I want CTest-discovered Catch2 tests, so that local and CI test execution use the same entry point.
26. As a contributor, I want the production core to remain standard-library-only, so that dependency and deployment costs stay predictable.
27. As a maintainer, I want semantic versioning starting at `0.1.0`, so that pre-1.0 API evolution is explicit.
28. As a maintainer, I want no stable ABI promise before `1.0`, so that the redesigned public boundary can still improve.
29. As a project owner, I want structured products, Monte Carlo, CUDA, integral engines, and performance benchmarking kept out of the first acceptance contract, so that the deterministic core can land without speculative infrastructure.

## Implementation Decisions

- The library identity is `ito`; the C# DerivaSharp implementation is an external reference oracle, not a source-level template.
- The domain is single-underlying Black–Scholes–Merton with flat volatility, continuously compounded rates, dividends, explicit schedules, and `double` values.
- Instruments are concrete immutable value types. Call/put is represented by one option type and an option-type enum. Runtime instrument plugins, generic payoff callbacks, and a generic model hierarchy are out of scope.
- Validated free factories return `std::expected<Instrument, Error>`. Errors contain a stable category and a human-readable message.
- Valuation context contains concrete BSM parameters, asset price, valuation date, and an owning type-erased trading calendar. The canonical date is `std::chrono::sys_days`.
- Pricing algorithms are explicit types/functions selected at compile time. There is no engine registry, plugin boundary, or future-backend abstraction in the first phases.
- The first vertical slice is European call/put analytic valuation, complete deterministic Greeks, implied volatility, and the parity fixture seam.
- Phase 1 adds analytic digital/barrier pricing, binomial American pricing, and finite-difference European/American pricing. Integral, Asian, structured-product, Monte Carlo, and CUDA engines are later phases.
- Greek conventions follow the established contract: theta/charm/color per calendar day; vega/vanna/zomma/rho per one percentage-point move; Actual/365 Fixed time.
- Numerical parity is assessed with per-engine/per-output tolerances. Written financial conventions are the default when C# behavior conflicts with them; material divergences are recorded in an ADR or parity ledger.
- The build requires C++23 and CMake 3.25 or newer, targets Windows x64 and Linux x64 first, and produces a static library by default with an opt-in shared target.
- Catch2 is a test-only dependency acquired through pinned CMake `FetchContent`; production code has no mandatory third-party dependency.
- The project starts at version `0.1.0` with semantic versioning and makes no stable ABI promise before `1.0`.

## Testing Decisions

- Tests verify caller-visible behavior: constructed values, validation failures, pricing results, Greeks, implied-volatility convergence/failure, date/calendar behavior, and parity against checked-in fixtures. Tests should not assert private helper structure.
- The first seam is the public European pricing path: validated instrument factory → BSM valuation context → explicit analytic engine → pricing result/error. Later engines reuse this result and validation contract.
- Catch2 tests run through CTest on both supported platforms.
- The C# implementation generates parity fixtures once; reviewed CSV/TSV fixtures are checked into `ito` and consumed without a live .NET dependency.
- Fixtures carry the inputs, expected outputs, and tolerances needed to distinguish analytic, binomial, finite-difference, and solver error profiles.
- Edge cases include invalid prices/rates/volatility, invalid dates, expiry valuation, call/put symmetry, zero or near-zero time, calendar schedule validation, and implied-volatility bracket failures.
- There is no performance acceptance test or benchmark requirement in this spec.

## Out of Scope

- A line-by-line C# port or preservation of the C# inheritance hierarchy.
- Curve/surface construction, stochastic volatility, multiple underlyings, portfolio risk, market-data ingestion, or trade lifecycle features.
- Integral engines, Asian options, autocallables, accumulators, other structured products, Monte Carlo, Torch/CUDA, and GPU packaging.
- A runtime instrument plugin system, generic payoff DSL, or backend registry.
- Stable ABI guarantees before `1.0`.
- Performance targets, benchmark gates, or optimization work without a concrete request.

## Further Notes

- The domain glossary and ADRs in the repository are authoritative for resolved terminology and architectural decisions.
- The C# oracle is used to discover and review parity differences, not to override written financial conventions automatically.
- Later phases must extend the existing public seams rather than reopen the Phase 1 domain boundary without a new decision record.
