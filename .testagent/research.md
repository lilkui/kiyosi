# Barrier parity research

Targets: `tests/fixtures/cpu_parity.tsv` and `tests/parity_fixture_tests.cpp`.
The public seams are `make_barrier_option`, `make_binary_barrier_option`,
`AnalyticBarrierEngine`, `FiniteDifferenceBarrierEngine`, and
`AnalyticBinaryBarrierEngine`. Independent expected values come from pinned
DerivaSharp revision `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`, specifically
`BarrierOptionTestData` and `BinaryBarrierOptionTestData`.

Acceptance checklist:
- analytic and finite-difference vanilla barrier fixture comparisons;
- binary up/down in/out cash and asset settlement comparisons;
- at-hit and at-expiry settlement plus rebate cases;
- continuous and scheduled monitoring with reconstructible observations;
- constructor validation through expected error categories.
