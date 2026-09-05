Type: task
Status: ready-for-agent
Blocked by: 04

# Language-neutral parity fixture harness

## Goal

Create the checked-in CSV/TSV fixture format and Catch2 helpers that compare `ito` results with reviewed values generated once from the C# oracle.

## Depends on

European analytic pricing vertical slice.

## Acceptance criteria

- Fixture columns identify instrument inputs, market assumptions, expected outputs, and per-output tolerances.
- The parser has no production dependency and reports malformed rows clearly.
- Tests compare value, all supported Greeks, and implied-volatility results using the fixture tolerances.
- Fixture provenance and financial convention are documented.
- A fixture failure identifies the case, output, expected value, actual value, and tolerance.

## Test

Include valid rows, invalid rows, tolerance boundaries, malformed input, and a known intentional mismatch to prove failure reporting.
