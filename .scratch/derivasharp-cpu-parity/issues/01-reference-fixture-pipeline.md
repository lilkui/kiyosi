# 01: Establish executable reference fixture pipeline

**What to build:** A developer-side workflow that executes the pinned DerivaSharp reference, exports complete language-neutral parity fixtures, and lets the public-boundary C++ test harness load and compare a representative case.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Exporting fails when the DerivaSharp checkout revision differs from the pinned revision.
- [x] Fixtures contain reconstructible instrument terms, market assumptions, engine settings, outputs, provenance, tolerances, and statistical budgets where applicable.
- [x] The C++ harness compares at least one executable reference case through public instruments and pricing engines.
- [x] Missing, malformed, or incomplete fixture metadata fails parsing or validation with an actionable message.
- [x] The normal C++ test build does not require the external .NET checkout.
