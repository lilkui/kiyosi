# AGENTS.md

Kiyosi is a Rust derivatives pricing library with Python as its primary user-facing interface. A first-class Python API is a core goal. The project is in alpha, with no backward-compatibility commitments.

## Repository guidance

- Before exploring, read [domain guidance](docs/agents/domain.md) for glossary and ADR conventions in this single-context repository.
- When working with issues or specs, read [issue-tracker conventions](docs/agents/issue-tracker.md); both live as Markdown files under `.scratch/`.
- When triaging issues, read [triage labels](docs/agents/triage-labels.md) for the five canonical roles and their local statuses.

## Rust library and Python API

- Keep domain semantics and validation in Rust; bindings adapt Python representations and calling conventions.
- Keep the Rust core's dependencies and public APIs independent of Python and binding frameworks. Support direct Rust use and building, testing, documentation and packaging with Cargo alone.
- Design Python APIs for Python workflows, with idiomatic types, calling conventions, discoverable signatures and type annotations. Keep Rust APIs idiomatic; interfaces and API coverage may differ.
- Define domain defaults once in Rust and derive Python defaults from them.
- Prefer Python `snake_case` choice strings mapped to core enums.
- Equivalent Rust and Python operations must agree on defaults, accepted domain inputs and boundaries, validation timing, error categories and failure-state guarantees.

## Validation and errors

- Validate protocols and representations at language boundaries; report failures through native conversion or signature errors. Reject unsafe or unintended lossy conversions, including implicit bool-to-number coercion, fractional-to-integer narrowing and out-of-range conversions.
- Run core domain validation before committing state that requires validity. Reuse it for binding prechecks, preserving the cross-language guarantees above.
- Return domain rejections through Rust `Result` with stable, structured categories mapped to appropriate Python exceptions. Never panic or require message parsing for domain rejections.
- Treat representable but unsupported instrument/engine combinations as domain errors.
- Preserve native semantics for recoverable operational failures and cancellation; document their binding mappings separately from panic and process-termination behavior.

## Ownership and concurrency

- Return owned values by default across Python boundaries and for newly computed results; Rust accessors may borrow.
- Tie borrowed views to owners; keep referenced storage valid and at stable addresses for each view's lifetime. Document mutation and invalidation rules.
- Express thread safety through `Send`, `Sync`, borrowing and synchronization.
- Document the library-wide concurrency contract in rustdoc: distinct instances, shared instances, concurrent mutation and type- or operation-specific exceptions.
- Document and uphold safety invariants for `unsafe` code, including manual `Send`/`Sync` implementations.
- Release the GIL only for core work that meets the concurrency contract and accesses no Python objects, invokes no Python callbacks and touches no GIL-protected state.

## Testing and performance

- Test required behavior and contracts; add tests only for meaningful coverage beyond the existing suite, not solely to prevent a return to a previous implementation.
- Test shared scenarios in Rust and Python, requiring results to agree within numerical acceptance criteria.
- Validate numerical results with appropriate tolerances and statistical criteria, allowing floating-point rounding and random-sequence variation. Use existing tests, independent QuantLib comparisons and convergence checks as applicable.
- Optimize measured hot paths while preserving pricing accuracy, numerical stability, validation and safety contracts. Verify performance changes with correctness tests and relevant benchmarks.

## Documentation

- Rust doc comments must follow all applicable rules in [RFC 1574](https://raw.githubusercontent.com/rust-lang/rfcs/refs/heads/main/text/1574-more-api-documentation-conventions.md) and the [Rust API Guidelines documentation chapter](https://rust-lang.github.io/api-guidelines/documentation.html).
- Python docstrings must follow NumPy style.
- Describe current behavior directly, without transitional language about changes.

## Completion checklist

- Format and lint modified code using repository-selected rules: rustfmt and Clippy for Rust, Ruff for Python.
- Use Conventional Commits.
