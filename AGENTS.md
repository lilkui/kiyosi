# AGENTS.md

## API Design and Binding Principles

- **Idiomatic C++ and Python APIs:** Provide idiomatic public C++ and Python APIs with equivalent domain semantics. Semantic equivalence does not require identical API shapes or one-to-one surface coverage. The C++ core—including its public headers, build, tests, installation, and runtime—must remain independent of Python and binding frameworks.
- **Single core authority:** The C++ core owns the domain model, rules, state transitions, invariants, domain defaults, and authoritative domain validation. Every language entry point must route domain operations through the core. Python may define language-specific interface contracts and conveniences, but must not independently implement, duplicate, or override domain rules or defaults.
- **Semantics-preserving bindings:** Keep the binding layer focused on adapting types, protocols, errors, ownership, lifetimes, concurrency, and calling conventions. Such adaptations must preserve the core’s domain semantics, state guarantees, and thread-safety contract. The binding layer must not become a second business implementation.
- **Cross-language behavior:** For equivalent domain inputs under the same state and configuration, public APIs must provide semantically consistent results, domain defaults, boundary behavior, domain error categories, and failure-state guarantees. Expose structured errors from C++ and map them stably to idiomatic Python exceptions without parsing message text.
- **Safe validation flow:** Validate language-specific protocols and representation constraints before or during conversion, rejecting unsafe or unintended lossy conversions. Then let the C++ core perform authoritative domain validation before committing state changes that depend on validity. Avoid duplicating domain checks outside the core; any necessary pre-checks must preserve core acceptance and domain failure semantics. Core validation must never depend on, be weakened by, or be bypassed through prior boundary checks.

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
