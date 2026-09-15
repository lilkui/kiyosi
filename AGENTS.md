# AGENTS.md

## API Design and Binding Principles

- **Idiomatic C++ and Python APIs:** Provide idiomatic public C++ and Python APIs with equivalent domain semantics. Semantic equivalence does not require identical API shapes or one-to-one surface coverage. The C++ core must configure, build, test, install, and run with no Python interpreter or binding framework present, and its public headers must reference neither.
- **Single core authority:** The C++ core owns the domain model, rules, state transitions, invariants, defaults, and authoritative domain validation, and must publish its thread-safety contract in its public headers. Every language entry point routes domain operations through the core. The binding layer must hold no domain literal: tolerances, grid sizes, path counts, seeds, shift sizes, solver bounds, iteration limits, calendars, default engine selection, and domain enum choices are core-owned. Express Python keyword defaults as core values—for example `"asset_steps"_a = FiniteDifferenceSettings{}.asset_steps`—so that a default is defined once and is testable from the C++ test suite. Python may define language-specific interface contracts and conveniences, but must not implement, duplicate, or override domain rules or defaults.
- **Semantics-preserving bindings:** Keep the binding layer focused on adapting types, protocols, errors, ownership, lifetimes, concurrency, and calling conventions, and keep those adaptations faithful to the core's domain semantics, state guarantees, and published thread-safety contract. Release the GIL only for operations the core declares safe to run concurrently. Expose core-owned storage as borrowed views tied to the owner's lifetime, and materialize copies only for values the core does not retain; apply the same choice uniformly across accessors of the same kind. The binding layer must not become a second business implementation.
- **Safe validation flow:** Validate language-specific protocols and representation constraints before or during conversion, rejecting unsafe or unintended lossy conversions—including implicit boolean widening, non-integral narrowing, and values outside the target type's range. Then let the C++ core perform authoritative domain validation before committing state changes that depend on validity. Avoid duplicating domain checks outside the core; any necessary pre-check must preserve core acceptance and domain failure semantics. Core validation must never depend on, be weakened by, or be bypassed through prior boundary checks.
- **Observable behavior and errors:** For equivalent domain inputs under the same state and configuration, public APIs must provide semantically consistent results, defaults, boundary behavior, error categories, and failure-state guarantees. Classify every failure once: representation and protocol violations raise the host language's own conversion errors, while every domain rejection—including an unsupported instrument or engine combination—surfaces as a structured core error carrying a stable category, never a message for callers to parse. Cover this parity with tests that drive the same scenarios from both languages.

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
