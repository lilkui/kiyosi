# 11: Enforce the cross-platform CPU parity gate

**What to build:** A final release gate that runs the complete checked-in parity suite on Windows and Linux and prevents CPU parity from being declared complete while provisional tolerances, metadata-only checks, or presence-only assertions remain.

**Blocked by:** 02: Close vanilla, digital, and Asian parity matrix; 03: Close barrier and binary-barrier case matrix; 04: Enforce explicit finite-difference grid semantics; 06: Complete structured reference and statistical matrix; 07: Add immutable structured transformations; 08: Align scheduled-barrier historical state; 09: Implement Bjerksund–Stensland 2002 parity; 10: Align shared analytics conventions

**Status:** ready-for-agent

- [ ] The same checked-in fixtures and public-boundary tests pass on Windows and Linux.
- [ ] Every in-scope CPU engine family and required variant has a direct executable comparison.
- [ ] Provisional tolerances are replaced by reviewed deterministic or statistical acceptance budgets.
- [ ] Presence-only parity assertions and metadata-only validation rows are removed.
- [ ] The final report identifies any documented parity exception and confirms the pinned reference revision.
