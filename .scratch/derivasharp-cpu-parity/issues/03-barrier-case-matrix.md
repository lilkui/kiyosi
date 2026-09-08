# 03: Close barrier and binary-barrier case matrix

**What to build:** Complete direct reference comparisons for vanilla barriers and binary barriers across every supported direction, in/out condition, cash or asset settlement, settlement timing, rebate, and monitoring mode.

**Blocked by:** 01: Establish executable reference fixture pipeline

**Status:** ready-for-human

- [x] Analytic and finite-difference vanilla barrier cases compare values through the public API.
- [x] Binary barrier cases cover up/down and in/out combinations with cash and asset settlement.
- [x] At-hit and at-expiry settlement and rebate variants are represented wherever expressible by the public contract.
- [x] Continuous and scheduled monitoring variants have complete, reconstructible fixture inputs and provenance.
- [x] Validation cases exercise the actual barrier constructors and expected error categories rather than metadata-only rows.
