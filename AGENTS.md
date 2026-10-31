# AGENTS.md

Kiyosi is a Rust library for derivatives pricing, with Python as its primary user-facing interface. Delivering a first-class Python API is a core project goal. It is in alpha; there are no backward-compatibility commitments.

## Read before working

- **Domain exploration:** read the [domain guide](docs/agents/domain.md).
- **Architecture changes:** read the relevant [ADRs](docs/adr/).
- **Issue or PR operations:** read the [issue tracker guide](docs/agents/issue-tracker.md). For triage, also read the [triage labels](docs/agents/triage-labels.md).
- **Rust doc comments:** before writing or editing them, read [RFC 1574](https://raw.githubusercontent.com/rust-lang/rfcs/refs/heads/main/text/1574-more-api-documentation-conventions.md) and the [Rust API Guidelines documentation chapter](https://rust-lang.github.io/api-guidelines/documentation.html). Follow every applicable rule and verify compliance before completing the change.

## Rust library and Python API

- Keep domain semantics and validation in Rust; the Python bindings adapt Python representations and calling conventions.
- Keep the Rust library's core dependencies and public APIs independent of Python and binding frameworks. It must be usable directly from Rust and support building, testing, documentation and packaging with Cargo alone.
- Design the Python API around Python user workflows, with idiomatic types, calling conventions, discoverable signatures and type annotations. Keep Rust APIs idiomatic for Rust callers; each interface and its API coverage may differ.
- Define domain defaults once in Rust and derive Python defaults from them.
- Prefer Python `snake_case` choice strings mapped to core enums.
- Keep equivalent operations consistent between the Python and Rust APIs: defaults, valid domain inputs, domain boundaries, validation timing, error categories and failure-state guarantees must agree. Test shared scenarios in both languages, with results agreeing within numerical acceptance criteria.

## Validation and errors

- Validate protocols and representations at language boundaries; report failures through native conversion or signature errors. Reject unsafe or unintended lossy conversions, including implicit bool-to-number coercion, fractional-to-integer narrowing and out-of-range conversions.
- Run core domain validation before committing state that requires validity. Reuse that validation for binding prechecks, preserving accepted inputs, validation timing, error categories and failure-state guarantees.
- Return domain rejections through Rust `Result` with stable, structured categories; map them to appropriate Python exceptions in the bindings. Domain rejections must not panic or require callers to parse messages.
- Treat representable but unsupported instrument/engine combinations as domain errors.
- Preserve native semantics for recoverable operational failures and cancellation; document binding mappings separately from panics and process termination.

## Ownership and concurrency

- Return owned values by default across Python boundaries and for newly computed results; Rust accessors may borrow.
- Tie borrowed views to owners. Keep referenced storage valid and at stable addresses throughout each view's lifetime, and document mutation and invalidation.
- Express thread safety through `Send`, `Sync`, borrowing and synchronization.
- Document the library-wide concurrency contract in rustdoc, including exceptions for types or operations. Cover distinct instances, shared instances and concurrent mutation.
- Document and uphold safety invariants for `unsafe` code, including manual `Send`/`Sync` implementations.
- Release the GIL only for core work that meets the concurrency contract and accesses no Python objects, invokes no Python callbacks and touches no GIL-protected state.

## Numerical correctness and performance

- Validate numerical results with appropriate tolerances and statistical criteria. Use existing tests, independent QuantLib comparisons and convergence checks as applicable; allow floating-point rounding and random-sequence variation within those criteria.
- Optimize measured hot paths while preserving pricing accuracy, numerical stability, validation and safety contracts. Verify performance changes with correctness tests and relevant benchmarks.

## Completion checklist

- Format and lint modified code using repository-selected rules: rustfmt and Clippy for Rust, Ruff for Python.
- Update Rust API docs and Python API and usage docs to reflect current behavior. For Python API changes, verify signatures, type annotations and examples against the public Python interface.
- Use Conventional Commits.
