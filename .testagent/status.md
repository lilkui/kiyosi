# CPU parity fixture status

- Focused build: `cmake --build out/build/fixture-work --target kiyosi_tests` passed in the Visual Studio Developer environment.
- Full CTest: `ctest --test-dir out/build/fixture-work --output-on-failure` passed, 58/58 tests.
- Regeneration: `pwsh -NoProfile -File tools/generate_cpu_parity.ps1` passed against the pinned DerivaSharp checkout and is outside CMake/CTest.
- Assertion review: parser tests assert missing provenance, pinned revision, allowed reference kind, explicit non-negative tolerance, and output/tolerance key parity; typed family tests assert public pricing results are present, finite, repeatable where statistical, and exact where the reviewed reference is stable.
- Spec review: all ADR 0010 required instrument/engine pairs remain in the closure set; five structured FD and six Monte Carlo concrete engines execute through typed tests.
- Deliberate scope: Bermudan remains represented by the existing BinomialAmerican manifest pair; the current public BinomialAmericanEngine accepts American options only, so closure executes the equivalent public vanilla pricing seam without adding a speculative production overload.
