# Use a value-oriented, vertical-slice architecture

## Decision

Kiyosi redesigns DerivaSharp's domain model and public contract rather than
translating its C# source or inheritance hierarchy. The public C++ API uses
concrete immutable instrument values, independent `Payoff` and `Exercise`
values for exercise-based options, and separate instrument types for
structured products. Public pricing engines expose one constrained `price`
entry point with settings fixed at construction.

The API is organized into vertical slices for core types, market context and
calendars, instruments, and pricing engine families. Slice headers are the
canonical dependency boundary; `kiyosi/kiyosi.hpp` is a convenience umbrella.
Numerical helpers remain private to pricing implementations.

Results use `std::expected` for validation and numerical failures and an
indexed `optional` slot for each risk measure. Engines return every risk
measure they naturally compute; unsupported combinations have no matching
entry point. Dates use day-based `std::chrono` values and the public scalar is
`double`.

## Consequences

The value model preserves compile-time capability boundaries without public
abstraction or overload matrices for every instrument and engine combination.
Numerical parity with established reference results remains a correctness
constraint, while source-level C# compatibility and public ABI compatibility
are not constraints.
