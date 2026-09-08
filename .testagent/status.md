# Barrier parity review

Focused tests pass with direct public-constructor reconstruction. Assertions
compare independent pinned DerivaSharp outputs; no metadata-only barrier row is
treated as behavioral evidence. The vanilla analytic engine was corrected from
its negative survival-integral result to the pinned closed-form barrier factors.

Validation:
- `kiyosi_tests.exe "Barrier fixtures reconstruct every pinned public variant"`
- `kiyosi_tests.exe "Barrier validation fixtures exercise public constructors"`
