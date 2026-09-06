# Expand CPU parity scope

## Context

Kiyosi's initial phase deferred DerivaSharp's integral, Asian, accumulator, and
autocallable families. The value-oriented rewrite now covers those concrete
CPU instruments and pricing engines while keeping CUDA and source-level C#
compatibility out of scope.

## Decision

The former phase-one scope is replaced by complete CPU parity. Kiyosi
implements every concrete DerivaSharp CPU instrument and pricing engine in
vertical slices, preserving Kiyosi factories and constrained engine entry
points. Each slice requires translated parity fixtures, validation checks, and
explicit numerical tolerances. CUDA backends, abstract/base classes, and
source-level C# compatibility remain out of scope. The library targets the
single-underlying Black–Scholes–Merton domain on Windows and Linux with C++23
and CMake. The production core remains standard-library-only, Monte Carlo uses
CPU-only standard-library random generation, and the initial release is
`0.1.0` with semantic versioning, no stable ABI promise before `1.0`, and no
performance acceptance requirement.

## Consequences

Integral, Asian, accumulator, and autocallable products are supported families.
CUDA and other accelerators can be added later without changing the value
model.
