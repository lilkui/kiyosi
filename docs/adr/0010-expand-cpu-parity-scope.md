# Expand CPU parity scope

## Context

Kiyosi's initial phase intentionally deferred DerivaSharp's integral, Asian,
accumulator, and autocallable families. The project now needs parity with all
concrete CPU instruments and pricing engines in DerivaSharp while keeping the
redesigned value-oriented C++ boundary.

## Decision

Implement every concrete DerivaSharp CPU instrument and pricing engine in
vertical slices, preserving Kiyosi factories and constrained engine entry
points. CUDA backends, abstract/base classes, and source-level C# compatibility
remain out of scope. Each slice requires translated parity fixtures,
validation checks, and explicit numerical tolerances.

## Consequences

Integral, Asian, accumulator, and autocallable products become supported
families. The production core remains standard-library-only; CUDA and other
accelerators can be added later without changing the value model.
