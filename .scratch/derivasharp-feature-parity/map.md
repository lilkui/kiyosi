# DerivaSharp CPU feature parity

The accepted design is in [spec.md](spec.md). Behavioral references are pinned
to DerivaSharp revision `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`.

## Delivery

| Phase | Issue | Status | Blocked by |
| --- | --- | --- | --- |
| 1 | [01 Effective dates, schedules, and calendar](issues/01-effective-dates-schedules-calendar.md) | resolved | — |
| 2 | [02 Binary and scheduled barrier parity](issues/02-binary-and-scheduled-barriers.md) | ready-for-agent | 01 |
| 3 | [03 Structured state, settlement, and validation](issues/03-structured-state-settlement-validation.md) | ready-for-agent | 01 |
| 4 | [04 Structured finite-difference engines](issues/04-structured-finite-difference-engines.md) | ready-for-agent | 03 |
| 5 | [05 Executable reference-fixture closure](issues/05-executable-reference-fixtures.md) | ready-for-agent | 01, 02, 03, 04 |

## Decisions So Far

- Kiyosi preserves behavioral rather than source/API parity; see
  [ADR 0010](../../docs/adr/0010-cpu-parity-scope-and-conventions.md).
- Instrument lives use explicit effective and expiry dates.
- Calendars select observation dates; Actual/365 Fixed governs monetary time.
- Scheduled barriers use BGK approximation, not exact discrete monitoring.
- Structured valuations are principal-inclusive and event ordering is explicit.
- Structured finite-difference settings describe the computation actually run.
- Checked-in executable fixtures carry pinned provenance and explicit tolerances.
- Effective dates, schedule adjustment rules, and the pinned SSE calendar are implemented; see [issue 01](issues/01-effective-dates-schedules-calendar.md).

## Fog

None. The design tree was confirmed complete before publication.
