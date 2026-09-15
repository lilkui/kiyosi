# API Design and Binding Compliance Audit

Audit date: 2026-09-15  
Revision: `cd5fe5bea3959be3864f5965cc871a6435859ebf`

This is a point-in-time audit; dated resolution notes record follow-up changes without rewriting the original findings.

## Conclusion

The codebase is **partially compliant** with the API Design and Binding Principles in `AGENTS.md`.

The architecture is sound: the C++ core is independently buildable and installable, domain construction and pricing route through core factories and engines, domain errors carry stable categories, Python-visible domain objects are immutable, and the binding generally converts Python protocols before calling the core. However, full compliance is blocked by five findings: inconsistent date/time adaptation, duplicated domain defaults, an undocumented concurrency contract despite GIL release, incorrectly classified numeric conversion failures, and missing cross-language parity coverage.

## Findings

### F1 — High: date and timestamp accessors do not preserve domain semantics

**Resolved 2026-09-15:** all core `date` accessors now use the explicit `datetime.date` conversion, `PricingContext.valuation_time` returns a timezone-aware UTC `datetime`, and [`tests/python/test_kiyosi.py`](../tests/python/test_kiyosi.py#L65) covers date and timestamp round trips.

Several bindings return `std::chrono` values through nanobind's generic chrono caster instead of the binding's explicit date conversion:

- `GeometricAverageOption.average_start` and `ArithmeticAverageOption.average_start` bind the core accessor directly in [`python/kiyosi/_instruments.cpp`](../python/kiyosi/_instruments.cpp#L37).
- Structured-product `observation_dates`, `effective`, and `expiry` bind directly in [`python/kiyosi/_structured.cpp`](../python/kiyosi/_structured.cpp#L16).
- `PricingContext.valuation_time` binds directly in [`python/kiyosi/_market.cpp`](../python/kiyosi/_market.cpp#L136).

On the audit host (`Asia/Shanghai`), a core date representing `2025-01-02` was returned as the naive value `datetime.datetime(2025, 1, 2, 8, 0)`. Structured dates behaved the same way. A timezone-aware UTC valuation input was also returned as a naive local-time `datetime`. This is inconsistent with the date-only accessors that use `python_date`, and the generated stubs expose the inconsistency as `datetime.datetime` rather than `datetime.date`.

This violates idiomatic API and semantics-preserving binding requirements. It also makes results depend on the process timezone.

Minimal remediation:

1. Use `python_date` for every core `date` accessor and sequence.
2. Add one explicit converter for `timestamp` that returns a timezone-aware UTC `datetime`.
3. Add round-trip tests under a non-UTC timezone for date-only and timestamp values.

### F2 — High: Python bindings duplicate core-owned domain defaults

**Resolved 2026-09-15:** pricing and instrument defaults now have a single C++ settings, terms, or named-constant authority, and Python keyword defaults reference those core values. Structured terms now default to untouched status and par principal in C++ as well as Python. [`tests/api/header_tests.cpp`](../tests/api/header_tests.cpp) locks the authoritative values at compile time; cross-language parity remains tracked by F5.

The binding repeats default values as literals rather than taking them from core-owned values:

- Finite-difference, Monte Carlo, numerical-shift, and implied-solver settings in [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L89), [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L108), [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L155), and [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L184).
- Binomial and vanilla Monte Carlo defaults in [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L295) and [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L316).
- Instrument defaults such as realized average, rebate, observation mode, settlement behavior, and accumulated quantity in [`python/kiyosi/_instruments.cpp`](../python/kiyosi/_instruments.cpp#L34), [`python/kiyosi/_instruments.cpp`](../python/kiyosi/_instruments.cpp#L104), and [`python/kiyosi/_instruments.cpp`](../python/kiyosi/_instruments.cpp#L137).
- Structured-product and preset defaults in [`python/kiyosi/_structured.cpp`](../python/kiyosi/_structured.cpp#L51) and [`python/kiyosi/_structured.cpp`](../python/kiyosi/_structured.cpp#L208).

The duplicated pricing values currently match the corresponding core settings in [`include/kiyosi/pricing/settings`](../include/kiyosi/pricing/settings), but future core changes can silently diverge from Python. Some instrument defaults are core aggregate defaults, while `principal_ratio = 1.0` for direct structured constructors is defined only by the binding rather than by the corresponding core terms aggregates.

This directly violates the single-core-authority rule.

Minimal remediation: make each domain default representable in a core settings/terms value, then express binding defaults through default-constructed core values, for example `FiniteDifferenceSettings{}.asset_steps`. Keep only representation defaults such as `None` and empty Python tuples in the binding, while mapping them to core-owned semantics.

### F3 — High: GIL release is not backed by a published thread-safety contract

**Resolved 2026-09-15:** [`include/kiyosi/kiyosi.hpp`](../include/kiyosi/kiyosi.hpp) now publishes the library-wide contract for different instances, same-instance reads, object lifetime, and mutation. Custom calendar predicates and generic analytics wrappers document their exceptions. All GIL-released paths were verified to access only C++ values and built-in calendars, while [`python/README.md`](../python/README.md) and the `PricingContext` property docstrings document Python concurrency and borrowed-view lifetimes.

All engine `price` calls release the GIL in [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L72), as do numerical analytics, scenario grids, and implied solvers beginning at [`python/kiyosi/_pricing.cpp`](../python/kiyosi/_pricing.cpp#L151). No public header or user documentation states the library-wide guarantees for concurrent calls on different instances, the same instance, or alongside mutation.

The reviewed core types are predominantly immutable values and the engine implementations appear to keep pricing state local, so no concrete race was identified. That does not satisfy the explicit requirement that the contract be published before bindings rely on it.

Minimal remediation: add a short library-wide contract to the public C++ API, documenting different-instance, same-instance, and mutation behavior. Retain GIL release only for operations covered by that contract. Document the lifetime/concurrency implications of the `reference_internal` accessors for `PricingContext.parameters` and `PricingContext.calendar` at the same time.

### F4 — Medium: out-of-range numeric inputs surface as generic `RuntimeError`

**Resolved 2026-09-15:** the shared numeric boundary helpers now preserve `TypeError` for protocol violations, raise `OverflowError` for values outside the target C++ type, and accept the complete `std::uint64_t` seed range. Representable values continue to the core for authoritative domain validation. [`tests/python/test_kiyosi.py`](../tests/python/test_kiyosi.py) covers each classification and the unsigned boundary.

Boundary helpers call `nb::cast` directly in [`python/kiyosi/_binding.hpp`](../python/kiyosi/_binding.hpp#L69). Runtime probes found:

- `FiniteDifferenceVanillaEngine(asset_steps=2**40)` raises `RuntimeError: bad cast`.
- `BsmParameters(risk_free_rate=10**400, ...)` raises `RuntimeError: bad cast`.
- `MonteCarloVanillaEngine(seed=2**64 - 1)` raises `RuntimeError: bad cast`, even though the core target is `std::uint64_t` and the value is valid for that type.

Boolean and non-integral inputs are correctly rejected as `TypeError`, but range failures are neither consistently classified as host conversion errors nor faithful to the target type's accepted range. `optional_seed` first narrows through `std::int64_t` in [`python/kiyosi/_binding.hpp`](../python/kiyosi/_binding.hpp#L87), unnecessarily excluding half of the unsigned core range.

Minimal remediation: perform checked conversions against the actual target type, accept the full `std::uint64_t` seed range, and translate representation overflow to `OverflowError` (or a documented `TypeError`) rather than leaking nanobind's generic `RuntimeError`.

### F5 — Medium: tests do not establish C++/Python domain parity

The C++ suite contains broad validation, engine, invariant, and reference coverage, while [`tests/python/test_kiyosi.py`](../tests/python/test_kiyosi.py#L1) contains five Python-only smoke tests. No fixture or parameterized matrix drives equivalent success, default, boundary, and domain-error scenarios through both public languages.

This leaves the duplicated defaults, timezone-dependent date conversions, and unsigned conversion behavior undetected, and does not meet the explicit parity-test requirement.

Minimal remediation: add a small shared parity case table covering representative construction, defaults, pricing results, domain error categories, date/timestamp round trips, and numeric boundaries; run the same cases from C++ and Python. Full duplication of the 104-test C++ suite is unnecessary.

## Compliance matrix

| Principle | Status | Evidence |
|---|---|---|
| Idiomatic C++ and Python APIs | Partial | Public surfaces are language-appropriate and need not be one-to-one, but F1 produces non-idiomatic and inconsistent Python date/time values. |
| C++ independence from Python | Pass | `KIYOSI_BUILD_PYTHON` defaults to `OFF`; public headers contain no Python or nanobind references; Python-free configure/build/test/install succeeded. |
| Single core authority | Partial | Constructors and operations route through core factories/engines, but binding defaults are duplicated (F2). |
| Semantics-preserving bindings | Partial | Values and container copies are returned by default; borrowed `PricingContext` members use `reference_internal`; F1 breaks temporal semantics. |
| Concurrency | Fail | GIL release is widespread but the required public contract is absent (F3). |
| Safe validation flow | Partial | Protocol checks reject booleans and non-integral values before core calls; core owns domain validation; range conversion is incomplete (F4). |
| Observable behavior and errors | Partial | `Error` carries a stable `error_category`, `KiyosiError.category` preserves it, and unsupported combinations are structurally excluded; F4 and F5 leave conversion/error parity incomplete. |

## Confirmed strengths

- [`CMakeLists.txt`](../CMakeLists.txt#L26) keeps Python optional and only discovers Python/nanobind inside the enabled binding block.
- Public C++ headers under `include/kiyosi` do not reference Python or a binding framework.
- Python constructors call core factories and unwrap `result<T>`; no duplicate domain-validation implementation was found.
- [`python/kiyosi/_native.cpp`](../python/kiyosi/_native.cpp#L11) maps core domain failures to `KiyosiError` with the stable core category attached.
- Python domain properties are read-only. Container-valued accessors return converted copies; the two borrowed `PricingContext` subobjects are owner-tied with `reference_internal`.
- Engine/instrument support is expressed with typed C++ overloads and matching Python overload registrations rather than a second dynamic dispatch implementation.
- Enum registration is representation mapping only and directly uses core enum values.

## Verification performed

- Scanned all 151 tracked source, header, binding, test, build, documentation, example, benchmark, and tooling files in the audited areas; generated/build outputs were not treated as source.
- Built the core with MSVC in the Visual Studio developer environment with `KIYOSI_BUILD_PYTHON=OFF`.
- Ran the C++ suite: **104/104 passed**.
- Installed the Python-free core and CMake package successfully to an isolated audit prefix.
- Built a fresh CPython 3.14 wheel from the current tree.
- Ran the Python suite against that wheel: **5/5 passed**.
- Inspected the generated type stubs and ran focused date/time and numeric-boundary probes, which produced F1 and F4.

## Recommended order

1. ~~Fix F1 and add its regression tests.~~ Resolved 2026-09-15.
2. ~~Replace duplicated defaults under F2 with core-derived values.~~ Resolved 2026-09-15.
3. ~~Publish the contract required by F3 and verify every GIL release against it.~~ Resolved 2026-09-15.
4. ~~Normalize boundary conversion failures under F4.~~ Resolved 2026-09-15.
5. Add the compact shared parity matrix in F5 so the preceding guarantees stay enforced.
