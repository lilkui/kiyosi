# CPU parity fixtures

`tools/generate_cpu_parity.ps1` is the developer-side refresh command. It:

1. Requires the sibling `DerivaSharp` checkout at revision `08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2`.
2. Runs the pinned DerivaSharp deterministic and Monte Carlo test projects.
3. Validates the referenced source symbols and refreshes `tests/fixtures/cpu_parity.tsv` with provenance, outputs, tolerances, convergence settings, and Monte Carlo budgets.

```powershell
pwsh ./tools/generate_cpu_parity.ps1
```

Use `-DerivaSharpRoot`, `-OutputPath`, or `-ReferenceTestProjects` when the checkout or test project locations differ; the output manifest must already exist. C++ tests load the checked-in TSV only; configuring or building Kiyosi never requires the external checkout.
