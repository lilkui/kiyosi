# Make reference fixtures executable closure gates

Type: task
Status: ready-for-agent
Blocked by: 01, 02, 03, 04

## Problem

`cpu_parity.tsv` contains rounded placeholder prices, while its current tests
mostly validate manifest shape. Only a few vanilla rows are actually priced.
The fixture therefore cannot prove the CPU parity claimed by ADR 0010.

## Work

- Add a checked-in developer script outside the C++ build that extracts or
  regenerates reference data from DerivaSharp revision
  `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`.
- Keep generated reference data checked in. Do not add a normal build, test,
  package, or runtime dependency on the sibling repository or C#.
- Extend fixture provenance to record source revision, source test/data symbol,
  calendar/day-count convention, reference kind, and explicit tolerance.
- Replace every remaining placeholder with a reviewed reference result.
- Keep `cpu_parity.tsv` readable, but execute rows through typed family-specific
  C++ tests so required inputs cannot be silently omitted or ignored.
- Cover at least one canonical executable case for every concrete
  instrument/pricing-engine pair plus the accepted payoff, state, settlement,
  monitoring, and convergence variants.
- For Monte Carlo, record the DerivaSharp reference mean and a reviewed fixed
  statistical tolerance. Require Kiyosi repeatability for equal seeds without
  requiring identical random streams. Record the automatically derived
  structured trading-step count as provenance only.
- Document regeneration and review in `docs/parity-fixtures.md`.
- Audit ADR 0010's claimed engine/instrument set against the executable cases.

## Acceptance

- No rounded placeholder reference remains.
- Removing or corrupting a value for any concrete CPU instrument/engine pair
  fails an executable pricing test, not only a manifest-shape assertion.
- Provenance for every row identifies the pinned revision, source symbol,
  convention, reference kind, and tolerance.
- Analytic cases use tight deterministic tolerances; finite-difference cases
  show refinement; Monte Carlo cases prove fixed-seed repeatability and remain
  within their reviewed statistical tolerances.
- The reference-generation script is reproducible when the pinned DerivaSharp
  checkout is available but is never invoked by normal CMake/CTest workflows.
- Focused tests and the full CTest target pass in the Visual Studio Developer
  environment.

## Comments
