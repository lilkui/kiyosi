# AGENTS.md

## Workflow

- Before domain exploration, read the [domain guide](docs/agents/domain.md).
- Before issue/PR operations, read [issue tracker](docs/agents/issue-tracker.md); for triage, also read [triage labels](docs/agents/triage-labels.md).
- On Windows, activate the Visual Studio Developer environment before invoking MSVC or CMake.
- Use Conventional Commits.
- Alpha: no backward-compatibility commitments; refactor for clarity.

## Core and API

- The C++ core alone defines domain semantics: the model, rules, state transitions, invariants, defaults, enums and validation. Route every language's domain operations through it.
- Provide idiomatic C++ and Python APIs; interfaces and coverage may differ. Shared operations on equivalent domain inputs, state and configuration must yield consistent results, defaults, domain boundary behavior, validation timing, error categories and failure-state guarantees. Test shared scenarios in both languages.
- The core must configure, build, test, install and run without Python or binding frameworks; public core headers must have neither dependency.

## Validation and Errors

- Validate language protocols and representations before or during conversion. Reject unsafe or unintended lossy conversions, including implicit bool-to-number coercion, fractional-to-integer narrowing and values outside the target representation's range.
- Run core domain validation before committing state that requires validity; boundary checks cannot replace it. Necessary domain pre-checks must reuse core validation, preserving acceptance, error categories and failure-state guarantees.
- Use language-native conversion/signature errors for representation, protocol and signature violations; structured core errors with stable categories for domain rejections. Callers must not parse messages.
- Instrument/engine combinations excluded by API types or signatures may fail at C++ compilation or Python conversion/signature checks; representable but unsupported combinations require core domain errors.
- Preserve language-native semantics for resource exhaustion and internal failures; document operational failure and cancellation mappings.

## Bindings and Ownership

- Bindings adapt language-specific types, protocols, errors, ownership, lifetimes, concurrency and calling conventions; contracts, conveniences and representation mappings must preserve core semantics.
- Prefer Python `snake_case` choice strings over enums; map them to core choices.
- Derive Python keyword defaults from core values, e.g. `"asset_step_count"_a = FiniteDifferenceSettings{}.asset_step_count`; keep one definition per domain default, testable in C++.
- Return owning values by default. Borrow only core storage guaranteed valid at stable addresses throughout the documented view lifetime; tie views to owners, prefer read-only access, and document mutation, invalidation and concurrency.

## Concurrency

- Document the library-wide thread-safety default and type/operation overrides in public C++ headers: concurrent calls on distinct instances, shared instances and alongside mutation.
- Release the GIL only when concurrent core execution satisfies that contract. While released, access no Python objects, invoke no Python callbacks and touch no GIL-protected state.

## Pricing Performance

Pricing accuracy comes first. Optimize valid-input hot paths using established invariants; preserve numerical stability, required validation, ownership/thread safety and error/state guarantees. Avoid redundant checks, speculative fallbacks and unnecessary defensive copies. Verify with pricing correctness tests and relevant benchmarks.

## C++ Naming

- Types: `PascalCase`.
- Namespaces, functions, variables, constants and enum values: `snake_case`; private non-static data members: `snake_case_`.
- Macros: `KIYOSI_UPPER_SNAKE_CASE`.
