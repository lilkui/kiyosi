# AGENTS.md

## Workflow

- Before domain exploration, read the [domain guide](docs/agents/domain.md).
- For architecture or migration changes, read the relevant [ADRs](docs/adr/).
- Before issue/PR operations, read [issue tracker](docs/agents/issue-tracker.md); for triage, also read [triage labels](docs/agents/triage-labels.md).
- Format and lint modified Rust with rustfmt/Clippy and Python with Ruff; use repository-selected rules.
- Run affected core and Python tests, plus documentation and packaging checks when affected. Use repository configuration and the [Python test guide](tests/README.md) for check commands.
- Use Conventional Commits.
- Alpha: no backward-compatibility commitments.
- Update API and usage docs to match current behavior; keep historical context in ADRs, migration guides or release notes.

## Core and API

- Keep domain semantics and validation in the Rust core; bindings adapt language representations and calling conventions.
- Provide idiomatic Rust and Python APIs; interfaces and coverage may differ. Equivalent operations must agree on results within numerical acceptance criteria, defaults, valid inputs, domain boundaries, validation timing, error categories and failure-state guarantees. Test shared scenarios in both languages.
- Keep core dependencies and public APIs independent of Python and binding frameworks; the core must build, test, document and package with Cargo alone.
- Define domain defaults once in Rust and derive Python defaults from them.
- Prefer Python `snake_case` choice strings mapped to core enums.

## Validation and Errors

- Validate protocols and representations at language boundaries using native conversion/signature errors. Reject unsafe or unintended lossy conversions, including implicit bool-to-number coercion, fractional-to-integer narrowing and out-of-range conversions.
- Run core domain validation before committing state that requires validity. Reuse it for binding pre-checks, preserving acceptance, validation timing, error categories and failure-state guarantees.
- Return domain rejections through `Result` with stable, structured categories mapped to Python exceptions; callers must not parse messages and domain rejections must not panic. Representable but unsupported instrument/engine combinations require domain errors.
- Preserve native semantics for recoverable operational failures and cancellation; document binding mappings separately from panics and process termination.

## Ownership and Concurrency

- Return owned values across Python boundaries and for newly computed results by default; Rust accessors may borrow. Tie borrowed views to owners, keep referenced storage valid and at stable addresses for the view lifetime, and document mutation and invalidation.
- Express thread-safety through `Send`, `Sync`, borrowing and synchronization. Document a library-wide concurrency contract and type/operation exceptions in rustdoc, covering distinct instances, shared instances and concurrent mutation.
- Document and uphold safety invariants for `unsafe` code, including manual `Send`/`Sync` implementations.
- Release the GIL only for core work that meets the concurrency contract and accesses no Python objects, invokes no Python callbacks and touches no GIL-protected state.

## Numerical Correctness and Performance

- Validate numerical results with appropriate tolerances and statistical criteria. Use existing tests, independent QuantLib comparisons and convergence checks as applicable; allow floating-point rounding and random-sequence variation within those criteria.
- Optimize measured hot paths while preserving pricing accuracy, numerical stability, validation and safety contracts. Verify performance changes with correctness tests and relevant benchmarks.
