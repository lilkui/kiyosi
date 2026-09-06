# Separate exercise-based options from structured products

## Context

Vanilla, digital, American, and Bermudan instruments share a useful mathematical boundary: a payoff is evaluated from the underlying at one contractual time or at a holder-selected admissible time. Their payoff and exercise rules are independent dimensions. Treating every instrument as a payoff/exercise product would force path-dependent instruments such as Barrier and Autocallable into artificial strategies carrying schedules, barriers, and state transitions.

The current API duplicates common instrument storage, exposes engine combinations as overload matrices, and represents risk-result availability in multiple places.

## Decision

This decision supersedes the public-name preservation clause in ADR 0007; compatibility is intentionally not a constraint for this rewrite.

- Model the exercise-based option family as independent `Payoff` and `Exercise` values composed by an instrument type. Share only the semantically common option terms.
- Keep structured products as independent instrument types with their own schedules, state transitions, and pricing engines. They may reuse common value objects but do not implement the exercise-based option abstraction.
- Make each public pricing engine expose one constrained `price` entry point with settings fixed at construction. Unsupported instrument/engine combinations have no matching overload.
- Represent each risk measure as one `optional` slot indexed by a contiguous risk-measure enumeration. Remove the NaN sentinel and all public risk-measure bitmasks for result availability, request selection, and engine capability discovery. Each pricing call returns every risk measure that the engine naturally computes.

## Consequences

This removes the instrument and overload Cartesian products while preserving compile-time capability boundaries. Adding Bermudan support extends the exercise-based option family without changing structured-product semantics. Adding Autocallable adds a dedicated instrument and engine rather than enlarging a generic payoff or exercise strategy. Callers must use indexed risk-measure access instead of positional public fields.
