# AGENTS.md

## API Design and Binding Principles

- **Idiomatic C++ and Python APIs:** Provide idiomatic public C++ and Python APIs with equivalent domain semantics. Semantic equivalence does not require identical API shapes or one-to-one surface coverage. The C++ core must configure, build, test, install, and run with no Python interpreter or binding framework present, and its public headers must reference neither.
- **Single core authority:** The C++ core owns the domain model, rules, state transitions, invariants, defaults, and authoritative domain validation. Every language entry point routes domain operations through the core. The binding layer must not duplicate domain rules or defaults: tolerances, grid sizes, path counts, seeds, shift sizes, solver bounds, iteration limits, calendars, default engine selection, and domain enum definitions are core-owned. Language names, enum registrations, protocol sentinels, and other representation mappings may remain in the binding. Express Python keyword defaults as core values—for example `"asset_steps"_a = FiniteDifferenceSettings{}.asset_steps`—so that a default is defined once and is testable from the C++ test suite. Python may define language-specific interface contracts and conveniences, but must not implement, duplicate, or override domain rules or defaults.
- **Semantics-preserving bindings:** Keep the binding layer focused on adapting types, protocols, errors, ownership, lifetimes, concurrency, and calling conventions, and keep those adaptations faithful to the core's domain semantics and state guarantees. Return values or copies by default. Expose core-owned storage as a borrowed, preferably read-only view only when the core guarantees that the storage and address remain valid for the view's documented lifetime; tie the view to its owner and document mutation, invalidation, and concurrency behavior. Apply the same policy across accessors only when their safety and semantics match. The binding layer must not become a second business implementation.
- **Concurrency:** Publish a library-wide thread-safety default in the public C++ headers, with per-type or per-operation exceptions. State whether concurrent calls are safe on different instances, on the same instance, and alongside mutation. Release the GIL only while invoking core operations covered by that contract, and do not access Python objects, callbacks, or GIL-protected state while it is released.
- **Safe validation flow:** Validate language-specific protocols and representation constraints before or during conversion, rejecting unsafe or unintended lossy conversions—including implicit boolean widening, non-integral narrowing, and values outside the target type's range. Then let the C++ core perform authoritative domain validation before committing state changes that depend on validity. Avoid duplicating domain checks outside the core; any necessary pre-check must preserve core acceptance and domain failure semantics. Core validation must never depend on, be weakened by, or be bypassed through prior boundary checks.
- **Observable behavior and errors:** For equivalent domain inputs under the same state and configuration, public APIs must provide semantically consistent results, defaults, boundary behavior, error categories, and failure-state guarantees. Classify every input-related failure once: representation and protocol violations raise the host language's own conversion errors, while domain rejections surface as structured core errors carrying stable categories, never messages for callers to parse. A combination excluded by the statically or structurally typed interface may be rejected at C++ compile time or as a Python conversion or signature error; a dynamically representable but unsupported instrument or engine combination is a domain rejection. Preserve native semantics for resource exhaustion and internal failures, and document any separate mapping for operational failures or cancellation. Cover domain parity with tests that drive the same scenarios from both languages.

## C++ Naming Conventions

- **Types:** `PascalCase`
- **Namespaces, functions, and variables:** `snake_case`
- **Private non-static data members:** `snake_case_`
- **Constants and enum values:** `snake_case`
- **Macros:** `PROJECT_UPPER_SNAKE_CASE`

## Build

Activate the Visual Studio Developer environment before using the MSVC or CMake toolchains.

## Commits

Write commit messages adhering to the Conventional Commits specification.

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the default canonical labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, and `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Uses a single-context layout with root `CONTEXT.md` and `docs/adr/`. See `docs/agents/domain.md`.

## Development Status

This project is in Alpha and currently makes no backward-compatibility commitments. Decisive refactoring for clarity is encouraged.
