# Limit Phase 1 engine coverage

## Context

The initial library boundary and the Phase 1 engine list were recorded separately. The current implementation has one narrower scope: Barrier is included, while additional structured products remain future work.

## Decision

Phase 1 targets a single-underlying Black–Scholes–Merton domain on Windows and Linux with C++23 and CMake. The production core remains standard-library-only. Prices, Greeks, solver results, and deterministic edge cases remain parity obligations. The phase provides one reference engine per core family plus numerical cross-checks:

- analytic European, digital, and barrier pricing;
- binomial American pricing; and
- finite-difference European and American pricing;
- standard-library Monte Carlo European and American pricing.

Integral, Asian, additional structured-product, and CUDA engines remain later work. Monte Carlo uses CPU-only standard-library random generation; no libtorch or TorchSharp dependency is part of Kiyosi. The library starts at version `0.1.0` with semantic versioning, no stable ABI promise before `1.0`, and no performance acceptance requirement.

## Consequences

The public scope stays small enough to keep deterministic parity and convergence checks meaningful. New numerical methods or product families require a separate decision rather than silently expanding Phase 1.
