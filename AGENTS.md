# AGENTS.md

Kiyosi is in alpha; there are no backward-compatibility commitments.

## Before starting

- Before domain exploration, read the [domain guide](docs/agents/domain.md).
- Before architecture or migration changes, read the relevant [ADRs](docs/adr/).
- Before issue or PR operations, read the [issue tracker guide](docs/agents/issue-tracker.md); for triage, also read the [triage labels](docs/agents/triage-labels.md).
- Before writing or editing Rust doc comments, read [RFC 1574](https://raw.githubusercontent.com/rust-lang/rfcs/refs/heads/main/text/1574-more-api-documentation-conventions.md) and the [Rust API Guidelines documentation chapter](https://rust-lang.github.io/api-guidelines/documentation.html). Follow every applicable rule and verify compliance before completing the change.

## Core and APIs

- Keep domain semantics and validation in the Rust core; bindings adapt language representations and calling conventions.
- Keep core dependencies and public APIs independent of Python and binding frameworks. The core must build, test, document and package with Cargo alone.
- Provide idiomatic Rust and Python APIs; interfaces and coverage may differ.
- Define domain defaults once in Rust and derive Python defaults from them.
- Prefer Python `snake_case` choice strings mapped to core enums.
- Equivalent operations must agree on defaults, valid inputs, domain boundaries, validation timing, error categories and failure-state guarantees. Results must agree within numerical acceptance criteria. Test shared scenarios in both languages.

## Validation and errors

- Validate protocols and representations at language boundaries using native conversion or signature errors. Reject unsafe or unintended lossy conversions, including implicit bool-to-number coercion, fractional-to-integer narrowing and out-of-range conversions.
- Run core domain validation before committing state that requires validity. Reuse it for binding prechecks, preserving accepted inputs, validation timing, error categories and failure-state guarantees.
- Return domain rejections through `Result` with stable, structured categories mapped to Python exceptions. Domain rejections must not panic or require callers to parse messages. Return domain errors for representable but unsupported instrument/engine combinations.
- Preserve native semantics for recoverable operational failures and cancellation; document binding mappings separately from panics and process termination.

## Ownership and concurrency

- Return owned values by default across Python boundaries and for newly computed results; Rust accessors may borrow. Tie borrowed views to owners, keep referenced storage valid and at stable addresses throughout the view lifetime, and document mutation and invalidation.
- Express thread safety through `Send`, `Sync`, borrowing and synchronization. In rustdoc, document a library-wide concurrency contract and exceptions for types or operations, covering distinct instances, shared instances and concurrent mutation.
- Document and uphold safety invariants for `unsafe` code, including manual `Send`/`Sync` implementations.
- Release the GIL only for core work that meets the concurrency contract and accesses no Python objects, invokes no Python callbacks and touches no GIL-protected state.

## Numerical correctness and performance

- Validate numerical results with appropriate tolerances and statistical criteria. Use existing tests, independent QuantLib comparisons and convergence checks as applicable; allow floating-point rounding and random-sequence variation within those criteria.
- Optimize measured hot paths while preserving pricing accuracy, numerical stability, validation and safety contracts. Verify performance changes with correctness tests and relevant benchmarks.

## Before completing

- Format and lint modified Rust with rustfmt and Clippy, and Python with Ruff, using repository-selected rules.
- Run affected Rust core and Rust-backed Python binding tests, plus documentation and packaging checks when affected. Find commands in repository configuration and the [Python test guide](tests/README.md).
- During the Rust rewrite, skip the legacy C++ and legacy Python test suites unless the user explicitly requests them.
- Update API and usage docs to reflect current behavior. Keep historical context in ADRs, migration guides or release notes.
- Use Conventional Commits.
