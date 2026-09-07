# CPU parity fixture test plan

1. Add failing parser/provenance tests for mandatory revision, source symbol, convention, reference kind, and tolerance fields.
2. Add failing executable closure tests grouped by typed public seams: vanilla, digital, barrier, Asian, and structured.
3. Extend the test-only schema and replace fixture rows with complete reviewed inputs, exact references, and provenance.
4. Add a developer-only PowerShell regeneration script that validates the pinned checkout and extracts the referenced source literals.
5. Add deterministic/discretized/statistical assertions, including refinement and equal-seed repeatability.
6. Document regeneration/review, audit the ADR scope in the executable required-pair set, and run focused plus full CTest validation.
7. Review the final diff for repository standards, specification coverage, behavioral gaps, and assertion quality; record results in `.testagent/status.md`.
