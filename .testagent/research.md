# CPU parity fixture research

## Target inventory

- `tests/fixtures/cpu_parity.tsv`: checked-in CPU reference manifest; currently has 10 columns, rounded placeholders, and provenance embedded inconsistently in `inputs`.
- `tests/parity_fixture.hpp`: test-only parser and fixture types.
- `tests/parity_fixture_tests.cpp`: Catch2 public-seam tests; currently executes only the analytic European row plus one binomial and one Monte Carlo row.
- `tools/generate_cpu_parity.ps1`: new developer-only regeneration entry point; it must verify the sibling checkout is at `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2` and must not be referenced by CMake.
- `docs/parity-fixtures.md`: regeneration and review instructions.
- `docs/adr/0010-cpu-parity-scope-and-conventions.md`: authoritative CPU engine/instrument scope.

## Existing conventions

- Tests use Catch2 and exercise public `make_*` factories and pricing-engine `price` methods.
- The normal Windows build uses CMake/CTest from a Visual Studio Developer environment.
- Reference comparisons use explicit absolute tolerances; Monte Carlo additionally requires equal-seed repeatability.
- Domain language follows `CONTEXT.md`: instrument, pricing engine, market assumptions, reference result, and numerical parity.

## Pinned source inventory

- Vanilla: `EuropeanOptionTestData.ValueData`, `EuropeanOptionTestData.GreekData`, `AmericanOptionTestData.ValueData`, and the corresponding analytic/binomial/finite-difference/integral/Bjerksund-Stensland tests.
- Digital: `DigitalOptionTestData.ValueData` and analytic/finite-difference/integral tests.
- Barriers: `BarrierOptionTestData.ValueData`, `BarrierOptionTestData.FdParameters`, and `BinaryBarrierOptionTestData.ValueData`.
- Asian: `GeometricAverageAsianEngineTest.Value_IsAccurate` and `AsianOptionTestData.ArithmeticValueData`.
- Monte Carlo and structured: named pinned engine tests; Kiyosi fixtures record reviewed means in its principal-inclusive convention where ADR 0010 intentionally differs from DerivaSharp's net-value convention.

## Acceptance checklist

- Add a checked-in developer script that verifies and reads the pinned DerivaSharp checkout, with no CMake/CTest dependency.
- Add required row provenance: revision, source symbol, convention, reference kind, explicit tolerance.
- Replace rounded placeholders with reviewed reference results.
- Execute every concrete instrument/pricing-engine pair through typed public C++ seams.
- Cover payoff, state, settlement, monitoring, and convergence variants.
- Check finite-difference refinement.
- Check Monte Carlo equal-seed repeatability and reviewed statistical tolerance; treat structured step count as provenance only.
- Document regeneration and human review.
- Audit ADR 0010's engine/instrument scope against executable cases.
